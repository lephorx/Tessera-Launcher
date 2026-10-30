//! Prepares an instance (version, libraries, assets, Java, natives) and builds the java command.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use md5::{Digest, Md5};

use super::assets::asset_downloads;
use super::download::{download_all, Download};
use super::forge;
use super::instances::{Instance, Loader};
use super::loaders::{self, Profile};
use super::java::ensure_java;
use super::paths::{maven_path, Paths};
use super::versions::{flatten_arguments, os_name, rules_allow, version_json, Library};

/// `progress(stage, done, total)`; stage is shown to the user as is.
pub type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Send + Sync);

pub struct LaunchPlan {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub game_dir: PathBuf,
}

pub struct Player {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
}

impl Player {
    /// Offline accounts use the same UUID the vanilla server derives for offline-mode players.
    pub fn offline(name: &str) -> Self {
        let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
        bytes[6] = (bytes[6] & 0x0f) | 0x30;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self { name: name.into(), uuid: hex::encode(bytes), access_token: "0".into() }
    }
}

/// `group:artifact[:classifier]` without the version, so a loader's library replaces vanilla's.
fn library_key(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    match parts.as_slice() {
        [g, a, _v, c, ..] => format!("{g}:{a}:{c}"),
        [g, a, ..] => format!("{g}:{a}"),
        _ => name.into(),
    }
}

struct Resolved {
    downloads: Vec<Download>,
    classpath: Vec<PathBuf>,
    natives_jars: Vec<PathBuf>,
}

fn resolve_libraries(paths: &Paths, libraries: &[Library]) -> Result<Resolved> {
    let lib_dir = paths.libraries();
    let mut out = Resolved { downloads: vec![], classpath: vec![], natives_jars: vec![] };
    let mut seen = HashSet::new();

    for lib in libraries {
        if !rules_allow(lib.rules.as_ref()) {
            continue;
        }
        let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());
        if let Some(a) = artifact {
            let path = lib_dir.join(a.path.clone().map(PathBuf::from).or_else(|| maven_path(&lib.name)).context("library path")?);
            if seen.insert(library_key(&lib.name)) {
                out.classpath.push(path.clone());
            }
            // Forge's own jars have no URL: its installer made them already.
            if !a.url.is_empty() {
                out.downloads.push(Download { url: a.url.clone(), path, sha1: a.sha1.clone(), size: a.size, executable: false });
            }
        } else if let Some(base) = &lib.url {
            // Fabric: plain maven coordinates plus a repository URL.
            let rel = maven_path(&lib.name).context("library path")?;
            let url = format!("{}/{}", base.trim_end_matches('/'), rel.to_string_lossy().replace('\\', "/"));
            let path = lib_dir.join(rel);
            if seen.insert(library_key(&lib.name)) {
                out.classpath.push(path.clone());
            }
            out.downloads.push(Download { url, path, sha1: lib.sha1.clone(), size: lib.size, executable: false });
        }

        // Pre-1.19 natives: a classifier jar to unpack into the natives folder.
        if let Some(natives) = &lib.natives {
            if let Some(classifier) = natives.get(os_name()) {
                let arch = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
                let classifier = classifier.replace("${arch}", arch);
                if let Some(a) = lib.downloads.as_ref().and_then(|d| d.classifiers.as_ref()).and_then(|c| c.get(&classifier)) {
                    let path = lib_dir.join(a.path.clone().map(PathBuf::from).context("natives path")?);
                    out.natives_jars.push(path.clone());
                    out.downloads.push(Download { url: a.url.clone(), path, sha1: a.sha1.clone(), size: a.size, executable: false });
                }
            }
        }
    }
    Ok(out)
}

fn extract_natives(jars: &[PathBuf], dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for jar in jars {
        let file = std::fs::File::open(jar)?;
        let mut zip = zip::ZipArchive::new(file)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let Some(name) = entry.enclosed_name() else { continue };
            if entry.is_dir() || name.starts_with("META-INF") {
                continue;
            }
            let target = dir.join(name.file_name().unwrap_or_default());
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(target, buf)?;
        }
    }
    Ok(())
}

