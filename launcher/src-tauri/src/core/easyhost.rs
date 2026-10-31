//! Easy hosting: Minecraft servers the launcher runs itself, without Docker or Tessera.
//!
//! Each server lives in `easy-servers/<id>/`: `server.json` (what it is and how to start it)
//! and `data/`, the server's own folder (world, mods, server.properties). The launcher gets
//! the Java Mojang names for the version, installs the server (vanilla jar, Paper, Fabric's
//! server launcher, or Forge/NeoForge through their installer), and runs it as a child
//! process while the launcher is open. Friends on the same network join by this computer's
//! address; Playit (see `playit.rs`) gives it a public one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::download::{download_all, fetch_json, Download};
use super::instances::{self, Loader};
use super::java::ensure_java;
use super::mrpack::safe_relative;
use super::paths::{ensure_parent, Paths};
use super::{loaders, serverpack, versions};

type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Send + Sync);

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

// ── what's running ──

struct Running {
    stdin: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>,
    /// "starting" until the server says "Done", then "running"; "stopping" after stop
    state: &'static str,
    players: HashSet<String>,
    kill: Option<tokio::sync::oneshot::Sender<()>>,
}

fn running() -> &'static Mutex<HashMap<String, Running>> {
    static R: std::sync::OnceLock<Mutex<HashMap<String, Running>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

fn logs() -> &'static Mutex<HashMap<String, VecDeque<String>>> {
    static L: std::sync::OnceLock<Mutex<HashMap<String, VecDeque<String>>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

#[derive(Serialize, Clone)]
pub struct View {
    #[serde(flatten)]
    pub server: EasyServer,
    /// "stopped" | "starting" | "running" | "stopping"
    pub state: &'static str,
    pub players: Vec<String>,
    /// How friends on the same network join.
    pub lan_address: Option<String>,
}

fn view(s: EasyServer) -> View {
    let (state, players) = match running().lock().unwrap().get(&s.id) {
        Some(r) => (r.state, {
            let mut p: Vec<String> = r.players.iter().cloned().collect();
            p.sort();
            p
        }),
        None => ("stopped", Vec::new()),
    };
    let lan_address = lan_ip().map(|ip| if s.port == 25565 { ip } else { format!("{ip}:{}", s.port) });
    View { server: s, state, players, lan_address }
}

pub fn list(paths: &Paths) -> Vec<View> {
    let mut all: Vec<EasyServer> = std::fs::read_dir(root(paths))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| get(paths, &e.file_name().to_string_lossy()).ok())
        .collect();
    all.sort_by_key(|s| s.created_at);
    all.into_iter().map(view).collect()
}

pub fn get_view(paths: &Paths, id: &str) -> Result<View> {
    Ok(view(get(paths, id)?))
}

pub fn log(id: &str) -> Vec<String> {
    logs().lock().unwrap().get(id).map(|l| l.iter().cloned().collect()).unwrap_or_default()
}

