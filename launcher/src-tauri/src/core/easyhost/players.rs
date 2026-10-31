//! Players: who's been on (from the console: joins, leaves, playtime, the busiest moment),
//! the whitelist, operators and bans, and one player's state (health, inventory, statistics)
//! from the world's files. Lists change over RCON while the server runs, and in their JSON
//! files while it's stopped.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::super::paths::Paths;
use super::{data_dir, dir, get, read_properties, run};

// ── from the console ──

#[derive(Debug, PartialEq)]
pub enum LogEvent {
    Joined(String),
    Left(String),
    /// "UUID of player <name> is <uuid>"
    Uuid(String, String),
}

fn valid_name(n: &str) -> bool {
    (3..=16).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// What a console line says about players, if anything. Chat (`<name> text`) never counts.
pub fn event(line: &str) -> Option<LogEvent> {
    let msg = line.rsplit_once("]: ").map(|(_, m)| m).unwrap_or(line).trim();
    if msg.starts_with('<') {
        return None;
    }
    if let Some(rest) = msg.strip_prefix("UUID of player ") {
        let (name, uuid) = rest.split_once(" is ")?;
        let uuid = uuid.trim();
        return (valid_name(name) && uuid.len() == 36 && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| LogEvent::Uuid(name.into(), uuid.into()));
    }
    let (name, joined) = if let Some(n) = msg.strip_suffix(" joined the game") {
        (n, true)
    } else if let Some(n) = msg.strip_suffix(" left the game") {
        (n, false)
    } else {
        return None;
    };
    valid_name(name).then(|| if joined { LogEvent::Joined(name.into()) } else { LogEvent::Left(name.into()) })
}

/// When a console line was written: its `HH:MM:SS` (local time) today, or yesterday if
/// that's in the future. `now` when the line has none.
pub fn line_time(line: &str, now: DateTime<Utc>) -> DateTime<Utc> {
    let head: String = line.chars().take(40).collect();
    let b = head.as_bytes();
    let found = (0..b.len().saturating_sub(7)).find_map(|i| {
        let s = &head[i..i + 8];
        let ok = s.as_bytes().iter().enumerate().all(|(j, c)| if j == 2 || j == 5 { *c == b':' } else { c.is_ascii_digit() });
        ok.then(|| NaiveTime::parse_from_str(s, "%H:%M:%S").ok()).flatten()
    });
    let Some(t) = found else { return now };
    let local_now = now.with_timezone(&Local);
    let Some(mut at) = Local.from_local_datetime(&local_now.date_naive().and_time(t)).earliest() else { return now };
    if at > local_now + chrono::Duration::minutes(1) {
        at -= chrono::Duration::days(1);
    }
    at.with_timezone(&Utc)
}

// ── history ──

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Seen {
    pub name: String,
    pub uuid: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub joins: u32,
    /// seconds, finished sessions
    pub playtime: u64,
    /// on since, while online
    pub online_since: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub name: String,
    pub joined: DateTime<Utc>,
    pub left: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct History {
    /// by lowercase name
    players: BTreeMap<String, Seen>,
    /// newest last, at most 2000
    sessions: Vec<Session>,
    peak: u32,
    peak_at: Option<DateTime<Utc>>,
    /// when the server was started, newest last, at most 200
    starts: Vec<DateTime<Utc>>,
}

fn history_file(paths: &Paths, id: &str) -> PathBuf {
    dir(paths, id).join("players.json")
}

fn lock() -> &'static Mutex<()> {
    static L: Mutex<()> = Mutex::new(());
    &L
}

fn load(paths: &Paths, id: &str) -> History {
    std::fs::read(history_file(paths, id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn change(paths: &Paths, id: &str, f: impl FnOnce(&mut History)) {
    let _g = lock().lock().unwrap();
    let mut h = load(paths, id);
    f(&mut h);
    if let Ok(bytes) = serde_json::to_vec(&h) {
        let _ = std::fs::write(history_file(paths, id), bytes);
    }
}

fn close(h: &mut History, key: &str, at: DateTime<Utc>) {
    let Some(p) = h.players.get_mut(key) else { return };
    let Some(since) = p.online_since.take() else { return };
    let at = at.max(since);
    p.playtime += (at - since).num_seconds().max(0) as u64;
    p.last_seen = at;
    h.sessions.push(Session { name: p.name.clone(), joined: since, left: at });
    if h.sessions.len() > 2000 {
        h.sessions.drain(..h.sessions.len() - 2000);
    }
}

pub fn record(paths: &Paths, id: &str, ev: &LogEvent, at: DateTime<Utc>) {
    change(paths, id, |h| match ev {
        LogEvent::Joined(name) => {
            let key = name.to_lowercase();
            let p = h.players.entry(key).or_insert_with(|| Seen { name: name.clone(), uuid: None, first_seen: at, last_seen: at, joins: 0, playtime: 0, online_since: None });
            p.name = name.clone();
            p.last_seen = at;
            if p.online_since.is_none() {
                p.joins += 1;
                p.online_since = Some(at);
            }
            let online = h.players.values().filter(|p| p.online_since.is_some()).count() as u32;
            if online > h.peak {
                h.peak = online;
                h.peak_at = Some(at);
            }
        }
        LogEvent::Left(name) => close(h, &name.to_lowercase(), at),
        LogEvent::Uuid(name, uuid) => {
            let p = h.players.entry(name.to_lowercase()).or_insert_with(|| Seen { name: name.clone(), uuid: None, first_seen: at, last_seen: at, joins: 0, playtime: 0, online_since: None });
            p.uuid = Some(uuid.clone());
        }
    });
}

pub fn started(paths: &Paths, id: &str, at: DateTime<Utc>) {
    change(paths, id, |h| {
        // left over from a crash: they left when it went down, at the latest
        let open: Vec<String> = h.players.iter().filter(|(_, p)| p.online_since.is_some()).map(|(k, _)| k.clone()).collect();
        for k in open {
            let last = h.players[&k].last_seen;
            close(h, &k, last);
        }
        h.starts.push(at);
        if h.starts.len() > 200 {
            h.starts.remove(0);
        }
    });
}

/// The server stopped: everyone still on left then.
pub fn stopped(paths: &Paths, id: &str, at: DateTime<Utc>) {
    change(paths, id, |h| {
        let open: Vec<String> = h.players.iter().filter(|(_, p)| p.online_since.is_some()).map(|(k, _)| k.clone()).collect();
        for k in open {
            close(h, &k, at);
        }
    });
}

#[derive(Serialize)]
pub struct Insights {
    pub players: Vec<Seen>,
    pub sessions: Vec<Session>,
    pub peak: u32,
    pub peak_at: Option<DateTime<Utc>>,
    pub starts: Vec<DateTime<Utc>>,
}

/// Everyone who's played (also from the server's usercache, for players from before the
/// launcher tracked them), newest first, and the sessions of the last 30 days.
pub fn insights(paths: &Paths, id: &str) -> Result<Insights> {
    get(paths, id)?;
    let h = {
        let _g = lock().lock().unwrap();
        load(paths, id)
    };
    let mut players: BTreeMap<String, Seen> = h.players.clone();
    for (name, uuid) in usercache(&data_dir(paths, id)) {
        players.entry(name.to_lowercase()).or_insert_with(|| Seen {
            name,
            uuid: Some(uuid),
            first_seen: DateTime::<Utc>::MIN_UTC,
            last_seen: DateTime::<Utc>::MIN_UTC,
            joins: 0,
            playtime: 0,
            online_since: None,
        });
    }
    let mut players: Vec<Seen> = players.into_values().collect();
    players.sort_by(|a, b| b.online_since.is_some().cmp(&a.online_since.is_some()).then(b.last_seen.cmp(&a.last_seen)));
    let month = Utc::now() - chrono::Duration::days(30);
    let sessions = h.sessions.iter().filter(|s| s.left >= month).cloned().collect();
    Ok(Insights { players, sessions, peak: h.peak, peak_at: h.peak_at, starts: h.starts })
}

fn usercache(data: &Path) -> Vec<(String, String)> {
    let v: Value = std::fs::read(data.join("usercache.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e["name"].as_str()?.to_string(), e["uuid"].as_str()?.to_string())))
        .filter(|(n, _)| valid_name(n))
        .collect()
}

// ── whitelist, operators, bans ──

#[derive(Serialize)]
pub struct Banned {
    pub name: String,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct Lists {
    pub whitelist: Vec<String>,
    pub ops: Vec<String>,
    pub banned: Vec<Banned>,
    pub whitelist_on: bool,
}

fn read_list(data: &Path, file: &str) -> Vec<Value> {
    std::fs::read(data.join(file)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

fn names(list: &[Value]) -> Vec<String> {
    let mut n: Vec<String> = list.iter().filter_map(|e| e["name"].as_str().map(String::from)).collect();
    n.sort_by_key(|s| s.to_lowercase());
    n
}

pub fn lists(paths: &Paths, id: &str) -> Result<Lists> {
    get(paths, id)?;
    let data = data_dir(paths, id);
    let props = read_properties(&data);
    let banned = read_list(&data, "banned-players.json")
        .iter()
        .filter_map(|e| Some(Banned { name: e["name"].as_str()?.to_string(), reason: e["reason"].as_str().map(String::from) }))
        .collect();
    Ok(Lists {
        whitelist: names(&read_list(&data, "whitelist.json")),
        ops: names(&read_list(&data, "ops.json")),
        banned,
        whitelist_on: props.iter().any(|(k, v)| k == "white-list" && v == "true"),
    })
}

/// A player's UUID as the server knows it: from Mojang (online mode), or the one Minecraft
/// makes from the name in offline mode.
async fn uuid_for(client: &reqwest::Client, paths: &Paths, id: &str, name: &str) -> Result<String> {
    let data = data_dir(paths, id);
    let online = read_properties(&data).iter().find(|(k, _)| k == "online-mode").map(|(_, v)| v != "false").unwrap_or(true);
    if !online {
        return Ok(offline_uuid(name));
    }
    if let Some((_, u)) = usercache(&data).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Ok(u);
    }
    let res = client.get(format!("https://api.mojang.com/users/profiles/minecraft/{name}")).send().await?;
    if res.status() == reqwest::StatusCode::NOT_FOUND || res.status() == reqwest::StatusCode::NO_CONTENT {
        bail!("There's no Minecraft player called {name}");
    }
    let v: Value = res.error_for_status()?.json().await?;
    let raw = v["id"].as_str().ok_or_else(|| anyhow!("There's no Minecraft player called {name}"))?;
    Ok(dashed(raw))
}

fn dashed(raw: &str) -> String {
    if raw.len() != 32 {
        return raw.to_string();
    }
    format!("{}-{}-{}-{}-{}", &raw[0..8], &raw[8..12], &raw[12..16], &raw[16..20], &raw[20..32])
}

/// `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`: MD5, version 3.
pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};
    let mut h: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    dashed(&hex::encode(h))
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ListKind {
    Whitelist,
    Ops,
    Banned,
}

/// Adds a player to a list or takes them off.
pub async fn list_change(client: &reqwest::Client, paths: &Paths, id: &str, list: ListKind, name: &str, add: bool, reason: Option<String>) -> Result<()> {
    get(paths, id)?;
    let name = name.trim();
    if !valid_name(name) {
        bail!("That isn't a Minecraft name (3–16 letters, digits or _)");
    }
    if run::is_running(id) && run::running().lock().unwrap().get(id).is_some_and(|r| r.state == "running") {
        let cmd = match (list, add) {
            (ListKind::Whitelist, true) => format!("whitelist add {name}"),
            (ListKind::Whitelist, false) => format!("whitelist remove {name}"),
            (ListKind::Ops, true) => format!("op {name}"),
            (ListKind::Ops, false) => format!("deop {name}"),
            (ListKind::Banned, true) => format!("ban {name}{}", reason.as_deref().map(|r| format!(" {}", r.replace(['\n', '\r'], " "))).unwrap_or_default()),
            (ListKind::Banned, false) => format!("pardon {name}"),
        };
        let answer = run::command_quiet(id, &cmd).await?;
        if answer.contains("does not exist") || answer.contains("That player does not exist") {
            bail!("There's no Minecraft player called {name}");
        }
        return Ok(());
    }
    // stopped: the files
    let data = data_dir(paths, id);
    let file = match list {
        ListKind::Whitelist => "whitelist.json",
        ListKind::Ops => "ops.json",
        ListKind::Banned => "banned-players.json",
    };
    let mut entries = read_list(&data, file);
    entries.retain(|e| !e["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    if add {
        let uuid = uuid_for(client, paths, id, name).await?;
        entries.push(match list {
            ListKind::Whitelist => json!({ "uuid": uuid, "name": name }),
            ListKind::Ops => json!({ "uuid": uuid, "name": name, "level": 4, "bypassesPlayerLimit": false }),
            ListKind::Banned => json!({
                "uuid": uuid, "name": name,
                "created": Utc::now().format("%Y-%m-%d %H:%M:%S +0000").to_string(),
                "source": "Tessera Launcher", "expires": "forever",
                "reason": reason.unwrap_or_else(|| "Banned by an operator.".into()),
            }),
        });
    }
    std::fs::write(data.join(file), serde_json::to_vec_pretty(&entries)?)?;
    Ok(())
}

// ── one player ──

/// Minecraft's NBT (gzipped, big endian) as JSON: compounds become objects, lists arrays.
pub fn nbt(bytes: &[u8]) -> Result<Value> {
    let mut raw = Vec::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut raw)?;
    } else {
        raw = bytes.to_vec();
    }
    let mut r = Nbt { b: &raw, i: 0 };
    let tag = r.u8()?;
    if tag != 10 {
        bail!("not a compound");
    }
    r.string()?;
    r.value(10, 0)
}

struct Nbt<'a> {
    b: &'a [u8],
    i: usize,
}

impl Nbt<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.b.get(self.i..self.i + n).ok_or_else(|| anyhow!("NBT ends early"))?;
        self.i += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.i16()? as u16 as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 || n as usize > self.b.len() {
            bail!("odd NBT length");
        }
        Ok(n as usize)
    }
    fn value(&mut self, tag: u8, depth: u32) -> Result<Value> {
        if depth > 64 {
            bail!("NBT nested too deep");
        }
        Ok(match tag {
            1 => json!(self.u8()? as i8),
            2 => json!(self.i16()?),
            3 => json!(self.i32()?),
            4 => json!(self.i64()?),
            5 => json!(f32::from_be_bytes(self.take(4)?.try_into()?)),
            6 => json!(f64::from_be_bytes(self.take(8)?.try_into()?)),
            7 => {
                let n = self.len()?;
                json!(self.take(n)?.iter().map(|b| *b as i8).collect::<Vec<_>>())
            }
            8 => json!(self.string()?),
            9 => {
                let inner = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::new();
                for _ in 0..n {
                    v.push(self.value(inner, depth + 1)?);
                }
                Value::Array(v)
            }
            10 => {
                let mut m = serde_json::Map::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.string()?;
                    m.insert(k, self.value(t, depth + 1)?);
                }
                Value::Object(m)
            }
            11 => {
                let n = self.len()?;
                Value::Array((0..n).map(|_| self.i32().map(|x| json!(x))).collect::<Result<_>>()?)
            }
            12 => {
                let n = self.len()?;
                Value::Array((0..n).map(|_| self.i64().map(|x| json!(x))).collect::<Result<_>>()?)
            }
            _ => bail!("unknown NBT tag {tag}"),
        })
    }
}

#[derive(Serialize, Default)]
pub struct Item {
    pub slot: i32,
    pub id: String,
    pub count: i32,
}

#[derive(Serialize, Default)]
pub struct PlayerDetail {
    pub name: String,
    pub uuid: Option<String>,
    pub online: bool,
    pub seen: Option<Seen>,
    /// from the world's player file (None before they've played)
    pub health: Option<f64>,
    pub food: Option<i64>,
    pub xp_level: Option<i64>,
    pub gamemode: Option<String>,
    pub position: Option<[f64; 3]>,
    pub dimension: Option<String>,
    pub inventory: Vec<Item>,
    pub ender_chest: Vec<Item>,
    /// from the world's statistics
    pub stats: Option<Stats>,
    pub whitelisted: bool,
    pub op: bool,
    pub banned: bool,
}

#[derive(Serialize, Default)]
pub struct Stats {
    /// seconds
    pub play_time: u64,
    pub deaths: u64,
    pub player_kills: u64,
    pub mob_kills: u64,
    /// meters
    pub distance: u64,
    pub blocks_mined: u64,
    pub jumps: u64,
}

fn world_dir(data: &Path) -> PathBuf {
    let name = read_properties(data).into_iter().find(|(k, _)| k == "level-name").map(|(_, v)| v).filter(|v| !v.is_empty()).unwrap_or_else(|| "world".into());
    data.join(super::super::mrpack::safe_relative(&name).unwrap_or_else(|_| PathBuf::from("world")))
}

fn items(list: &Value) -> Vec<Item> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            Some(Item {
                slot: e["Slot"].as_i64().map(|s| s as i32).unwrap_or(-1),
                id: e["id"].as_str()?.to_string(),
                count: e["count"].as_i64().or(e["Count"].as_i64()).unwrap_or(1) as i32,
            })
        })
        .collect()
}

