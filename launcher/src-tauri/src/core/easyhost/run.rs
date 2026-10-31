//! Running a server. It's its own process, not tied to the launcher: it keeps running when
//! the launcher quits, and the next launcher finds it again (`adopt`). So nothing goes
//! through pipes: the server writes its console to `console.log`, which the launcher reads,
//! and commands go over RCON on 127.0.0.1 with a password made at each start. `run.json`
//! remembers the process (pid, start time) and the RCON port and password.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

use super::super::java::ensure_java;
use super::super::paths::Paths;
use super::super::rcon;
use super::{data_dir, dir, get, players, write_properties, Start};

/// What `run.json` keeps about a started server.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(super) struct RunFile {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub rcon_port: u16,
    pub rcon_password: String,
    /// How far into `console.log` player events were counted (so a launcher that finds the
    /// server again doesn't count them twice)
    #[serde(default)]
    pub counted: u64,
}

fn run_file(paths: &Paths, id: &str) -> PathBuf {
    dir(paths, id).join("run.json")
}

fn console_file(paths: &Paths, id: &str) -> PathBuf {
    dir(paths, id).join("console.log")
}

fn read_run(paths: &Paths, id: &str) -> Option<RunFile> {
    serde_json::from_slice(&std::fs::read(run_file(paths, id)).ok()?).ok()
}

fn write_run(paths: &Paths, id: &str, run: &RunFile) -> Result<()> {
    // holds the RCON password
    crate::core::account::write_private(&run_file(paths, id), &serde_json::to_vec(run)?)?;
    Ok(())
}

pub(super) struct Running {
    pub run: RunFile,
    /// "starting" until the server says "Done", then "running"; "stopping" after stop
    pub state: &'static str,
    /// online now, with when they joined
    pub players: HashMap<String, DateTime<Utc>>,
    /// CPU (100 = one core) and memory, from the process
    pub cpu: f32,
    pub memory: u64,
}

pub(super) fn running() -> &'static Mutex<HashMap<String, Running>> {
    static R: std::sync::OnceLock<Mutex<HashMap<String, Running>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

fn logs() -> &'static Mutex<HashMap<String, VecDeque<String>>> {
    static L: std::sync::OnceLock<Mutex<HashMap<String, VecDeque<String>>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

pub fn log(id: &str) -> Vec<String> {
    logs().lock().unwrap().get(id).map(|l| l.iter().cloned().collect()).unwrap_or_default()
}

pub fn is_running(id: &str) -> bool {
    running().lock().unwrap().contains_key(id)
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

// ── processes ──

/// `java` instead of `javaw` on Windows, so the server has a console to write to (no window
/// opens: see `command`).
pub(super) fn console_java(java: &Path) -> PathBuf {
    let c = java.with_file_name(if cfg!(windows) { "java.exe" } else { "java" });
    if c.exists() {
        c
    } else {
        java.to_path_buf()
    }
}

pub(super) fn command(program: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000 | 0x0000_0200); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    // its own process group: Ctrl+C in a terminal running the launcher doesn't reach it
    #[cfg(unix)]
    c.process_group(0);
    // `cargo run` / `tauri dev` set these; inherited by Java they crash older runtimes on macOS
    c.env_remove("DYLD_FALLBACK_LIBRARY_PATH").env_remove("DYLD_LIBRARY_PATH");
    c.stdin(Stdio::null());
    c
}

fn system() -> Option<System> {
    System::new().ok()
}

/// The process is still the server: alive, and started when `run.json` says (a pid can be
/// reused by another program after a reboot).
fn alive(sys: &mut System, run: &RunFile) -> bool {
    let pid = Pid::from_u32(run.pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_cpu().with_memory());
    match sys.process(pid) {
        Some(p) => !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead) && (p.start_time() as i64 - run.started_at.timestamp()).abs() < 120,
        None => false,
    }
}

fn kill_pid(pid: u32) {
    let Some(mut sys) = system() else { return };
    let pid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(p) = sys.process(pid) {
        p.kill();
    }
}

/// Any process whose working folder is in `data`: a server the launcher lost track of
/// still holds its files (on Windows they can't be deleted then).
fn kill_strays(data: &Path) -> usize {
    let Some(mut sys) = system() else { return 0 };
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always));
    let data = std::fs::canonicalize(data).unwrap_or_else(|_| data.to_path_buf());
    let mut n = 0;
    for p in sys.processes().values() {
        let Some(cwd) = p.cwd() else { continue };
        let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        if cwd.starts_with(&data) && p.kill() {
            n += 1;
        }
    }
    n
}

