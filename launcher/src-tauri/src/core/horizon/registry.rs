//! Horizon's mod registry (github.com/horizon-rust/horizon-mods, docs/api/mod-index.md there):
//! one `index.json` of reviewed content mods (`.hmod`), native mods (`.hnmod`), resource packs
//! and Horizon shader packs. A file is installed only when its size and SHA-256 are the index's,
//! it isn't revoked, and, for a project with a key, its minisign signature verifies.
//!
//! In the game folder: content mods in `horizon-mods/` (turned on in `horizon-mods.txt` as
//! `mod.<id>:true`, the game's own switch), native mods in `horizon-native-mods/` (built into the
//! game, native.rs), packs in `resourcepacks/` and `shaderpacks/`.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::instances::{self, InstalledMod, Instance};
use super::super::paths::Paths;
use super::Log;

const INDEX: &str = "https://horizon-rust.github.io/horizon-mods/index.json";
pub const CONTENT: &str = "horizon-mods";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Index {
    pub schema: u32,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub revoked: Vec<Revoked>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    /// "mod" | "native-mod" | "modpack" | "resourcepack" | "shader" (others: shown nowhere)
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub downloads: Option<u64>,
    pub versions: Vec<Version>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Version {
    pub version: String,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub changelog: String,
    #[serde(default)]
    pub game: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
    pub file: File,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct File {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Revoked {
    pub id: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub reason: String,
}

/// The kinds this launcher installs, and the folder each goes into.
fn folder(kind: &str) -> Option<&'static str> {
    match kind {
        "mod" => Some(CONTENT),
        "native-mod" => Some(super::native::FOLDER),
        "resourcepack" => Some("resourcepacks"),
        "shader" => Some("shaderpacks"),
        _ => None,
    }
}

/// Our content kind for a registry kind (the instance's content lists).
pub fn content_kind(kind: &str) -> &'static str {
    match kind {
        "mod" => "horizon-mod",
        "native-mod" => "native-mod",
        "resourcepack" => "resourcepack",
        "shader" => "shader",
        _ => "mod",
    }
}

/// The index, asked at most every 10 minutes (the last one read when it can't be).
pub async fn index(client: &reqwest::Client, paths: &Paths) -> Result<Index> {
    let cache = super::root(paths).join("mod-index.json");
    let fresh = std::fs::metadata(&cache).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() < 600);
    let bytes = match fresh.then(|| std::fs::read(&cache).ok()).flatten() {
        Some(b) => b,
        None => match client.get(INDEX).send().await {
            Ok(r) if r.status().is_success() => {
                let b = r.bytes().await?.to_vec();
                let _ = std::fs::create_dir_all(super::root(paths));
                let _ = std::fs::write(&cache, &b);
                b
            }
            other => std::fs::read(&cache).map_err(|_| anyhow!("Couldn't reach Horizon's mod registry ({})", other.map(|r| r.status().to_string()).unwrap_or_else(|e| e.to_string())))?,
        },
    };
    let index: Index = serde_json::from_slice(&bytes).context("reading Horizon's mod registry")?;
    if index.schema != 1 {
        bail!("Horizon's mod registry changed its format: update Tessera Launcher Nightly");
    }
    Ok(index)
}

/// The registry's projects of the kinds Tessera installs, filtered by `query` and `kind`, most
/// downloaded first.
pub async fn search(client: &reqwest::Client, paths: &Paths, query: &str, kind: Option<&str>) -> Result<Vec<Project>> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<Project> = index(client, paths)
        .await?
        .projects
        .into_iter()
        .filter(|p| folder(&p.kind).is_some() && kind.is_none_or(|k| k == p.kind) && !p.versions.is_empty())
        .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.id.contains(&q) || p.summary.to_lowercase().contains(&q))
        .collect();
    out.sort_by(|a, b| b.downloads.unwrap_or(0).cmp(&a.downloads.unwrap_or(0)).then(a.name.cmp(&b.name)));
    Ok(out)
}

fn revoked<'a>(index: &'a Index, id: &str, sha: &str) -> Option<&'a Revoked> {
    index.revoked.iter().find(|r| r.id == id && r.sha256.as_deref().is_none_or(|s| s.eq_ignore_ascii_case(sha)))
}

/// The registry's file names: one plain name with its kind's extension.
fn plain(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c)) && !name.starts_with('.')
}

