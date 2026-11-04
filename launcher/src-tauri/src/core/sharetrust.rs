//! What this computer lets in from a shared pack. Live sync used to install whatever an
//! editor uploaded; now the member's launcher decides (it's their computer):
//!
//! - mods Modrinth knows (checked by file hash, not by what the pack claims), configs,
//!   resource packs and shaders come in without asking;
//! - any other jar is held back until the member accepts it. Held jars wait in the
//!   instance's `held/` folder, so accepting later needs no download, and publishing from
//!   here passes them on: declining leaves a jar out of this copy, not out of everyone's;
//! - a new Minecraft or loader version pauses syncing until it's accepted.
//!
//! Answers are kept by SHA-512 in `share-trust.json` next to instance.json: a renamed jar is
//! still recognised, a changed one asks again.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

use super::download::{download_all, Download};
use super::instances::{self, Instance, Loader};
use super::modrinth;
use super::mrpack::safe_relative;
use super::paths::{ensure_parent, Paths};
use super::share::shared_path;
use super::tapack::{self, rel_string, walk, ContentRef, Source, MANIFEST};

/// Unknown jars bigger than this aren't downloaded at all (no mod is this big).
const MAX_JAR: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Trust {
    /// SHA-512 of jars this copy said yes to (or already had).
    #[serde(default)]
    pub accepted: BTreeSet<String>,
    /// SHA-512 of jars this copy said no to: they stay out on later syncs too.
    #[serde(default)]
    pub declined: BTreeSet<String>,
    /// Jars in the share's pack that aren't installed here: waiting for an answer, or declined.
    #[serde(default)]
    pub held: Vec<HeldJar>,
    /// A new Minecraft or loader version from the share: syncing waits until it's accepted.
    #[serde(default)]
    pub version_change: Option<VersionChange>,
    /// The versions the member said yes to, until the instance is on them.
    #[serde(default)]
    pub accepted_versions: Option<Versions>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HeldJar {
    /// Where the pack puts it, relative to the game folder.
    pub path: String,
    pub sha512: String,
    pub size: u64,
    /// Who made the pack revision it came with.
    pub by: Option<String>,
    pub declined: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Versions {
    pub minecraft: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
}

impl Versions {
    pub fn of(inst: &Instance) -> Self {
        Versions { minecraft: inst.game_version.clone(), loader: inst.loader, loader_version: inst.loader_version.clone() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VersionChange {
    pub revision: i64,
    pub from: Versions,
    pub to: Versions,
    pub by: Option<String>,
    pub declined: bool,
}

fn file(paths: &Paths, id: &str) -> PathBuf {
    paths.instance(id).join("share-trust.json")
}

fn held_dir(paths: &Paths, id: &str) -> PathBuf {
    paths.instance(id).join("held")
}

pub fn load(paths: &Paths, id: &str) -> Trust {
    std::fs::read(file(paths, id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(paths: &Paths, id: &str, trust: &Trust) -> Result<()> {
    let to = file(paths, id);
    let tmp = to.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(trust)?)?;
    std::fs::rename(tmp, to)?;
    Ok(())
}

/// Code the game (or a loader) runs: jars anywhere, and anything in mods/ a loader might
/// load. ".disabled" doesn't count as safe: turning it on is one click.
pub fn is_code(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    let name = lower.strip_suffix(".disabled").unwrap_or(&lower);
    // Horizon's native mods are Rust compiled into the game: code too
    name.ends_with(".jar") || name.ends_with(".litemod") || name.ends_with(".hnmod") || (name.starts_with("mods/") && name.ends_with(".zip"))
}

fn is_native_mod(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    lower.strip_suffix(".disabled").unwrap_or(&lower).ends_with(".hnmod")
}

/// SHA-256s of the native mods Horizon's registry lists and hasn't taken down: those come in
/// like Modrinth jars, without asking. Empty when the pack isn't Horizon's or the registry
/// can't be reached (then they're asked about).
async fn horizon_registry(client: &reqwest::Client, paths: &Paths, m: &tapack::Manifest) -> HashSet<String> {
    if m.loader.kind != super::loaders::Loader::Horizon {
        return HashSet::new();
    }
    let Ok(index) = super::horizon::registry::index(client, paths).await else { return HashSet::new() };
    let revoked: HashSet<String> = index.revoked.iter().filter_map(|r| r.sha256.as_ref().map(|s| s.to_lowercase())).collect();
    index
        .projects
        .iter()
        .filter(|p| p.kind == "native-mod")
        .flat_map(|p| p.versions.iter().map(|v| v.file.sha256.to_lowercase()))
        .filter(|h| !revoked.contains(h))
        .collect()
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Install,
    Ask,
    Declined,
}

/// What happens to a jar Modrinth doesn't know. `local` = SHA-512s of jars already in this
/// copy's game folder: installing the same bytes again adds nothing new.
pub fn verdict(trust: &Trust, sha512: &str, local: &HashSet<String>) -> Verdict {
    if trust.accepted.contains(sha512) {
        Verdict::Install
    } else if trust.declined.contains(sha512) {
        Verdict::Declined
    } else if local.contains(sha512) {
        Verdict::Install
    } else {
        Verdict::Ask
    }
}

/// The Modrinth file a content entry really is: Modrinth must know its hash, as the project
/// the entry claims. Anything else (a made-up link, another project's title) is unknown.
fn modrinth_file<'a>(c: &ContentRef, known: &'a HashMap<String, modrinth::Version>) -> Option<&'a modrinth::VersionFile> {
    let v = known.get(&c.sha1)?;
    let claimed = match &c.source {
        Some(Source::Modrinth { project_id, .. }) => Some(project_id.as_str()),
        None => None,
    };
    if claimed.is_some_and(|p| p != v.project_id) {
        return None;
    }
    v.file_with_sha1(&c.sha1)
}

fn sha256_of(mut r: impl Read) -> Result<String> {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    std::io::copy(&mut r, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

fn sha512_of(mut r: impl Read) -> Result<String> {
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Copies `r` into held/ under its SHA-512; returns (sha512, size).
fn stash(dir: &Path, mut r: impl Read) -> Result<(String, u64)> {
    std::fs::create_dir_all(dir)?;
    let part = dir.join(format!(".part-{}", std::process::id()));
    let mut out = std::fs::File::create(&part)?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_JAR {
            drop(out);
            let _ = std::fs::remove_file(&part);
            bail!("A jar in the pack is too big");
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    drop(out);
    let sha = hex::encode(h.finalize());
    std::fs::rename(&part, dir.join(&sha))?;
    Ok((sha, size))
}

pub enum Prepared {
    /// Safe to install: the pack without held-back jars and without anything outside the
    /// shared folders. `asking` = jars waiting for an answer.
    Ready { pack: PathBuf, asking: usize },
    /// A new Minecraft or loader version: nothing installed, syncing waits for an answer.
    Version(VersionChange),
}

/// Checks a downloaded share pack before it's installed over instance `id`, and writes the
/// part of it this copy accepts to a new pack file.
pub async fn prepare(client: &reqwest::Client, paths: &Paths, id: &str, pack: &Path, revision: i64, by: Option<&str>) -> Result<Prepared> {
    let inst = instances::get(paths, id)?;
    let mut trust = load(paths, id);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack)?).context("not a pack file")?;
    let m = tapack::read_manifest(&mut zip)?;

    let (from, to) = (Versions::of(&inst), Versions { minecraft: m.minecraft.clone(), loader: m.loader.kind, loader_version: m.loader.version.clone() });
    if from == to {
        trust.accepted_versions = None;
    } else if trust.accepted_versions.as_ref() != Some(&to) {
        // asked about this one before: keep the answer
        let declined = trust.version_change.as_ref().is_some_and(|v| v.to == to && v.declined);
        let vc = VersionChange { revision, from, to, by: by.map(String::from), declined };
        trust.version_change = Some(vc.clone());
        save(paths, id, &trust)?;
        return Ok(Prepared::Version(vc));
    }
    trust.version_change = None;

    let out = std::env::temp_dir().join(format!("tessera-safe-{id}.tapack"));
    let registry = horizon_registry(client, paths, &m).await;
    let asking = check_jars(client, &mut zip, m, &held_dir(paths, id), Some(&paths.game_dir(id)), &mut trust, by, &out, &registry).await?;
    save(paths, id, &trust)?;
    tidy(paths, id, &trust);
    Ok(Prepared::Ready { pack: out, asking })
}

/// Installs a shared pack as a new instance (joining it): the same check as updates, so
/// jars Tessera doesn't know are held back and asked about right after.
pub async fn install_new(
    client: &reqwest::Client,
    paths: &Paths,
    pack: &Path,
    by: Option<&str>,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Instance> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack)?).context("not a pack file")?;
    let m = tapack::read_manifest(&mut zip)?;
    // the instance doesn't exist yet: held jars wait here until it does
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let staging = std::env::temp_dir().join(format!("tessera-held-{}-{stamp}", std::process::id()));
    let out = std::env::temp_dir().join(format!("tessera-safe-join-{stamp}.tapack"));
    let mut trust = Trust::default();
    progress("Checking the mods", 0, 0);
    let registry = horizon_registry(client, paths, &m).await;
    let checked = check_jars(client, &mut zip, m, &staging, None, &mut trust, by, &out, &registry).await;
    let installed = match checked {
        Ok(_) => tapack::import(client, paths, &out, progress).await,
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_file(&out);
    let result = installed.and_then(|inst| {
        let dir = held_dir(paths, &inst.id);
        for h in &trust.held {
            std::fs::create_dir_all(&dir)?;
            // a copy: the temp folder may be on another drive
            std::fs::copy(staging.join(&h.sha512), dir.join(&h.sha512))?;
        }
        save(paths, &inst.id, &trust)?;
        Ok(inst)
    });
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Sorts the jars of an opened pack: Modrinth files stay links (from Modrinth), other jars
/// go to `dir` (held/) and are installed only if `trust` (or the same bytes already in
/// `game`) says so. Writes what may be installed to `out`, sets `trust.held`, and returns
/// how many jars wait for an answer.
#[allow(clippy::too_many_arguments)]
async fn check_jars(
    client: &reqwest::Client,
    zip: &mut zip::ZipArchive<std::fs::File>,
    mut m: tapack::Manifest,
    dir: &Path,
    game: Option<&Path>,
    trust: &mut Trust,
    by: Option<&str>,
    out: &Path,
    registry: &HashSet<String>,
) -> Result<usize> {
    // (path in the game folder, sha512, size) of every jar Modrinth doesn't vouch for
    let mut unknown: Vec<(String, String, u64)> = Vec::new();

    // content links: Modrinth has to know the file, by its hash
    let code: Vec<usize> = (0..m.content.len()).filter(|&i| is_code(&m.content[i].path)).collect();
    let sha1s: Vec<String> = code.iter().map(|&i| m.content[i].sha1.clone()).collect();
    let known = if sha1s.is_empty() { HashMap::new() } else { modrinth::versions_by_sha1(client, sha1s).await.context("Couldn't check the pack's mods with Modrinth")? };
    let mut drop_content = HashSet::new();
    for &i in &code {
        let c = &mut m.content[i];
        if let Some(f) = modrinth_file(c, &known) {
            // download it from Modrinth, wherever the pack said
            c.url = f.url.clone();
            c.size = f.size;
            continue;
        }
        drop_content.insert(i);
        if c.size > MAX_JAR || safe_relative(&c.path).is_err() {
            continue;
        }
        let part = dir.join(format!(".dl-{}", c.sha1.chars().filter(char::is_ascii_hexdigit).collect::<String>()));
        ensure_parent(&part)?;
        download_all(client, vec![Download { url: c.url.clone(), path: part.clone(), sha1: Some(c.sha1.clone()), size: Some(c.size), executable: false }], |_, _| {}).await?;
        let staged = stash(dir, std::fs::File::open(&part)?);
        let _ = std::fs::remove_file(&part);
        let (sha, size) = staged?;
        let path = if c.enabled { c.path.clone() } else { format!("{}.disabled", c.path) };
        unknown.push((path, sha, size));
    }
    let mut i = 0;
    m.content.retain(|c| {
        i += 1;
        !drop_content.contains(&(i - 1)) && shared_path(&c.path)
    });

    // files carried in the pack: only the shared folders, and jars wait in held/
    let mut keep = Vec::new();
    let mut icon = None;
    let mut options = None;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        if name == "icon.png" {
            icon = Some(i);
            continue;
        }
        let Some(rel) = name.strip_prefix("overrides/") else { continue };
        if rel.is_empty() || entry.is_dir() || safe_relative(rel).is_err() || !shared_path(rel) {
            continue;
        }
        if rel == "options.txt" {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            options = Some(String::from_utf8_lossy(&bytes).into_owned());
        } else if is_code(rel) {
            let (sha, size) = stash(dir, &mut entry)?;
            unknown.push((rel.to_string(), sha, size));
        } else {
            keep.push(i);
        }
    }

    // jars this copy has already: hashed only when the size could match
    let sizes: HashSet<u64> = unknown.iter().map(|u| u.2).collect();
    let mut local_files = Vec::new();
    let game = game.unwrap_or(Path::new(""));
    if !game.as_os_str().is_empty() {
        for top in ["mods", "config", "kubejs", "scripts", "defaultconfigs", "resourcepacks", "shaderpacks", "horizon-native-mods"] {
            walk(game, PathBuf::from(top), &mut local_files);
        }
    }
    let local: HashSet<String> = local_files
        .iter()
        .filter(|f| is_code(&rel_string(f)))
        .filter(|f| std::fs::metadata(game.join(f)).is_ok_and(|md| sizes.contains(&md.len())))
        .filter_map(|f| std::fs::File::open(game.join(f)).ok().and_then(|r| sha512_of(r).ok()))
        .collect();

    let mut install = Vec::new();
    let mut held = Vec::new();
    for (path, sha, size) in unknown {
        // a native mod the Horizon registry lists, byte for byte
        let listed = is_native_mod(&path)
            && !registry.is_empty()
            && std::fs::File::open(dir.join(&sha)).ok().and_then(|f| sha256_of(f).ok()).is_some_and(|h| registry.contains(&h));
        if listed {
            install.push((path, sha));
            continue;
        }
        match verdict(trust, &sha, &local) {
            Verdict::Install => {
                trust.accepted.insert(sha.clone());
                install.push((path, sha));
            }
            v => held.push(HeldJar { path, sha512: sha, size, by: by.map(String::from), declined: v == Verdict::Declined }),
        }
    }
    // a jar that was asked about before keeps who brought it
    for h in &mut held {
        if let Some(old) = trust.held.iter().find(|o| o.sha512 == h.sha512 && o.path == h.path) {
            h.by = old.by.clone().or(h.by.take());
        }
    }

    let written = (|| -> Result<()> {
        let mut w = zip::ZipWriter::new(std::fs::File::create(out)?);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file(MANIFEST, deflated)?;
        w.write_all(&serde_json::to_vec_pretty(&m)?)?;
        for i in icon.into_iter().chain(keep) {
            w.raw_copy_file(zip.by_index_raw(i)?)?;
        }
        if let Some(text) = &options {
            // this copy's own last server stays, nobody else's comes in
            let own = std::fs::read_to_string(game.join("options.txt")).ok().filter(|_| !game.as_os_str().is_empty());
            w.start_file("overrides/options.txt", deflated)?;
            w.write_all(options_for(text, own.as_deref()).as_bytes())?;
        }
        for (path, sha) in &install {
            w.start_file(format!("overrides/{path}"), stored)?;
            std::io::copy(&mut std::fs::File::open(dir.join(sha))?, &mut w)?;
        }
        w.finish()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(out);
        return Err(e);
    }

    let asking = held.iter().filter(|h| !h.declined).count();
    trust.held = held;
    Ok(asking)
}

/// options.txt as shared: without `lastServer:` (the address of the last server joined),
/// plus `own`'s line when there is one (this computer's options.txt).
pub fn options_for(text: &str, own: Option<&str>) -> String {
    let last = |l: &&str| l.starts_with("lastServer:");
    let mut lines: Vec<&str> = text.lines().filter(|l| !last(l)).collect();
    if let Some(mine) = own.and_then(|o| o.lines().find(last)) {
        lines.push(mine);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Takes `lastServer:` out of the options.txt in a pack about to be uploaded.
pub fn strip_last_server(pack: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack)?)?;
    let Ok(mut entry) = zip.by_name("overrides/options.txt") else { return Ok(()) };
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    drop(entry);
    let text = String::from_utf8_lossy(&bytes);
    if !text.lines().any(|l| l.starts_with("lastServer:")) {
        return Ok(());
    }
    let tmp = pack.with_extension("tapack.opts");
    let written = (|| -> Result<()> {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        for i in 0..zip.len() {
            let e = zip.by_index_raw(i)?;
            if e.name() != "overrides/options.txt" {
                w.raw_copy_file(e)?;
            }
        }
        w.start_file("overrides/options.txt", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))?;
        w.write_all(options_for(&text, None).as_bytes())?;
        w.finish()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, pack)?;
    Ok(())
}

/// Removes held files no held jar points at any more.
fn tidy(paths: &Paths, id: &str, trust: &Trust) {
    let wanted: HashSet<&str> = trust.held.iter().map(|h| h.sha512.as_str()).collect();
    let Ok(dir) = std::fs::read_dir(held_dir(paths, id)) else { return };
    for e in dir.flatten() {
        if !wanted.contains(e.file_name().to_string_lossy().as_ref()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn valid_sha512(s: &str) -> bool {
    s.len() == 128 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// The member's answer about held jars. Accepted ones are installed now.
pub fn decide_jars(paths: &Paths, id: &str, accept: &[String], decline: &[String]) -> Result<Trust> {
    let inst = instances::get(paths, id)?;
    let game = paths.game_dir(id);
    let dir = held_dir(paths, id);
    let mut trust = load(paths, id);
    // nothing else changed here since the last sync: what we install now is the share's
    // content, so it shouldn't look like a change of this copy to publish
    let in_sync = inst.share_sync.as_ref().is_some_and(|s| s.signature == super::livesync::local_state(paths, id).0);

    let mut installed = false;
    for sha in accept.iter().filter(|s| valid_sha512(s)) {
        trust.declined.remove(sha);
        trust.accepted.insert(sha.clone());
        for h in trust.held.iter().filter(|h| &h.sha512 == sha) {
            let to = game.join(safe_relative(&h.path)?);
            ensure_parent(&to)?;
            std::fs::copy(dir.join(sha), &to).context("installing the mod")?;
            installed = true;
        }
        trust.held.retain(|h| &h.sha512 != sha);
    }
    for sha in decline.iter().filter(|s| valid_sha512(s) && !accept.contains(s)) {
        trust.accepted.remove(sha);
        trust.declined.insert(sha.clone());
        for h in trust.held.iter_mut().filter(|h| &h.sha512 == sha) {
            h.declined = true;
        }
    }
    save(paths, id, &trust)?;
    tidy(paths, id, &trust);
    if installed && in_sync {
        if let Some(s) = &inst.share_sync {
            super::livesync::mark_synced(paths, id, s.revision)?;
        }
    }
    Ok(trust)
}

/// The member's answer about a new Minecraft or loader version. Declined: syncing stays
/// paused. Accepted: the next sync installs it.
pub fn decide_version(paths: &Paths, id: &str, accept: bool) -> Result<()> {
    instances::get(paths, id)?;
    let mut trust = load(paths, id);
    let Some(vc) = trust.version_change.as_mut() else { return Ok(()) };
    if accept {
        trust.accepted_versions = Some(vc.to.clone());
        trust.version_change = None;
    } else {
        vc.declined = true;
    }
    save(paths, id, &trust)
}

/// Adds held jars to a pack about to be published from this copy, so declining one here
/// doesn't take it out of everyone's pack.
pub fn carry_held(paths: &Paths, id: &str, pack: &Path) -> Result<()> {
    let trust = load(paths, id);
    let game = paths.game_dir(id);
    let dir = held_dir(paths, id);
    let held: Vec<&HeldJar> = trust.held.iter().filter(|h| !game.join(&h.path).exists() && dir.join(&h.sha512).is_file()).collect();
    if held.is_empty() {
        return Ok(());
    }
    let f = std::fs::OpenOptions::new().read(true).write(true).open(pack)?;
    let mut w = zip::ZipWriter::new_append(f)?;
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut seen = HashSet::new();
    for h in held {
        if !seen.insert(h.path.as_str()) {
            continue;
        }
        w.start_file(format!("overrides/{}", h.path), stored)?;
        std::io::copy(&mut std::fs::File::open(dir.join(&h.sha512))?, &mut w)?;
    }
    w.finish()?;
    Ok(())
}

/// What one instance is waiting on, for the popup and the Share tab.
#[derive(Serialize, Clone, Debug)]
pub struct Review {
    pub instance: String,
    pub name: String,
    pub held: Vec<HeldJar>,
    pub version_change: Option<VersionChange>,
}

pub fn review(paths: &Paths, inst: &Instance) -> Review {
    let t = load(paths, &inst.id);
    Review { instance: inst.id.clone(), name: inst.name.clone(), held: t.held, version_change: t.version_change }
}

/// Every shared instance with something waiting for an answer (not the declined ones).
pub fn pending(paths: &Paths) -> Vec<Review> {
    instances::list(paths)
        .unwrap_or_default()
        .iter()
        .filter(|i| i.share_code.is_some() || i.joined.is_some())
        .map(|i| review(paths, i))
        .map(|mut r| {
            r.held.retain(|h| !h.declined);
            r.version_change = r.version_change.filter(|v| !v.declined);
            r
        })
        .filter(|r| !r.held.is_empty() || r.version_change.is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tapack::{ExportOptions, LoaderRef, Manifest, PackFormat};

    #[test]
    fn what_counts_as_code() {
        assert!(is_code("mods/sodium.jar"));
        assert!(is_code("mods/evil.jar.disabled"));
        assert!(is_code("mods/1.12.2/old.zip"));
        assert!(is_code("config/hidden/payload.JAR"));
        assert!(is_code("kubejs/x.litemod"));
        assert!(!is_code("resourcepacks/faithful.zip"));
        assert!(!is_code("config/sodium.json"));
        assert!(!is_code("options.txt"));
    }

    #[test]
    fn verdicts() {
        let mut t = Trust::default();
        let local: HashSet<String> = ["have".to_string()].into();
        assert_eq!(verdict(&t, "new", &local), Verdict::Ask);
        assert_eq!(verdict(&t, "have", &local), Verdict::Install, "the same bytes as a jar already here");
        t.accepted.insert("yes".into());
        t.declined.insert("no".into());
        t.declined.insert("have".into());
        assert_eq!(verdict(&t, "yes", &local), Verdict::Install);
        assert_eq!(verdict(&t, "no", &local), Verdict::Declined);
        assert_eq!(verdict(&t, "have", &local), Verdict::Declined, "a no stays a no");
    }

    fn content(sha1: &str, project: Option<&str>) -> ContentRef {
        ContentRef {
            kind: "mod".into(),
            path: "mods/x.jar".into(),
            enabled: true,
            url: "https://example.com/x.jar".into(),
            sha1: sha1.into(),
            size: 3,
            title: "X".into(),
            version: None,
            icon_url: None,
            source: project.map(|p| Source::Modrinth { project_id: p.into(), version_id: "v".into() }),
            env: None,
        }
    }

    #[test]
    fn modrinth_claims_are_checked() {
        let known: HashMap<String, modrinth::Version> = serde_json::from_value(serde_json::json!({
            "aaa": { "id": "v1", "project_id": "sodium", "version_number": "1", "files": [
                { "url": "https://cdn.modrinth.com/s.jar", "filename": "s.jar", "primary": true, "hashes": { "sha1": "aaa" }, "size": 3 }
            ] }
        }))
        .unwrap();
        assert_eq!(modrinth_file(&content("aaa", Some("sodium")), &known).map(|f| f.url.as_str()), Some("https://cdn.modrinth.com/s.jar"));
        assert!(modrinth_file(&content("aaa", None), &known).is_some(), "a hand-added Modrinth file");
        assert!(modrinth_file(&content("aaa", Some("iris")), &known).is_none(), "claims another project");
        assert!(modrinth_file(&content("bbb", Some("sodium")), &known).is_none(), "hash Modrinth doesn't know");
    }

    /// A pack with an unknown jar, a config and a file outside the shared folders.
    fn write_pack(path: &Path, minecraft: &str, files: &[(&str, &[u8])]) {
        let m = Manifest {
            format: tapack::FORMAT,
            name: "P".into(),
            version: "1".into(),
            author: String::new(),
            summary: String::new(),
            minecraft: minecraft.into(),
            loader: LoaderRef { kind: Loader::Fabric, version: Some("0.16.9".into()) },
            icon: None,
            memory_mb: None,
            content: vec![],
        };
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        w.start_file(MANIFEST, o).unwrap();
        w.write_all(&serde_json::to_vec(&m).unwrap()).unwrap();
        for (name, bytes) in files {
            w.start_file(format!("overrides/{name}"), o).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    fn names(pack: &Path) -> Vec<String> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(pack).unwrap()).unwrap();
        let mut n: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        n.sort();
        n
    }

    #[tokio::test]
    async fn unknown_jars_wait_and_answers_stick() {
        let root = std::env::temp_dir().join(format!("tessera-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let mut inst = instances::create(&paths, "Trust", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        inst.share_code = Some("TESTCODE".into());
        instances::save(&paths, &inst).unwrap();
        let game = paths.game_dir(&inst.id);
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/mine.jar"), b"already here").unwrap();

        let pack = root.join("in.tapack");
        let files: &[(&str, &[u8])] = &[
            ("mods/evil.jar", b"evil"),
            ("mods/renamed-mine.jar", b"already here"),
            ("config/a.toml", b"a = 1"),
            ("accounts.json", b"{\"token\":1}"),
            ("saves/x/level.dat", b"world"),
        ];
        write_pack(&pack, "1.21.1", files);
        let Prepared::Ready { pack: safe, asking } = prepare(&client, &paths, &inst.id, &pack, 2, Some("ed")).await.unwrap() else { panic!("no version change") };
        assert_eq!(asking, 1);
        assert_eq!(names(&safe), vec!["overrides/config/a.toml", "overrides/mods/renamed-mine.jar", MANIFEST], "only shared folders; the unknown jar waits");
        let t = load(&paths, &inst.id);
        assert_eq!(t.held.len(), 1);
        assert_eq!((t.held[0].path.as_str(), t.held[0].size, t.held[0].by.as_deref(), t.held[0].declined), ("mods/evil.jar", 4, Some("ed"), false));
        let evil = t.held[0].sha512.clone();
        assert_eq!(pending(&paths).len(), 1);

        // declined: stays out next time, without asking
        decide_jars(&paths, &inst.id, &[], std::slice::from_ref(&evil)).unwrap();
        assert!(pending(&paths).is_empty());
        let Prepared::Ready { pack: safe, asking } = prepare(&client, &paths, &inst.id, &pack, 3, Some("ed")).await.unwrap() else { panic!() };
        assert_eq!(asking, 0);
        assert!(!names(&safe).contains(&"overrides/mods/evil.jar".to_string()));
        assert!(load(&paths, &inst.id).held[0].declined, "listed as declined");

        // accepted later: installed from held/, and comes in silently after that
        decide_jars(&paths, &inst.id, std::slice::from_ref(&evil), &[]).unwrap();
        assert_eq!(std::fs::read(game.join("mods/evil.jar")).unwrap(), b"evil");
        assert!(load(&paths, &inst.id).held.is_empty());
        let Prepared::Ready { pack: safe, .. } = prepare(&client, &paths, &inst.id, &pack, 4, Some("ed")).await.unwrap() else { panic!() };
        assert!(names(&safe).contains(&"overrides/mods/evil.jar".to_string()));

        // the same name with other bytes asks again
        write_pack(&pack, "1.21.1", &[("mods/evil.jar", b"evil v2")]);
        let Prepared::Ready { asking, .. } = prepare(&client, &paths, &inst.id, &pack, 5, Some("ed")).await.unwrap() else { panic!() };
        assert_eq!(asking, 1);

        // a new Minecraft version asks first; declined keeps it paused
        write_pack(&pack, "1.21.4", &[]);
        let Prepared::Version(vc) = prepare(&client, &paths, &inst.id, &pack, 6, Some("owner")).await.unwrap() else { panic!("asks") };
        assert_eq!((vc.from.minecraft.as_str(), vc.to.minecraft.as_str(), vc.declined), ("1.21.1", "1.21.4", false));
        decide_version(&paths, &inst.id, false).unwrap();
        let Prepared::Version(vc) = prepare(&client, &paths, &inst.id, &pack, 7, Some("owner")).await.unwrap() else { panic!() };
        assert!(vc.declined, "the answer sticks for the same versions");
        decide_version(&paths, &inst.id, true).unwrap();
        assert!(matches!(prepare(&client, &paths, &inst.id, &pack, 7, Some("owner")).await.unwrap(), Prepared::Ready { .. }));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Joining: the instance is installed without the unknown jar, which waits to be accepted.
    #[tokio::test]
    async fn joining_holds_unknown_jars() {
        let root = std::env::temp_dir().join(format!("tessera-join-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let pack = root.join("in.tapack");
        std::fs::create_dir_all(&root).unwrap();
        write_pack(&pack, "1.21.1", &[("mods/theirs.jar", b"theirs"), ("config/a.toml", b"a = 1"), ("logs/latest.log", b"x")]);
        let mut inst = install_new(&client, &paths, &pack, Some("owner"), &|_, _, _| {}).await.unwrap();
        let game = paths.game_dir(&inst.id);
        assert!(game.join("config/a.toml").exists() && !game.join("logs").exists());
        assert!(!game.join("mods/theirs.jar").exists(), "nothing unknown before an answer");
        inst.joined = Some(instances::JoinedShare { code: "TESTCODE".into(), role: "viewer".into(), owner: None, updated_at: None });
        instances::save(&paths, &inst).unwrap();
        let waiting = pending(&paths);
        assert_eq!((waiting.len(), waiting[0].held[0].by.as_deref()), (1, Some("owner")));
        decide_jars(&paths, &inst.id, &[waiting[0].held[0].sha512.clone()], &[]).unwrap();
        assert_eq!(std::fs::read(game.join("mods/theirs.jar")).unwrap(), b"theirs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn last_server_stays_home() {
        let shared = "fov:0.5\nlastServer:play.example.net\nlang:en_us\n";
        assert_eq!(options_for(shared, None), "fov:0.5\nlang:en_us\n");
        assert_eq!(options_for(shared, Some("fov:1\nlastServer:10.0.0.2:25565\n")), "fov:0.5\nlang:en_us\nlastServer:10.0.0.2:25565\n");
        assert_eq!(options_for("fov:0.5\n", Some("fov:1\n")), "fov:0.5\n");
    }

    /// Both ways: the upload has no lastServer, and a pulled options.txt keeps the member's.
    #[tokio::test]
    async fn options_without_last_server() {
        let root = std::env::temp_dir().join(format!("tessera-opts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        std::fs::create_dir_all(&root).unwrap();
        let pack = root.join("out.tapack");
        write_pack(&pack, "1.21.1", &[("options.txt", b"fov:0.5\nlastServer:owner.example.net\n"), ("config/a.toml", b"a")]);
        strip_last_server(&pack).unwrap();
        let read = |pack: &Path, name: &str| {
            let mut z = zip::ZipArchive::new(std::fs::File::open(pack).unwrap()).unwrap();
            let mut s = String::new();
            z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        assert_eq!(read(&pack, "overrides/options.txt"), "fov:0.5\n");
        assert_eq!(read(&pack, "overrides/config/a.toml"), "a");

        write_pack(&pack, "1.21.1", &[("options.txt", b"fov:0.5\nlastServer:owner.example.net\n")]);
        let inst = instances::create(&paths, "Opts", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        std::fs::write(paths.game_dir(&inst.id).join("options.txt"), "fov:1\nlastServer:mine.example.net\n").unwrap();
        let Prepared::Ready { pack: safe, .. } = prepare(&client, &paths, &inst.id, &pack, 2, None).await.unwrap() else { panic!() };
        assert_eq!(read(&safe, "overrides/options.txt"), "fov:0.5\nlastServer:mine.example.net\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Held jars go along when this copy publishes, so declining isn't removing.
    #[tokio::test]
    async fn publishing_keeps_held_jars() {
        let root = std::env::temp_dir().join(format!("tessera-carry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let inst = instances::create(&paths, "Carry", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        let pack = root.join("in.tapack");
        write_pack(&pack, "1.21.1", &[("mods/theirs.jar", b"theirs")]);
        prepare(&client, &paths, &inst.id, &pack, 2, None).await.unwrap();

        std::fs::create_dir_all(paths.game_dir(&inst.id).join("config")).unwrap();
        std::fs::write(paths.game_dir(&inst.id).join("config/c.toml"), b"c").unwrap();
        let out = root.join("out.tapack");
        let opts = ExportOptions { name: "Carry".into(), version: "1".into(), author: String::new(), summary: String::new(), include: vec!["config".into()], format: PackFormat::Tapack };
        tapack::export(&client, &paths, &inst.id, &out, opts, &|_, _, _| {}).await.unwrap();
        carry_held(&paths, &inst.id, &out).unwrap();
        let n = names(&out);
        assert!(n.contains(&"overrides/mods/theirs.jar".to_string()) && n.contains(&"overrides/config/c.toml".to_string()), "{n:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