/// Nothing listens on the port. Binding alone isn't enough: on macOS 127.0.0.1 binds while
/// another program listens on all addresses.
pub(super) fn port_free(port: u16) -> bool {
    let answers = std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok();
    !answers && std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Free from 25575 (Minecraft's RCON port): not another server's, and nothing listens.
fn rcon_port() -> u16 {
    let taken: Vec<u16> = running().lock().unwrap().values().map(|r| r.run.rcon_port).collect();
    (25575..25800).find(|p| !taken.contains(p) && port_free(*p)).unwrap_or(25575)
}

fn password() -> Result<String> {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).map_err(|e| anyhow!("{e}"))?;
    Ok(hex::encode(b))
}

pub async fn start(client: &reqwest::Client, paths: &Paths, id: &str, emit: Emit) -> Result<()> {
    let s = get(paths, id)?;
    if is_running(id) {
        return Ok(());
    }
    // still running from before (found before `adopt` got to it)
    if adopt_one(paths, id, &emit) {
        return Ok(());
    }
    let java = console_java(&ensure_java(client, paths, &s.java_component, |_, _| {}).await?);
    let data = data_dir(paths, id);
    // the same address twice would fail late, with a Java stack trace
    if !port_free(s.port) {
        bail!("Port {} is in use by another program. Pick another port in the server's settings.", s.port);
    }
    let rcon_port = rcon_port();
    let rcon_password = password()?;
    write_properties(
        &data,
        &[
            ("server-port", s.port.to_string()),
            ("enable-rcon", "true".into()),
            ("rcon.port", rcon_port.to_string()),
            ("rcon.password", rcon_password.clone()),
            ("broadcast-rcon-to-ops", "false".into()),
        ],
    )?;

    let file = console_file(paths, id);
    let mut out = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&file)?;
    writeln!(out, "[Tessera] Starting {} ({:?} {})", s.name, s.kind, s.game_version)?;
    let err = out.try_clone()?;

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
    cmd.arg("nogui").current_dir(&data).stdout(Stdio::from(out)).stderr(Stdio::from(err));
    let child = cmd.spawn().context("starting Java")?;
    let pid = child.id().ok_or_else(|| anyhow!("Java exited right away"))?;
    let run = RunFile { pid, started_at: Utc::now(), rcon_port, rcon_password, counted: 0 };
    write_run(paths, id, &run)?;
    players::started(paths, id, run.started_at);
    running().lock().unwrap().insert(id.to_string(), Running { run, state: "starting", players: HashMap::new(), cpu: 0.0, memory: 0 });
    logs().lock().unwrap().insert(id.to_string(), VecDeque::new());
    emit(Event::Changed(id));
    tokio::spawn(watch(paths.clone(), id.to_string(), Some(child), 0, emit));
    Ok(())
}

/// Servers still running from an earlier launcher: picks them up again. At launcher start.
pub fn adopt_all(paths: &Paths, emit: Emit) {
    for e in std::fs::read_dir(super::root(paths)).into_iter().flatten().flatten() {
        adopt_one(paths, &e.file_name().to_string_lossy(), &emit);
    }
}

fn adopt_one(paths: &Paths, id: &str, emit: &Emit) -> bool {
    if is_running(id) {
        return true;
    }
    let Some(run) = read_run(paths, id) else { return false };
    if !system().is_some_and(|mut sys| alive(&mut sys, &run)) {
        // it stopped while the launcher was closed
        finish_counting(paths, id, &run);
        let _ = std::fs::remove_file(run_file(paths, id));
        return false;
    }
    running().lock().unwrap().insert(id.to_string(), Running { run: run.clone(), state: "starting", players: HashMap::new(), cpu: 0.0, memory: 0 });
    logs().lock().unwrap().insert(id.to_string(), VecDeque::new());
    // what happened so far: who's on, whether it's up; player events only from where the
    // last launcher stopped counting
    let mut offset = 0;
    let mut partial = String::new();
    for (line, end) in read_new(&console_file(paths, id), &mut offset, &mut partial) {
        on_line(paths, id, &line, end > run.counted, emit);
    }
    remember_counted(paths, id, offset);
    emit(Event::Changed(id));
    tokio::spawn(watch(paths.clone(), id.to_string(), None, offset, emit.clone()));
    true
}

/// Player events after the last count, for a server that stopped while the launcher was
/// closed; then everyone still "online" left when it stopped.
fn finish_counting(paths: &Paths, id: &str, run: &RunFile) {
    let file = console_file(paths, id);
    let mut offset = 0;
    let mut partial = String::new();
    for (line, end) in read_new(&file, &mut offset, &mut partial) {
        if end > run.counted {
            if let Some(ev) = players::event(&line) {
                players::record(paths, id, &ev, players::line_time(&line, Utc::now()));
            }
        }
    }
    let at = std::fs::metadata(&file).and_then(|m| m.modified()).map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now());
    players::stopped(paths, id, at);
}

