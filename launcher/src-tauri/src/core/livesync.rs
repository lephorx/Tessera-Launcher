//! Live sync of shared instances. Every few seconds the launcher asks the API for the
//! revision of each share it's on (one request for all of them) and:
//!
//! - **pulls** a newer revision right away (after the game closes, if it's running);
//! - **publishes** local changes (owners and editors) once the folders stop changing, made
//!   in the launcher or straight in the folder;
//! - **merges** when both happened: the newer pack is pulled, then this copy's added and
//!   removed mods, resource packs and shaders are applied on top and published. For config
//!   files the newer pack wins.
//!
//! Uploads carry the revision they're based on; the API refuses outdated ones (409), which
//! is what sends us down the merge path.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::{anyhow, Result};
use reqwest::Method;
use serde::Serialize;
use serde_json::Value;
use sha1::{Digest, Sha1};

use super::account::api;
use super::instances::{self, Instance, ShareSync};
use super::paths::Paths;
use super::share::{pack_bytes, query};
use super::tapack::{self, rel_string, walk};

/// Folders that make up a shared instance (what the pack carries and what we watch).
const WATCHED: [&str; 4] = ["mods", "resourcepacks", "shaderpacks", "config"];
const CONTENT: [&str; 3] = ["mods", "resourcepacks", "shaderpacks"];

#[derive(Serialize, Clone)]
pub struct SyncEvent {
    pub instance: String,
    pub name: String,
    /// "pulled" | "published" | "waiting" | "error"
    pub state: &'static str,
    /// Who made the change that was pulled.
    pub by: Option<String>,
    pub message: Option<String>,
}

/// Local fingerprint seen on the previous tick: publish only once it holds still.
static SEEN: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// One sync at a time per instance (the loop and "Sync now" can overlap).
static BUSY: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Fingerprint of the watched folders (paths, sizes, change times) and the content files.
pub fn local_state(paths: &Paths, id: &str) -> (String, Vec<String>) {
    let game = paths.game_dir(id);
    let mut files = Vec::new();
    for folder in WATCHED {
        walk(&game, PathBuf::from(folder), &mut files);
    }
    let mut lines: Vec<String> = files
        .iter()
        .filter_map(|rel| {
            let meta = std::fs::metadata(game.join(rel)).ok()?;
            let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some(format!("{}|{}|{}", rel_string(rel), meta.len(), mtime))
        })
        .collect();
    lines.sort();
    let content: Vec<String> = files
        .iter()
        .map(|r| rel_string(r))
        .filter(|r| CONTENT.iter().any(|c| r.starts_with(&format!("{c}/"))))
        .collect();
    (hex::encode(Sha1::digest(lines.join("\n").as_bytes())), content)
}

/// The share an instance is on, and whether this copy may change it.
fn link(inst: &Instance) -> Option<(String, bool)> {
    if let Some(code) = &inst.share_code {
        return Some((code.clone(), true));
    }
    inst.joined.as_ref().map(|j| (j.code.clone(), j.role == "editor"))
}

/// Records that the instance now matches `revision`.
pub fn mark_synced(paths: &Paths, id: &str, revision: i64) -> Result<()> {
    let (signature, files) = local_state(paths, id);
    let mut inst = instances::get(paths, id)?;
    inst.share_sync = Some(ShareSync { revision, signature: signature.clone(), files });
    instances::save(paths, &inst)?;
    SEEN.lock().unwrap().insert(id.to_string(), signature);
    Ok(())
}

/// Uploads this copy as the next revision. Ok(None) = someone was faster (409).
async fn publish(client: &reqwest::Client, paths: &Paths, inst: &Instance, code: &str, base: i64) -> Result<Option<i64>> {
    let bytes = pack_bytes(client, paths, inst, &|_, _, _| {}).await?;
    match api(client, Method::PUT, &format!("/shares/{code}?{}&base_revision={base}", query(inst)), None, Some(bytes)).await {
        Ok(v) => Ok(Some(v["revision"].as_i64().unwrap_or(base + 1))),
        Err(e) if e.to_string().contains("changed the pack meanwhile") => Ok(None),
        Err(e) => Err(e),
    }
}

