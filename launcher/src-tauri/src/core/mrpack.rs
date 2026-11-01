//! Modrinth modpacks (.mrpack): a zip with `modrinth.index.json` (files to download) and
//! `overrides/` + `client-overrides/` (configs etc. copied into the game folder).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::download::{download_all, Download};
use super::instances::{self, Icon, InstalledMod, Instance, Loader, PackSource};
use super::modrinth;
use super::paths::{ensure_parent, Paths};

#[derive(Deserialize)]
struct Index {
    name: String,
    #[serde(rename = "versionId", default)]
    version_id: Option<String>,
    files: Vec<PackFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
struct PackFile {
    path: String,
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    downloads: Vec<String>,
    #[serde(rename = "fileSize", default)]
    file_size: Option<u64>,
}

/// Only plain relative paths: a pack must not write outside the game folder.
pub fn safe_relative(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    if p.components().all(|c| matches!(c, Component::Normal(_))) {
        Ok(p.to_path_buf())
    } else {
        bail!("unsafe path in modpack: {path}")
    }
}

/// The hosts the Modrinth pack format allows downloads from, over https only: a pack can't
/// fetch from anywhere else (each file is also checked against its SHA-1).
const DOWNLOAD_HOSTS: [&str; 4] = ["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"];

pub fn allowed_download(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    u.scheme() == "https" && u.port().is_none() && u.username().is_empty() && u.password().is_none() && u.host_str().is_some_and(|h| DOWNLOAD_HOSTS.contains(&h))
}

pub struct Origin {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub icon_url: Option<String>,
}

/// An opened .mrpack: its index and the zip with the overrides.
pub struct Pack {
    index: Index,
    zip: zip::ZipArchive<std::fs::File>,
}

pub fn open(file: &Path) -> Result<Pack> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).context("opening the modpack")?).context("not a modpack file")?;
    let index = {
        let mut entry = zip.by_name("modrinth.index.json").map_err(|_| anyhow!("Not a Modrinth modpack (.mrpack)"))?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        serde_json::from_slice(&buf)?
    };
    Ok(Pack { index, zip })
}

/// One file of a modpack, for showing what's inside.
#[derive(Serialize, Clone, Debug)]
pub struct Item {
    pub path: String,
    /// mod | resourcepack | shader | other
    pub kind: String,
    pub title: String,
    pub project_id: Option<String>,
    pub version: Option<String>,
    pub icon_url: Option<String>,
    /// false: only the server uses it
    pub client: bool,
    pub server: bool,
    /// shipped inside the pack (overrides) rather than downloaded
    pub bundled: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Contents {
    pub name: String,
    pub version: Option<String>,
    pub game_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub files: Vec<Item>,
    /// how many config and other files the pack brings besides `files`
    pub other_files: usize,
}

/// Where a file goes by its path: mods/x.jar is a mod, and so on.
fn kind_of(path: &str) -> &'static str {
    match path.split_once('/') {
        Some(("mods", rest)) if !rest.contains('/') => "mod",
        Some(("resourcepacks", rest)) if !rest.contains('/') => "resourcepack",
        Some(("shaderpacks", rest)) if !rest.contains('/') => "shader",
        _ => "other",
    }
}

impl Pack {
    /// Minecraft version, loader and loader version the pack wants.
    fn target(&self) -> Result<(String, Loader, Option<String>)> {
        let deps = &self.index.dependencies;
        let game = deps.get("minecraft").ok_or_else(|| anyhow!("The modpack doesn't say which Minecraft version it needs"))?;
        let (loader, version) = [("neoforge", Loader::NeoForge), ("forge", Loader::Forge), ("quilt-loader", Loader::Quilt), ("fabric-loader", Loader::Fabric)]
            .iter()
            .find_map(|(k, l)| deps.get(*k).map(|v| (*l, Some(v.clone()))))
            .unwrap_or((Loader::Vanilla, None));
        Ok((game.clone(), loader, version))
    }

    /// Downloaded files the client uses.
    fn client_files(&self) -> impl Iterator<Item = &PackFile> {
        self.index.files.iter().filter(|f| !f.env.as_ref().and_then(|e| e.get("client")).is_some_and(|c| c == "unsupported"))
    }