fn remember_counted(paths: &Paths, id: &str, offset: u64) {
    let run = {
        let mut all = running().lock().unwrap();
        let Some(r) = all.get_mut(id) else { return };
        r.run.counted = offset;
        r.run.clone()
    };
    let _ = write_run(paths, id, &run);
}

/// New whole lines in the console file since `offset`, each with the offset of its end.
fn read_new(file: &Path, offset: &mut u64, partial: &mut String) -> Vec<(String, u64)> {
    let Ok(mut f) = std::fs::File::open(file) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len < *offset {
        // started again: a new file
        *offset = 0;
        partial.clear();
    }
    if len == *offset || f.seek(SeekFrom::Start(*offset)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.take(4 << 20).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let start = *offset - partial.len() as u64;
    *offset += buf.len() as u64;
    partial.push_str(&String::from_utf8_lossy(&buf));
    let mut out = Vec::new();
    let mut pos = start;
    while let Some(i) = partial.find('\n') {
        let line: String = partial.drain(..=i).collect();
        pos += line.len() as u64;
        out.push((line.trim_end_matches(['\r', '\n']).to_string(), pos));
    }
    out
}

/// One console line: whether it's up, who joined or left, and the player history.
fn on_line(paths: &Paths, id: &str, line: &str, count: bool, emit: &Emit) {
    let mut changed = false;
    let ev = players::event(line);
    if let Some(r) = running().lock().unwrap().get_mut(id) {
        if r.state == "starting" && line.contains("Done (") && line.contains(")! For help") {
            r.state = "running";
            changed = true;
        }
        match &ev {
            Some(players::LogEvent::Joined(name)) => changed |= r.players.insert(name.clone(), players::line_time(line, Utc::now())).is_none(),
            Some(players::LogEvent::Left(name)) => changed |= r.players.remove(name).is_some(),
            _ => {}
        }
    }
    if count {
        if let Some(ev) = &ev {
            players::record(paths, id, ev, players::line_time(line, Utc::now()));
        }
    }
    push_log(id, line, emit);
    if changed {
        emit(Event::Changed(id));
    }
}

/// Follows the server until it stops: its console, whether it's alive, CPU and memory.
async fn watch(paths: Paths, id: String, mut child: Option<tokio::process::Child>, mut offset: u64, emit: Emit) {
    let file = console_file(&paths, &id);
    let mut partial = String::new();
    let mut sys = system();
    let mut tick = 0u32;
    let mut code = None;
    loop {
        let lines = read_new(&file, &mut offset, &mut partial);
        let had = !lines.is_empty();
        for (line, _) in lines {
            on_line(&paths, &id, &line, true, &emit);
        }
        if had && tick.is_multiple_of(10) {
            remember_counted(&paths, &id, offset);
        }
        // forgotten (force-closed for deleting): nothing to tidy up
        let Some(run) = running().lock().unwrap().get(&id).map(|r| r.run.clone()) else { return };
        let gone = match &mut child {
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => {
                    code = status.code();
                    true
                }
                Ok(None) => false,
                Err(_) => true,
            },
            None => tick.is_multiple_of(3) && !sys.as_mut().is_some_and(|s| alive(s, &run)),
        };
        if gone {
            tokio::time::sleep(Duration::from_millis(200)).await;
            for (line, _) in read_new(&file, &mut offset, &mut partial) {
                on_line(&paths, &id, &line, true, &emit);
            }
            break;
        }
        // CPU and memory every 2 seconds
        if tick.is_multiple_of(7) {
            if let Some(s) = sys.as_mut() {
                let pid = Pid::from_u32(run.pid);
                s.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_cpu().with_memory());
                if let Some(p) = s.process(pid) {
                    if let Some(r) = running().lock().unwrap().get_mut(&id) {
                        r.cpu = p.cpu_usage();
                        r.memory = p.memory();
                    }
                }
            }
        }
        // some servers print "Done" differently: RCON answering means it's up too
        if tick % 15 == 14 && running().lock().unwrap().get(&id).is_some_and(|r| r.state == "starting") && rcon::ready(run.rcon_port, &run.rcon_password).await {
            if let Some(r) = running().lock().unwrap().get_mut(&id) {
                r.state = "running";
            }
            emit(Event::Changed(&id));
        }
        tick = tick.wrapping_add(1);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    running().lock().unwrap().remove(&id);
    let _ = std::fs::remove_file(run_file(&paths, &id));
    players::stopped(&paths, &id, Utc::now());
    push_log(&id, &format!("[Tessera] The server stopped{}", code.filter(|c| *c != 0).map(|c| format!(" (exit code {c})")).unwrap_or_default()), &emit);
    emit(Event::Changed(&id));
}

