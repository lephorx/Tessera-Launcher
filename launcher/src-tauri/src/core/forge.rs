//! Forge and NeoForge (Minecraft 1.13+). Their installer jar holds `install_profile.json`
//! (libraries and "processors" that patch the game jar) and `version.json` (the launch
//! profile). We do what the installer's client mode does, without running its UI:
//! download the libraries, run the processors once, and keep the profile.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use super::download::{download_all, Download};
use super::loaders::{Loader, Profile};
use super::paths::{ensure_parent, maven_path, Paths};
use super::versions::Library;

#[derive(Deserialize)]
struct InstallProfile {
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    data: HashMap<String, DataEntry>,
    #[serde(default)]
    processors: Vec<Processor>,
    #[serde(default)]
    libraries: Vec<Library>,
}

#[derive(Deserialize)]
struct DataEntry {
    client: String,
}

#[derive(Deserialize)]
struct Processor {
    #[serde(default)]
    sides: Option<Vec<String>>,
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
}

fn installer_url(loader: Loader, game: &str, version: &str) -> Result<(String, String)> {
    Ok(match loader {
        Loader::Forge => (
            format!("forge-{game}-{version}"),
            format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{game}-{version}/forge-{game}-{version}-installer.jar"),
        ),
        Loader::NeoForge => (
            format!("neoforge-{version}"),
            format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"),
        ),
        _ => bail!("not Forge or NeoForge"),
    })
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut entry = zip.by_name(name.trim_start_matches('/')).with_context(|| format!("{name} missing in installer"))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

fn main_class(jar: &Path) -> Result<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).with_context(|| format!("opening {}", jar.display()))?)?;
    let manifest = String::from_utf8(read_entry(&mut zip, "META-INF/MANIFEST.MF")?)?;
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:").map(|s| s.trim().to_string()))
        .ok_or_else(|| anyhow!("{} has no Main-Class", jar.display()))
}

