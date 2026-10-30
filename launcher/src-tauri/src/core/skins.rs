//! Skins: your saved skins, Minecraft's 9 default skins (read from a client jar), and which
//! one each player uses.
//!
//! Offline accounts have no skin on Mojang's servers, so Minecraft gives them a default
//! skin. To still show the chosen skin in game, the launcher puts it into a small resource
//! pack ("Tessera Skin") in the instance, replacing the default skins, and turns the pack
//! on in options.txt. That shows in singleplayer and to anyone who has the same pack; with
//! a Microsoft account the skin (and cape) is uploaded to Mojang instead.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::download::{download_all, Download};
use super::paths::Paths;
use super::versions;

pub const DEFAULT_NAMES: [&str; 9] = ["steve", "alex", "ari", "efe", "kai", "makena", "noor", "sunny", "zuri"];
const PACK_DIR: &str = "Tessera Skin";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    /// 4-pixel arms (Steve)
    Classic,
    /// 3-pixel arms (Alex)
    Slim,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedSkin {
    pub id: String,
    pub name: String,
    pub model: Model,
    /// Cape id from the Microsoft account's Minecraft profile.
    #[serde(default)]
    pub cape: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct Store {
    skins: Vec<SavedSkin>,
    /// Player name → skin id ("default:<name>:<model>" for a default skin).
    #[serde(default)]
    selected: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
pub struct SkinView {
    pub id: String,
    pub name: String,
    pub model: Model,
    pub cape: Option<String>,
    /// data:image/png;base64,…
    pub texture: String,
    pub default: bool,
}

fn dir(paths: &Paths) -> PathBuf {
    paths.root.join("skins")
}

fn load(paths: &Paths) -> Store {
    std::fs::read(dir(paths).join("skins.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(paths: &Paths, store: &Store) -> Result<()> {
    std::fs::create_dir_all(dir(paths))?;
    std::fs::write(dir(paths).join("skins.json"), serde_json::to_vec_pretty(store)?)?;
    Ok(())
}

fn data_url(bytes: &[u8]) -> String {
    use base64::Engine;
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Width and height of a PNG, from its header.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((u32::from_be_bytes(bytes[16..20].try_into().ok()?), u32::from_be_bytes(bytes[20..24].try_into().ok()?)))
}

/// A skin file Minecraft accepts: PNG, 64×64 (or the old 64×32), at most 24 KB like Mojang's limit.
pub fn validate(bytes: &[u8]) -> Result<()> {
    match png_size(bytes) {
        Some((64, 64)) | Some((64, 32)) if bytes.len() <= 24 * 1024 => Ok(()),
        Some((64, 64)) | Some((64, 32)) => bail!("That skin file is too big (24 KB at most)"),
        Some((w, h)) => bail!("A skin is 64×64 pixels; that image is {w}×{h}"),
        None => bail!("That isn't a PNG image"),
    }
}

// ── default skins ──

fn defaults_dir(paths: &Paths) -> PathBuf {
    dir(paths).join("defaults")
}

/// Extracts the default skins from a client jar (1.19.3+ has all nine). Uses one that's
/// already downloaded, or downloads the latest release's jar once.
pub async fn ensure_defaults(client: &reqwest::Client, paths: &Paths) -> Result<()> {
    let out = defaults_dir(paths);
    if out.join("wide/zuri.png").exists() && out.join("slim/zuri.png").exists() {
        return Ok(());
    }
    let mut jar = find_jar(paths);
    if jar.is_none() {
        let manifest = versions::manifest(client).await?;
        let v = versions::version_json(client, paths, &manifest.latest.release).await?;
        let dl = v.downloads.as_ref().ok_or_else(|| anyhow!("No client download"))?;
        let path = paths.version_dir(&v.id).join(format!("{}.jar", v.id));
        download_all(client, vec![Download { url: dl.client.url.clone(), path: path.clone(), sha1: dl.client.sha1.clone(), size: dl.client.size, executable: false }], |_, _| {}).await?;
        jar = Some(path);
    }
    let jar = jar.ok_or_else(|| anyhow!("No Minecraft jar to read the default skins from"))?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&jar)?)?;
    for model in ["wide", "slim"] {
        std::fs::create_dir_all(out.join(model))?;
        for name in DEFAULT_NAMES {
            let mut f = zip.by_name(&format!("assets/minecraft/textures/entity/player/{model}/{name}.png")).context("the jar has no default skins")?;
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            std::fs::write(out.join(model).join(format!("{name}.png")), bytes)?;
        }
    }
    Ok(())
}

/// A downloaded client jar that has the nine default skins (newest first).
fn find_jar(paths: &Paths) -> Option<PathBuf> {
    let mut jars: Vec<PathBuf> = std::fs::read_dir(paths.meta().join("versions"))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            let jar = e.path().join(format!("{id}.jar"));
            jar.exists().then_some(jar)
        })
        .collect();
    jars.sort_by_key(|j| std::fs::metadata(j).and_then(|m| m.modified()).ok());
    jars.into_iter().rev().find(|j| {
        zip::ZipArchive::new(match std::fs::File::open(j) {
            Ok(f) => f,
            Err(_) => return false,
        })
        .map(|mut z| z.by_name("assets/minecraft/textures/entity/player/slim/zuri.png").is_ok())
        .unwrap_or(false)
    })
}