/// Where the game opens straight into.
#[derive(Clone, Debug)]
pub enum QuickPlay {
    /// A singleplayer world (its folder in `saves`).
    World(String),
    /// A multiplayer server address (host or host:port).
    Server(String),
}

pub async fn prepare(
    client: &reqwest::Client,
    paths: &Paths,
    inst: &Instance,
    player: &Player,
    memory_mb: u32,
    quick_play: Option<&QuickPlay>,
    progress: Progress<'_>,
) -> Result<LaunchPlan> {
    progress("Reading version", 0, 0);
    let vanilla = version_json(client, paths, &inst.game_version).await?;
    let component = vanilla.java_version.as_ref().map(|j| j.component.clone()).unwrap_or_else(|| "jre-legacy".into());
    let java = match inst.java_path.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(custom) => {
            let p = PathBuf::from(custom.trim());
            if !p.is_file() {
                return Err(anyhow!("The Java set for this instance doesn't exist: {}", p.display()));
            }
            p
        }
        None => ensure_java(client, paths, &component, &|d, t| progress("Downloading Java", d, t)).await?,
    };

    // Vanilla first: Forge and NeoForge patch the client jar with its libraries in place.
    let mut vanilla_libs = resolve_libraries(paths, &vanilla.libraries)?;
    let client_jar = paths.version_dir(&vanilla.id).join(format!("{}.jar", vanilla.id));
    let client_dl = vanilla.downloads.as_ref().ok_or_else(|| anyhow!("Version has no client download"))?;
    vanilla_libs.downloads.push(Download {
        url: client_dl.client.url.clone(),
        path: client_jar.clone(),
        sha1: client_dl.client.sha1.clone(),
        size: client_dl.client.size,
        executable: false,
    });
    download_all(client, vanilla_libs.downloads, |d, t| progress("Downloading libraries", d, t)).await?;

    let loader: Option<Profile> = match inst.loader {
        Loader::Vanilla => None,
        other => {
            let lv = inst.loader_version.as_deref().ok_or_else(|| anyhow!("This instance has no loader version"))?;
            Some(match other {
                Loader::Fabric | Loader::Quilt => loaders::meta_profile(client, paths, other, &inst.game_version, lv).await?,
                _ => forge::install(client, paths, other, &inst.game_version, lv, &java, &client_jar, progress).await?,
            })
        }
    };

    // Loader libraries first: they win over vanilla's copy of the same library.
    let mut libraries: Vec<Library> = loader.as_ref().map(|p| p.libraries.clone()).unwrap_or_default();
    libraries.extend(vanilla.libraries.iter().cloned());
    let mut resolved = resolve_libraries(paths, &libraries)?;
    resolved.classpath.push(client_jar);
    download_all(client, resolved.downloads, |d, t| progress("Downloading loader libraries", d, t)).await?;

    let asset_index = vanilla.asset_index.as_ref().ok_or_else(|| anyhow!("Version has no assets"))?;
    let assets = asset_downloads(client, paths, asset_index).await?;
    download_all(client, assets, |d, t| progress("Downloading assets", d, t)).await?;

    progress("Starting", 0, 0);
    let natives_dir = paths.natives(&inst.id);
    let _ = std::fs::remove_dir_all(&natives_dir);
    extract_natives(&resolved.natives_jars, &natives_dir)?;
    let game_dir = paths.game_dir(&inst.id);
    std::fs::create_dir_all(&game_dir)?;

    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath = resolved.classpath.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join(sep);

    let mut jvm: Vec<String>;
    let mut game: Vec<String>;
    if let Some(args) = &vanilla.arguments {
        jvm = flatten_arguments(&args.jvm);
        game = flatten_arguments(&args.game);
    } else {
        jvm = vec!["-Djava.library.path=${natives_directory}".into(), "-cp".into(), "${classpath}".into()];
        if cfg!(target_os = "macos") {
            jvm.insert(0, "-XstartOnFirstThread".into());
        }
        game = vanilla.minecraft_arguments.as_deref().unwrap_or_default().split_whitespace().map(String::from).collect();
    }
    if let Some(args) = loader.as_ref().and_then(|p| p.arguments.as_ref()) {
        jvm.extend(flatten_arguments(&args.jvm));
        game.extend(flatten_arguments(&args.game));
    }
    let main_class = loader.as_ref().map(|p| p.main_class.clone()).unwrap_or(vanilla.main_class.clone());

    let vars: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", player.name.clone()),
        ("version_name", inst.game_version.clone()),
        ("game_directory", game_dir.to_string_lossy().into_owned()),
        ("assets_root", paths.assets().to_string_lossy().into_owned()),
        ("game_assets", paths.assets().to_string_lossy().into_owned()),
        ("assets_index_name", asset_index.id.clone()),
        ("auth_uuid", player.uuid.clone()),
        ("auth_access_token", player.access_token.clone()),
        ("auth_session", format!("token:{}", player.access_token)),
        ("clientid", String::new()),
        ("auth_xuid", String::new()),
        ("user_type", "legacy".into()),
        ("user_properties", "{}".into()),
        ("version_type", if vanilla.kind.is_empty() { "release".into() } else { vanilla.kind.clone() }),
        ("natives_directory", natives_dir.to_string_lossy().into_owned()),
        ("launcher_name", "tessera".into()),
        ("launcher_version", env!("CARGO_PKG_VERSION").into()),
        ("classpath", classpath),
        ("classpath_separator", sep.into()),
        ("library_directory", paths.libraries().to_string_lossy().into_owned()),
    ]);
    let fill = |s: &String| {
        let mut out = s.clone();
        for (k, v) in &vars {
            out = out.replace(&format!("${{{k}}}"), v);
        }
        out
    };

    let mut args = vec![format!("-Xmx{memory_mb}M"), "-Xms512M".into()];
    if let Some(extra) = &inst.jvm_args {
        args.extend(extra.split_whitespace().map(String::from));
    }
    args.extend(jvm.iter().map(fill));
    args.push(main_class);
    args.extend(game.iter().map(fill));
    if let Some(r) = inst.resolution {
        args.extend(["--width".into(), r.width.to_string(), "--height".into(), r.height.to_string()]);
    }
    // Straight into a world or server (quick play: 1.20+, the versions whose arguments know it).
    let quick_play_supported = vanilla.arguments.as_ref().is_some_and(|a| serde_json::to_string(&a.game).unwrap_or_default().contains("quickPlaySingleplayer"));
    match quick_play {
        Some(QuickPlay::World(world)) => {
            if !quick_play_supported {
                return Err(anyhow!("Opening a world directly needs Minecraft 1.20 or newer"));
            }
            args.extend(["--quickPlaySingleplayer".into(), world.clone()]);
        }
        Some(QuickPlay::Server(address)) if quick_play_supported => {
            args.extend(["--quickPlayMultiplayer".into(), address.clone()]);
        }
        // older versions join with --server/--port
        Some(QuickPlay::Server(address)) => {
            let (host, port) = match address.rsplit_once(':') {
                Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.to_string()),
                _ => (address.clone(), "25565".to_string()),
            };
            args.extend(["--server".into(), host, "--port".into(), port]);
        }
        None => {}
    }
    Ok(LaunchPlan { java, args, game_dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_vanilla_server() {
        // UUID.nameUUIDFromBytes("OfflinePlayer:Notch") = b50ad385-829d-3141-a216-7e7d7539ba7f
        assert_eq!(Player::offline("Notch").uuid, "b50ad385829d3141a2167e7d7539ba7f");
    }

    #[test]
    fn loader_library_replaces_vanilla() {
        assert_eq!(library_key("org.ow2.asm:asm:9.7"), library_key("org.ow2.asm:asm:9.6"));
        assert_ne!(library_key("org.lwjgl:lwjgl:3.3.3"), library_key("org.lwjgl:lwjgl:3.3.3:natives-macos"));
    }
}
