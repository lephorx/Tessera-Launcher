//! Import instances from other launchers: Prism Launcher / MultiMC (instance.cfg +
//! mmc-pack.json) and CurseForge (minecraftinstance.json). The game folder is copied.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::instances::{self, Instance, Loader};
use super::paths::Paths;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Found {
    pub name: String,
    pub path: String,
    pub game_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LauncherDir {
    pub launcher: String,
    pub path: String,
    pub instances: Vec<Found>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// Where launchers keep their instances by default on this OS.
fn default_dirs() -> Vec<(&'static str, PathBuf)> {
    let h = home();
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        let sup = h.join("Library/Application Support");
        dirs.push(("Prism Launcher", sup.join("PrismLauncher/instances")));
        dirs.push(("MultiMC", sup.join("MultiMC/instances")));
        dirs.push(("CurseForge", h.join("Documents/curseforge/minecraft/Instances")));
    } else if cfg!(target_os = "windows") {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| h.join("AppData/Roaming"));
        dirs.push(("Prism Launcher", appdata.join("PrismLauncher/instances")));
        dirs.push(("CurseForge", h.join("curseforge/minecraft/Instances")));
    } else {
        dirs.push(("Prism Launcher", h.join(".local/share/PrismLauncher/instances")));
        dirs.push(("MultiMC", h.join(".local/share/multimc/instances")));
        dirs.push(("CurseForge", h.join("curseforge/minecraft/Instances")));
    }
    dirs
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// One Prism/MultiMC instance folder.
fn prism_instance(dir: &Path) -> Option<Found> {
    let cfg = std::fs::read_to_string(dir.join("instance.cfg")).ok()?;
    let name = cfg.lines().find_map(|l| l.strip_prefix("name=")).map(String::from).unwrap_or_else(|| dir.file_name().unwrap().to_string_lossy().into());
    let pack = read_json(&dir.join("mmc-pack.json"))?;
    let mut game_version = None;
    let mut loader = (Loader::Vanilla, None);
    for c in pack["components"].as_array()? {
        let version = c["version"].as_str().map(String::from);
        match c["uid"].as_str()? {
            "net.minecraft" => game_version = version,
            "net.fabricmc.fabric-loader" => loader = (Loader::Fabric, version),
            "org.quiltmc.quilt-loader" => loader = (Loader::Quilt, version),
            "net.minecraftforge" => loader = (Loader::Forge, version),
            "net.neoforged" => loader = (Loader::NeoForge, version),
            _ => {}
        }
    }
    let game = [".minecraft", "minecraft"].iter().map(|d| dir.join(d)).find(|p| p.is_dir())?;
    Some(Found { name, path: game.to_string_lossy().into(), game_version: game_version?, loader: loader.0, loader_version: loader.1 })
}

/// One CurseForge instance folder.
fn curseforge_instance(dir: &Path) -> Option<Found> {
    let json = read_json(&dir.join("minecraftinstance.json"))?;
    let name = json["name"].as_str()?.to_string();
    let game_version = json["gameVersion"].as_str()?.to_string();
    // baseModLoader.name: "forge-47.2.0", "neoforge-21.1.1", "fabric-0.15.7-1.20.1", "quilt-0.26.0-1.20.1"
    let loader = match json["baseModLoader"]["name"].as_str() {
        Some(n) => {
            let mut parts = n.splitn(2, '-');
            let kind = parts.next().unwrap_or_default();
            let rest = parts.next().unwrap_or_default();
            let version = rest.split('-').next().map(String::from);
            match kind {
                "forge" => (Loader::Forge, version),
                "neoforge" => (Loader::NeoForge, version),
                "fabric" => (Loader::Fabric, version),
                "quilt" => (Loader::Quilt, version),
                _ => (Loader::Vanilla, None),
            }
        }
        None => (Loader::Vanilla, None),
    };
    Some(Found { name, path: dir.to_string_lossy().into(), game_version, loader: loader.0, loader_version: loader.1 })
}

/// Lists importable instances below `dir` (a launcher's instances folder, or the launcher folder itself).
pub fn scan(launcher: &str, dir: &Path) -> LauncherDir {
    // Canonical paths: on macOS and Windows "instances" and "Instances" are the same folder.
    let mut candidates: Vec<PathBuf> = [dir.to_path_buf(), dir.join("instances"), dir.join("Instances")]
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    candidates.sort();
    candidates.dedup();
    let mut instances = Vec::new();
    for base in candidates {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if let Some(found) = prism_instance(&p).or_else(|| curseforge_instance(&p)) {
                if !instances.iter().any(|f: &Found| f.path == found.path) {
                    instances.push(found);
                }
            }
        }
    }
    instances.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    LauncherDir { launcher: launcher.into(), path: dir.to_string_lossy().into(), instances }
}

pub fn detect() -> Vec<LauncherDir> {
    default_dirs().into_iter().filter(|(_, d)| d.is_dir()).map(|(name, d)| scan(name, &d)).filter(|l| !l.instances.is_empty()).collect()
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        let kind = e.file_type()?;
        if kind.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// Imports one found instance: a new Tessera instance with a copy of its game folder.
pub fn import(paths: &Paths, found: &Found) -> Result<Instance> {
    let source = PathBuf::from(&found.path);
    if !source.is_dir() {
        return Err(anyhow!("{} no longer exists", found.path));
    }
    if found.loader != Loader::Vanilla && found.loader_version.is_none() {
        return Err(anyhow!("{} has no loader version", found.name));
    }
    let inst = instances::create(paths, &found.name, &found.game_version, found.loader, found.loader_version.clone(), None)?;
    if let Err(e) = copy_dir(&source, &paths.game_dir(&inst.id)) {
        let _ = instances::delete(paths, &inst.id);
        return Err(e);
    }
    Ok(inst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_prism_and_curseforge() {
        let root = std::env::temp_dir().join(format!("tessera-import-{}", std::process::id()));
        let prism = root.join("instances/My Pack");
        std::fs::create_dir_all(prism.join(".minecraft/mods")).unwrap();
        std::fs::write(prism.join("instance.cfg"), "InstanceType=OneSix\nname=My Pack\n").unwrap();
        std::fs::write(
            prism.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.3.0"}]}"#,
        )
        .unwrap();
        let cf = root.join("instances/CF Pack");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("minecraftinstance.json"), r#"{"name":"CF Pack","gameVersion":"1.20.1","baseModLoader":{"name":"fabric-0.15.7-1.20.1"}}"#).unwrap();

        let found = scan("Test", &root);
        assert_eq!(found.instances.len(), 2);
        let cfp = &found.instances[0];
        assert_eq!((cfp.name.as_str(), cfp.loader, cfp.loader_version.as_deref()), ("CF Pack", Loader::Fabric, Some("0.15.7")));
        let mp = &found.instances[1];
        assert_eq!((mp.loader, mp.loader_version.as_deref()), (Loader::Forge, Some("47.3.0")));
        assert!(mp.path.ends_with(".minecraft"));

        let paths = Paths::new(root.join("tessera"));
        let inst = import(&paths, mp).unwrap();
        assert!(paths.game_dir(&inst.id).join("mods").is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
}
