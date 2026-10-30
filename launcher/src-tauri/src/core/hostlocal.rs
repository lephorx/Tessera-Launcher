//! "Host on this computer": installs the Tessera server manager here with one click, the
//! same way its install script does (github.com/lephorx/Tessera, scripts/install.sh):
//! clone the repo, write `.env`, `docker compose up --build -d`. Its first-run setup (your
//! account with an authenticator app, then Playit/Cloudflare) happens in Tessera itself.
//!
//! Works on macOS, Windows and Linux. Docker Desktop is installed for you when it's missing
//! (macOS and Windows; Linux needs Docker Engine). Tessera is downloaded with git inside a
//! container, so no git is needed here (and Windows line endings can't break its scripts).
//! On Windows the Docker daemon runs in a Linux VM, so the data folders are handed to it as
//! /run/desktop/mnt/host/c/… paths.

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

/// Apps started from the Dock or Start menu may not have Docker on their PATH yet (right
/// after installing it, for example); add where it lives. No console windows on Windows.
fn command(program: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    let path = std::env::var("PATH").unwrap_or_default();
    #[cfg(windows)]
    {
        c.env("PATH", format!("{path};C:\\Program Files\\Docker\\Docker\\resources\\bin"));
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    c.env("PATH", format!("{path}:/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin"));
    c.stdin(Stdio::null());
    c
}

fn home() -> Result<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var(var).map(PathBuf::from).context("no home folder")
}

/// A folder as the Docker daemon sees it. Docker Desktop on Windows runs the daemon in a
/// Linux VM that has the Windows drives under /run/desktop/mnt/host/<drive>.
pub fn daemon_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let b = s.as_bytes();
    if cfg!(windows) && b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return format!("/run/desktop/mnt/host/{}/{}", (b[0] as char).to_ascii_lowercase(), s[2..].trim_start_matches('/'));
    }
    s
}

async fn docker_ready() -> bool {
    output("docker", &["info", "--format", "{{.ServerVersion}}"]).await.is_some_and(|v| !v.is_empty())
}

async fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = command(program).args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub async fn status(client: &reqwest::Client, paths: &Paths) -> Status {
    let supported = cfg!(any(target_os = "macos", target_os = "linux", windows));
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

/// Opens Docker Desktop and waits up to three minutes for it to be ready (the first start
/// asks you to accept Docker's terms).
pub async fn start_docker() -> Result<()> {
    if docker_ready().await {
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        command("open").args(["-a", "Docker"]).status().await.context("opening Docker Desktop")?;
    } else if cfg!(windows) {
        let exe = r"C:\Program Files\Docker\Docker\Docker Desktop.exe";
        command(exe).spawn().context("opening Docker Desktop")?;
    }
    for _ in 0..90 {
        if docker_ready().await {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    bail!("Docker didn't start yet. If Docker Desktop shows its terms, accept them; then press Install again.")
}

/// Downloads and installs Docker Desktop (macOS: into Applications; Windows: its installer,
/// which asks once for admin rights). Linux needs Docker Engine from the distribution.
pub async fn install_docker(client: &reqwest::Client, log: &(dyn Fn(&str) + Send + Sync)) -> Result<()> {
    let url = if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "https://desktop.docker.com/mac/main/arm64/Docker.dmg" } else { "https://desktop.docker.com/mac/main/amd64/Docker.dmg" }
    } else if cfg!(windows) {
        "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe"
    } else {
        bail!("Install Docker Engine for your Linux distribution (docs.docker.com/engine/install), then press Install again.");
    };
    let file = std::env::temp_dir().join(if cfg!(windows) { "DockerDesktopInstaller.exe" } else { "Docker.dmg" });
    download(client, url, &file, log).await.context("downloading Docker Desktop")?;

    if cfg!(target_os = "macos") {
        log("Installing Docker Desktop into Applications…");
        let mount = std::env::temp_dir().join("tessera-docker-dmg");
        let _ = std::fs::create_dir_all(&mount);
        let ok = command("hdiutil").arg("attach").arg(&file).args(["-nobrowse", "-quiet", "-mountpoint"]).arg(&mount).status().await?.success();
        if !ok {
            bail!("Couldn't open the Docker Desktop download");
        }
        let copied = command("ditto").arg(mount.join("Docker.app")).arg("/Applications/Docker.app").status().await;
        let _ = command("hdiutil").arg("detach").arg(&mount).arg("-quiet").status().await;
        if !copied.is_ok_and(|s| s.success()) {
            bail!("Couldn't copy Docker into Applications. Install Docker Desktop yourself from docker.com, then press Install again.");
        }
    } else {
        log("Installing Docker Desktop. Windows asks once for permission; choose Yes.");
        let ps = format!(
            "$p = Start-Process -FilePath '{}' -ArgumentList 'install','--quiet','--accept-license','--backend=wsl-2' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
            file.display()
        );
        let status = command("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &ps]).status().await?;
        if !status.success() {
            bail!("Docker Desktop wasn't installed (the permission prompt was declined, or the installer failed). Press Install to try again.");
        }
    }
    let _ = std::fs::remove_file(&file);
    log("Docker Desktop is installed. Starting it…");
    start_docker().await.map_err(|_| {
        anyhow!(if cfg!(windows) {
            "Docker Desktop is installed. Restart your computer once (Windows needs it for Docker), then open the launcher and press Install again."
        } else {
            "Docker Desktop is installed. Open it once, accept its terms, then press Install again."
        })
    })
}

/// Streams a big download to a file, logging progress every 50 MB.
async fn download(client: &reqwest::Client, url: &str, to: &Path, log: &(dyn Fn(&str) + Send + Sync)) -> Result<()> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;
    let res = client.get(url).timeout(std::time::Duration::from_secs(3600)).send().await?.error_for_status()?;
    let total = res.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(to).await?;
    let mut stream = res.bytes_stream();
    let (mut done, mut next) = (0u64, 0u64);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if done >= next {
            log(&format!("Downloading Docker Desktop: {} of {} MB", done / 1_000_000, total / 1_000_000));
            next += 50_000_000;
        }
    }
    file.flush().await?;
    Ok(())
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
    if let Some(existing) = installed(paths) {
        if existing.dir.join("docker-compose.yml").exists() {
            bail!("Tessera is already installed in {}", existing.dir.display());
        }
    }

    step("docker");
    if !command("docker").arg("--version").output().await.is_ok_and(|o| o.status.success()) {
        install_docker(client, log).await?;
    }
    if !docker_ready().await {
        log("Starting Docker Desktop…");
        start_docker().await?;
    }
    if output("docker", &["compose", "version"]).await.is_none() {
        bail!("Docker Compose is missing. Update Docker Desktop and try again.");
    }

    step("download");
    let home = home()?;
    let dir = home.join("tessera-server");
    let data = home.join("tessera-server-data");
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        bail!("{} already exists and isn't empty. Move it away (it may hold another Tessera) and try again.", dir.display());
    }
    // git runs in a container: nothing to install here, and no Windows line endings
    let parent = dir.parent().ok_or_else(|| anyhow!("no parent folder"))?;
    let mut clone = command("docker");
    clone.args(["run", "--rm", "-v"]).arg(format!("{}:/work", parent.display())).args(["-w", "/work", "-e", "HOME=/tmp"]);
    #[cfg(unix)]
    {
        // files belong to you, not root (matters on Linux)
        let (uid, gid) = (own_ids().0, own_ids().1);
        clone.args(["--user", &format!("{uid}:{gid}")]);
    }
    clone.args(["alpine/git", "clone", "--depth", "1", REPO, "tessera-server"]);
    run_logged(clone, log).await.context("downloading Tessera")?;
    for sub in ["servers", "backups", "data"] {
        std::fs::create_dir_all(data.join(sub))?;
    }
    let port = free_port();
    let env = [
        "# Tessera settings, written by Tessera Launcher. Playit and Cloudflare are set up in the\n# dashboard after you create your account.\n".to_string(),
        env_line("WEB_PORT", &port.to_string()),
        env_line("DATA_DIR_HOST", &daemon_path(&data.join("data"))),
        env_line("SERVERS_DIR_HOST", &daemon_path(&data.join("servers"))),
        env_line("BACKUPS_DIR_HOST", &daemon_path(&data.join("backups"))),
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

/// This user's uid and gid, from the folder they own.
#[cfg(unix)]
fn own_ids() -> (u32, u32) {
    use std::os::unix::fs::MetadataExt;
    home().ok().and_then(|h| std::fs::metadata(h).ok()).map(|m| (m.uid(), m.gid())).unwrap_or((1000, 1000))
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
    fn daemon_paths() {
        let p = Path::new("/Users/ramon/tessera-server-data/servers");
        assert_eq!(daemon_path(p), "/Users/ramon/tessera-server-data/servers");
        #[cfg(windows)]
        assert_eq!(daemon_path(Path::new(r"C:\Users\Ramon\tessera-server-data\servers")), "/run/desktop/mnt/host/c/Users/Ramon/tessera-server-data/servers");
    }

    #[test]
    fn env_lines_like_install_sh() {
        assert_eq!(env_line("WEB_PORT", "3000"), "WEB_PORT='3000'\n");
        assert_eq!(env_line("X", "it's"), "X='it\\'s'\n");
    }
}
