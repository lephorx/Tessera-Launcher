//! "Host on this computer": installs the Tessera server manager here with one click, the
//! same way its install script does (github.com/lephorx/Tessera, scripts/install.sh):
//! clone the repo, write `.env`, `docker compose up --build -d`. Its first-run setup (your
//! account with an authenticator app, then Playit/Cloudflare) happens in Tessera itself.
//!
//! Needs Docker (Docker Desktop on macOS) and git. Windows isn't supported by Tessera's
//! installer yet (it runs in WSL 2).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::paths::Paths;

const REPO: &str = "https://github.com/lephorx/Tessera.git";

/// Where this computer's Tessera lives, remembered after installing.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocalServer {
    pub dir: PathBuf,
    pub data_dir: PathBuf,
    pub port: u16,
}

#[derive(Serialize, Debug, Default)]
pub struct Status {
    pub supported: bool,
    pub docker_installed: bool,
    pub docker_running: bool,
    pub docker_version: Option<String>,
    pub git: bool,
    pub installed: Option<LocalServer>,
    /// The dashboard answers on its port.
    pub reachable: bool,
    /// Tessera still needs its first-run setup (no account yet).
    pub setup_required: bool,
}

fn record(paths: &Paths) -> PathBuf {
    paths.file("local-server.json")
}