/// Installs the share's current pack over the instance; with `keep`, this copy's own added
/// and removed content files (compared with the last sync) are put back on top.
async fn pull(client: &reqwest::Client, paths: &Paths, id: &str, code: &str, keep: Option<&[String]>) -> Result<()> {
    let game = paths.game_dir(id);
    let before = instances::get(paths, id)?;
    // this copy's changes since the last sync
    let (_, now_files) = local_state(paths, id);
    let base: HashSet<&str> = keep.unwrap_or(&[]).iter().map(String::as_str).collect();
    let now: HashSet<&str> = now_files.iter().map(String::as_str).collect();
    let added: Vec<String> = if keep.is_some() { now_files.iter().filter(|f| !base.contains(f.as_str())).cloned().collect() } else { Vec::new() };
    let removed: Vec<String> = if keep.is_some() { base.iter().filter(|f| !now.contains(*f)).map(|f| f.to_string()).collect() } else { Vec::new() };
    let stash = std::env::temp_dir().join(format!("tessera-merge-{id}"));
    let _ = std::fs::remove_dir_all(&stash);
    for rel in &added {
        let to = stash.join(rel);
        std::fs::create_dir_all(to.parent().unwrap_or(&stash))?;
        std::fs::copy(game.join(rel), &to)?;
    }

    let dl = api(client, Method::GET, &format!("/shares/{code}/download"), None, None).await?;
    let url = dl["url"].as_str().ok_or_else(|| anyhow!("No download link"))?;
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let tmp = std::env::temp_dir().join(format!("tessera-sync-{code}.tapack"));
    std::fs::write(&tmp, &bytes)?;
    let result = tapack::apply_update(client, paths, id, &tmp, &|_, _, _| {}).await;
    let _ = std::fs::remove_file(&tmp);
    let mut inst = result?;

    // put this copy's changes back on top
    for rel in &added {
        let to = game.join(rel);
        std::fs::create_dir_all(to.parent().unwrap_or(&game))?;
        std::fs::copy(stash.join(rel), &to)?;
        // keep what we knew about the file (title, icon, Modrinth project)
        let name = Path::new(rel).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(m) = before.mods.iter().find(|m| m.filename == name || format!("{}.disabled", m.filename) == name) {
            if !inst.mods.iter().any(|x| x.filename == m.filename) {
                inst.mods.push(m.clone());
            }
        }
    }
    for rel in &removed {
        let _ = std::fs::remove_file(game.join(rel));
        let name = Path::new(rel).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        inst.mods.retain(|m| m.filename != name && format!("{}.disabled", m.filename) != name);
    }
    let _ = std::fs::remove_dir_all(&stash);
    // the share link and role stay as they were
    inst.share_code = before.share_code;
    inst.joined = before.joined;
    instances::save(paths, &inst)?;
    Ok(())
}

/// One round for every shared instance. `running` = instances whose game is open;
/// `only` limits it to one instance ("Sync now").
pub async fn tick(client: &reqwest::Client, paths: &Paths, running: &HashSet<String>, only: Option<&str>) -> Vec<SyncEvent> {
    let mut events = Vec::new();
    let Ok(list) = instances::list(paths) else { return events };
    let linked: Vec<(Instance, String, bool)> = list
        .into_iter()
        .filter(|i| only.is_none_or(|o| o == i.id))
        .filter_map(|i| link(&i).map(|(c, w)| (i, c, w)))
        .collect();
    if linked.is_empty() {
        return events;
    }
    let codes: Vec<&str> = linked.iter().map(|(_, c, _)| c.as_str()).collect();
    let Ok(remote) = api(client, Method::GET, &format!("/shares/sync?codes={}", codes.join(",")), None, None).await else {
        return events; // signed out or offline: try again next round
    };
    let remote: HashMap<String, Value> = remote["shares"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["code"].as_str()?.to_string(), s.clone())))
        .collect();

    for (inst, code, can_write) in linked {
        let Some(r) = remote.get(&code) else { continue }; // removed from the share, or it's gone
        if !BUSY.lock().unwrap().insert(inst.id.clone()) {
            continue;
        }
        let ev = |state: &'static str, by: Option<String>, message: Option<String>| SyncEvent { instance: inst.id.clone(), name: inst.name.clone(), state, by, message };
        let result: Result<Option<SyncEvent>> = async {
            let rev = r["revision"].as_i64().unwrap_or(0);
            // first time: whatever is here now counts as in sync, unless this copy joined
            // an older version than the share has now
            let sync = match inst.share_sync.clone() {
                Some(s) => s,
                None => {
                    let behind = inst.joined.as_ref().is_some_and(|j| j.updated_at.as_deref() != r["updated_at"].as_str());
                    if !behind {
                        mark_synced(paths, &inst.id, rev)?;
                        return Ok(None);
                    }
                    let (signature, files) = local_state(paths, &inst.id);
                    ShareSync { revision: 0, signature, files }
                }
            };
            let remote_newer = rev > sync.revision;
            if running.contains(&inst.id) {
                return Ok(remote_newer.then(|| ev("waiting", r["updated_by"].as_str().map(String::from), None)));
            }
            let (sig, _) = local_state(paths, &inst.id);
            let changed = sig != sync.signature;
            // only publish once the folders held still for a round
            let settled = {
                let mut seen = SEEN.lock().unwrap();
                let same = seen.get(&inst.id) == Some(&sig);
                seen.insert(inst.id.clone(), sig.clone());
                same
            };
            if changed && can_write && settled {
                let mut base = sync.revision;
                if remote_newer {
                    pull(client, paths, &inst.id, &code, Some(&sync.files)).await?;
                    base = rev;
                }
                let fresh = instances::get(paths, &inst.id)?;
                match publish(client, paths, &fresh, &code, base).await? {
                    Some(new_rev) => {
                        mark_synced(paths, &inst.id, new_rev)?;
                        Ok(Some(ev("published", None, None)))
                    }
                    // someone published in between: merge next round
                    None => Ok(None),
                }
            } else if remote_newer && !(changed && can_write) {
                pull(client, paths, &inst.id, &code, None).await?;
                mark_synced(paths, &inst.id, rev)?;
                Ok(Some(ev("pulled", r["updated_by"].as_str().map(String::from), None)))
            } else {
                Ok(None)
            }
        }
        .await;
        BUSY.lock().unwrap().remove(&inst.id);
        match result {
            Ok(Some(e)) => events.push(e),
            Ok(None) => {}
            Err(e) => events.push(ev("error", None, Some(format!("{e:#}")))),
        }
    }
    events
}

