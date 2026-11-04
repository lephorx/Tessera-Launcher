//! The content mods a Horizon server runs (docs/mods.md there): a dedicated server reads the
//! `.hmod` files in `horizon-mods/` of its own folder that `horizon-mods.txt` turns on, and
//! checks that joining players run the same `side = "both"` mods (id, version and SHA-256).
//! So a server gets the instance's turned-on content mods that are `both` or `server`, as
//! the same files; `client` mods (a HUD, a minimap) stay on the player's computer. Native
//! mods aren't sent: a server needs its own engine compiled with them.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::registry::CONTENT;

/// The manifest inside a `.hmod`.
const MANIFEST: &str = "horizon-mod.toml";

#[derive(Debug, PartialEq)]
pub struct ServerMod {
    pub id: String,
    pub file: PathBuf,
}

/// A content mod's id and side, from its manifest (no `side` line is `client`).
fn manifest(file: &Path) -> Option<(String, String)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).ok()?).ok()?;
    let mut text = String::new();
    zip.by_name(MANIFEST).ok()?.take(64 << 10).read_to_string(&mut text).ok()?;
    let m: toml::Value = toml::from_str(&text).ok()?;
    let id = m.get("id")?.as_str()?.to_string();
    let side = m.get("side").and_then(|s| s.as_str()).unwrap_or("client").to_string();
    Some((id, side))
}

/// The instance's content mods that a server runs: turned on, and `both` or `server`.
pub fn list(game_dir: &Path) -> Vec<ServerMod> {
    let switches = std::fs::read_to_string(game_dir.join("horizon-mods.txt")).unwrap_or_default();
    let on = |id: &str| switches.lines().any(|l| l.split_once(':').is_some_and(|(k, v)| k.trim() == format!("mod.{id}") && v.trim() == "true"));
    let Ok(read) = std::fs::read_dir(game_dir.join(CONTENT)) else { return Vec::new() };
    let mut mods: Vec<ServerMod> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "hmod"))
        .filter_map(|file| {
            let (id, side) = manifest(&file)?;
            (on(&id) && (side == "both" || side == "server")).then_some(ServerMod { id, file })
        })
        .collect();
    mods.sort_by(|a, b| a.file.cmp(&b.file));
    mods
}

pub struct Built {
    /// A zip of the `.hmod` files, as they are
    pub bytes: Vec<u8>,
    /// Changes when a mod is added, removed or replaced
    pub hash: String,
    pub count: usize,
}

/// The zip a Tessera server takes (PUT /api/servers/<id>/horizon-mods). Empty when no mod
/// runs on servers, which removes the ones sent before.
pub fn build(game_dir: &Path) -> Result<Built> {
    let mods = list(game_dir);
    let mut hash = Sha256::new();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // .hmod files are zips already
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for m in &mods {
        let name = m.file.file_name().and_then(|n| n.to_str()).context("a mod's file name")?;
        let bytes = std::fs::read(&m.file).with_context(|| format!("reading {name}"))?;
        hash.update(name.as_bytes());
        hash.update(Sha256::digest(&bytes));
        zip.start_file(name, opts)?;
        zip.write_all(&bytes)?;
    }
    let bytes = zip.finish()?.into_inner();
    Ok(Built { bytes, hash: format!("hmods-{}", hex::encode(hash.finalize())), count: mods.len() })
}

/// Copies them into a server folder on this computer and turns them on there.
pub fn install(game_dir: &Path, server_dir: &Path) -> Result<usize> {
    let mods = list(game_dir);
    let folder = server_dir.join(CONTENT);
    std::fs::create_dir_all(&folder)?;
    for m in &mods {
        if let Some(name) = m.file.file_name() {
            std::fs::copy(&m.file, folder.join(name))?;
        }
    }
    let file = server_dir.join("horizon-mods.txt");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !mods.iter().any(|m| l.starts_with(&format!("mod.{}:", m.id)))).map(str::to_string).collect();
    lines.extend(mods.iter().map(|m| format!("mod.{}:true", m.id)));
    std::fs::write(file, lines.iter().map(|l| format!("{l}\n")).collect::<String>())?;
    Ok(mods.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hmod(dir: &Path, file: &str, manifest: &str) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(dir.join(file)).unwrap());
        zip.start_file(MANIFEST, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn servers_get_the_turned_on_mods_that_run_there() {
        let game = std::env::temp_dir().join(format!("tessera-hmods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&game);
        let mods = game.join(CONTENT);
        std::fs::create_dir_all(&mods).unwrap();
        hmod(&mods, "ruby.hmod", "id = \"ruby\"\nside = \"both\"\n");
        hmod(&mods, "hud.hmod", "id = \"hud\"\nside = \"client\"\n");
        hmod(&mods, "old.hmod", "id = \"old\"\n");
        hmod(&mods, "rules.hmod", "id = \"rules\"\nside = \"server\"\n");
        hmod(&mods, "off.hmod", "id = \"off\"\nside = \"both\"\n");
        std::fs::write(game.join("horizon-mods.txt"), "mod.ruby:true\nmod.hud:true\nmod.old:true\nmod.rules:true\nmod.off:false\n").unwrap();

        let ids: Vec<_> = list(&game).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["ruby", "rules"]);
        let built = build(&game).unwrap();
        assert_eq!(built.count, 2);
        let names: Vec<_> = zip::ZipArchive::new(std::io::Cursor::new(&built.bytes)).unwrap().file_names().map(str::to_string).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ruby.hmod".into()) && names.contains(&"rules.hmod".into()));
        // the same files: the same hash
        assert_eq!(build(&game).unwrap().hash, built.hash);

        let server = game.join("server");
        std::fs::create_dir_all(&server).unwrap();
        std::fs::write(server.join("horizon-mods.txt"), "mod.ruby:false\nkey.ruby.glow:key.keyboard.g\n").unwrap();
        assert_eq!(install(&game, &server).unwrap(), 2);
        assert!(server.join(CONTENT).join("ruby.hmod").is_file());
        let text = std::fs::read_to_string(server.join("horizon-mods.txt")).unwrap();
        assert_eq!(text, "key.ruby.glow:key.keyboard.g\nmod.ruby:true\nmod.rules:true\n");
        let _ = std::fs::remove_dir_all(&game);
    }
}
