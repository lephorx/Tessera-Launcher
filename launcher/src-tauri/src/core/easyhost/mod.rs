//! Easy hosting: Minecraft servers the launcher runs itself, without Docker or Tessera.
//!
//! Each server lives in `easy-servers/<id>/`: `server.json` (what it is and how to start it)
//! and `data/`, the server's own folder (world, mods, server.properties). The launcher gets
//! the Java Mojang names for the version, installs the server (vanilla jar, Paper, Fabric's
//! server launcher, or Forge/NeoForge through their installer), and runs it as a child
//! process while the launcher is open. Friends on the same network join by this computer's
//! address; Playit (see `playit.rs`) gives it a public one.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod backups;
pub mod files;
pub mod players;
mod run;

pub use run::{adopt_all, kill, log, send, start, stop, Emit, Event};
use run::{command, console_java};

use super::download::{download_all, fetch_json, Download};
use super::instances::{self, Loader};
use super::java::ensure_java;
use super::mrpack::safe_relative;
use super::paths::{ensure_parent, Paths};
use super::{loaders, serverpack, versions};

pub(crate) type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Send + Sync);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Vanilla,
    /// Plugins (Bukkit/Spigot/Paper), vanilla clients join
    Paper,
    Fabric,
    Forge,
    NeoForge,
}