pub fn installed(paths: &Paths) -> Option<LocalServer> {
    std::fs::read(record(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Apps started from the Dock don't get the shell's PATH; add where Docker and Homebrew live.
fn command(program: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    let path = std::env::var("PATH").unwrap_or_default();
    c.env("PATH", format!("{path}:/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin"));
    c.stdin(Stdio::null());
    c
}

async fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = command(program).args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub async fn status(client: &reqwest::Client, paths: &Paths) -> Status {
    let supported = cfg!(any(target_os = "macos", target_os = "linux"));
    let docker_installed = command("docker").arg("--version").output().await.is_ok_and(|o| o.status.success());
    let docker_version = output("docker", &["info", "--format", "{{.ServerVersion}}"]).await.filter(|v| !v.is_empty());
    let git = output("git", &["--version"]).await.is_some();
    let installed = installed(paths);
    let mut s = Status { supported, docker_installed, docker_running: docker_version.is_some(), docker_version, git, installed: installed.clone(), ..Default::default() };
    if let Some(local) = installed {
        if let Ok(res) = client.get(format!("http://localhost:{}/api/auth/status", local.port)).timeout(std::time::Duration::from_secs(4)).send().await {
            if let Ok(body) = res.json::<serde_json::Value>().await {
                s.reachable = true;
                s.setup_required = body["setup_required"].as_bool().unwrap_or(false);
            }
        }
    }
    s
}

/// Opens Docker Desktop (macOS) and waits up to two minutes for it to be ready.
pub async fn start_docker() -> Result<()> {
    if cfg!(target_os = "macos") {
        command("open").args(["-a", "Docker"]).status().await.context("opening Docker Desktop")?;
    }
    for _ in 0..60 {
        if output("docker", &["info", "--format", "{{.ServerVersion}}"]).await.is_some_and(|v| !v.is_empty()) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    bail!("Docker didn't start. Open Docker Desktop, accept its terms if it asks, and try again.")
}

/// The first free port from 3000 on, for the dashboard.
fn free_port() -> u16 {
    (3000..3100).find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok()).unwrap_or(3000)
}

/// A dotenv line the way install.sh writes them: single-quoted, ' escaped.
fn env_line(key: &str, value: &str) -> String {
    format!("{key}='{}'\n", value.replace('\'', "\\'"))
}

/// Runs a command and passes each output line to `log`; fails with the last lines.
async fn run_logged(mut cmd: tokio::process::Command, log: &(dyn Fn(&str) + Send + Sync)) -> Result<()> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().context("starting the command")?;
    let out = child.stdout.take().ok_or_else(|| anyhow!("no output"))?;
    let err = child.stderr.take().ok_or_else(|| anyhow!("no output"))?;
    let mut lines = BufReader::new(out).lines();
    let mut errs = BufReader::new(err).lines();
    let mut tail: Vec<String> = Vec::new();
    let mut push = |line: String| {
        log(&line);
        tail.push(line);
        if tail.len() > 8 {
            tail.remove(0);
        }
    };
    let (mut out_done, mut err_done) = (false, false);
    while !(out_done && err_done) {
        tokio::select! {
            l = lines.next_line(), if !out_done => match l? { Some(l) => push(l), None => out_done = true },
            l = errs.next_line(), if !err_done => match l? { Some(l) => push(l), None => err_done = true },
        }
    }
    let status = child.wait().await?;
    if !status.success() {
        bail!("{}", tail.join("\n"));
    }
    Ok(())
}

/// The whole installation. `step(name)` marks where it is, `log(line)` streams output.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    step: &(dyn Fn(&str) + Send + Sync),
    log: &(dyn Fn(&str) + Send + Sync),
) -> Result<LocalServer> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        bail!("Hosting on this computer needs macOS or Linux for now. On Windows, install Tessera in WSL 2 and add it as another machine.");
    }
    if let Some(existing) = installed(paths) {
        if existing.dir.join("docker-compose.yml").exists() {
            bail!("Tessera is already installed in {}", existing.dir.display());
        }
    }

    step("docker");
    if output("git", &["--version"]).await.is_none() {
        bail!("git is missing. On macOS, run `xcode-select --install` once, then try again.");
    }
    if !command("docker").arg("--version").output().await.is_ok_and(|o| o.status.success()) {
        bail!("Docker isn't installed. Install Docker Desktop, open it once, then try again.");
    }
    if output("docker", &["info", "--format", "{{.ServerVersion}}"]).await.is_none_or(|v| v.is_empty()) {
        log("Starting Docker Desktop…");
        start_docker().await?;
    }
    if output("docker", &["compose", "version"]).await.is_none() {
        bail!("Docker Compose is missing. Update Docker Desktop and try again.");
    }

    step("download");
    let home = std::env::var("HOME").map(PathBuf::from).context("no home folder")?;
    let dir = home.join("tessera-server");
    let data = home.join("tessera-server-data");
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        bail!("{} already exists and isn't empty. Move it away (it may hold another Tessera) and try again.", dir.display());
    }
    let mut clone = command("git");
    clone.args(["clone", "--depth", "1", REPO]).arg(&dir);
    run_logged(clone, log).await.context("downloading Tessera")?;
    for sub in ["servers", "backups", "data"] {
        std::fs::create_dir_all(data.join(sub))?;
    }
    let port = free_port();
    let env = [
        "# Tessera settings, written by Tessera Launcher. Playit and Cloudflare are set up in the\n# dashboard after you create your account.\n".to_string(),
        env_line("WEB_PORT", &port.to_string()),
        env_line("DATA_DIR_HOST", &data.join("data").to_string_lossy()),
        env_line("SERVERS_DIR_HOST", &data.join("servers").to_string_lossy()),
        env_line("BACKUPS_DIR_HOST", &data.join("backups").to_string_lossy()),
        env_line("PLAYIT_HEADLESS", "true"),
        env_line("JWT_LIFETIME_HOURS", "8"),
        env_line("TOTP_ISSUER", "Tessera"),
    ]
    .concat();
    write_private(&dir.join(".env"), &env)?;
    let local = LocalServer { dir: dir.clone(), data_dir: data, port };
    // remember it now, so a failed start can be retried without downloading again
    std::fs::write(record(paths), serde_json::to_vec_pretty(&local)?)?;

    step("start");
    start(&local, log).await?;

    step("ready");
    wait_ready(client, port).await?;
    Ok(local)
}

/// `docker compose up --build -d` in the install folder (also "Try again" after a failure).
pub async fn start(local: &LocalServer, log: &(dyn Fn(&str) + Send + Sync)) -> Result<()> {
    let mut up = command("docker");
    up.args(["compose", "up", "--build", "-d"]).current_dir(&local.dir);
    run_logged(up, log).await.context("starting Tessera")
}

pub async fn wait_ready(client: &reqwest::Client, port: u16) -> Result<()> {
    for _ in 0..90 {
        if client.get(format!("http://localhost:{port}/api/auth/status")).timeout(std::time::Duration::from_secs(3)).send().await.is_ok_and(|r| r.status().is_success()) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    bail!("Tessera started but doesn't answer on port {port} yet. Give it a minute and press Check again.")
}

#[cfg(unix)]
fn write_private(file: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(file)?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(file: &Path, text: &str) -> Result<()> {
    std::fs::write(file, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_lines_like_install_sh() {
        assert_eq!(env_line("WEB_PORT", "3000"), "WEB_PORT='3000'\n");
        assert_eq!(env_line("X", "it's"), "X='it\\'s'\n");
    }
}
