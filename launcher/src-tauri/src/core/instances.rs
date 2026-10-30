use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::paths::Paths;

pub use super::loaders::Loader;

/// Instance icon: one of the built-in pixel blocks on a gradient, or `icon.png` in the instance folder.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Icon {
    Generated { background: String, symbol: String },
    File,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Where a modpack instance came from, for updates later.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PackSource {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub game_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    /// Max heap in MB; None = the launcher default.
    #[serde(default)]
    pub memory_mb: Option<u32>,
    /// Custom Java executable; None = the runtime Mojang names for this version.
    #[serde(default)]
    pub java_path: Option<String>,
    /// Extra JVM arguments, space separated.
    #[serde(default)]
    pub jvm_args: Option<String>,
    /// Game window size; None = the game's default.
    #[serde(default)]
    pub resolution: Option<Resolution>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_played: Option<DateTime<Utc>>,
    #[serde(default)]
    pub mods: Vec<InstalledMod>,
    #[serde(default)]
    pub icon: Option<Icon>,
    #[serde(default)]
    pub pack: Option<PackSource>,
    /// Invite code when this instance is shared through the Tessera API.
    #[serde(default)]
    pub share_code: Option<String>,
    /// Library groups this instance is in (an instance can be in several).
    #[serde(default)]
    pub groups: Vec<String>,
    /// CSCM servers made from this instance; they get its server-side mods.
    #[serde(default)]
    pub servers: Vec<LinkedServer>,
    /// Installed from someone else's share: where updates come from, and what we may do.
    #[serde(default)]
    pub joined: Option<JoinedShare>,
    /// Live sync state of the share this instance is on (as owner or member).
    #[serde(default)]
    pub share_sync: Option<ShareSync>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ShareSync {
    /// The share revision this copy matches.
    pub revision: i64,
    /// Fingerprint of the synced folders right after the last sync, to notice local changes.
    pub signature: String,
    /// Content files (mods, resource packs, shaders) at the last sync, to merge local changes.
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JoinedShare {
    pub code: String,
    /// "viewer" | "editor"
    pub role: String,
    #[serde(default)]
    pub owner: Option<String>,
    /// The share's updated_at when this copy was installed or last pulled.
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinkedServer {
    pub machine_id: String,
    pub server_id: i64,
    pub name: String,
    /// Fingerprint of the server pack the server has, to know when it needs the new one.
    #[serde(default)]
    pub pack_hash: Option<String>,
    /// The address written to the game's server list (servers.dat).
    #[serde(default)]
    pub address: Option<String>,
    /// The server's port on its machine, to join by IP when CSCM can't be asked.
    #[serde(default)]
    pub port: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstalledMod {
    /// "mod" | "resourcepack" | "shader" | "datapack"
    #[serde(default = "default_kind")]
    pub kind: String,
    /// For datapacks: the world they were added to.
    #[serde(default)]
    pub world: Option<String>,
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version: String,
    pub filename: String,
    pub icon_url: Option<String>,
}

fn default_kind() -> String {
    "mod".into()
}

/// Folder (below the game dir) a kind of content goes into.
pub fn content_dir(kind: &str) -> &'static str {
    match kind {
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        _ => "mods",
    }
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() { "instance".into() } else { slug }
}

pub fn list(paths: &Paths) -> Result<Vec<Instance>> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(paths.instances()) else { return Ok(out) };
    for entry in dir.flatten() {
        let file = entry.path().join("instance.json");
        if let Ok(bytes) = std::fs::read(&file) {
            if let Ok(inst) = serde_json::from_slice::<Instance>(&bytes) {
                out.push(inst);
            }
        }
    }
    out.sort_by(|a, b| b.last_played.or(Some(b.created_at)).cmp(&a.last_played.or(Some(a.created_at))));
    Ok(out)
}

pub fn get(paths: &Paths, id: &str) -> Result<Instance> {
    if id.contains(['/', '\\', '.']) {
        bail!("invalid instance id");
    }
    let bytes = std::fs::read(paths.instance(id).join("instance.json")).map_err(|_| anyhow!("Instance not found"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save(paths: &Paths, inst: &Instance) -> Result<()> {
    let dir = paths.instance(&inst.id);
    std::fs::create_dir_all(paths.game_dir(&inst.id))?;
    let tmp = dir.join("instance.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(inst)?)?;
    std::fs::rename(tmp, dir.join("instance.json"))?;
    Ok(())
}

pub fn create(
    paths: &Paths,
    name: &str,
    game_version: &str,
    loader: Loader,
    loader_version: Option<String>,
    icon: Option<Icon>,
) -> Result<Instance> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Give the instance a name");
    }
    let base = slugify(name);
    let mut id = base.clone();
    let mut n = 2;
    while paths.instance(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let inst = Instance {
        id,
        name: name.into(),
        game_version: game_version.into(),
        loader,
        loader_version,
        memory_mb: None,
        java_path: None,
        jvm_args: None,
        resolution: None,
        created_at: Utc::now(),
        last_played: None,
        mods: Vec::new(),
        icon,
        pack: None,
        share_code: None,
        groups: Vec::new(),
        servers: Vec::new(),
        joined: None,
        share_sync: None,
    };
    save(paths, &inst)?;
    Ok(inst)
}

pub fn delete(paths: &Paths, id: &str) -> Result<()> {
    get(paths, id)?; // validates the id
    std::fs::remove_dir_all(paths.instance(id))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("  Create: Above & Beyond! "), "create-above-beyond");
        assert_eq!(slugify("???"), "instance");
    }

    #[test]
    fn create_list_delete() {
        let dir = std::env::temp_dir().join(format!("tessera-test-{}", std::process::id()));
        let paths = Paths::new(dir.clone());
        let a = create(&paths, "My Pack", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        let b = create(&paths, "My Pack", "1.21.1", Loader::Vanilla, None, None).unwrap();
        assert_eq!((a.id.as_str(), b.id.as_str()), ("my-pack", "my-pack-2"));
        assert_eq!(list(&paths).unwrap().len(), 2);
        assert!(get(&paths, "../etc").is_err());
        delete(&paths, "my-pack").unwrap();
        assert_eq!(list(&paths).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Sets `icon.png` from an image file (copied into the instance folder).
pub fn set_icon_file(paths: &Paths, id: &str, source: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(source)?;
    if bytes.len() > 4 * 1024 * 1024 {
        bail!("The image is too big (max 4 MB)");
    }
    std::fs::write(paths.instance(id).join("icon.png"), bytes)?;
    Ok(())
}

/// `icon.png` as a data URL for the UI.
pub fn icon_data_url(paths: &Paths, id: &str) -> Option<String> {
    image_data_url(&paths.instance(id).join("icon.png"))
}

/// Any small image file as a data URL (previews before an instance exists).
pub fn image_data_url(path: &std::path::Path) -> Option<String> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 4 * 1024 * 1024 {
        return None;
    }
    let mime = if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF") {
        "image/gif"
    } else {
        "image/webp"
    };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[derive(Serialize, Clone, Debug)]
pub struct ContentFile {
    pub kind: String,
    pub filename: String,
    pub enabled: bool,
    pub title: String,
    pub version: Option<String>,
    pub icon_url: Option<String>,
    pub project_id: Option<String>,
    pub world: Option<String>,
}

/// Everything in mods/, resourcepacks/, shaderpacks/ and the worlds' datapacks/, including
/// files that weren't installed through Modrinth (imported, copied by hand).
pub fn content(paths: &Paths, inst: &Instance) -> Vec<ContentFile> {
    let game = paths.game_dir(&inst.id);
    let mut out = Vec::new();
    let mut scan = |dir: std::path::PathBuf, kind: &str, world: Option<String>| {
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for e in entries.flatten() {
            let filename = e.file_name().to_string_lossy().into_owned();
            if filename.starts_with('.') {
                continue;
            }
            let enabled = !filename.ends_with(".disabled");
            let base = filename.trim_end_matches(".disabled");
            let known = inst.mods.iter().find(|m| m.filename == base && m.kind == kind);
            out.push(ContentFile {
                kind: kind.into(),
                filename: filename.clone(),
                enabled,
                title: known.map(|m| m.title.clone()).unwrap_or_else(|| base.trim_end_matches(".jar").trim_end_matches(".zip").to_string()),
                version: known.map(|m| m.version.clone()),
                icon_url: known.and_then(|m| m.icon_url.clone()),
                project_id: known.map(|m| m.project_id.clone()),
                world: world.clone(),
            });
        }
    };
    scan(game.join("mods"), "mod", None);
    scan(game.join("resourcepacks"), "resourcepack", None);
    scan(game.join("shaderpacks"), "shader", None);
    for world in worlds(paths, &inst.id) {
        scan(game.join("saves").join(&world).join("datapacks"), "datapack", Some(world));
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    out
}

pub fn worlds(paths: &Paths, id: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(paths.game_dir(id).join("saves")) else { return vec![] };
    let mut list: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join("level.dat").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    list.sort();
    list
}

fn content_path(paths: &Paths, id: &str, kind: &str, world: Option<&str>, filename: &str) -> Result<std::path::PathBuf> {
    if filename.contains(['/', '\\']) || filename.starts_with('.') || world.is_some_and(|w| w.contains(['/', '\\']) || w.starts_with('.')) {
        bail!("invalid file name");
    }
    let game = paths.game_dir(id);
    Ok(match (kind, world) {
        ("datapack", Some(w)) => game.join("saves").join(w).join("datapacks").join(filename),
        ("datapack", None) => bail!("datapack without world"),
        _ => game.join(content_dir(kind)).join(filename),
    })
}

/// Deletes a content file and forgets it if it was tracked.
pub fn remove_content(paths: &Paths, id: &str, kind: &str, world: Option<&str>, filename: &str) -> Result<Instance> {
    let mut inst = get(paths, id)?;
    let path = content_path(paths, id, kind, world, filename)?;
    if path.is_dir() {
        std::fs::remove_dir_all(&path)?;
    } else {
        std::fs::remove_file(&path)?;
    }
    let base = filename.trim_end_matches(".disabled");
    inst.mods.retain(|m| !(m.filename == base && m.kind == kind));
    save(paths, &inst)?;
    Ok(inst)
}

/// Turns a mod/pack on or off by adding or removing ".disabled".
pub fn toggle_content(paths: &Paths, id: &str, kind: &str, world: Option<&str>, filename: &str) -> Result<()> {
    get(paths, id)?;
    let path = content_path(paths, id, kind, world, filename)?;
    let target = match filename.strip_suffix(".disabled") {
        Some(base) => path.with_file_name(base),
        None => path.with_file_name(format!("{filename}.disabled")),
    };
    std::fs::rename(path, target)?;
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct World {
    pub folder: String,
    pub size: u64,
    pub last_played: Option<DateTime<Utc>>,
    /// The world's icon.png as a data URL.
    pub icon: Option<String>,
}

fn dir_size(path: &std::path::Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(m) if m.is_dir() => std::fs::read_dir(path).map(|d| d.flatten().map(|e| dir_size(&e.path())).sum()).unwrap_or(0),
        _ => 0,
    }
}

/// Worlds with size, last played (level.dat time) and icon, most recent first.
pub fn worlds_detail(paths: &Paths, id: &str) -> Result<Vec<World>> {
    get(paths, id)?;
    let saves = paths.game_dir(id).join("saves");
    let mut out: Vec<World> = worlds(paths, id)
        .into_iter()
        .map(|folder| {
            let dir = saves.join(&folder);
            let last_played = std::fs::metadata(dir.join("level.dat")).and_then(|m| m.modified()).ok().map(DateTime::<Utc>::from);
            World { size: dir_size(&dir), icon: image_data_url(&dir.join("icon.png")), last_played, folder }
        })
        .collect();
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    Ok(out)
}

pub fn delete_world(paths: &Paths, id: &str, folder: &str) -> Result<()> {
    if folder.is_empty() || folder.contains(['/', '\\']) || folder.starts_with('.') {
        bail!("invalid world");
    }
    if !worlds(paths, id).iter().any(|w| w == folder) {
        bail!("That world doesn't exist");
    }
    std::fs::remove_dir_all(paths.game_dir(id).join("saves").join(folder))?;
    Ok(())
}
