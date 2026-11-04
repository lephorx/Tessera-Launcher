//! Servers linked to a shared pack: each copy publishes the servers linked to it (Create
//! server / Tessera servers made from it), and the others on the pack get them in the game's
//! server list (`servers.dat`, which the pack itself never carries). The API keeps one list
//! per person (PUT /shares/:code/servers) and hands everyone the others' in /shares/sync.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::account::api;
use super::instances::{self, Instance};
use super::paths::Paths;
use super::serverlist;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SharedServer {
    pub name: String,
    pub address: String,
}

/// What each instance published last (since the launcher started), so the list is sent only
/// when it changes.
static PUBLISHED: LazyLock<Mutex<HashMap<String, Vec<SharedServer>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The servers this copy shares: the ones linked to it with a public address (a LAN address
/// is no use to the others).
pub fn mine(inst: &Instance) -> Vec<SharedServer> {
    let mut out: Vec<SharedServer> = Vec::new();
    for s in &inst.servers {
        if let Some(address) = s.public.as_deref().filter(|a| !a.is_empty()) {
            if !out.iter().any(|o| o.address.eq_ignore_ascii_case(address)) {
                out.push(SharedServer { name: s.name.chars().take(64).collect(), address: address.to_string() });
            }
        }
    }
    out.truncate(10);
    out
}

/// When sending an instance's list last failed: tried again after a few minutes, not every
/// round (offline, or an API from before the route).
static FAILED: LazyLock<Mutex<HashMap<String, std::time::Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Sends this copy's servers when they changed.
pub async fn publish(client: &reqwest::Client, inst: &Instance, code: &str) {
    let list = mine(inst);
    if PUBLISHED.lock().unwrap().get(&inst.id) == Some(&list) {
        return;
    }
    if FAILED.lock().unwrap().get(&inst.id).is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(300)) {
        return;
    }
    let body = serde_json::json!({ "servers": list });
    match api(client, Method::PUT, &format!("/shares/{code}/servers"), Some(body), None).await {
        Ok(_) => {
            FAILED.lock().unwrap().remove(&inst.id);
            PUBLISHED.lock().unwrap().insert(inst.id.clone(), list);
        }
        Err(_) => {
            FAILED.lock().unwrap().insert(inst.id.clone(), std::time::Instant::now());
        }
    }
}

/// The others' servers from a /shares/sync entry; None when the API doesn't send them.
pub fn from_sync(entry: &Value) -> Option<Vec<SharedServer>> {
    let list = entry.get("servers")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|s| {
                let name = s["name"].as_str()?.trim();
                let address = s["address"].as_str()?.trim();
                (!name.is_empty() && !address.is_empty()).then(|| SharedServer { name: name.to_string(), address: address.to_string() })
            })
            .collect(),
    )
}

/// Puts the others' servers in the game's list: new ones added, renamed or moved ones
/// updated, ones they took off removed. Entries this copy links itself are left alone.
/// While the game runs only new servers are added (Minecraft reads the list each time
/// Multiplayer opens, so they show up right away) and nothing is recorded: the full round
/// runs once it's closed, putting back anything the game wrote over. Returns the names of
/// the servers that weren't in the list before.
pub fn apply(paths: &Paths, id: &str, remote: &[SharedServer], game_running: bool) -> Result<Vec<String>> {
    let mut inst = instances::get(paths, id)?;
    if inst.shared_servers == remote && !game_running {
        return Ok(Vec::new());
    }
    let game = paths.game_dir(id);
    let own: Vec<String> = mine(&inst).into_iter().map(|s| s.address.to_ascii_lowercase()).collect();
    let is_own = |a: &str| own.contains(&a.to_ascii_lowercase());
    let listed: Vec<String> = serverlist::list(&game).into_iter().map(|e| e.ip.to_ascii_lowercase()).collect();
    let mut added = Vec::new();
    if game_running {
        for s in remote {
            if !is_own(&s.address) && !inst.shared_servers.contains(s) && !listed.contains(&s.address.to_ascii_lowercase()) {
                serverlist::upsert(&game, None, &s.name, &s.address)?;
                added.push(s.name.clone());
            }
        }
        return Ok(added);
    }
    for old in &inst.shared_servers {
        if !remote.iter().any(|r| r.address.eq_ignore_ascii_case(&old.address)) && !is_own(&old.address) {
            serverlist::remove(&game, &old.address)?;
        }
    }
    for s in remote {
        if is_own(&s.address) {
            continue;
        }
        let missing = !listed.contains(&s.address.to_ascii_lowercase());
        if missing || !inst.shared_servers.contains(s) {
            serverlist::upsert(&game, None, &s.name, &s.address)?;
        }
        if missing {
            added.push(s.name.clone());
        }
    }
    inst.shared_servers = remote.to_vec();
    instances::save(paths, &inst)?;
    Ok(added)
}