fn stats(file: &Path) -> Option<Stats> {
    let v: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let s = &v["stats"];
    let custom = &s["minecraft:custom"];
    let n = |k: &str| custom[k].as_u64().unwrap_or(0);
    let sum = |o: &Value| o.as_object().map(|m| m.values().filter_map(Value::as_u64).sum()).unwrap_or(0);
    let cm: u64 = custom.as_object().map(|m| m.iter().filter(|(k, _)| k.ends_with("_one_cm")).filter_map(|(_, v)| v.as_u64()).sum()).unwrap_or(0);
    Some(Stats {
        play_time: n("minecraft:play_time").max(n("minecraft:play_one_minute")) / 20,
        deaths: n("minecraft:deaths"),
        player_kills: n("minecraft:player_kills"),
        mob_kills: n("minecraft:mob_kills"),
        distance: cm / 100,
        blocks_mined: sum(&s["minecraft:mined"]),
        jumps: n("minecraft:jump"),
    })
}

pub async fn player(client: &reqwest::Client, paths: &Paths, id: &str, name: &str) -> Result<PlayerDetail> {
    if !valid_name(name) {
        bail!("Unknown player");
    }
    let data = data_dir(paths, id);
    let seen = insights(paths, id)?.players.into_iter().find(|p| p.name.eq_ignore_ascii_case(name));
    let uuid = match seen.as_ref().and_then(|s| s.uuid.clone()) {
        Some(u) => Some(u),
        None => uuid_for(client, paths, id, name).await.ok(),
    };
    let l = lists(paths, id)?;
    let has = |v: &[String]| v.iter().any(|n| n.eq_ignore_ascii_case(name));
    let mut d = PlayerDetail {
        name: seen.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| name.to_string()),
        online: run::running().lock().unwrap().get(id).is_some_and(|r| r.players.keys().any(|n| n.eq_ignore_ascii_case(name))),
        whitelisted: has(&l.whitelist),
        op: has(&l.ops),
        banned: l.banned.iter().any(|b| b.name.eq_ignore_ascii_case(name)),
        seen,
        ..Default::default()
    };
    if let Some(u) = &uuid {
        let world = world_dir(&data);
        // 26.1+ keeps players in players/data
        let file = [world.join("playerdata").join(format!("{u}.dat")), world.join("players").join("data").join(format!("{u}.dat"))].into_iter().find(|f| f.exists());
        if let Some(v) = file.and_then(|f| std::fs::read(f).ok()).and_then(|b| nbt(&b).ok()) {
            d.health = v["Health"].as_f64();
            d.food = v["foodLevel"].as_i64();
            d.xp_level = v["XpLevel"].as_i64();
            d.gamemode = v["playerGameType"].as_i64().map(|g| ["survival", "creative", "adventure", "spectator"].get(g as usize).unwrap_or(&"survival").to_string());
            if let Some(p) = v["Pos"].as_array().filter(|p| p.len() == 3) {
                d.position = Some([p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0), p[2].as_f64().unwrap_or(0.0)]);
            }
            d.dimension = v["Dimension"].as_str().map(String::from);
            d.inventory = items(&v["Inventory"]);
            // 1.21.5+ keeps armor and offhand in `equipment`
            if let Some(eq) = v["equipment"].as_object() {
                for (slot, key) in [(103, "head"), (102, "chest"), (101, "legs"), (100, "feet"), (-106, "offhand")] {
                    if let Some(id) = eq.get(key).and_then(|e| e["id"].as_str()) {
                        d.inventory.push(Item { slot, id: id.into(), count: eq[key]["count"].as_i64().unwrap_or(1) as i32 });
                    }
                }
            }
            d.ender_chest = items(&v["EnderItems"]);
        }
        d.stats = stats(&world.join("stats").join(format!("{u}.json")));
        d.uuid = Some(u.clone());
    }
    Ok(d)
}