/// How the server starts, after the JVM options.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Start {
    /// `-jar <file>`
    Jar { file: String },
    /// `@<file>`: Forge/NeoForge 1.17+ write their classpath and main class into one
    Args { file: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EasyServer {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub game_version: String,
    pub loader_version: Option<String>,
    pub port: u16,
    pub memory_mb: u32,
    /// The instance it was made from (its mods); players join with that instance.
    pub instance: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    java_component: String,
    start: Start,
    /// Automatic backups: every this many hours (None: off), keeping the newest `backup_keep`
    #[serde(default)]
    pub backup_every_hours: Option<u32>,
    #[serde(default = "five")]
    pub backup_keep: u32,
}

fn five() -> u32 {
    5
}

fn root(paths: &Paths) -> PathBuf {
    paths.root.join("easy-servers")
}

fn dir(paths: &Paths, id: &str) -> PathBuf {
    root(paths).join(id)
}

pub fn data_dir(paths: &Paths, id: &str) -> PathBuf {
    dir(paths, id).join("data")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn get(paths: &Paths, id: &str) -> Result<EasyServer> {
    if !valid_id(id) {
        bail!("Unknown server");
    }
    let bytes = std::fs::read(dir(paths, id).join("server.json")).map_err(|_| anyhow!("Unknown server"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save(paths: &Paths, s: &EasyServer) -> Result<()> {
    std::fs::create_dir_all(dir(paths, &s.id))?;
    std::fs::write(dir(paths, &s.id).join("server.json"), serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct View {
    #[serde(flatten)]
    pub server: EasyServer,
    /// "stopped" | "starting" | "running" | "stopping"
    pub state: &'static str,
    pub players: Vec<String>,
    pub max_players: u32,
    /// How friends on the same network join.
    pub lan_address: Option<String>,
    /// While it runs: since when, CPU (100 = one core) and memory in bytes
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cpu: f32,
    pub memory_used: u64,
    pub has_icon: bool,
}

fn view(paths: &Paths, s: EasyServer) -> View {
    let (state, players) = match run::running().lock().unwrap().get(&s.id) {
        Some(r) => (r.state, {
            let mut p: Vec<String> = r.players.keys().cloned().collect();
            p.sort_by_key(|n| n.to_lowercase());
            p
        }),
        None => ("stopped", Vec::new()),
    };
    let usage = run::usage(&s.id);
    let data = data_dir(paths, &s.id);
    let max_players = read_properties(&data).into_iter().find(|(k, _)| k == "max-players").and_then(|(_, v)| v.parse().ok()).unwrap_or(20);
    let lan_address = lan_ip().map(|ip| if s.port == 25565 { ip } else { format!("{ip}:{}", s.port) });
    View {
        state,
        players,
        max_players,
        lan_address,
        started_at: usage.map(|u| u.2),
        cpu: usage.map(|u| u.0).unwrap_or(0.0),
        memory_used: usage.map(|u| u.1).unwrap_or(0),
        has_icon: data.join("server-icon.png").exists(),
        server: s,
    }
}

pub fn list(paths: &Paths) -> Vec<View> {
    let mut all: Vec<EasyServer> = std::fs::read_dir(root(paths))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| get(paths, &e.file_name().to_string_lossy()).ok())
        .collect();
    all.sort_by_key(|s| s.created_at);
    all.into_iter().map(|s| view(paths, s)).collect()
}

pub fn get_view(paths: &Paths, id: &str) -> Result<View> {
    Ok(view(paths, get(paths, id)?))
}

/// This computer's address on the local network (the one a connection to the internet uses).
fn lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// The first port from 25565 that no server here uses and nothing else listens on.
fn free_port(paths: &Paths) -> u16 {
    let taken: HashSet<u16> = list(paths).iter().map(|v| v.server.port).collect();
    (25565..25700).find(|p| !taken.contains(p) && run::port_free(*p)).unwrap_or(25565)
}

// ── creating ──

#[derive(Deserialize)]
pub struct NewServer {
    pub name: String,
    pub kind: Kind,
    /// Empty = the newest release
    pub game_version: String,
    pub memory_mb: u32,
    /// Make it from this instance: its version, loader and mods
    pub instance: Option<String>,
    /// The player agreed to Minecraft's EULA (https://aka.ms/MinecraftEULA)
    pub eula: bool,
}

pub async fn create(client: &reqwest::Client, paths: &Paths, new: NewServer, progress: Progress<'_>) -> Result<View> {
    if !new.eula {
        bail!("Agree to the Minecraft EULA to run a server");
    }
    let name = new.name.trim().chars().take(48).collect::<String>();
    if name.is_empty() {
        bail!("Give the server a name");
    }
    let (kind, mut game, loader_version) = match &new.instance {
        Some(id) => {
            let inst = instances::get(paths, id)?;
            let kind = match inst.loader {
                Loader::Vanilla => Kind::Vanilla,
                Loader::Fabric => Kind::Fabric,
                Loader::Forge => Kind::Forge,
                Loader::NeoForge => Kind::NeoForge,
                Loader::Quilt => bail!("Quilt servers aren't supported yet; use Tessera with Docker for this one"),
            };
            (kind, inst.game_version, inst.loader_version)
        }
        None => (new.kind, new.game_version.trim().to_string(), None),
    };
    if game.is_empty() {
        progress("Finding the newest Minecraft", 0, 0);
        game = versions::manifest(client).await?.latest.release;
    }

    let id = format!("s{}", chrono::Utc::now().timestamp_millis());
    let data = data_dir(paths, &id);
    std::fs::create_dir_all(&data)?;
    let made: Result<EasyServer> = async {
        let installed = install(client, paths, &data, kind, &game, loader_version, progress).await?;
        if let Some(inst) = &new.instance {
            let pack = serverpack::build(client, paths, inst, progress).await?;
            install_pack(client, &pack.bytes, &data, progress).await?;
        }
        let server = EasyServer {
            id: id.clone(),
            name: name.clone(),
            kind,
            game_version: game.clone(),
            loader_version: installed.loader_version,
            port: free_port(paths),
            memory_mb: new.memory_mb.clamp(1024, 65536),
            instance: new.instance.clone(),
            created_at: chrono::Utc::now(),
            java_component: installed.java_component,
            start: installed.start,
            backup_every_hours: Some(24),
            backup_keep: 5,
        };
        std::fs::write(data.join("eula.txt"), "# Agreed in Tessera Launcher (https://aka.ms/MinecraftEULA)\neula=true\n")?;
        write_properties(&data, &[("server-port", server.port.to_string()), ("motd", name.clone()), ("enable-command-block", "false".into())])?;
        save(paths, &server)?;
        Ok(server)
    }
    .await;
    match made {
        Ok(s) => Ok(view(paths, s)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(dir(paths, &id));
            Err(e)
        }
    }
}

pub(crate) struct Installed {
    start: Start,
    loader_version: Option<String>,
    java_component: String,
}

/// Puts the server software into `data`: Minecraft's server, Paper, Fabric's server
/// launcher, or Forge/NeoForge through their installer. The newest loader if none is given.
pub(crate) async fn install(client: &reqwest::Client, paths: &Paths, data: &Path, kind: Kind, game: &str, loader_version: Option<String>, progress: Progress<'_>) -> Result<Installed> {
    progress("Getting Minecraft", 0, 0);
    let vanilla = versions::version_json(client, paths, game).await?;
    let component = vanilla.java_version.as_ref().map(|j| j.component.clone()).unwrap_or_else(|| "jre-legacy".into());
    let java = ensure_java(client, paths, &component, |d, t| progress("Getting Java", d, t)).await?;
    let java = console_java(&java);
    let mut lv = loader_version;
    let start = match kind {
        Kind::Vanilla => {
            let jar = vanilla.downloads.as_ref().and_then(|d| d.server.clone()).ok_or_else(|| anyhow!("Minecraft {game} has no server"))?;
            fetch(client, &jar.url, &data.join("server.jar"), jar.sha1.clone(), progress).await?;
            lv = None;
            Start::Jar { file: "server.jar".into() }
        }
        Kind::Paper => {
            let (url, build) = paper_jar(client, game).await?;
            lv = Some(build);
            fetch(client, &url, &data.join("server.jar"), None, progress).await?;
            Start::Jar { file: "server.jar".into() }
        }
        Kind::Fabric => {
            let v = match lv.clone() {
                Some(v) => v,
                None => newest_loader(client, Loader::Fabric, game).await?,
            };
            let installers: Vec<Value> = fetch_json(client, "https://meta.fabricmc.net/v2/versions/installer").await?;
            let installer = installers.iter().find(|i| i["stable"] == true).or(installers.first()).and_then(|i| i["version"].as_str()).ok_or_else(|| anyhow!("No Fabric installer"))?;
            let url = format!("https://meta.fabricmc.net/v2/versions/loader/{game}/{v}/{installer}/server/jar");
            fetch(client, &url, &data.join("fabric-server-launch.jar"), None, progress).await?;
            // Fabric's launcher downloads the vanilla server itself on first start; give it
            // the jar now so the first start doesn't stall on that
            if let Some(jar) = vanilla.downloads.as_ref().and_then(|d| d.server.clone()) {
                fetch(client, &jar.url, &data.join("server.jar"), jar.sha1.clone(), progress).await?;
            }
            lv = Some(v);
            Start::Jar { file: "fabric-server-launch.jar".into() }
        }
        Kind::Forge | Kind::NeoForge => {
            let loader = if kind == Kind::Forge { Loader::Forge } else { Loader::NeoForge };
            let v = match lv.clone() {
                Some(v) => v,
                None => newest_loader(client, loader, game).await?,
            };
            let url = if kind == Kind::Forge {
                format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{game}-{v}/forge-{game}-{v}-installer.jar")
            } else {
                format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar")
            };
            let installer = data.join("installer.jar");
            fetch(client, &url, &installer, None, progress).await?;
            progress(if kind == Kind::Forge { "Installing Forge" } else { "Installing NeoForge" }, 0, 0);
            let out = command(&java).arg("-jar").arg(&installer).arg("--installServer").current_dir(data).output().await?;
            if !out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let tail: Vec<&str> = text.lines().rev().take(5).collect();
                bail!("The installer failed: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" / "));
            }
            let _ = std::fs::remove_file(&installer);
            let _ = std::fs::remove_file(data.join("installer.jar.log"));
            lv = Some(v);
            forge_start(data)?
        }
    };
    Ok(Installed { start, loader_version: lv, java_component: component })
}

async fn newest_loader(client: &reqwest::Client, loader: Loader, game: &str) -> Result<String> {
    let list = loaders::versions(client, loader, game).await?;
    Ok(list.iter().find(|l| l.stable).or(list.first()).ok_or_else(|| anyhow!("No {loader:?} for Minecraft {game}"))?.version.clone())
}

async fn fetch(client: &reqwest::Client, url: &str, path: &Path, sha1: Option<String>, progress: Progress<'_>) -> Result<()> {
    let _ = std::fs::remove_file(path);
    download_all(client, vec![Download { url: url.into(), path: path.into(), sha1, size: None, executable: false }], |d, t| progress("Downloading the server", d, t)).await
}

/// The newest Paper build for a version: its jar URL and build number.
async fn paper_jar(client: &reqwest::Client, game: &str) -> Result<(String, String)> {
    let builds: Value = fetch_json(client, &format!("https://fill.papermc.io/v3/projects/paper/versions/{game}/builds"))
        .await
        .with_context(|| format!("Paper isn't available for Minecraft {game}"))?;
    let list = builds.as_array().ok_or_else(|| anyhow!("Paper isn't available for Minecraft {game}"))?;
    // newest first; prefer a stable build
    let pick = list.iter().find(|b| b["channel"] == "STABLE").or(list.first()).ok_or_else(|| anyhow!("Paper has no builds for Minecraft {game}"))?;
    let url = pick["downloads"]["server:default"]["url"].as_str().ok_or_else(|| anyhow!("Paper sent no download"))?;
    Ok((url.to_string(), pick["id"].to_string()))
}

/// After the Forge/NeoForge installer: the `@args` file (1.17+), or the forge jar (older).
fn forge_start(data: &Path) -> Result<Start> {
    let name = if cfg!(windows) { "win_args.txt" } else { "unix_args.txt" };
    let mut found = None;
    walk_find(&data.join("libraries"), name, &mut found);
    if let Some(f) = found {
        let rel = f.strip_prefix(data).unwrap_or(&f).to_string_lossy().replace('\\', "/");
        return Ok(Start::Args { file: rel });
    }
    let jar = std::fs::read_dir(data)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| (n.starts_with("forge-") || n.starts_with("neoforge-")) && n.ends_with(".jar") && !n.contains("installer"))
        .ok_or_else(|| anyhow!("The installer didn't leave a server to start"))?;
    Ok(Start::Jar { file: jar })
}

fn walk_find(dir: &Path, name: &str, found: &mut Option<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_find(&p, name, found);
        } else if e.file_name() == name {
            *found = Some(p);
        }
    }
}

/// The server side of an instance's server pack: files whose `env.server` isn't unsupported,
/// then `overrides/` and `server-overrides/`.
async fn install_pack(client: &reqwest::Client, pack: &[u8], data: &Path, progress: Progress<'_>) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(pack))?;
    let index: Value = {
        let mut f = zip.by_name("modrinth.index.json")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        serde_json::from_slice(&buf)?
    };
    let mut downloads = Vec::new();
    for f in index["files"].as_array().into_iter().flatten() {
        if f["env"]["server"] == "unsupported" {
            continue;
        }
        let (Some(path), Some(url)) = (f["path"].as_str(), f["downloads"][0].as_str()) else { continue };
        downloads.push(Download { url: url.into(), path: data.join(safe_relative(path)?), sha1: f["hashes"]["sha1"].as_str().map(String::from), size: f["fileSize"].as_u64(), executable: false });
    }
    download_all(client, downloads, |d, t| progress("Downloading mods", d, t)).await?;
    for prefix in ["overrides/", "server-overrides/"] {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let Some(rel) = entry.name().strip_prefix(prefix).map(String::from) else { continue };
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            let target = data.join(safe_relative(&rel)?);
            ensure_parent(&target)?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(target, buf)?;
        }
    }
    Ok(())
}

// ── server.properties ──

/// server.properties' values: `\:` and `\=` escapes, `\uXXXX` and `\n`, like Java writes them.
fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| it.next()).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) => out.push(ch),
                    None => out.push_str(&hex),
                }
            }
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn escape(v: &str) -> String {
    let mut out = String::new();
    for c in v.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\\' => out.push_str("\\\\"),
            ':' => out.push_str("\\:"),
            '=' => out.push_str("\\="),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn read_properties(data: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(data.join("server.properties"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), unescape(v.trim()))))
        .collect()
}