pub fn any_running() -> bool {
    !running().lock().unwrap().is_empty()
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
    (25565..25700).find(|p| !taken.contains(p) && std::net::TcpListener::bind(("0.0.0.0", *p)).is_ok()).unwrap_or(25565)
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
    let (kind, mut game, mut loader_version) = match &new.instance {
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
        progress("Getting Minecraft", 0, 0);
        let vanilla = versions::version_json(client, paths, &game).await?;
        let component = vanilla.java_version.as_ref().map(|j| j.component.clone()).unwrap_or_else(|| "jre-legacy".into());
        let java = ensure_java(client, paths, &component, |d, t| progress("Getting Java", d, t)).await?;
        let java = console_java(&java);

        let start = match kind {
            Kind::Vanilla => {
                let jar = vanilla.downloads.as_ref().and_then(|d| d.server.clone()).ok_or_else(|| anyhow!("Minecraft {game} has no server"))?;
                fetch(client, &jar.url, &data.join("server.jar"), jar.sha1.clone(), progress).await?;
                Start::Jar { file: "server.jar".into() }
            }
            Kind::Paper => {
                let (url, build) = paper_jar(client, &game).await?;
                loader_version = Some(build);
                fetch(client, &url, &data.join("server.jar"), None, progress).await?;
                Start::Jar { file: "server.jar".into() }
            }
            Kind::Fabric => {
                let lv = match loader_version.clone() {
                    Some(v) => v,
                    None => newest_loader(client, Loader::Fabric, &game).await?,
                };
                let installers: Vec<Value> = fetch_json(client, "https://meta.fabricmc.net/v2/versions/installer").await?;
                let installer = installers.iter().find(|i| i["stable"] == true).or(installers.first()).and_then(|i| i["version"].as_str()).ok_or_else(|| anyhow!("No Fabric installer"))?;
                let url = format!("https://meta.fabricmc.net/v2/versions/loader/{game}/{lv}/{installer}/server/jar");
                fetch(client, &url, &data.join("fabric-server-launch.jar"), None, progress).await?;
                // Fabric's launcher downloads the vanilla server itself on first start; give it
                // the jar now so the first start doesn't stall on that
                if let Some(jar) = vanilla.downloads.as_ref().and_then(|d| d.server.clone()) {
                    fetch(client, &jar.url, &data.join("server.jar"), jar.sha1.clone(), progress).await?;
                }
                loader_version = Some(lv);
                Start::Jar { file: "fabric-server-launch.jar".into() }
            }
            Kind::Forge | Kind::NeoForge => {
                let loader = if kind == Kind::Forge { Loader::Forge } else { Loader::NeoForge };
                let lv = match loader_version.clone() {
                    Some(v) => v,
                    None => newest_loader(client, loader, &game).await?,
                };
                let url = if kind == Kind::Forge {
                    format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{game}-{lv}/forge-{game}-{lv}-installer.jar")
                } else {
                    format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{lv}/neoforge-{lv}-installer.jar")
                };
                let installer = data.join("installer.jar");
                fetch(client, &url, &installer, None, progress).await?;
                progress(if kind == Kind::Forge { "Installing Forge" } else { "Installing NeoForge" }, 0, 0);
                let out = command(&java).arg("-jar").arg(&installer).arg("--installServer").current_dir(&data).output().await?;
                if !out.status.success() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let tail: Vec<&str> = text.lines().rev().take(5).collect();
                    bail!("The installer failed: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" / "));
                }
                let _ = std::fs::remove_file(&installer);
                let _ = std::fs::remove_file(data.join("installer.jar.log"));
                loader_version = Some(lv);
                forge_start(&data)?
            }
        };

        if let Some(inst) = &new.instance {
            let pack = serverpack::build(client, paths, inst, progress).await?;
            install_pack(client, &pack.bytes, &data, progress).await?;
        }

        let server = EasyServer {
            id: id.clone(),
            name: name.clone(),
            kind,
            game_version: game.clone(),
            loader_version,
            port: free_port(paths),
            memory_mb: new.memory_mb.clamp(1024, 65536),
            instance: new.instance.clone(),
            created_at: chrono::Utc::now(),
            java_component: component,
            start,
        };
        std::fs::write(data.join("eula.txt"), "# Agreed in Tessera Launcher (https://aka.ms/MinecraftEULA)\neula=true\n")?;
        write_properties(&data, &[("server-port", server.port.to_string()), ("motd", name.clone()), ("enable-command-block", "false".into())])?;
        save(paths, &server)?;
        Ok(server)
    }
    .await;
    match made {
        Ok(s) => Ok(view(s)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(dir(paths, &id));
            Err(e)
        }
    }
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

pub fn read_properties(data: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(data.join("server.properties"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
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
            Some((k, _)) if !l.starts_with('#') => match set.iter().find(|(key, _)| *key == k.trim()) {
                Some((key, v)) => {
                    done.insert(*key);
                    format!("{key}={v}")
                }
                None => l.to_string(),
            },
            _ => l.to_string(),
        })
        .collect();
    for (k, v) in set {
        if !done.contains(k) {
            lines.push(format!("{k}={v}"));
        }
    }
    std::fs::write(file, lines.join("\n") + "\n")?;
    Ok(())
}

#[derive(Deserialize)]
pub struct Changes {
    pub name: Option<String>,
    pub memory_mb: Option<u32>,
    pub port: Option<u16>,
    pub max_players: Option<u32>,
    pub difficulty: Option<String>,
    pub gamemode: Option<String>,
    pub pvp: Option<bool>,
    pub whitelist: Option<bool>,
}

pub fn update(paths: &Paths, id: &str, c: Changes) -> Result<View> {
    let mut s = get(paths, id)?;
    let data = data_dir(paths, id);
    let mut props: Vec<(&str, String)> = Vec::new();
    if let Some(n) = c.name.map(|n| n.trim().chars().take(48).collect::<String>()).filter(|n| !n.is_empty()) {
        props.push(("motd", n.clone()));
        s.name = n;
    }
    if let Some(m) = c.memory_mb {
        s.memory_mb = m.clamp(1024, 65536);
    }
    if let Some(p) = c.port {
        if p < 1024 {
            bail!("Pick a port from 1024 up");
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
    write_properties(&data, &props)?;
    save(paths, &s)?;
    Ok(view(s))
}

pub fn delete(paths: &Paths, id: &str) -> Result<()> {
    if running().lock().unwrap().contains_key(id) {
        bail!("Stop the server first");
    }
    get(paths, id)?;
    std::fs::remove_dir_all(dir(paths, id))?;
    logs().lock().unwrap().remove(id);
    Ok(())
}

// ── running ──

/// `java` instead of `javaw` on Windows, so the server has a console to write to (no window
/// opens: see `command`).
fn console_java(java: &Path) -> PathBuf {
    let c = java.with_file_name(if cfg!(windows) { "java.exe" } else { "java" });
    if c.exists() {
        c
    } else {
        java.to_path_buf()
    }
}

fn command(program: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    // `cargo run` / `tauri dev` set these; inherited by Java they crash older runtimes on macOS
    c.env_remove("DYLD_FALLBACK_LIBRARY_PATH").env_remove("DYLD_LIBRARY_PATH");
    c.stdin(Stdio::null());
    c
}

/// Events for the UI: a log line, or the state / player list changed.
pub enum Event<'a> {
    Line(&'a str, &'a str),
    Changed(&'a str),
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

fn push_log(id: &str, line: &str, emit: &Emit) {
    let mut all = logs().lock().unwrap();
    let l = all.entry(id.to_string()).or_default();
    l.push_back(line.to_string());
    while l.len() > 3000 {
        l.pop_front();
    }
    drop(all);
    emit(Event::Line(id, line));
}

/// "<name> joined the game" / "<name> left the game" in a server log line.
fn player_event(line: &str) -> Option<(bool, String)> {
    let msg = line.rsplit_once("]: ").map(|(_, m)| m).unwrap_or(line).trim();
    let (name, joined) = if let Some(n) = msg.strip_suffix(" joined the game") {
        (n, true)
    } else if let Some(n) = msg.strip_suffix(" left the game") {
        (n, false)
    } else {
        return None;
    };
    let ok = (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| (joined, name.to_string()))
}

pub async fn start(client: &reqwest::Client, paths: &Paths, id: &str, emit: Emit) -> Result<()> {
    let s = get(paths, id)?;
    if running().lock().unwrap().contains_key(id) {
        return Ok(());
    }
    let java = console_java(&ensure_java(client, paths, &s.java_component, |_, _| {}).await?);
    let data = data_dir(paths, id);
    // the same address twice would fail late, with a Java stack trace
    if std::net::TcpListener::bind(("0.0.0.0", s.port)).is_err() {
        bail!("Port {} is in use by another program. Pick another port in the server's settings.", s.port);
    }
    let mut cmd = command(&java);
    cmd.arg(format!("-Xmx{}M", s.memory_mb)).arg(format!("-Xms{}M", (s.memory_mb / 2).max(512)));
    match &s.start {
        Start::Jar { file } => {
            cmd.arg("-jar").arg(file);
        }
        Start::Args { file } => {
            cmd.arg(format!("@{file}"));
        }
    }
    cmd.arg("nogui").current_dir(&data).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().context("starting Java")?;
    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel();
    running().lock().unwrap().insert(
        id.to_string(),
        Running { stdin: Arc::new(tokio::sync::Mutex::new(stdin)), state: "starting", players: HashSet::new(), kill: Some(kill_tx) },
    );
    logs().lock().unwrap().insert(id.to_string(), VecDeque::new());
    emit(Event::Changed(id));
    push_log(id, &format!("[Tessera] Starting {} ({:?} {})", s.name, s.kind, s.game_version), &emit);

    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|p| Box::new(p) as _)].into_iter().flatten() {
        let id = id.to_string();
        let emit = emit.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut changed = false;
                if let Some(r) = running().lock().unwrap().get_mut(&id) {
                    if r.state == "starting" && line.contains("Done (") && line.contains(")! For help") {
                        r.state = "running";
                        changed = true;
                    }
                    if let Some((joined, name)) = player_event(&line) {
                        changed = if joined { r.players.insert(name) } else { r.players.remove(&name) };
                    }
                }
                push_log(&id, &line, &emit);
                if changed {
                    emit(Event::Changed(&id));
                }
            }
        });
    }

    let id = id.to_string();
    tokio::spawn(async move {
        let code = tokio::select! {
            status = child.wait() => status.ok().and_then(|s| s.code()),
            _ = kill_rx => {
                let _ = child.kill().await;
                None
            }
        };
        running().lock().unwrap().remove(&id);
        push_log(&id, &format!("[Tessera] The server stopped{}", code.filter(|c| *c != 0).map(|c| format!(" (exit code {c})")).unwrap_or_default()), &emit);
        emit(Event::Changed(&id));
    });
    Ok(())
}