    /// Overrides as (zip entry, relative path): overrides/ first, then client-overrides/ on top.
    fn overrides(&mut self) -> Result<Vec<(usize, String)>> {
        let mut out = Vec::new();
        for prefix in ["overrides/", "client-overrides/"] {
            for i in 0..self.zip.len() {
                let entry = self.zip.by_index(i)?;
                let Some(rel) = entry.name().strip_prefix(prefix) else { continue };
                if rel.is_empty() || entry.is_dir() {
                    continue;
                }
                safe_relative(rel)?;
                out.push((i, rel.to_string()));
            }
        }
        Ok(out)
    }

    /// Every path the pack puts into the game folder.
    pub fn all_paths(&mut self) -> Result<Vec<String>> {
        let mut out: Vec<String> = self.client_files().map(|f| f.path.clone()).collect();
        out.extend(self.overrides()?.into_iter().map(|(_, p)| p));
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// What's inside, with Modrinth's names and icons for what it knows.
    pub async fn contents(&mut self, client: &reqwest::Client) -> Result<Contents> {
        let (game_version, loader, loader_version) = self.target()?;
        let known = modrinth::identify(client, self.index.files.iter().filter_map(|f| f.hashes.get("sha1").cloned()).collect()).await;
        let mut files = Vec::new();
        for f in &self.index.files {
            let env = |side: &str| !f.env.as_ref().and_then(|e| e.get(side)).is_some_and(|c| c == "unsupported");
            let k = f.hashes.get("sha1").and_then(|h| known.get(h));
            let name = f.path.rsplit('/').next().unwrap_or(&f.path);
            files.push(Item {
                path: f.path.clone(),
                kind: kind_of(&f.path).into(),
                title: k.map(|k| k.title.clone()).unwrap_or_else(|| name.trim_end_matches(".jar").trim_end_matches(".zip").into()),
                project_id: k.map(|k| k.project_id.clone()),
                version: k.map(|k| k.version.clone()),
                icon_url: k.and_then(|k| k.icon_url.clone()),
                client: env("client"),
                server: env("server"),
                bundled: false,
            });
        }
        let mut other_files = 0;
        for (_, rel) in self.overrides()? {
            let kind = kind_of(&rel);
            if kind == "other" {
                other_files += 1;
                continue;
            }
            let name = rel.rsplit('/').next().unwrap_or(&rel);
            files.push(Item {
                title: name.trim_end_matches(".jar").trim_end_matches(".zip").into(),
                path: rel.clone(),
                kind: kind.into(),
                project_id: None,
                version: None,
                icon_url: None,
                client: true,
                server: true,
                bundled: true,
            });
        }
        files.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.title.to_lowercase().cmp(&b.title.to_lowercase())));
        Ok(Contents { name: self.index.name.clone(), version: self.index.version_id.clone(), game_version, loader, loader_version, files, other_files })
    }
}

/// How `apply` treats what's in the instance already.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Replace the old pack's files with the new pack's; what was added since stays.
    Update,
    /// Back to exactly the pack: mods, resource packs, shaders and the folders the pack ships
    /// (config, …) are cleared first. Worlds stay.
    Reset,
}