/// Sets some keys, keeping the rest (and comments) as they are.
pub fn write_properties(data: &Path, set: &[(&str, String)]) -> Result<()> {
    let file = data.join("server.properties");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut done = HashSet::new();
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, _)) if !l.trim_start().starts_with('#') => match set.iter().find(|(key, _)| *key == k.trim()) {
                Some((key, v)) => {
                    done.insert(*key);
                    format!("{key}={}", escape(v))
                }
                None => l.to_string(),
            },
            _ => l.to_string(),
        })
        .collect();
    for (k, v) in set {
        if !done.contains(k) {
            lines.push(format!("{k}={}", escape(v)));
        }
    }
    std::fs::write(file, lines.join("\n") + "\n")?;
    Ok(())
}

/// Keys the launcher looks after itself: the port is in the server's settings, and RCON is
/// how the launcher talks to the server.
const MANAGED: [&str; 5] = ["server-port", "enable-rcon", "rcon.port", "rcon.password", "broadcast-rcon-to-ops"];

/// Every key in server.properties (without the ones the launcher manages).
pub fn properties(paths: &Paths, id: &str) -> Result<HashMap<String, String>> {
    get(paths, id)?;
    Ok(read_properties(&data_dir(paths, id)).into_iter().filter(|(k, _)| !MANAGED.contains(&k.as_str())).collect())
}

