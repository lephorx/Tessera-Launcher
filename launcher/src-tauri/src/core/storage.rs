//! What the launcher keeps on disk, for Settings → Resources: instances (only theirs),
//! shared game files (versions, libraries, assets, Java) and shared files no instance uses
//! anymore. Clearing unused files is always safe: a launch downloads whatever is missing.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use super::instances::{self, Instance};
use super::paths::Paths;

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Usage {
    /// Instances: worlds, mods, configs
    pub unique: u64,
    /// Game files instances share
    pub shared: u64,
    /// Shared game files no instance needs
    pub unused: u64,
}

fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map(|rd| rd.flatten().map(|e| size(&e.path())).sum()).unwrap_or(0)
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// A version folder belongs to an instance: its Minecraft version, or its loader's profile
/// (named after both, e.g. `fabric-loader-0.16.9-1.21.1`, `forge-1.20.1-47.4.0`, `neoforge-21.1.77`).
fn version_used(id: &str, all: &[Instance]) -> bool {
    all.iter().any(|i| {
        if id == i.game_version {
            return true;
        }
        let Some(lv) = i.loader_version.as_deref() else { return false };
        let parts: Vec<&str> = id.split('-').collect();
        parts.contains(&lv) && (parts.contains(&i.game_version.as_str()) || id.starts_with("neoforge-"))
    })
}

/// Everything unused: version folders, Java runtimes, and asset files.
fn unused(paths: &Paths) -> Vec<std::path::PathBuf> {
    let all = instances::list(paths).unwrap_or_default();
    let mut out = Vec::new();

    let mut components = HashSet::new();
    let mut indexes = HashSet::new();
    let versions = paths.meta().join("versions");
    for e in std::fs::read_dir(&versions).into_iter().flatten().flatten() {
        let id = e.file_name().to_string_lossy().into_owned();
        if !version_used(&id, &all) {
            out.push(e.path());
            continue;
        }
        if let Some(v) = read_json(&e.path().join(format!("{id}.json"))) {
            if let Some(c) = v["javaVersion"]["component"].as_str() {
                components.insert(c.to_string());
            }
            if let Some(a) = v["assetIndex"]["id"].as_str() {
                indexes.insert(a.to_string());
            }
        }
    }
    // without the game's own version file there's no telling which runtime it needs
    let unknown = all.iter().any(|i| !versions.join(&i.game_version).join(format!("{}.json", i.game_version)).exists());
    if !unknown {
        for e in std::fs::read_dir(paths.meta().join("java")).into_iter().flatten().flatten() {
            if !components.contains(&*e.file_name().to_string_lossy()) {
                out.push(e.path());
            }
        }
        let mut objects = HashSet::new();
        for id in &indexes {
            let Some(index) = read_json(&paths.assets().join("indexes").join(format!("{id}.json"))) else { return out };
            if let Some(map) = index["objects"].as_object() {
                objects.extend(map.values().filter_map(|o| o["hash"].as_str().map(String::from)));
            }
        }
        for prefix in std::fs::read_dir(paths.assets().join("objects")).into_iter().flatten().flatten() {
            for f in std::fs::read_dir(prefix.path()).into_iter().flatten().flatten() {
                if !objects.contains(&*f.file_name().to_string_lossy()) {
                    out.push(f.path());
                }
            }
        }
    }
    out
}

pub fn usage(paths: &Paths) -> Usage {
    let unused: u64 = unused(paths).iter().map(|p| size(p)).sum();
    Usage { unique: size(&paths.instances()), shared: size(&paths.meta()).saturating_sub(unused), unused }
}

/// Deletes the unused shared files; returns how many bytes that freed.
pub fn clear_unused(paths: &Paths) -> Result<u64> {
    let mut freed = 0;
    for p in unused(paths) {
        let n = size(&p);
        let gone = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        if gone.is_ok() {
            freed += n;
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(gv: &str, lv: Option<&str>) -> Instance {
        let mut i: Instance = serde_json::from_value(serde_json::json!({
            "id": "x", "name": "x", "game_version": gv, "loader": if lv.is_some() { "fabric" } else { "vanilla" },
            "created_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap();
        i.loader_version = lv.map(String::from);
        i
    }

    #[test]
    fn knows_which_versions_are_used() {
        let all = [inst("1.21.3", Some("0.20.1")), inst("1.20.1", Some("47.4.28"))];
        assert!(version_used("1.21.3", &all));
        assert!(version_used("fabric-loader-0.20.1-1.21.3", &all));
        assert!(version_used("forge-1.20.1-47.4.28", &all));
        assert!(!version_used("fabric-loader-0.20.1-26.3", &all));
        assert!(!version_used("26.3", &all));
        assert!(!version_used("forge-1.20.1-47.3.0", &all));
    }
}

#[cfg(test)]
mod real_folder {
    /// TESSERA_DATA=<launcher data folder> cargo test real_usage -- --ignored --nocapture (reads only)
    #[test]
    #[ignore]
    fn real_usage() {
        let paths = super::Paths::new(std::env::var("TESSERA_DATA").expect("TESSERA_DATA").into());
        println!("{:?}", super::usage(&paths));
        for p in super::unused(&paths).iter().filter(|p| !p.starts_with(paths.assets())) {
            println!("unused: {}", p.display());
        }
    }
}
