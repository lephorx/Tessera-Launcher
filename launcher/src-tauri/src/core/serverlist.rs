//! The game's multiplayer server list (`servers.dat` in the game folder, uncompressed NBT:
//! `{servers: [{name, ip, icon?, acceptTextures?, hidden?}]}`). Servers made from an
//! instance are added here so they show up in Multiplayer and in the Worlds tab.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use fastnbt::Value;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub ip: String,
}

fn read(game_dir: &Path) -> Result<HashMap<String, Value>> {
    let file = game_dir.join("servers.dat");
    if !file.exists() {
        return Ok(HashMap::new());
    }
    let bytes = std::fs::read(&file)?;
    match fastnbt::from_bytes::<Value>(&bytes).context("servers.dat is damaged")? {
        Value::Compound(root) => Ok(root),
        _ => anyhow::bail!("servers.dat is damaged"),
    }
}

fn servers(root: &HashMap<String, Value>) -> Vec<HashMap<String, Value>> {
    match root.get("servers") {
        Some(Value::List(list)) => list
            .iter()
            .filter_map(|v| match v {
                Value::Compound(c) => Some(c.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn text(c: &HashMap<String, Value>, key: &str) -> String {
    match c.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

pub fn list(game_dir: &Path) -> Vec<Entry> {
    let Ok(root) = read(game_dir) else { return Vec::new() };
    servers(&root)
        .iter()
        .filter(|c| !matches!(c.get("hidden"), Some(Value::Byte(1))))
        .map(|c| Entry { name: text(c, "name"), ip: text(c, "ip") })
        .filter(|e| !e.ip.is_empty())
        .collect()
}

/// Adds the server, or updates the entry that had `previous_ip` (the address we wrote last
/// time), keeping whatever else the player has in the list.
pub fn upsert(game_dir: &Path, previous_ip: Option<&str>, name: &str, ip: &str) -> Result<()> {
    let mut root = read(game_dir)?;
    let mut list = servers(&root);
    let existing = list.iter().position(|c| {
        let at = text(c, "ip");
        at == ip || previous_ip.is_some_and(|p| p == at)
    });
    match existing {
        Some(i) => {
            list[i].insert("name".into(), Value::String(name.into()));
            list[i].insert("ip".into(), Value::String(ip.into()));
        }
        None => {
            let mut c = HashMap::new();
            c.insert("name".into(), Value::String(name.into()));
            c.insert("ip".into(), Value::String(ip.into()));
            list.insert(0, c);
        }
    }
    write(game_dir, &mut root, list)
}

pub fn remove(game_dir: &Path, ip: &str) -> Result<()> {
    let mut root = read(game_dir)?;
    let mut list = servers(&root);
    let before = list.len();
    list.retain(|c| text(c, "ip") != ip);
    if list.len() == before {
        return Ok(());
    }
    write(game_dir, &mut root, list)
}

fn write(game_dir: &Path, root: &mut HashMap<String, Value>, list: Vec<HashMap<String, Value>>) -> Result<()> {
    root.insert("servers".into(), Value::List(list.into_iter().map(Value::Compound).collect()));
    std::fs::create_dir_all(game_dir)?;
    let bytes = fastnbt::to_bytes(&Value::Compound(root.clone()))?;
    let tmp = game_dir.join("servers.dat.tessera-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, game_dir.join("servers.dat"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_updates_and_keeps_other_servers() {
        let dir = std::env::temp_dir().join(format!("tessera-serverlist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        upsert(&dir, None, "Hypixel", "mc.hypixel.net").unwrap();
        upsert(&dir, None, "Friends SMP", "abc.joinmc.link").unwrap();
        assert_eq!(list(&dir)[0], Entry { name: "Friends SMP".into(), ip: "abc.joinmc.link".into() });
        // the custom domain arrived: same entry, new address
        upsert(&dir, Some("abc.joinmc.link"), "Friends SMP", "smp.example.com").unwrap();
        let now = list(&dir);
        assert_eq!(now.len(), 2);
        assert_eq!(now[0].ip, "smp.example.com");
        assert_eq!(now[1].ip, "mc.hypixel.net");
        remove(&dir, "smp.example.com").unwrap();
        assert_eq!(list(&dir), vec![Entry { name: "Hypixel".into(), ip: "mc.hypixel.net".into() }]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod real_file {
    use super::*;

    /// TESSERA_SERVERS_DAT=<a servers.dat written by Minecraft> cargo test real_servers_dat -- --ignored
    /// Leaves the edited copy in TESSERA_SERVERS_DAT_OUT for checking with another NBT reader.
    #[test]
    #[ignore]
    fn real_servers_dat() {
        let src = std::env::var("TESSERA_SERVERS_DAT").expect("TESSERA_SERVERS_DAT");
        let out = std::path::PathBuf::from(std::env::var("TESSERA_SERVERS_DAT_OUT").expect("TESSERA_SERVERS_DAT_OUT"));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::copy(&src, out.join("servers.dat")).unwrap();
        let before = list(&out);
        println!("before: {before:?}");
        upsert(&out, None, "Tessera Test", "smp.example.com").unwrap();
        let after = list(&out);
        println!("after: {after:?}");
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(&after[1..], &before[..]);
    }
}