pub fn set_properties(paths: &Paths, id: &str, values: HashMap<String, String>) -> Result<()> {
    get(paths, id)?;
    let mut set = Vec::new();
    for (k, v) in &values {
        if MANAGED.contains(&k.as_str()) {
            bail!("{k} is set by the launcher");
        }
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c)) {
            bail!("{k} isn't a server.properties key");
        }
        set.push((k.as_str(), v.replace('\r', "")));
    }
    write_properties(&data_dir(paths, id), &set)
}

#[derive(Deserialize)]
pub struct Changes {
    pub name: Option<String>,
    pub motd: Option<String>,
    pub memory_mb: Option<u32>,
    pub port: Option<u16>,
    pub max_players: Option<u32>,
    pub difficulty: Option<String>,
    pub gamemode: Option<String>,
    pub pvp: Option<bool>,
    pub whitelist: Option<bool>,
    /// 0 turns automatic backups off
    pub backup_every_hours: Option<u32>,
    pub backup_keep: Option<u32>,
}

pub fn update(paths: &Paths, id: &str, c: Changes) -> Result<View> {
    let mut s = get(paths, id)?;
    let data = data_dir(paths, id);
    let mut props: Vec<(&str, String)> = Vec::new();
    if let Some(n) = c.name.map(|n| n.trim().chars().take(48).collect::<String>()).filter(|n| !n.is_empty()) {
        s.name = n;
    }
    if let Some(m) = c.motd {
        let lines: Vec<&str> = m.lines().take(2).collect();
        props.push(("motd", lines.join("\n").chars().take(200).collect()));
    }
    if let Some(m) = c.memory_mb {
        s.memory_mb = m.clamp(1024, 65536);
    }
    if let Some(p) = c.port {
        if p < 1024 {
            bail!("Pick a port from 1024 up");
        }
        if p != s.port && list(paths).iter().any(|v| v.server.id != s.id && v.server.port == p) {
            bail!("Another server here uses port {p}");
        }
        s.port = p;
        props.push(("server-port", p.to_string()));
    }
    if let Some(m) = c.max_players {
        props.push(("max-players", m.clamp(1, 1000).to_string()));
    }
    if let Some(d) = c.difficulty.filter(|d| ["peaceful", "easy", "normal", "hard"].contains(&d.as_str())) {
        props.push(("difficulty", d));
    }
    if let Some(g) = c.gamemode.filter(|g| ["survival", "creative", "adventure", "spectator"].contains(&g.as_str())) {
        props.push(("gamemode", g));
    }
    if let Some(p) = c.pvp {
        props.push(("pvp", p.to_string()));
    }
    if let Some(w) = c.whitelist {
        props.push(("white-list", w.to_string()));
        props.push(("enforce-whitelist", w.to_string()));
    }
    if let Some(h) = c.backup_every_hours {
        s.backup_every_hours = (h > 0).then_some(h.min(24 * 30));
    }
    if let Some(k) = c.backup_keep {
        s.backup_keep = k.clamp(1, 100);
    }
    write_properties(&data, &props)?;
    save(paths, &s)?;
    Ok(view(paths, s))
}

