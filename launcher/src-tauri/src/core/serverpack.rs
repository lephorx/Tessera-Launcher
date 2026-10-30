//! A server-side Modrinth pack (.mrpack) built from an instance, for "Create server".
//!
//! CSCM hands it to the itzg/minecraft-server image (TYPE=MODRINTH), which installs the
//! Minecraft version, the loader and every file whose `env.server` isn't "unsupported".
//! Mods Modrinth knows become download links with their side info; mods it doesn't know and
//! the config folders go into `overrides/`.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};

use super::instances::{self, Loader};
use super::modrinth;
use super::paths::Paths;
use super::tapack::{rel_string, sha1_file, walk};

/// Folders a server uses; everything client-only (resource packs, shaders, options) stays out.
const SERVER_FOLDERS: &[&str] = &["mods", "config", "defaultconfigs", "kubejs", "scripts", "global_packs", "datapacks"];

pub(crate) fn sha512_file(path: &std::path::Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub struct Built {
    pub bytes: Vec<u8>,
    pub linked: usize,
    pub embedded: usize,
    /// Mods skipped because Modrinth says they only run on the client.
    pub client_only: usize,
}

pub async fn build(client: &reqwest::Client, paths: &Paths, id: &str, progress: &(dyn Fn(&str, usize, usize) + Send + Sync)) -> Result<Built> {
    let inst = instances::get(paths, id)?;
    let game = paths.game_dir(id);
    progress("Collecting files", 0, 0);
    let mut files = Vec::new();
    for folder in SERVER_FOLDERS {
        if game.join(folder).is_dir() {
            walk(&game, PathBuf::from(folder), &mut files);
        }
    }
    // disabled mods stay off on the server too
    files.retain(|f| !rel_string(f).ends_with(".disabled"));

    let mods: Vec<PathBuf> = files.iter().filter(|f| rel_string(f).starts_with("mods/") && f.components().count() == 2).cloned().collect();
    let mut hashes = Vec::new();
    for (i, m) in mods.iter().enumerate() {
        progress("Checking mods", i, mods.len());
        hashes.push(sha1_file(&game.join(m))?);
    }
    progress("Looking up Modrinth", 0, 0);
    let known = modrinth::versions_by_sha1(client, hashes.clone()).await.unwrap_or_default();
    let ids: Vec<String> = known.values().map(|v| v.project_id.clone()).collect::<HashSet<_>>().into_iter().collect();
    let projects = modrinth::projects(client, &ids).await.unwrap_or_default();

    let mut index_files = Vec::new();
    let mut linked_paths = HashSet::new();
    let mut client_only = 0;
    for (m, sha1) in mods.iter().zip(&hashes) {
        let Some(version) = known.get(sha1) else { continue };
        let Some(file) = version.file_with_sha1(sha1) else { continue };
        let project = projects.get(&version.project_id);
        let side = |s: Option<&String>| match s.map(String::as_str) {
            Some("unsupported") => "unsupported",
            Some("optional") => "optional",
            _ => "required",
        };
        let (client_side, server_side) = (side(project.map(|p| &p.client_side)), side(project.map(|p| &p.server_side)));
        linked_paths.insert(m.clone());
        if server_side == "unsupported" {
            client_only += 1;
            continue;
        }
        index_files.push(json!({
            "path": rel_string(m),
            "hashes": { "sha1": sha1, "sha512": sha512_file(&game.join(m))? },
            "env": { "client": client_side, "server": server_side },
            "downloads": [file.url],
            "fileSize": file.size,
        }));
    }
    let embedded: Vec<&PathBuf> = files.iter().filter(|f| !linked_paths.contains(*f)).collect();

    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), json!(inst.game_version));
    let loader_key = match inst.loader {
        Loader::Vanilla => None,
        Loader::Fabric => Some("fabric-loader"),
        Loader::Quilt => Some("quilt-loader"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
    };
    if let (Some(k), Some(v)) = (loader_key, &inst.loader_version) {
        deps.insert(k.into(), json!(v));
    }
    let index: Value = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": "tessera",
        "name": inst.name,
        "summary": "Server pack built by Tessera Launcher",
        "files": index_files,
        "dependencies": deps,
    });

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("modrinth.index.json", deflated)?;
        zip.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;
        for (i, rel) in embedded.iter().enumerate() {
            progress("Packing files", i, embedded.len());
            let name = rel_string(rel);
            let opts = if name.ends_with(".jar") || name.ends_with(".zip") { stored } else { deflated };
            zip.start_file(format!("server-overrides/{name}"), opts)?;
            std::io::copy(&mut std::fs::File::open(game.join(rel))?, &mut zip)?;
        }
        zip.finish()?;
    }
    let bytes = buf.into_inner();
    if bytes.len() > 200 * 1024 * 1024 {
        bail!("The server pack is bigger than 200 MB. Remove big files from the config folders.");
    }
    Ok(Built { linked: index["files"].as_array().map_or(0, Vec::len), embedded: embedded.len(), client_only, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabric API (server) is linked, Sodium (client-only) left out, configs embedded.
    #[tokio::test]
    async fn builds_a_server_pack_from_real_mods() {
        let root = std::env::temp_dir().join(format!("tessera-serverpack-{}", std::process::id()));
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let inst = instances::create(&paths, "Server Test", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        modrinth::install(&client, &paths, &inst.id, "P7dR8mSH", "mod", None).await.unwrap(); // Fabric API
        modrinth::install(&client, &paths, &inst.id, "AANobbMI", "mod", None).await.unwrap(); // Sodium
        let game = paths.game_dir(&inst.id);
        std::fs::create_dir_all(game.join("config")).unwrap();
        std::fs::write(game.join("config/server.toml"), b"x = 1").unwrap();
        std::fs::write(game.join("options.txt"), b"client only").unwrap();

        let built = build(&client, &paths, &inst.id, &|_, _, _| {}).await.unwrap();
        assert_eq!((built.linked, built.client_only, built.embedded), (1, 1, 1));

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(built.bytes)).unwrap();
        let mut index = String::new();
        zip.by_name("modrinth.index.json").unwrap().read_to_string(&mut index).unwrap();
        let index: Value = serde_json::from_str(&index).unwrap();
        assert_eq!(index["dependencies"]["fabric-loader"], "0.16.9");
        let f = &index["files"][0];
        assert!(f["path"].as_str().unwrap().starts_with("mods/fabric-api"));
        assert_ne!(f["env"]["server"], "unsupported"); // Fabric API is "optional" on servers
        assert_eq!(f["hashes"]["sha512"].as_str().unwrap().len(), 128);
        assert!(zip.by_name("server-overrides/config/server.toml").is_ok());
        assert!(zip.by_name("server-overrides/options.txt").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// What decides whether a server needs a new pack: Minecraft and loader versions, the mods
/// (path and hash) and the bundled files (path and CRC). Not the zip itself, whose times
/// change on every build.
pub fn fingerprint(pack: &[u8]) -> Result<String> {
    use sha1::Sha1;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(pack))?;
    let mut parts: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if file.is_dir() {
            continue;
        }
        if file.name() == "modrinth.index.json" {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            let index: serde_json::Value = serde_json::from_str(&text)?;
            parts.push(format!("deps {}", index["dependencies"]));
            for f in index["files"].as_array().into_iter().flatten() {
                parts.push(format!("mod {} {}", f["path"].as_str().unwrap_or_default(), f["hashes"]["sha1"].as_str().unwrap_or_default()));
            }
        } else {
            parts.push(format!("file {} {:08x}", file.name(), file.crc32()));
        }
    }
    parts.sort();
    Ok(hex::encode(Sha1::digest(parts.join("\n").as_bytes())))
}
