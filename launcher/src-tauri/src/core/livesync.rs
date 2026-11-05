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
//!
//! What a pulled pack may bring in is decided here, not by the editor (see `sharetrust`):
//! jars Modrinth doesn't know wait for the member's answer, and a new Minecraft or loader
//! version pauses syncing until it's accepted.

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
use super::sharelocal;
use super::sharetrust::{self, Prepared};
use super::tapack::{self, rel_string, walk};

/// Folders whose changes are published (the pack carries all of `share::SHARED`; options.txt
/// isn't watched: the game rewrites it every time it closes).
const WATCHED: [&str; 9] = ["mods", "resourcepacks", "shaderpacks", "config", "defaultconfigs", "kubejs", "scripts", "horizon-mods", "horizon-native-mods"];
const CONTENT: [&str; 5] = ["mods", "resourcepacks", "shaderpacks", "horizon-mods", "horizon-native-mods"];

#[derive(Serialize, Clone)]
pub struct SyncEvent {
    pub instance: String,
    pub name: String,
    /// "pulled" | "published" | "waiting" | "error", "review" (jars wait for an answer) or
    /// "paused" (a new Minecraft or loader version waits for an answer)
    pub state: &'static str,
    /// Who made the change that was pulled.
    pub by: Option<String>,
    /// For "pulled": what changed ("added Sodium, removed X"); for "error": what went wrong.
    pub message: Option<String>,
}