// ── icon ──

/// The server's icon (server-icon.png) as a data URL.
pub fn icon(paths: &Paths, id: &str) -> Result<Option<String>> {
    use base64::Engine;
    get(paths, id)?;
    Ok(std::fs::read(data_dir(paths, id).join("server-icon.png")).ok().map(|b| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))))
}

/// A 64×64 PNG (the launcher scales the picture), or None to remove it.
pub fn set_icon(paths: &Paths, id: &str, png: Option<Vec<u8>>) -> Result<()> {
    get(paths, id)?;
    let file = data_dir(paths, id).join("server-icon.png");
    let Some(png) = png else {
        let _ = std::fs::remove_file(file);
        return Ok(());
    };
    // the PNG signature, then IHDR's width and height
    let ok = png.len() > 24 && png.len() < 1 << 20 && png.starts_with(b"\x89PNG\r\n\x1a\n") && &png[12..16] == b"IHDR" && png[16..20] == 64u32.to_be_bytes() && png[20..24] == 64u32.to_be_bytes();
    if !ok {
        bail!("The icon has to be a 64×64 PNG");
    }
    std::fs::write(file, png)?;
    Ok(())
}

// ── changing what it is ──

/// What a Minecraft server needs to start, left by an install. Moved away while another
/// is installed, so a failed install can be undone.
const SOFTWARE: [&str; 9] = ["server.jar", "fabric-server-launch.jar", ".fabric", "libraries", "versions", "cache", "run.sh", "run.bat", "user_jvm_args.txt"];