fn title(name: &str) -> String {
    let mut c = name.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn default_id(name: &str, model: &Model) -> String {
    format!("default:{name}:{}", if *model == Model::Slim { "slim" } else { "classic" })
}

fn texture_of(paths: &Paths, id: &str) -> Result<(Vec<u8>, Model)> {
    if let Some(rest) = id.strip_prefix("default:") {
        let (name, model) = rest.split_once(':').ok_or_else(|| anyhow!("Unknown skin"))?;
        if !DEFAULT_NAMES.contains(&name) {
            bail!("Unknown skin");
        }
        let (folder, model) = if model == "slim" { ("slim", Model::Slim) } else { ("wide", Model::Classic) };
        return Ok((std::fs::read(defaults_dir(paths).join(folder).join(format!("{name}.png")))?, model));
    }
    let store = load(paths);
    let skin = store.skins.iter().find(|s| s.id == id).ok_or_else(|| anyhow!("Unknown skin"))?;
    Ok((std::fs::read(dir(paths).join(format!("{}.png", skin.id)))?, skin.model.clone()))
}

// ── listing and editing ──

#[derive(Serialize)]
pub struct Skins {
    pub saved: Vec<SkinView>,
    pub defaults: Vec<SkinView>,
    pub selected: Option<String>,
}

pub fn list(paths: &Paths, player: &str) -> Skins {
    let store = load(paths);
    let saved = store
        .skins
        .iter()
        .filter_map(|s| {
            let bytes = std::fs::read(dir(paths).join(format!("{}.png", s.id))).ok()?;
            Some(SkinView { id: s.id.clone(), name: s.name.clone(), model: s.model.clone(), cape: s.cape.clone(), texture: data_url(&bytes), default: false })
        })
        .collect();
    let mut defaults = Vec::new();
    for (folder, model) in [("wide", Model::Classic), ("slim", Model::Slim)] {
        for name in DEFAULT_NAMES {
            if let Ok(bytes) = std::fs::read(defaults_dir(paths).join(folder).join(format!("{name}.png"))) {
                let label = format!("{}{}", title(name), if model == Model::Slim { " (slim)" } else { "" });
                defaults.push(SkinView { id: default_id(name, &model), name: label, model: model.clone(), cape: None, texture: data_url(&bytes), default: true });
            }
        }
    }
    Skins { saved, defaults, selected: store.selected.get(&player.to_lowercase()).cloned() }
}

/// Saves a skin from a file, or (with `from`) as a copy of another skin, e.g. a default
/// one with a different name or cape.
pub fn add(paths: &Paths, file: Option<&Path>, from: Option<&str>, name: &str, model: Model, cape: Option<String>) -> Result<SavedSkin> {
    let bytes = match (file, from) {
        (Some(f), _) => std::fs::read(f).context("reading the skin file")?,
        (None, Some(id)) => texture_of(paths, id)?.0,
        _ => bail!("No skin given"),
    };
    validate(&bytes)?;
    let mut store = load(paths);
    if store.skins.len() >= 100 {
        bail!("You have 100 saved skins already. Delete some first.");
    }
    let id = format!("s{}", chrono::Utc::now().timestamp_millis());
    std::fs::create_dir_all(dir(paths))?;
    std::fs::write(dir(paths).join(format!("{id}.png")), &bytes)?;
    let name = name.trim();
    let skin = SavedSkin { id, name: if name.is_empty() { "My skin".into() } else { name.chars().take(40).collect() }, model, cape };
    store.skins.push(skin.clone());
    save(paths, &store)?;
    Ok(skin)
}

pub fn update(paths: &Paths, id: &str, name: &str, model: Model, cape: Option<String>) -> Result<()> {
    let mut store = load(paths);
    let skin = store.skins.iter_mut().find(|s| s.id == id).ok_or_else(|| anyhow!("Unknown skin"))?;
    if !name.trim().is_empty() {
        skin.name = name.trim().chars().take(40).collect();
    }
    skin.model = model;
    skin.cape = cape;
    save(paths, &store)
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let mut store = load(paths);
    store.skins.retain(|s| s.id != id);
    store.selected.retain(|_, v| v != id);
    let _ = std::fs::remove_file(dir(paths).join(format!("{id}.png")));
    save(paths, &store)
}

/// The skin's picture, whether it's slim, and its cape: what Mojang gets for a Microsoft account.
pub fn for_upload(paths: &Paths, id: &str) -> Result<(Vec<u8>, bool, Option<String>)> {
    let (bytes, model) = texture_of(paths, id)?;
    let cape = load(paths).skins.into_iter().find(|s| s.id == id).and_then(|s| s.cape);
    Ok((bytes, model == Model::Slim, cape))
}

pub fn select(paths: &Paths, player: &str, id: Option<&str>) -> Result<()> {
    let mut store = load(paths);
    match id {
        Some(id) => {
            texture_of(paths, id)?; // exists
            store.selected.insert(player.to_lowercase(), id.to_string());
        }
        None => {
            store.selected.remove(&player.to_lowercase());
        }
    }
    save(paths, &store)
}

// ── in game (offline accounts) ──

/// Before a launch: puts the player's skin into the instance's "Tessera Skin" resource pack
/// and turns it on, or removes the pack when they use no skin or play with a Microsoft
/// account (whose skin comes from Mojang). Never fails the launch.
pub fn prepare_instance(paths: &Paths, game_dir: &Path, player: &str, online: bool) {
    if let Err(e) = try_prepare(paths, game_dir, player, online) {
        eprintln!("skin pack: {e:#}");
    }
}

fn try_prepare(paths: &Paths, game_dir: &Path, player: &str, online: bool) -> Result<()> {
    let pack = game_dir.join("resourcepacks").join(PACK_DIR);
    let selected = if online { None } else { load(paths).selected.get(&player.to_lowercase()).cloned() };
    let Some(id) = selected else {
        if pack.exists() {
            std::fs::remove_dir_all(&pack)?;
        }
        return set_pack_enabled(game_dir, false);
    };
    let (bytes, _) = texture_of(paths, &id)?;
    let textures = pack.join("assets/minecraft/textures/entity");
    // 1.19.3+: nine defaults in two models; older versions: steve.png and alex.png
    for folder in ["player/wide", "player/slim"] {
        std::fs::create_dir_all(textures.join(folder))?;
        for name in DEFAULT_NAMES {
            std::fs::write(textures.join(folder).join(format!("{name}.png")), &bytes)?;
        }
    }
    std::fs::write(textures.join("steve.png"), &bytes)?;
    std::fs::write(textures.join("alex.png"), &bytes)?;
    std::fs::write(
        pack.join("pack.mcmeta"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "pack": {
                "description": "Your skin from Tessera Launcher",
                "pack_format": 34,
                "supported_formats": [1, 999],
                "min_format": 1,
                "max_format": 999
            }
        }))?,
    )?;
    set_pack_enabled(game_dir, true)
}