/// Something done to a player on the running server.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Kick { reason: Option<String> },
    Gamemode { mode: String },
    Heal,
    Feed,
    Kill,
    Give { item: String, count: u32 },
    ClearInventory,
    TeleportTo { target: String },
    Message { text: String },
}

pub async fn act(paths: &Paths, id: &str, name: &str, action: Action) -> Result<String> {
    get(paths, id)?;
    if !valid_name(name) {
        bail!("Unknown player");
    }
    let clean = |s: &str| s.replace(['\n', '\r'], " ").chars().take(200).collect::<String>();
    let cmd = match action {
        Action::Kick { reason } => format!("kick {name}{}", reason.map(|r| format!(" {}", clean(&r))).unwrap_or_default()),
        Action::Gamemode { mode } => {
            if !["survival", "creative", "adventure", "spectator"].contains(&mode.as_str()) {
                bail!("Unknown game mode");
            }
            format!("gamemode {mode} {name}")
        }
        Action::Heal => format!("effect give {name} minecraft:instant_health 1 255 true"),
        Action::Feed => format!("effect give {name} minecraft:saturation 1 255 true"),
        Action::Kill => format!("kill {name}"),
        Action::Give { item, count } => {
            let item = item.trim().to_lowercase();
            if item.is_empty() || !item.chars().all(|c| c.is_ascii_alphanumeric() || "_:./-".contains(c)) {
                bail!("That isn't an item id, e.g. minecraft:diamond");
            }
            format!("give {name} {item} {}", count.clamp(1, 6400))
        }
        Action::ClearInventory => format!("clear {name}"),
        Action::TeleportTo { target } => {
            if !valid_name(&target) {
                bail!("Unknown player");
            }
            format!("tp {name} {target}")
        }
        Action::Message { text } => format!("tell {name} {}", clean(&text)),
    };
    run::command_quiet(id, &cmd).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_player_events() {
        assert_eq!(event("[12:00:01] [Server thread/INFO]: Notch joined the game"), Some(LogEvent::Joined("Notch".into())));
        assert_eq!(event("[12:00:01] [Server thread/INFO] [minecraft/MinecraftServer]: jeb_ left the game"), Some(LogEvent::Left("jeb_".into())));
        assert_eq!(event("[12:00:01] [Server thread/INFO]: <Notch> I joined the game"), None);
        assert_eq!(
            event("[12:00:00] [User Authenticator #1/INFO]: UUID of player Notch is 069a79f4-44e9-4726-a5be-fca90e38aaf5"),
            Some(LogEvent::Uuid("Notch".into(), "069a79f4-44e9-4726-a5be-fca90e38aaf5".into()))
        );
    }

    #[test]
    fn reads_line_times() {
        let now = Local.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap().with_timezone(&Utc);
        let t = line_time("[11:30:05] [Server thread/INFO]: x", now).with_timezone(&Local);
        assert_eq!(t.format("%d %H:%M:%S").to_string(), "31 11:30:05");
        // Forge's format; later than now means yesterday
        let t = line_time("[31Oct2026 23:10:00.123] [Server thread/INFO] [x/]: y", now).with_timezone(&Local);
        assert_eq!(t.format("%d %H:%M").to_string(), "30 23:10");
        assert_eq!(line_time("no time here", now), now);
    }

    #[test]
    fn counts_playtime_and_peak() {
        let root = std::env::temp_dir().join(format!("tessera-players-{}", std::process::id()));
        let paths = Paths::new(root.clone());
        std::fs::create_dir_all(dir(&paths, "s1")).unwrap();
        let t0 = Utc::now() - chrono::Duration::hours(2);
        let at = |m: i64| t0 + chrono::Duration::minutes(m);
        record(&paths, "s1", &LogEvent::Joined("Notch".into()), at(0));
        record(&paths, "s1", &LogEvent::Joined("jeb_".into()), at(5));
        record(&paths, "s1", &LogEvent::Left("Notch".into()), at(30));
        record(&paths, "s1", &LogEvent::Joined("Notch".into()), at(40));
        stopped(&paths, "s1", at(60));
        let h = load(&paths, "s1");
        assert_eq!(h.players["notch"].playtime, 50 * 60);
        assert_eq!(h.players["notch"].joins, 2);
        assert_eq!(h.players["jeb_"].playtime, 55 * 60);
        assert_eq!((h.peak, h.sessions.len()), (2, 3));
        assert!(h.players.values().all(|p| p.online_since.is_none()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn makes_offline_uuids() {
        // what an offline-mode server gives "Notch"
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn reads_nbt() {
        // { "": { Health: 20.0f, Inventory: [ { Slot: 0b, id: "minecraft:dirt", count: 3 } ] } }
        let mut b = vec![10, 0, 0];
        b.extend([5, 0, 6]);
        b.extend(b"Health");
        b.extend(20f32.to_be_bytes());
        b.extend([9, 0, 9]);
        b.extend(b"Inventory");
        b.extend([10, 0, 0, 0, 1]);
        b.extend([1, 0, 4]);
        b.extend(b"Slot");
        b.push(0);
        b.extend([8, 0, 2]);
        b.extend(b"id");
        b.extend([0, 14]);
        b.extend(b"minecraft:dirt");
        b.extend([3, 0, 5]);
        b.extend(b"count");
        b.extend(3i32.to_be_bytes());
        b.extend([0, 0]);
        let v = nbt(&b).unwrap();
        assert_eq!(v["Health"].as_f64(), Some(20.0));
        let inv = items(&v["Inventory"]);
        assert_eq!((inv[0].slot, inv[0].id.as_str(), inv[0].count), (0, "minecraft:dirt", 3));
    }
}
