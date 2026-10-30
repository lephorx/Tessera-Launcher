//! Modrinth modpacks (.mrpack): a zip with `modrinth.index.json` (files to download) and
//! `overrides/` + `client-overrides/` (configs etc. copied into the game folder).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use super::download::{download_all, Download};
use super::instances::{self, Icon, Instance, Loader, PackSource};
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

pub struct Origin {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub icon_url: Option<String>,
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
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).context("opening the modpack")?).context("not a modpack file")?;
    let index: Index = {
        let mut entry = zip.by_name("modrinth.index.json").map_err(|_| anyhow!("Not a Modrinth modpack (.mrpack)"))?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        serde_json::from_slice(&buf)?
    };

    let game = index.dependencies.get("minecraft").ok_or_else(|| anyhow!("The modpack doesn't say which Minecraft version it needs"))?;
    let (loader, loader_version) = [
        ("neoforge", Loader::NeoForge),
        ("forge", Loader::Forge),
        ("quilt-loader", Loader::Quilt),
        ("fabric-loader", Loader::Fabric),
    ]
    .iter()
    .find_map(|(k, l)| index.dependencies.get(*k).map(|v| (*l, Some(v.clone()))))
    .unwrap_or((Loader::Vanilla, None));

    let mut inst = instances::create(paths, &index.name, game, loader, loader_version, None)?;
    let game_dir = paths.game_dir(&inst.id);

    let result: Result<()> = async {
        // overrides first, then client-overrides on top
        for prefix in ["overrides/", "client-overrides/"] {
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i)?;
                let Some(rel) = entry.name().strip_prefix(prefix).map(String::from) else { continue };
                if rel.is_empty() || entry.is_dir() {
                    continue;
                }
                let target = game_dir.join(safe_relative(&rel)?);
                ensure_parent(&target)?;
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                std::fs::write(target, buf)?;
            }
        }

        let mut downloads = Vec::new();
        for f in &index.files {
            if f.env.as_ref().and_then(|e| e.get("client")).is_some_and(|c| c == "unsupported") {
                continue;
            }
            let url = f.downloads.first().ok_or_else(|| anyhow!("{} has no download", f.path))?;
            downloads.push(Download {
                url: url.clone(),
                path: game_dir.join(safe_relative(&f.path)?),
                sha1: f.hashes.get("sha1").cloned(),
                size: f.file_size,
                executable: false,
            });
        }
        download_all(client, downloads, |d, t| progress("Downloading mods", d, t)).await?;

        if let Some(url) = &origin.icon_url {
            if let Ok(res) = client.get(url).send().await {
                if let Ok(bytes) = res.bytes().await {
                    if std::fs::write(paths.instance(&inst.id).join("icon.png"), &bytes).is_ok() {
                        inst.icon = Some(Icon::File);
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    if let Err(e) = result {
        let _ = instances::delete(paths, &inst.id);
        return Err(e);
    }
    inst.pack = Some(PackSource {
        project_id: origin.project_id,
        version_id: origin.version_id,
        name: index.name.clone(),
        version: index.version_id.clone(),
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
}