pub(crate) fn software_files(data: &Path) -> Vec<String> {
    let mut out: Vec<String> = SOFTWARE.iter().filter(|n| data.join(n).exists()).map(|n| n.to_string()).collect();
    for e in std::fs::read_dir(data).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let jar = n.ends_with(".jar") && (n.starts_with("forge-") || n.starts_with("neoforge-") || n.starts_with("minecraft_server") || n.starts_with("paper"));
        if jar && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Which mods or plugins work together: Paper's plugins, Fabric's mods, Forge's and NeoForge's.
fn family(k: Kind) -> &'static str {
    match k {
        Kind::Vanilla => "vanilla",
        Kind::Paper => "plugins",
        Kind::Fabric => "fabric",
        Kind::Forge | Kind::NeoForge => "forge",
    }
}

#[derive(Deserialize)]
pub struct Change {
    pub kind: Kind,
    /// Empty = the newest release
    pub game_version: String,
    /// None = the newest for that version
    pub loader_version: Option<String>,
}

/// Switches the server to another type or version, keeping its world and settings. A backup
/// is made first (a world can't go back to an older Minecraft). Mods or plugins that can't
/// work with the new type are moved to `mods-<old type>` / `plugins-<old type>`.
pub async fn change(client: &reqwest::Client, paths: &Paths, id: &str, c: Change, progress: Progress<'_>) -> Result<View> {
    let mut s = get(paths, id)?;
    if run::is_running(id) {
        bail!("Stop the server first");
    }
    let game = match c.game_version.trim() {
        "" => versions::manifest(client).await?.latest.release,
        v => v.to_string(),
    };
    progress("Backing up the server", 0, 0);
    backups::create(paths, id, false).await.context("backing up first")?;

    let data = data_dir(paths, id);
    let old = dir(paths, id).join("software.before-change");
    run::remove_dir_retrying(&old).await?;
    std::fs::create_dir_all(&old)?;
    let moved = software_files(&data);
    for n in &moved {
        std::fs::rename(data.join(n), old.join(n))?;
    }
    let lv = c.loader_version.filter(|v| !v.trim().is_empty());
    match install(client, paths, &data, c.kind, &game, lv, progress).await {
        Ok(installed) => {
            let _ = run::remove_dir_retrying(&old).await;
            let old_kind = format!("{:?}", s.kind).to_lowercase();
            if family(s.kind) != family(c.kind) {
                for folder in ["mods", "plugins"] {
                    let from = data.join(folder);
                    let to = data.join(format!("{folder}-{old_kind}"));
                    if from.is_dir() && !to.exists() && std::fs::read_dir(&from).map(|mut r| r.next().is_some()).unwrap_or(false) {
                        std::fs::rename(&from, &to)?;
                    }
                }
            }
            s.kind = c.kind;
            s.game_version = game;
            s.loader_version = installed.loader_version;
            s.java_component = installed.java_component;
            s.start = installed.start;
            // the instance it came from has other versions now
            s.instance = None;
            save(paths, &s)?;
            Ok(view(paths, s))
        }
        Err(e) => {
            for n in software_files(&data) {
                let p = data.join(&n);
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
            for n in &moved {
                let _ = std::fs::rename(old.join(n), data.join(n));
            }
            let _ = std::fs::remove_dir_all(&old);
            Err(e)
        }
    }
}

/// Deletes the server, its world and its backups. A running server (or anything else still
/// using its files) is closed first.
pub async fn delete(paths: &Paths, id: &str) -> Result<()> {
    get(paths, id)?;
    run::force_stop(paths, id).await;
    run::remove_dir_retrying(&dir(paths, id)).await?;
    run::forget(id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_server_properties() {
        let dir = std::env::temp_dir().join(format!("tessera-props-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("server.properties"), "#Minecraft server properties\nmotd=A Minecraft Server\nserver-port=25565\npvp=true\n").unwrap();
        write_properties(&dir, &[("server-port", "25570".into()), ("max-players", "8".into())]).unwrap();
        let p = read_properties(&dir);
        assert!(p.contains(&("server-port".into(), "25570".into())));
        assert!(p.contains(&("max-players".into(), "8".into())));
        assert!(p.contains(&("motd".into(), "A Minecraft Server".into())));
        assert!(std::fs::read_to_string(dir.join("server.properties")).unwrap().starts_with("#Minecraft"));
        // a two-line MOTD with colours survives the round trip
        write_properties(&dir, &[("motd", "§aHello: world\nLine two ✓".into())]).unwrap();
        assert!(std::fs::read_to_string(dir.join("server.properties")).unwrap().contains("motd=\\u00a7aHello\\: world\\nLine two \\u2713"));
        assert!(read_properties(&dir).contains(&("motd".into(), "§aHello: world\nLine two ✓".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_server_software() {
        let dir = std::env::temp_dir().join(format!("tessera-software-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("libraries")).unwrap();
        std::fs::create_dir_all(dir.join("world")).unwrap();
        for f in ["server.jar", "forge-1.20.1-47.4.28-shim.jar", "server.properties", "run.sh"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let mut found = software_files(&dir);
        found.sort();
        assert_eq!(found, ["forge-1.20.1-47.4.28-shim.jar", "libraries", "run.sh", "server.jar"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod real_server {
    use super::*;
    use std::sync::Arc;

    /// TESSERA_DATA=<launcher data folder> TESSERA_KIND=fabric|paper|vanilla|forge|neoforge TESSERA_GAME=1.21.1
    ///   cargo test real_easy_server -- --ignored --nocapture
    /// Makes and starts a server in a temporary folder that borrows the data folder's `meta`
    /// (Java and game files); forgets it as if the launcher quit, finds it again, sends a
    /// command, backs it up, stops it, changes its version (TESSERA_CHANGE=kind:version) and
    /// force-deletes it while it runs.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn real_easy_server() {
        let data = PathBuf::from(std::env::var("TESSERA_DATA").expect("TESSERA_DATA"));
        let root = std::env::temp_dir().join(format!("tessera-easy-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(data.join("meta"), root.join("meta")).unwrap();
            // read only: a server from an instance reads its mods
            std::os::unix::fs::symlink(data.join("instances"), root.join("instances")).unwrap();
        }
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let kind: Kind = serde_json::from_value(serde_json::json!(std::env::var("TESSERA_KIND").unwrap_or("fabric".into()))).unwrap();
        let game = std::env::var("TESSERA_GAME").unwrap_or("1.21.1".into());
        let new = NewServer { name: "Test".into(), kind, game_version: game, memory_mb: 2048, instance: std::env::var("TESSERA_INSTANCE").ok(), eula: true };
        let v = create(&client, &paths, new, &|s, d, t| if d == 0 || d == t { println!("  {s} {d}/{t}") }).await.unwrap();
        println!("made {:?} {} {:?} port {} start {:?}", v.server.kind, v.server.game_version, v.server.loader_version, v.server.port, v.server.start);
        let id = v.server.id.clone();
        let emit: Emit = Arc::new(|e| {
            if let Event::Line(_, l) = e {
                if l.contains("Done (") || l.contains("ERROR") || l.contains("[Tessera]") || l.contains("players online") || l.starts_with("> ") {
                    println!("  | {l}");
                }
            }
        });
        let up = |paths: Paths, id: String| async move {
            let t0 = std::time::Instant::now();
            while get_view(&paths, &id).unwrap().state != "running" {
                assert!(run::is_running(&id), "the server stopped while starting:\n{}", log(&id).join("\n"));
                assert!(t0.elapsed().as_secs() < 300, "not up after 5 minutes");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            t0.elapsed().as_secs()
        };
        start(&client, &paths, &id, emit.clone()).await.unwrap();
        println!("up after {}s", up(paths.clone(), id.clone()).await);

        // the launcher quits (without stopping it) and starts again
        run::running().lock().unwrap().remove(&id);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        adopt_all(&paths, emit.clone());
        assert!(run::is_running(&id), "not found again");
        println!("found again: {}", get_view(&paths, &id).unwrap().state);
        up(paths.clone(), id.clone()).await;
        send(&id, "list", &emit).await.unwrap();
        let b = backups::create(&paths, &id, false).await.unwrap();
        println!("backup {} ({} KB)", b.name, b.size / 1024);
        stop(&id, std::time::Duration::from_secs(60)).await.unwrap();
        for _ in 0..40 {
            if !run::is_running(&id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert_eq!(get_view(&paths, &id).unwrap().state, "stopped");
        assert!(data_dir(&paths, &id).join("world").is_dir(), "no world saved");
        // the world's own NBT reads
        let level = players::nbt(&std::fs::read(data_dir(&paths, &id).join("world/level.dat")).unwrap()).unwrap();
        println!("level.dat: {} DataVersion {}", level["Data"]["LevelName"], level["Data"]["DataVersion"]);
        assert!(level["Data"]["DataVersion"].as_i64().unwrap() > 1000);

        if let Some((k, g)) = std::env::var("TESSERA_CHANGE").ok().as_deref().and_then(|c| c.split_once(':')) {
            let kind: Kind = serde_json::from_value(serde_json::json!(k)).unwrap();
            let v = change(&client, &paths, &id, Change { kind, game_version: g.into(), loader_version: None }, &|s, d, t| if d == 0 || d == t { println!("  {s} {d}/{t}") }).await.unwrap();
            println!("changed to {:?} {} {:?}", v.server.kind, v.server.game_version, v.server.loader_version);
            start(&client, &paths, &id, emit.clone()).await.unwrap();
            println!("up again after {}s", up(paths.clone(), id.clone()).await);
            // back to how it was, from the first backup
            stop(&id, std::time::Duration::from_secs(60)).await.unwrap();
            while run::is_running(&id) {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            let s = backups::restore(&client, &paths, &id, &b.name, &|s, d, t| if d == 0 || d == t { println!("  {s} {d}/{t}") }).await.unwrap();
            println!("restored: {:?} {} {:?}", s.kind, s.game_version, s.loader_version);
            start(&client, &paths, &id, emit.clone()).await.unwrap();
            println!("up after restoring: {}s", up(paths.clone(), id.clone()).await);
        } else {
            start(&client, &paths, &id, emit.clone()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        // deleting while it runs closes it
        delete(&paths, &id).await.unwrap();
        assert!(!dir(&paths, &id).exists());
        let _ = std::fs::remove_dir_all(&root);
        println!("deleted");
    }
}