/// Creates a new instance from a .mrpack file.
pub async fn import(
    client: &reqwest::Client,
    paths: &Paths,
    file: &Path,
    origin: Origin,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Instance> {
    progress("Reading modpack", 0, 0);
    let pack = open(file)?;
    let (game, loader, loader_version) = pack.target()?;
    let icon_url = origin.icon_url.clone();
    let inst = instances::create(paths, &pack.index.name, &game, loader, loader_version, None)?;
    let id = inst.id.clone();
    drop(pack);
    let mut inst = match apply(client, paths, inst, file, origin, Mode::Reset, &[], progress).await {
        Ok(inst) => inst,
        Err(e) => {
            let _ = instances::delete(paths, &id);
            return Err(e);
        }
    };
    if let Some(url) = &icon_url {
        if let Ok(res) = client.get(url).send().await {
            if let Ok(bytes) = res.bytes().await {
                if std::fs::write(paths.instance(&inst.id).join("icon.png"), &bytes).is_ok() {
                    inst.icon = Some(Icon::File);
                    instances::save(paths, &inst)?;
                }
            }
        }
    }
    Ok(inst)
}

/// Puts a modpack's files into an instance. Everything is downloaded and unpacked into a
/// staging folder first, so a failed download leaves the instance as it was. `old_files` are
/// the paths the instance's previous pack version put there. Mods that were turned off stay off.
#[allow(clippy::too_many_arguments)]
pub async fn apply(
    client: &reqwest::Client,
    paths: &Paths,
    mut inst: Instance,
    file: &Path,
    origin: Origin,
    mode: Mode,
    old_files: &[String],
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Instance> {
    progress("Reading modpack", 0, 0);
    let mut pack = open(file)?;
    let (game_version, loader, loader_version) = pack.target()?;
    let game = paths.game_dir(&inst.id);
    let staging = paths.instance(&inst.id).join("pack-staging");
    let _ = std::fs::remove_dir_all(&staging);

    let staged: Result<Vec<String>> = async {
        let mut files = Vec::new();
        let mut downloads = Vec::new();
        for f in pack.client_files() {
            let url = f.downloads.iter().find(|u| allowed_download(u)).ok_or_else(|| anyhow!("{} doesn't download from Modrinth, GitHub or GitLab", f.path))?;
            let sha1 = f.hashes.get("sha1").filter(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())).ok_or_else(|| anyhow!("{} has no checksum", f.path))?;
            downloads.push(Download { url: url.clone(), path: staging.join(safe_relative(&f.path)?), sha1: Some(sha1.to_lowercase()), size: f.file_size, executable: false });
            files.push(f.path.clone());
        }
        download_all(client, downloads, |d, t| progress("Downloading mods", d, t)).await?;
        progress("Unpacking", 0, 0);
        for (i, rel) in pack.overrides()? {
            let target = staging.join(safe_relative(&rel)?);
            ensure_parent(&target)?;
            let mut entry = pack.zip.by_index(i)?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(target, buf)?;
            files.push(rel);
        }
        files.sort();
        files.dedup();
        Ok(files)
    }
    .await;
    let new_files = match staged {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    // turned off before (an update keeps that): by Modrinth project, as a new version is a new
    // file name, else by path. A reset turns everything back on.
    let keep_off = mode == Mode::Update;
    let tracked_rel = |m: &InstalledMod| format!("{}/{}", instances::content_dir(&m.kind), m.filename);
    let off_projects: HashSet<String> = inst
        .mods
        .iter()
        .filter(|m| keep_off && m.kind != "datapack" && game.join(format!("{}.disabled", tracked_rel(m))).exists())
        .map(|m| m.project_id.clone())
        .collect();
    let off_paths: HashSet<&String> = old_files.iter().filter(|p| keep_off && game.join(format!("{p}.disabled")).exists()).collect();

    progress("Installing modpack", 0, 0);
    for rel in old_files {
        let path = game.join(safe_relative(rel)?);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(game.join(format!("{rel}.disabled")));
    }
    if mode == Mode::Reset {
        let mut dirs: HashSet<&str> = ["mods", "resourcepacks", "shaderpacks"].into();
        dirs.extend(new_files.iter().filter_map(|p| p.split_once('/').map(|(d, _)| d)).filter(|d| *d != "saves"));
        for d in dirs {
            let _ = std::fs::remove_dir_all(game.join(d));
        }
    }

    let known = modrinth::identify(client, pack.client_files().filter_map(|f| f.hashes.get("sha1").cloned()).collect()).await;
    let mut pack_mods = Vec::new();
    for f in pack.client_files() {
        let kind = kind_of(&f.path);
        let Some(k) = f.hashes.get("sha1").and_then(|h| known.get(h)) else { continue };
        if kind == "other" {
            continue;
        }
        pack_mods.push(InstalledMod {
            kind: kind.into(),
            world: None,
            project_id: k.project_id.clone(),
            version_id: k.version_id.clone(),
            title: k.title.clone(),
            version: k.version.clone(),
            filename: f.path.rsplit('/').next().unwrap_or(&f.path).into(),
            icon_url: k.icon_url.clone(),
        });
    }

    for rel in &new_files {
        let off = off_paths.contains(rel) || pack_mods.iter().any(|m| &tracked_rel(m) == rel && off_projects.contains(&m.project_id));
        let target = game.join(if off { format!("{rel}.disabled") } else { rel.clone() });
        ensure_parent(&target)?;
        let _ = std::fs::remove_file(&target);
        std::fs::rename(staging.join(safe_relative(rel)?), &target).with_context(|| format!("moving {rel} into the instance"))?;
    }
    let _ = std::fs::remove_dir_all(&staging);

    let old: HashSet<&String> = old_files.iter().collect();
    inst.mods.retain(|m| match mode {
        Mode::Reset => m.kind == "datapack",
        Mode::Update => m.kind == "datapack" || !old.contains(&tracked_rel(m)),
    });
    // a mod added by hand that the pack now brings too is the pack's from now on
    inst.mods.retain(|m| !pack_mods.iter().any(|p| p.kind == m.kind && p.filename == m.filename));
    inst.mods.extend(pack_mods);

    inst.game_version = game_version;
    inst.loader = loader;
    inst.loader_version = loader_version;
    inst.pack = Some(PackSource {
        project_id: origin.project_id,
        version_id: origin.version_id,
        name: pack.index.name.clone(),
        version: pack.index.version_id.clone(),
        files: new_files,
    });
    instances::save(paths, &inst)?;
    Ok(inst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escaping_paths() {
        assert!(safe_relative("mods/sodium.jar").is_ok());
        assert!(safe_relative("../evil.jar").is_err());
        assert!(safe_relative("/etc/passwd").is_err());
    }

    #[test]
    fn downloads_only_from_the_format_hosts() {
        assert!(allowed_download("https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar"));
        assert!(allowed_download("https://github.com/a/b/releases/download/1/x.jar"));
        assert!(allowed_download("https://raw.githubusercontent.com/a/b/main/x.jar"));
        assert!(allowed_download("https://gitlab.com/a/b/-/x.jar"));
        assert!(!allowed_download("http://cdn.modrinth.com/x.jar"), "no plain http");
        assert!(!allowed_download("https://evil.example/x.jar"));
        assert!(!allowed_download("https://cdn.modrinth.com.evil.example/x.jar"));
        assert!(!allowed_download("https://cdn.modrinth.com@evil.example/x.jar"));
        assert!(!allowed_download("https://cdn.modrinth.com:8443/x.jar"));
        assert!(!allowed_download("file:///etc/passwd"));
        assert!(!allowed_download("not a url"));
    }

    /// Builds a real .mrpack (one Modrinth download, a server-only file, overrides) and imports it.
    #[tokio::test]
    async fn imports_a_real_mrpack() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("tessera-mrpack-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let client = crate::core::http_client();
        let versions: Vec<serde_json::Value> = crate::core::download::fetch_json(
            &client,
            "https://api.modrinth.com/v2/project/P7dR8mSH/version?loaders=%5B%22fabric%22%5D&game_versions=%5B%221.21.1%22%5D",
        )
        .await
        .unwrap();
        let file = &versions[0]["files"][0];
        let index = serde_json::json!({
            "formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Drop Test",
            "files": [
                { "path": format!("mods/{}", file["filename"].as_str().unwrap()), "hashes": { "sha1": file["hashes"]["sha1"] },
                  "env": { "client": "required", "server": "required" }, "downloads": [file["url"]], "fileSize": file["size"] },
                { "path": "mods/server-only.jar", "hashes": { "sha1": "0" }, "env": { "client": "unsupported", "server": "required" },
                  "downloads": ["https://example.invalid/x.jar"], "fileSize": 1 }
            ],
            "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.16.9" }
        });
        let pack = root.join("test.mrpack");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&pack).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("modrinth.index.json", o).unwrap();
        zip.write_all(index.to_string().as_bytes()).unwrap();
        zip.start_file("overrides/config/test.toml", o).unwrap();
        zip.write_all(b"x = 1").unwrap();
        zip.start_file("client-overrides/options.txt", o).unwrap();
        zip.write_all(b"fov:1.0").unwrap();
        zip.finish().unwrap();

        let paths = Paths::new(root.join("data"));
        let origin = Origin { project_id: None, version_id: None, icon_url: None };
        let inst = import(&client, &paths, &pack, origin, &|_, _, _| {}).await.unwrap();
        let game = paths.game_dir(&inst.id);
        assert_eq!((inst.name.as_str(), inst.loader, inst.loader_version.as_deref()), ("Drop Test", Loader::Fabric, Some("0.16.9")));
        assert!(game.join("mods").join(file["filename"].as_str().unwrap()).exists());
        assert!(!game.join("mods/server-only.jar").exists(), "server-only files are skipped");
        assert_eq!(std::fs::read(game.join("config/test.toml")).unwrap(), b"x = 1");
        assert_eq!(std::fs::read(game.join("options.txt")).unwrap(), b"fov:1.0");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn write_pack(path: &Path, version: &str, files: &[(&str, &str)]) {
        use std::io::Write;
        let index = serde_json::json!({
            "formatVersion": 1, "game": "minecraft", "versionId": version, "name": "Update Test", "files": [],
            "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.16.9" }
        });
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("modrinth.index.json", o).unwrap();
        zip.write_all(index.to_string().as_bytes()).unwrap();
        for (name, body) in files {
            zip.start_file(format!("overrides/{name}"), o).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    /// An update swaps the pack's files and keeps what was added and what was turned off;
    /// a reset puts back exactly the pack.
    #[tokio::test]
    async fn updates_and_resets() {
        let root = std::env::temp_dir().join(format!("tessera-mrpack-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let client = crate::core::http_client();
        let paths = Paths::new(root.join("data"));
        let (v1, v2) = (root.join("v1.mrpack"), root.join("v2.mrpack"));
        write_pack(&v1, "1.0", &[("mods/a.jar", "a1"), ("mods/old.jar", "o"), ("config/c.toml", "c=1")]);
        write_pack(&v2, "2.0", &[("mods/a.jar", "a2"), ("mods/b.jar", "b"), ("config/d.toml", "d=1")]);
        let origin = || Origin { project_id: None, version_id: None, icon_url: None };

        let inst = import(&client, &paths, &v1, origin(), &|_, _, _| {}).await.unwrap();
        let game = paths.game_dir(&inst.id);
        assert_eq!(inst.pack.as_ref().unwrap().files, ["config/c.toml", "mods/a.jar", "mods/old.jar"]);
        std::fs::write(game.join("mods/mine.jar"), "m").unwrap();
        std::fs::write(game.join("config/mine.toml"), "x").unwrap();
        std::fs::rename(game.join("mods/a.jar"), game.join("mods/a.jar.disabled")).unwrap();

        let old = inst.pack.as_ref().unwrap().files.clone();
        let inst = apply(&client, &paths, inst, &v2, origin(), Mode::Update, &old, &|_, _, _| {}).await.unwrap();
        assert_eq!(std::fs::read_to_string(game.join("mods/a.jar.disabled")).unwrap(), "a2", "stays off, new version");
        assert!(!game.join("mods/a.jar").exists());
        assert!(game.join("mods/b.jar").exists() && game.join("config/d.toml").exists());
        assert!(!game.join("mods/old.jar").exists() && !game.join("config/c.toml").exists(), "dropped by the pack");
        assert!(game.join("mods/mine.jar").exists() && game.join("config/mine.toml").exists(), "added by hand");
        assert_eq!(inst.pack.as_ref().unwrap().version.as_deref(), Some("2.0"));
        assert!(!paths.instance(&inst.id).join("pack-staging").exists());

        let old = inst.pack.as_ref().unwrap().files.clone();
        let inst = apply(&client, &paths, inst, &v2, origin(), Mode::Reset, &old, &|_, _, _| {}).await.unwrap();
        assert_eq!(std::fs::read_to_string(game.join("mods/a.jar")).unwrap(), "a2", "on again");
        assert!(!game.join("mods/mine.jar").exists() && !game.join("config/mine.toml").exists());
        assert_eq!(instances::content(&paths, &inst).len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}