/// Local fingerprint seen on the previous tick: publish only once it holds still.
static SEEN: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// One sync at a time per instance (the loop and "Sync now" can overlap).
static BUSY: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Fingerprint of the watched folders (paths, sizes, change times) and the content files,
/// without what this copy keeps to itself (`sharelocal`).
pub fn local_state(paths: &Paths, id: &str) -> (String, Vec<String>) {
    let game = paths.game_dir(id);
    let mut files = Vec::new();
    for folder in WATCHED {
        walk(&game, PathBuf::from(folder), &mut files);
    }
    if let Ok(inst) = instances::get(paths, id) {
        files.retain(|f| !sharelocal::own(&inst, &rel_string(f)));
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
pub fn link(inst: &Instance) -> Option<(String, bool)> {
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

enum Pulled {
    /// `asking` = jars from the pack waiting for an answer; `summary` = what changed here.
    Applied { asking: usize, summary: String },
    /// Nothing installed: the pack wants another Minecraft or loader version.
    Paused,
}

/// "added a.jar, b.jar, removed c.jar" from the content files before and after a pull.
fn summary(before: &[String], after: &[String]) -> String {
    let name = |f: &String| f.rsplit('/').next().unwrap_or(f).trim_end_matches(".disabled").to_string();
    let list = |v: Vec<String>| {
        let more = v.len().saturating_sub(3);
        let mut s = v.into_iter().take(3).collect::<Vec<_>>().join(", ");
        if more > 0 {
            s.push_str(&format!(" and {more} more"));
        }
        s
    };
    let (b, a): (HashSet<&String>, HashSet<&String>) = (before.iter().collect(), after.iter().collect());
    let mut added: Vec<String> = after.iter().filter(|f| !b.contains(f)).map(name).collect();
    let mut removed: Vec<String> = before.iter().filter(|f| !a.contains(f)).map(name).collect();
    added.sort();
    removed.sort();
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!("added {}", list(added)));
    }
    if !removed.is_empty() {
        parts.push(format!("removed {}", list(removed)));
    }
    if parts.is_empty() { "changed settings".into() } else { parts.join(", ") }
}

/// Installs the share's current pack over the instance (what this copy accepts of it); with
/// `keep`, this copy's own added and removed content files (compared with the last sync) are
/// put back on top.
async fn pull(client: &reqwest::Client, paths: &Paths, id: &str, code: &str, keep: Option<&[String]>, revision: i64, by: Option<&str>) -> Result<Pulled> {
    let game = paths.game_dir(id);
    let before = instances::get(paths, id)?;
    // this copy's changes since the last sync
    let (_, now_files) = local_state(paths, id);
    let base: HashSet<&str> = keep.unwrap_or(&[]).iter().map(String::as_str).collect();
    let now: HashSet<&str> = now_files.iter().map(String::as_str).collect();
    let added: Vec<String> = if keep.is_some() { now_files.iter().filter(|f| !base.contains(f.as_str())).cloned().collect() } else { Vec::new() };
    let removed: Vec<String> = if keep.is_some() { base.iter().filter(|f| !now.contains(*f)).map(|f| f.to_string()).collect() } else { Vec::new() };

    let dl = api(client, Method::GET, &format!("/shares/{code}/download"), None, None).await?;
    let url = dl["url"].as_str().ok_or_else(|| anyhow!("No download link"))?;
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let tmp = std::env::temp_dir().join(format!("tessera-sync-{code}.tapack"));
    std::fs::write(&tmp, &bytes)?;
    let checked = sharetrust::prepare(client, paths, id, &tmp, revision, by).await;
    let _ = std::fs::remove_file(&tmp);
    let (safe, asking) = match checked? {
        Prepared::Ready { pack, asking } => (pack, asking),
        Prepared::Version(_) => return Ok(Pulled::Paused),
    };

    let stash = std::env::temp_dir().join(format!("tessera-merge-{id}"));
    let _ = std::fs::remove_dir_all(&stash);
    for rel in &added {
        let to = stash.join(rel);
        std::fs::create_dir_all(to.parent().unwrap_or(&stash))?;
        std::fs::copy(game.join(rel), &to)?;
    }
    // the pack's own version of what this copy doesn't take, for its uploads
    let _ = std::fs::copy(&safe, sharelocal::base_path(paths, id));
    let kept = match sharelocal::stash(paths, &before) {
        Ok(k) => k,
        Err(e) => {
            let _ = std::fs::remove_file(&safe);
            return Err(e);
        }
    };
    let result = tapack::apply_update(client, paths, id, &safe, &|_, _, _| {}).await;
    let _ = std::fs::remove_file(&safe);
    let mut inst = result?;
    sharelocal::restore(paths, &before, kept, &mut inst)?;

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
    let (_, after) = local_state(paths, id);
    Ok(Pulled::Applied { asking, summary: summary(&now_files, &after) })
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
        // the servers linked to the pack: mine go up, the others' into the game's list
        super::shareservers::publish(client, &inst, &code).await;
        if let Some(servers) = super::shareservers::from_sync(r).filter(|_| !inst.unsynced.iter().any(|k| k == "servers")) {
            match super::shareservers::apply(paths, &inst.id, &servers, running.contains(&inst.id)) {
                Ok(added) if !added.is_empty() => events.push(SyncEvent { instance: inst.id.clone(), name: inst.name.clone(), state: "servers", by: None, message: Some(added.join(", ")) }),
                Ok(_) => {}
                Err(e) => events.push(SyncEvent { instance: inst.id.clone(), name: inst.name.clone(), state: "error", by: None, message: Some(format!("adding the pack's servers: {e:#}")) }),
            }
        }
        if !BUSY.lock().unwrap().insert(inst.id.clone()) {
            continue;
        }
        let ev = |state: &'static str, by: Option<String>, message: Option<String>| SyncEvent { instance: inst.id.clone(), name: inst.name.clone(), state, by, message };
        let result: Result<Vec<SyncEvent>> = async {
            let rev = r["revision"].as_i64().unwrap_or(0);
            // first time: whatever is here now counts as in sync, unless this copy joined
            // an older version than the share has now
            let sync = match inst.share_sync.clone() {
                Some(s) => s,
                None => {
                    let behind = inst.joined.as_ref().is_some_and(|j| j.updated_at.as_deref() != r["updated_at"].as_str());
                    if !behind {
                        mark_synced(paths, &inst.id, rev)?;
                        return Ok(Vec::new());
                    }
                    let (signature, files) = local_state(paths, &inst.id);
                    ShareSync { revision: 0, signature, files }
                }
            };
            let remote_newer = rev > sync.revision;
            let by = r["updated_by"].as_str().map(String::from);
            // a new Minecraft or loader version waits for an answer: nothing syncs meanwhile,
            // unless a newer revision came (it may change what's asked, or undo it)
            if let Some(vc) = sharetrust::load(paths, &inst.id).version_change {
                if rev <= vc.revision {
                    return Ok(Vec::new());
                }
            }
            if running.contains(&inst.id) {
                return Ok(remote_newer.then(|| ev("waiting", by.clone(), None)).into_iter().collect());
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
                let mut extra = Vec::new();
                if remote_newer {
                    match pull(client, paths, &inst.id, &code, Some(&sync.files), rev, by.as_deref()).await? {
                        Pulled::Paused => return Ok(vec![ev("paused", by.clone(), None)]),
                        Pulled::Applied { asking, .. } if asking > 0 => extra.push(ev("review", by.clone(), None)),
                        Pulled::Applied { .. } => {}
                    }
                    base = rev;
                }
                let fresh = instances::get(paths, &inst.id)?;
                match publish(client, paths, &fresh, &code, base).await? {
                    Some(new_rev) => {
                        mark_synced(paths, &inst.id, new_rev)?;
                        extra.insert(0, ev("published", None, None));
                        Ok(extra)
                    }
                    // someone published in between: merge next round
                    None => Ok(extra),
                }
            } else if remote_newer && !(changed && can_write) {
                // a member's content put in the folder by hand stays theirs
                if !can_write {
                    sharelocal::adopt(paths, &inst.id, &sync.files)?;
                }
                match pull(client, paths, &inst.id, &code, None, rev, by.as_deref()).await? {
                    Pulled::Paused => Ok(vec![ev("paused", by.clone(), None)]),
                    Pulled::Applied { asking, summary } => {
                        mark_synced(paths, &inst.id, rev)?;
                        let mut out = vec![ev("pulled", by.clone(), Some(summary))];
                        if asking > 0 {
                            out.push(ev("review", by.clone(), None));
                        }
                        Ok(out)
                    }
                }
            } else {
                Ok(Vec::new())
            }
        }
        .await;
        BUSY.lock().unwrap().remove(&inst.id);
        match result {
            Ok(e) => events.extend(e),
            Err(e) => events.push(ev("error", None, Some(format!("{e:#}")))),
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn says_what_changed() {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(summary(&v(&["mods/a.jar", "mods/b.jar"]), &v(&["mods/b.jar", "mods/c.jar.disabled"])), "added c.jar, removed a.jar");
        assert_eq!(summary(&v(&["mods/a.jar"]), &v(&["mods/a.jar"])), "changed settings");
        assert_eq!(summary(&v(&[]), &v(&["mods/1.jar", "mods/2.jar", "mods/3.jar", "mods/4.jar"])), "added 1.jar, 2.jar, 3.jar and 1 more");
    }
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
        account::sign_up(&client, &format!("e2esync{stamp}@example.com"), &invite, &format!("sync_{}", stamp % 100000), "correct horse battery", &account::Consent { terms_version: "2026-10-31".into(), age: "16+".into(), parental_consent: false }).await.unwrap();

        let base = std::env::temp_dir().join(format!("tessera-live-{stamp}"));
        let (p1, p2) = (Paths::new(base.join("one")), Paths::new(base.join("two")));
        let none = HashSet::new();
        let write = |p: &Paths, id: &str, rel: &str, text: &str| {
            let f = p.game_dir(id).join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        };
        let has = |p: &Paths, id: &str, rel: &str| p.game_dir(id).join(rel).exists();
        // the dummy jars aren't on Modrinth: they wait in held/ until accepted (the popup's
        // Accept all, share_decide_jars)
        let held = |p: &Paths, id: &str| sharetrust::load(p, id).held.iter().map(|h| h.path.clone()).collect::<Vec<_>>();
        let accept_all = |p: &Paths, id: &str| {
            let shas: Vec<String> = sharetrust::load(p, id).held.iter().map(|h| h.sha512.clone()).collect();
            sharetrust::decide_jars(p, id, &shas, &[]).unwrap();
        };
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
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled", "review"]);
        assert!(has(&p2, &b.id, "config/x.txt") && !has(&p2, &b.id, "mods/a.jar"), "configs come in, the unknown jar waits");
        assert_eq!(held(&p2, &b.id), vec!["mods/a.jar"]);
        accept_all(&p2, &b.id);
        assert!(has(&p2, &b.id, "mods/a.jar"));
        assert!(rounds(&client, &p2, &none).await.is_empty(), "accepting isn't a change to publish");

        // one adds a mod: it goes up, and two gets it once accepted
        write(&p1, &a.id, "mods/b.jar", "b");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled", "review"]);
        assert!(!has(&p2, &b.id, "mods/b.jar"));
        accept_all(&p2, &b.id);
        assert!(has(&p2, &b.id, "mods/b.jar"));

        // both change at once: one removes a.jar, two adds c.jar; both changes survive
        // (two's own c.jar and the accepted b.jar don't ask again)
        std::fs::remove_file(p1.game_dir(&a.id).join("mods/a.jar")).unwrap();
        write(&p2, &b.id, "mods/c.jar", "c");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        assert_eq!(rounds(&client, &p2, &none).await, vec!["published"]);
        assert!(!has(&p2, &b.id, "mods/a.jar") && has(&p2, &b.id, "mods/b.jar") && has(&p2, &b.id, "mods/c.jar"));
        // one gets two's c.jar, which it doesn't know
        assert_eq!(rounds(&client, &p1, &none).await, vec!["pulled", "review"]);
        assert_eq!(held(&p1, &a.id), vec!["mods/c.jar"]);
        accept_all(&p1, &a.id);
        assert!(!has(&p1, &a.id, "mods/a.jar") && has(&p1, &a.id, "mods/b.jar") && has(&p1, &a.id, "mods/c.jar"));

        // while the game runs, updates wait
        write(&p1, &a.id, "mods/d.jar", "d");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        let playing: HashSet<String> = [b.id.clone()].into();
        assert!(rounds(&client, &p2, &playing).await.iter().all(|s| *s == "waiting"));
        assert!(!has(&p2, &b.id, "mods/d.jar"));
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled", "review"]);
        assert!(!has(&p2, &b.id, "mods/d.jar"));
        accept_all(&p2, &b.id);
        assert!(has(&p2, &b.id, "mods/d.jar"));

        // declined: stays out here, but two publishing doesn't take it from everyone
        write(&p1, &a.id, "mods/e.jar", "e");
        assert_eq!(rounds(&client, &p1, &none).await, vec!["published"]);
        assert_eq!(rounds(&client, &p2, &none).await, vec!["pulled", "review"]);
        let e = sharetrust::load(&p2, &b.id).held[0].sha512.clone();
        sharetrust::decide_jars(&p2, &b.id, &[], &[e]).unwrap();
        write(&p2, &b.id, "config/y.txt", "2");
        assert_eq!(rounds(&client, &p2, &none).await, vec!["published"]);
        assert!(!has(&p2, &b.id, "mods/e.jar"));
        assert_eq!(rounds(&client, &p1, &none).await, vec!["pulled"]);
        assert!(has(&p1, &a.id, "mods/e.jar") && has(&p1, &a.id, "config/y.txt"));

        share::stop(&client, &p1, &a.id).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