/// Installs (once) and returns the loader profile. `java` runs the processors; `client_jar` must exist.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    loader: Loader,
    game: &str,
    version: &str,
    java: &Path,
    client_jar: &Path,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Profile> {
    let (id, url) = installer_url(loader, game, version)?;
    let dir = paths.version_dir(&id);
    let profile_path = dir.join("version.json");
    let marker = dir.join(".installed");
    if marker.exists() {
        // Reuse the install only while the processors' outputs are still there.
        let outputs = std::fs::read_to_string(&marker).unwrap_or_default();
        if outputs.lines().filter(|l| !l.is_empty()).all(|l| Path::new(l).exists()) {
            let bytes = std::fs::read(&profile_path)?;
            return Ok(serde_json::from_slice(&bytes)?);
        }
    }

    progress("Downloading installer", 0, 0);
    let installer = dir.join("installer.jar");
    download_all(client, vec![Download { url, path: installer.clone(), sha1: None, size: None, executable: false }], |_, _| {}).await?;

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&installer).context("opening the installer")?)?;
    let install: InstallProfile = serde_json::from_slice(&read_entry(&mut zip, "install_profile.json")?)?;
    let version_bytes = read_entry(&mut zip, install.json.as_deref().unwrap_or("version.json"))?;
    let profile: Profile = serde_json::from_slice(&version_bytes).context("reading the loader's version.json")?;

    // Libraries: download what has a URL, take the rest from the installer's maven/ folder.
    // Anything in neither place is produced by the processors below.
    let lib_dir = paths.libraries();
    let mut downloads = Vec::new();
    for lib in install.libraries.iter().chain(profile.libraries.iter()) {
        let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());
        let rel = artifact.and_then(|a| a.path.clone()).map(PathBuf::from).or_else(|| maven_path(&lib.name)).context("library path")?;
        let path = lib_dir.join(&rel);
        match artifact.filter(|a| !a.url.is_empty()) {
            Some(a) => downloads.push(Download { url: a.url.clone(), path, sha1: a.sha1.clone(), size: a.size, executable: false }),
            None => {
                let inner = format!("maven/{}", rel.to_string_lossy().replace('\\', "/"));
                if let Ok(bytes) = read_entry(&mut zip, &inner) {
                    ensure_parent(&path)?;
                    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
                }
            }
        }
    }
    download_all(client, downloads, |d, t| progress("Downloading loader libraries", d, t)).await?;

    // Processor inputs: "[maven coords]" → library path, "'text'" → text, "/path" → file from the installer.
    let extract_dir = std::env::temp_dir().join(format!("tessera-{id}"));
    std::fs::create_dir_all(&extract_dir).context("creating temp dir")?;
    let mut resolve = |value: &str| -> Result<String> {
        if let Some(coords) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            return Ok(lib_dir.join(maven_path(coords).context("bad coords")?).to_string_lossy().into_owned());
        }
        if let Some(text) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return Ok(text.to_string());
        }
        if value.starts_with('/') {
            let out = extract_dir.join(value.trim_start_matches('/'));
            ensure_parent(&out)?;
            std::fs::write(&out, read_entry(&mut zip, value)?).with_context(|| format!("writing {}", out.display()))?;
            return Ok(out.to_string_lossy().into_owned());
        }
        Ok(value.to_string())
    };
    let mut data: HashMap<String, String> = HashMap::new();
    for (key, entry) in &install.data {
        data.insert(key.clone(), resolve(&entry.client)?);
    }
    data.insert("SIDE".into(), "client".into());
    data.insert("MINECRAFT_JAR".into(), client_jar.to_string_lossy().into_owned());
    data.insert("MINECRAFT_VERSION".into(), game.into());
    data.insert("ROOT".into(), paths.meta().to_string_lossy().into_owned());
    data.insert("INSTALLER".into(), installer.to_string_lossy().into_owned());
    data.insert("LIBRARY_DIR".into(), lib_dir.to_string_lossy().into_owned());

    let processors: Vec<&Processor> = install
        .processors
        .iter()
        .filter(|p| p.sides.as_ref().is_none_or(|s| s.iter().any(|x| x == "client")))
        .collect();
    let sep = if cfg!(windows) { ";" } else { ":" };
    for (i, p) in processors.iter().enumerate() {
        progress("Patching the game", i, processors.len());
        let jar = lib_dir.join(maven_path(&p.jar).context("processor jar")?);
        let mut cp = vec![jar.to_string_lossy().into_owned()];
        for c in &p.classpath {
            cp.push(lib_dir.join(maven_path(c).context("processor classpath")?).to_string_lossy().into_owned());
        }
        let mut args = Vec::new();
        for a in &p.args {
            let arg = if let Some(key) = a.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
                data.get(key).cloned().ok_or_else(|| anyhow!("unknown installer value {key}"))?
            } else if a.starts_with('[') {
                resolve(a)?
            } else {
                let mut s = a.clone();
                for (k, v) in &data {
                    s = s.replace(&format!("{{{k}}}"), v);
                }
                s
            };
            args.push(arg);
        }
        let out = tokio::process::Command::new(java)
            .arg("-cp")
            .arg(cp.join(sep))
            .arg(main_class(&jar)?)
            .args(&args)
            .output()
            .await
            .with_context(|| format!("running {}", java.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(8).collect();
            bail!("{} failed:\n{}", p.jar, tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
    }
    progress("Patching the game", processors.len(), processors.len());

    std::fs::write(&profile_path, &version_bytes).context("saving version.json")?;
    // Remember the files the processors made (library paths they were given), to detect a broken install.
    let made: Vec<String> = data
        .values()
        .filter(|v| v.starts_with(&*lib_dir.to_string_lossy()) && Path::new(v.as_str()).exists())
        .cloned()
        .collect();
    std::fs::write(&marker, made.join("\n"))?;
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(profile)
}