/// Leaving a pack: its servers come off the game's list (not the ones this copy links).
pub fn clear(paths: &Paths, inst: &mut Instance) -> Result<()> {
    let game = paths.game_dir(&inst.id);
    let own: Vec<String> = mine(inst).into_iter().map(|s| s.address.to_ascii_lowercase()).collect();
    for s in &inst.shared_servers {
        if !own.contains(&s.address.to_ascii_lowercase()) {
            serverlist::remove(&game, &s.address)?;
        }
    }
    inst.shared_servers.clear();
    PUBLISHED.lock().unwrap().remove(&inst.id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_others_servers_come_and_go_in_the_game_list() {
        let root = std::env::temp_dir().join(format!("tessera-shareservers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.clone());
        let inst = instances::create(&paths, "Pack", "1.21.1", instances::Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        let game = paths.game_dir(&inst.id);
        let s = |n: &str, a: &str| SharedServer { name: n.into(), address: a.into() };
        let names = || serverlist::list(&game).into_iter().map(|e| format!("{}={}", e.name, e.ip)).collect::<Vec<_>>();

        // a server of my own, and one I added by hand
        let mut me = instances::get(&paths, &inst.id).unwrap();
        me.servers.push(serde_json::from_value(serde_json::json!({ "machine_id": "m", "server_id": 1, "name": "Mine", "address": "mine.example.com", "public": "mine.example.com" })).unwrap());
        instances::save(&paths, &me).unwrap();
        serverlist::upsert(&game, None, "Mine", "mine.example.com").unwrap();
        serverlist::upsert(&game, None, "Hand", "hand.example.com").unwrap();
        assert_eq!(mine(&instances::get(&paths, &inst.id).unwrap()), vec![s("Mine", "mine.example.com")]);

        // a new one while the game runs: added right away, nothing recorded yet
        let both = [s("Owner SMP", "smp.example.com"), s("Also mine", "mine.example.com")];
        assert_eq!(apply(&paths, &inst.id, &both, true).unwrap(), vec!["Owner SMP".to_string()]);
        assert!(names().contains(&"Owner SMP=smp.example.com".to_string()));
        assert!(names().contains(&"Mine=mine.example.com".to_string()), "my own entry isn't renamed");
        assert!(apply(&paths, &inst.id, &both, true).unwrap().is_empty());
        // the game wrote over it; closing it puts it back
        serverlist::remove(&game, "smp.example.com").unwrap();
        assert_eq!(apply(&paths, &inst.id, &both, false).unwrap(), vec!["Owner SMP".to_string()]);
        assert!(names().contains(&"Owner SMP=smp.example.com".to_string()));
        assert!(apply(&paths, &inst.id, &both, false).unwrap().is_empty());

        // renamed, and the other one taken off: mine stays
        apply(&paths, &inst.id, &[s("Owner's SMP", "smp.example.com")], false).unwrap();
        assert!(names().contains(&"Owner's SMP=smp.example.com".to_string()));
        assert!(names().contains(&"Mine=mine.example.com".to_string()));

        let mut i = instances::get(&paths, &inst.id).unwrap();
        clear(&paths, &mut i).unwrap();
        let left = names();
        assert!(!left.iter().any(|n| n.contains("smp.example.com")));
        assert!(left.contains(&"Mine=mine.example.com".to_string()) && left.contains(&"Hand=hand.example.com".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reads_the_sync_entry() {
        assert_eq!(from_sync(&serde_json::json!({ "code": "x" })), None);
        let v = serde_json::json!({ "servers": [{ "name": "A", "address": "a.example.com", "by": "bob" }, { "name": "", "address": "x" }] });
        assert_eq!(from_sync(&v), Some(vec![SharedServer { name: "A".into(), address: "a.example.com".into() }]));
    }
}