fn rcon_of(id: &str) -> Result<(u16, String)> {
    running().lock().unwrap().get(id).map(|r| (r.run.rcon_port, r.run.rcon_password.clone())).ok_or_else(|| anyhow!("The server isn't running"))
}

/// Runs a command (without the slash) and returns the server's answer.
pub async fn command_quiet(id: &str, line: &str) -> Result<String> {
    let (port, pw) = rcon_of(id)?;
    rcon::command(port, &pw, line.trim().trim_start_matches('/')).await
}

/// A command typed in the console: it and the answer show in the console.
pub async fn send(id: &str, line: &str, emit: &Emit) -> Result<()> {
    let line = line.trim().trim_start_matches('/');
    push_log(id, &format!("> {line}"), emit);
    let answer = command_quiet(id, line).await?;
    for l in answer.lines().filter(|l| !l.trim().is_empty()) {
        push_log(id, l, emit);
    }
    Ok(())
}

/// Saves and stops; kills it if it hasn't stopped after `grace`.
pub async fn stop(id: &str, grace: Duration) -> Result<()> {
    let run = {
        let mut all = running().lock().unwrap();
        let r = all.get_mut(id).ok_or_else(|| anyhow!("The server isn't running"))?;
        r.state = "stopping";
        r.run.clone()
    };
    let deadline = std::time::Instant::now() + grace;
    let mut asked = false;
    while std::time::Instant::now() < deadline {
        if !is_running(id) {
            return Ok(());
        }
        if !asked {
            asked = rcon::command(run.rcon_port, &run.rcon_password, "stop").await.is_ok();
            // still starting (no RCON yet): Minecraft saves and stops on SIGTERM too
            #[cfg(unix)]
            if !asked {
                if let Some(mut sys) = system() {
                    let pid = Pid::from_u32(run.pid);
                    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                    asked = sys.process(pid).and_then(|p| p.kill_with(sysinfo::Signal::Term)).unwrap_or(false);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    kill(id);
    Ok(())
}

/// Ends it at once, without saving.
pub fn kill(id: &str) {
    if let Some(pid) = running().lock().unwrap().get(id).map(|r| r.run.pid) {
        kill_pid(pid);
    }
}

/// Stops it however it takes, and anything else holding its files, so it can be deleted.
pub async fn force_stop(paths: &Paths, id: &str) {
    if is_running(id) {
        let _ = stop(id, Duration::from_secs(10)).await;
        for _ in 0..20 {
            if !is_running(id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    if let Some(run) = read_run(paths, id) {
        kill_pid(run.pid);
    }
    if kill_strays(&dir(paths, id)) > 0 {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    running().lock().unwrap().remove(id);
}

pub fn forget(id: &str) {
    logs().lock().unwrap().remove(id);
}

/// Deletes a folder; Windows keeps files locked for a moment after their program ends.
pub async fn remove_dir_retrying(path: &Path) -> Result<()> {
    let mut last = None;
    for i in 0..10 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        if i == 4 {
            kill_strays(path);
        }
        tokio::time::sleep(Duration::from_millis(300 * (i + 1))).await;
    }
    Err(last.map(anyhow::Error::from).unwrap_or_else(|| anyhow!("couldn't delete"))).with_context(|| format!("deleting {}", path.display()))
}

/// CPU, memory and uptime of a running server.
pub fn usage(id: &str) -> Option<(f32, u64, DateTime<Utc>)> {
    running().lock().unwrap().get(id).map(|r| (r.cpu, r.memory, r.run.started_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_whole_lines_as_they_come() {
        let file = std::env::temp_dir().join(format!("tessera-console-{}.log", std::process::id()));
        std::fs::write(&file, "one\ntw").unwrap();
        let (mut offset, mut partial) = (0, String::new());
        assert_eq!(read_new(&file, &mut offset, &mut partial), vec![("one".to_string(), 4)]);
        std::fs::write(&file, "one\ntwo\r\nthree\n").unwrap();
        assert_eq!(read_new(&file, &mut offset, &mut partial), vec![("two".to_string(), 9), ("three".to_string(), 15)]);
        // a new start: a shorter file
        std::fs::write(&file, "a\n").unwrap();
        assert_eq!(read_new(&file, &mut offset, &mut partial), vec![("a".to_string(), 2)]);
        let _ = std::fs::remove_file(file);
    }
}