/// Adds or removes "file/Tessera Skin" in options.txt's resourcePacks list.
fn set_pack_enabled(game_dir: &Path, on: bool) -> Result<()> {
    let file = game_dir.join("options.txt");
    let entry = format!("file/{PACK_DIR}");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            let Some(list) = line.strip_prefix("resourcePacks:") else { return line.to_string() };
            found = true;
            let mut packs: Vec<String> = serde_json::from_str(list).unwrap_or_else(|_| vec!["vanilla".into()]);
            packs.retain(|p| p != &entry);
            if on {
                packs.push(entry.clone()); // last = on top
            }
            format!("resourcePacks:{}", serde_json::to_string(&packs).unwrap_or_default())
        })
        .collect();
    if !found {
        if !on {
            return Ok(());
        }
        lines.push(format!("resourcePacks:{}", serde_json::to_string(&["vanilla".to_string(), entry])?));
    }
    std::fs::create_dir_all(game_dir)?;
    let mut out = lines.join("\n");
    out.push('\n');
    if out != text {
        std::fs::write(&file, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    #[test]
    fn checks_skin_files() {
        assert!(validate(&png(64, 64)).is_ok());
        assert!(validate(&png(64, 32)).is_ok());
        assert!(validate(&png(128, 128)).unwrap_err().to_string().contains("128×128"));
        assert!(validate(b"GIF89a").is_err());
    }

    #[test]
    fn turns_the_pack_on_and_off_in_options() {
        let dir = std::env::temp_dir().join(format!("tessera-skin-opts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("options.txt"), "fov:0.5\nresourcePacks:[\"vanilla\",\"file/Faithful.zip\"]\nlang:en_us\n").unwrap();
        set_pack_enabled(&dir, true).unwrap();
        let text = std::fs::read_to_string(dir.join("options.txt")).unwrap();
        assert!(text.contains(r#"resourcePacks:["vanilla","file/Faithful.zip","file/Tessera Skin"]"#), "{text}");
        assert!(text.contains("fov:0.5") && text.contains("lang:en_us"));
        set_pack_enabled(&dir, true).unwrap(); // no duplicates
        assert_eq!(std::fs::read_to_string(dir.join("options.txt")).unwrap().matches("Tessera Skin").count(), 1);
        set_pack_enabled(&dir, false).unwrap();
        assert!(std::fs::read_to_string(dir.join("options.txt")).unwrap().contains(r#"resourcePacks:["vanilla","file/Faithful.zip"]"#));
        // first launch: no options.txt yet
        let fresh = dir.join("fresh");
        set_pack_enabled(&fresh, true).unwrap();
        assert_eq!(std::fs::read_to_string(fresh.join("options.txt")).unwrap(), "resourcePacks:[\"vanilla\",\"file/Tessera Skin\"]\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