/// Installs a project's newest version (or `version`) into a Horizon instance, replacing an
/// older one of it. The instance as it is then.
pub async fn install(client: &reqwest::Client, paths: &Paths, instance: &str, project: &str, version: Option<&str>) -> Result<Instance> {
    let mut inst = instances::get(paths, instance)?;
    if inst.loader != instances::Loader::Horizon {
        bail!("Horizon's mods are for Horizon instances");
    }
    let index = index(client, paths).await?;
    let p = index.projects.iter().find(|p| p.id == project).ok_or_else(|| anyhow!("{project} isn't in Horizon's registry"))?;
    let dir = folder(&p.kind).ok_or_else(|| anyhow!("Tessera can't install a Horizon {} yet", p.kind))?;
    let v = match version {
        Some(want) => p.versions.iter().find(|v| v.version == want),
        None => p.versions.first(),
    }
    .ok_or_else(|| anyhow!("{} has no such version", p.name))?;
    let f = &v.file;
    if !plain(&f.name) {
        bail!("{} has an odd file name", p.name);
    }
    let allowed = f.url.starts_with("https://github.com/") && f.url.contains("/releases/download/");
    if !allowed || f.size > 64 << 20 {
        bail!("{} isn't hosted where Horizon's registry hosts files", p.name);
    }
    if let Some(r) = revoked(&index, &p.id, &f.sha256) {
        bail!("{} {} was taken down: {}", p.name, v.version, r.reason);
    }
    let res = client.get(&f.url).send().await.with_context(|| format!("GET {}", f.url))?;
    if !res.status().is_success() {
        bail!("Downloading {} failed ({})", p.name, res.status());
    }
    let bytes = res.bytes().await?;
    if bytes.len() as u64 != f.size || hex::encode(Sha256::digest(&bytes)) != f.sha256.to_lowercase() {
        bail!("{} isn't the file the registry lists: not installed", p.name);
    }
    match (&p.key, &f.signature) {
        (Some(key), Some(sig)) => {
            let pk = minisign_verify::PublicKey::from_base64(key.trim()).map_err(|e| anyhow!("{}'s key: {e}", p.name))?;
            let sig = minisign_verify::Signature::decode(sig).map_err(|e| anyhow!("{}'s signature: {e}", p.name))?;
            pk.verify(&bytes, &sig, true).or_else(|_| pk.verify(&bytes, &sig, false)).map_err(|_| anyhow!("{}'s signature doesn't verify: not installed", p.name))?;
        }
        (Some(_), None) => bail!("{} must be signed, and this version isn't: not installed", p.name),
        _ => {}
    }
    // what the file says it is matches the listing
    let game = paths.game_dir(&inst.id);
    let target_dir = game.join(dir);
    std::fs::create_dir_all(&target_dir)?;
    let tmp = target_dir.join(format!(".{}.part", f.name));
    std::fs::write(&tmp, &bytes)?;
    if p.kind == "native-mod" {
        let (id, _) = super::native::identify(&tmp).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        if id != p.id {
            let _ = std::fs::remove_file(&tmp);
            bail!("{} names another mod inside: not installed", p.name);
        }
    }
    let kind = content_kind(&p.kind).to_string();
    // the version it replaces
    for old in inst.mods.iter().filter(|m| m.kind == kind && m.project_id == p.id) {
        for name in [old.filename.clone(), format!("{}.disabled", old.filename)] {
            let _ = std::fs::remove_file(target_dir.join(name));
        }
    }
    inst.mods.retain(|m| !(m.kind == kind && m.project_id == p.id));
    std::fs::rename(&tmp, target_dir.join(&f.name))?;
    if p.kind == "mod" {
        set_switch(&game, &p.id, true)?;
    }
    inst.mods.push(InstalledMod { kind, world: None, project_id: p.id.clone(), version_id: v.version.clone(), title: p.name.clone(), version: v.version.clone(), filename: f.name.clone(), icon_url: p.icon.clone() });
    instances::save(paths, &inst)?;
    Ok(inst)
}

/// Turns a content mod on or off in the game's own list, `horizon-mods.txt` (`mod.<id>:true`).
pub fn set_switch(game_dir: &Path, id: &str, on: bool) -> Result<()> {
    let file = game_dir.join("horizon-mods.txt");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let key = format!("mod.{id}:");
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with(&key)).map(str::to_string).collect();
    lines.push(format!("{key}{on}"));
    std::fs::write(file, lines.join("\n") + "\n")?;
    Ok(())
}

/// Turns off installed files the registry took down since, saying why in the log.
pub async fn check_revoked(client: &reqwest::Client, paths: &Paths, game_dir: &Path, log: Log<'_>) {
    let Ok(index) = index(client, paths).await else { return };
    if index.revoked.is_empty() {
        return;
    }
    for dir in [CONTENT, super::native::FOLDER, "resourcepacks", "shaderpacks"] {
        for e in std::fs::read_dir(game_dir.join(dir)).into_iter().flatten().flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".disabled") || !path.is_file() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let sha = hex::encode(Sha256::digest(&bytes));
            let Some(r) = index.revoked.iter().find(|r| r.sha256.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&sha)) || (r.sha256.is_none() && name.starts_with(&format!("{}-", r.id)))) else { continue };
            if std::fs::rename(&path, path.with_file_name(format!("{name}.disabled"))).is_ok() {
                log(&format!("[Tessera] Turned {name} off: Horizon's registry took it down ({})", r.reason));
            }
        }
    }
}

/// Newer versions of an instance's registry files: (project id, newest version).
pub async fn updates(client: &reqwest::Client, paths: &Paths, instance: &str) -> Result<Vec<(String, String)>> {
    let inst = instances::get(paths, instance)?;
    let index = index(client, paths).await?;
    Ok(inst
        .mods
        .iter()
        .filter_map(|m| {
            let p = index.projects.iter().find(|p| p.id == m.project_id && content_kind(&p.kind) == m.kind)?;
            let newest = p.versions.first()?;
            (newest.version != m.version).then(|| (p.id.clone(), newest.version.clone()))
        })
        .collect())
}