#[cfg(test)]
mod live_test {
    use super::*;
    use crate::core::{account, instances::JoinedShare, loaders::Loader, share};

    /// Two launchers (two data folders) on one share, against the live API.
    /// TESSERA_TEST_INVITE=<unused invite code> cargo test live_sync_round_trip -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_sync_round_trip() {
        let invite = std::env::var("TESSERA_TEST_INVITE").expect("TESSERA_TEST_INVITE");
        let client = crate::core::http_client();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        account::sign_up(&client, &format!("e2esync{stamp}@example.com"), &invite, &format!("sync_{}", stamp % 100000), "correct horse battery").await.unwrap();

        let base = std::env::temp_dir().join(format!("tessera-live-{stamp}"));
        let (p1, p2) = (Paths::new(base.join("one")), Paths::new(base.join("two")));
        let none = HashSet::new();
        let write = |p: &Paths, id: &str, rel: &str, text: &str| {
            let f = p.game_dir(id).join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        };
        let has = |p: &Paths, id: &str, rel: &str| p.game_dir(id).join(rel).exists();
        // a round, twice: changes are published once they held still for a round
        async fn rounds(client: &reqwest::Client, p: &Paths, running: &HashSet<String>) -> Vec<&'static str> {
            let mut states = Vec::new();
            for _ in 0..2 {
                for e in tick(client, p, running, None).await {
                    if let Some(m) = &e.message {
                        println!("error: {m}");
                    }
                    states.push(e.state);
                }
            }
            states
        }

        // computer one shares an instance
        let a = instances::create(&p1, "Live One", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        write(&p1, &a.id, "mods/a.jar", "a");
        write(&p1, &a.id, "config/x.txt", "1");
        let shared = share::create(&client, &p1, &a.id, &|_, _, _| {}).await.unwrap();
        let code = shared["code"].as_str().unwrap().to_string();

        // computer two has a copy that's behind
        let mut b = instances::create(&p2, "Live Two", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        b.joined = Some(JoinedShare { code: code.clone(), role: "editor".into(), owner: None, updated_at: None });
        instances::save(&p2, &b).unwrap();
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled"]);
        assert!(has(&p2, &b.id, "mods/a.jar") && has(&p2, &b.id, "config/x.txt"));

        // one adds a mod: it goes up, and two gets it
        write(&p1, &a.id, "mods/b.jar", "b");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled"]);
        assert!(has(&p2, &b.id, "mods/b.jar"));

        // both change at once: one removes a.jar, two adds c.jar; both changes survive
        std::fs::remove_file(p1.game_dir(&a.id).join("mods/a.jar")).unwrap();
        write(&p2, &b.id, "mods/c.jar", "c");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        assert_eq!(rounds(&client, &p2, &none).await, vec!["published"]);
        assert!(!has(&p2, &b.id, "mods/a.jar") && has(&p2, &b.id, "mods/b.jar") && has(&p2, &b.id, "mods/c.jar"));
        assert_eq!(rounds(&client, &p1, &none).await, vec!["pulled"]);
        assert!(!has(&p1, &a.id, "mods/a.jar") && has(&p1, &a.id, "mods/c.jar"));

        // while the game runs, updates wait
        write(&p1, &a.id, "mods/d.jar", "d");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        let playing: HashSet<String> = [b.id.clone()].into();
        assert!(rounds(&client, &p2, &playing).await.iter().all(|s| *s == "waiting"));
        assert!(!has(&p2, &b.id, "mods/d.jar"));
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled"]);
        assert!(has(&p2, &b.id, "mods/d.jar"));

        share::stop(&client, &p1, &a.id).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