/// Types a console command (without the slash).
pub async fn send(id: &str, line: &str) -> Result<()> {
    let stdin = running().lock().unwrap().get(id).map(|r| r.stdin.clone()).ok_or_else(|| anyhow!("The server isn't running"))?;
    let mut s = stdin.lock().await;
    s.write_all(format!("{}\n", line.trim().trim_start_matches('/')).as_bytes()).await?;
    s.flush().await?;
    Ok(())
}

/// Saves and stops; kills it if it hasn't stopped after `grace`.
pub async fn stop(id: &str, grace: std::time::Duration) -> Result<()> {
    {
        let mut all = running().lock().unwrap();
        let r = all.get_mut(id).ok_or_else(|| anyhow!("The server isn't running"))?;
        r.state = "stopping";
    }
    let _ = send(id, "stop").await;
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if !running().lock().unwrap().contains_key(id) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    if let Some(k) = running().lock().unwrap().get_mut(id).and_then(|r| r.kill.take()) {
        let _ = k.send(());
    }
    Ok(())
}

/// When the launcher quits: every server saves and stops.
pub async fn stop_all() {
    let ids: Vec<String> = running().lock().unwrap().keys().cloned().collect();
    futures::future::join_all(ids.iter().map(|id| stop(id, std::time::Duration::from_secs(20)))).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_players_joining_and_leaving() {
        assert_eq!(player_event("[12:00:01] [Server thread/INFO]: Notch joined the game"), Some((true, "Notch".into())));
        assert_eq!(player_event("[12:00:01] [Server thread/INFO] [minecraft/MinecraftServer]: jeb_ left the game"), Some((false, "jeb_".into())));
        assert_eq!(player_event("[12:00:01] [Server thread/INFO]: <Notch> I joined the game"), None);
    }

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
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod real_server {
    use super::*;

    /// TESSERA_DATA=<launcher data folder> TESSERA_KIND=fabric|paper|vanilla|forge|neoforge TESSERA_GAME=1.21.1
    ///   cargo test real_easy_server -- --ignored --nocapture
    /// Makes, starts and stops a server in a temporary folder that borrows the data folder's
    /// `meta` (Java and game files), so nothing is downloaded twice.
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
                if l.contains("Done (") || l.contains("ERROR") || l.contains("[Tessera]") || l.contains("players online") {
                    println!("  | {l}");
                }
            }
        });
        start(&client, &paths, &id, emit).await.unwrap();
        let t0 = std::time::Instant::now();
        while get_view(&paths, &id).unwrap().state != "running" {
            assert!(running().lock().unwrap().contains_key(&id), "the server stopped while starting:\n{}", log(&id).join("\n"));
            assert!(t0.elapsed().as_secs() < 300, "not up after 5 minutes");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let mods = std::fs::read_dir(data_dir(&paths, &id).join("mods")).map(|r| r.count()).unwrap_or(0);
        println!("mods on the server: {mods}");
        println!("up after {}s, LAN address {:?}", t0.elapsed().as_secs(), get_view(&paths, &id).unwrap().lan_address);
        send(&id, "/list").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        stop(&id, std::time::Duration::from_secs(60)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        assert_eq!(get_view(&paths, &id).unwrap().state, "stopped");
        assert!(data_dir(&paths, &id).join("world").exists() || data_dir(&paths, &id).join("world").is_dir(), "no world saved");
        delete(&paths, &id).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        println!("stopped and deleted");
    }
}
