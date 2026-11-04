//! Horizon (Tessera Launcher Nightly only): Minecraft: Java Edition 26.3 rewritten in Rust
//! (github.com/horizon-rust/horizon, MIT). This follows Horizon's Launch API for other
//! launchers (docs/api/launch.md and launch-guide.md there):
//!
//! 1. a build is a GitHub release of horizon-rust/horizon, one archive per platform, checked
//!    against its `SHA256SUMS`, which Horizon signs with Ed25519;
//! 2. `horizon-prepare` makes the game's resources from the player's own Minecraft 26.3 (the
//!    files this launcher downloads for vanilla, for a signed-in owner) and the 26.3 server jar;
//! 3. `horizon-client` starts with the instance's folder and the session on standard input.
//!
//! Native mods compile the game with them (`native.rs`), content mods and packs are files
//! (`registry.rs`); servers run `horizon-server` (easyhost).
//!
//! ```text
//! horizon/builds/<build>/          an unpacked release: horizon-client, -prepare, -server...
//! horizon/prepared/<build>/        its resources (--resources)
//! horizon/server-26.3.jar          the server jar horizon-prepare imports once
//! horizon/native/...               native mods' builds (native.rs)
//! ```

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::download::{download_all, Download};
use super::instances::Instance;
use super::launch::{LaunchPlan, Player, Progress, QuickPlay};
use super::paths::Paths;

pub mod native;
pub mod registry;
pub mod server_mods;
mod toolchain;

pub const REPO: &str = "horizon-rust/horizon";
/// The Minecraft version Horizon is (research-lock.json's pin).
pub const MINECRAFT: &str = "26.3";
/// "Follow the newest Nightly": an instance's `loader_version` until it is pinned to a build.
pub const LATEST: &str = "latest";

/// Horizon's release signing keys (crates/horizon-release-sign/release-keys.txt there).
const RELEASE_KEYS: &[&str] = &["MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw="];

/// A log line for the player (the instance's or server's log).
pub type Log<'a> = &'a (dyn Fn(&str) + Send + Sync);

pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// This computer's archive in a release, if Horizon builds for it.
pub fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("horizon-macos-arm64.zip"),
        ("macos", "x86_64") => Some("horizon-macos-x64.zip"),
        ("linux", "x86_64") => Some("horizon-linux-x86_64.tar.gz"),
        ("windows", "x86_64") => Some("horizon-windows-x64.zip"),
        _ => None,
    }
}

pub fn root(paths: &Paths) -> PathBuf {
    paths.root.join("horizon")
}

pub fn build_dir(paths: &Paths, build: &str) -> PathBuf {
    root(paths).join("builds").join(build)
}

pub fn prepared_dir(paths: &Paths, build: &str) -> PathBuf {
    root(paths).join("prepared").join(build)
}

// ── builds ──

/// A Horizon build to install: a Nightly, as `nightly-<date>-<commit>`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Build {
    pub id: String,
    /// "Nightly 2026-11-03 · f42ef42"
    pub name: String,
    /// The release tag carrying it (`nightly`, or `nightly-<date>` for older ones)
    pub tag: String,
    pub commit: String,
    pub published: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    name: Option<String>,
    draft: bool,
    published_at: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
}

/// "Nightly 2026-11-03 · f42ef42" → ("2026-11-03", "f42ef42").
fn nightly_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("Nightly ")?;
    let (date, commit) = rest.split_once('·')?;
    let (date, commit) = (date.trim(), commit.trim());
    let ok = date.len() == 10 && date.chars().all(|c| c.is_ascii_digit() || c == '-') && (7..=40).contains(&commit.len()) && commit.chars().all(|c| c.is_ascii_hexdigit());
    ok.then(|| (date.to_string(), commit.to_lowercase()))
}

/// Horizon's Nightlies for this computer, newest first. They only carry the Launch API this
/// follows (its releases from tags don't yet). GitHub's list is asked at most every 15 minutes.
pub async fn builds(client: &reqwest::Client, paths: &Paths) -> Result<Vec<Build>> {
    let Some(asset) = asset_name() else {
        bail!("Horizon isn't made for {} {} yet", std::env::consts::OS, std::env::consts::ARCH);
    };
    let cache = root(paths).join("releases.json");
    let fresh = std::fs::metadata(&cache).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() < 900);
    let releases: Vec<Release> = match fresh.then(|| std::fs::read(&cache).ok()).flatten() {
        Some(bytes) => serde_json::from_slice(&bytes)?,
        None => {
            let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=40");
            let res = client.get(&url).header("Accept", "application/vnd.github+json").send().await;
            match res {
                Ok(r) if r.status().is_success() => {
                    let bytes = r.bytes().await?;
                    let _ = std::fs::create_dir_all(root(paths));
                    let _ = std::fs::write(&cache, &bytes);
                    serde_json::from_slice(&bytes)?
                }
                // offline, or GitHub's limit: what was read last
                other => match std::fs::read(&cache) {
                    Ok(bytes) => serde_json::from_slice(&bytes)?,
                    Err(_) => bail!("Couldn't reach GitHub for Horizon's builds ({})", other.map(|r| r.status().to_string()).unwrap_or_else(|e| e.to_string())),
                },
            }
        }
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in releases {
        if r.draft || !r.tag_name.starts_with("nightly") {
            continue;
        }
        let Some((date, commit)) = r.name.as_deref().and_then(nightly_name) else { continue };
        let Some(a) = r.assets.iter().find(|a| a.name == asset) else { continue };
        if !r.assets.iter().any(|a| a.name == "SHA256SUMS.sig") {
            continue;
        }
        let id = format!("nightly-{date}-{commit}");
        // the moving `nightly` tag and its dated copy are the same build
        if seen.insert(id.clone()) {
            out.push(Build { id, name: r.name.clone().unwrap_or_default(), tag: r.tag_name, commit, published: r.published_at.unwrap_or_default(), size: a.size });
        }
    }
    out.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(out)
}

/// The builds unpacked here, newest first.
pub fn installed(paths: &Paths) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root(paths).join("builds"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join(".installed").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort_by(|a, b| b.cmp(a));
    out
}

/// The build an instance plays: its pinned one, or the newest Nightly (the newest installed
/// one when GitHub can't be asked). Installed when it isn't yet.
pub async fn resolve(client: &reqwest::Client, paths: &Paths, wanted: Option<&str>, progress: Progress<'_>, log: Log<'_>) -> Result<String> {
    let wanted = wanted.filter(|w| !w.is_empty() && *w != LATEST);
    if let Some(w) = wanted {
        if build_dir(paths, w).join(".installed").is_file() {
            return Ok(w.to_string());
        }
    }
    progress("Looking for Horizon", 0, 0);
    let list = match builds(client, paths).await {
        Ok(l) => l,
        Err(e) => match (wanted, installed(paths).first()) {
            (None, Some(newest)) => {
                log(&format!("[Tessera] {e:#}; playing Horizon {newest}"));
                return Ok(newest.clone());
            }
            _ => return Err(e),
        },
    };
    let build = match wanted {
        Some(w) => list.into_iter().find(|b| b.id == w).ok_or_else(|| anyhow!("Horizon {w} isn't on GitHub any more (Nightlies are kept for a week): pick another version in the instance's settings"))?,
        None => list.into_iter().next().ok_or_else(|| anyhow!("Horizon has no Nightly for this computer right now"))?,
    };
    if !build_dir(paths, &build.id).join(".installed").is_file() {
        install(client, paths, &build, progress, log).await?;
    }
    Ok(build.id)
}

/// Downloads `url` into `path` with byte progress, checking its SHA-256.
async fn fetch_sha256(client: &reqwest::Client, url: &str, path: &Path, sha256: &str, stage: &str, progress: Progress<'_>) -> Result<()> {
    super::paths::ensure_parent(path)?;
    let res = client.get(url).send().await.with_context(|| format!("GET {url}"))?;
    if !res.status().is_success() {
        bail!("GET {url} → {}", res.status());
    }
    let total = res.content_length().unwrap_or(0) as usize;
    let part = path.with_extension("part");
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hash = Sha256::new();
    let mut done = 0usize;
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download interrupted")?;
        hash.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len();
        progress(stage, done / 1024, total / 1024);
    }
    file.flush().await?;
    drop(file);
    if hex::encode(hash.finalize()) != sha256.to_lowercase() {
        let _ = tokio::fs::remove_file(&part).await;
        bail!("{url} isn't the file Horizon signed (its SHA-256 differs)");
    }
    tokio::fs::rename(&part, path).await?;
    Ok(())
}

/// `SHA256SUMS` of a release, checked against Horizon's keys: the signature covers
/// "horizon release <tag>\n" and the list ("nightly" for every Nightly, whichever date).
async fn checksums(client: &reqwest::Client, tag: &str) -> Result<Vec<(String, String)>> {
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let get = |name: &'static str| {
        let url = format!("{base}/{name}");
        async move {
            let r = client.get(&url).send().await.with_context(|| format!("GET {url}"))?;
            if !r.status().is_success() {
                bail!("GET {url} → {}", r.status());
            }
            Ok::<_, anyhow::Error>(r.bytes().await?.to_vec())
        }
    };
    let sums = get("SHA256SUMS").await?;
    let sig = get("SHA256SUMS.sig").await?;
    let signed_tag = if tag.starts_with("nightly") { "nightly" } else { tag };
    verify_sums(&sums, &sig, signed_tag)?;
    Ok(String::from_utf8_lossy(&sums)
        .lines()
        .filter_map(|l| {
            let (hash, name) = l.split_once(char::is_whitespace)?;
            Some((name.trim().trim_start_matches('*').to_string(), hash.trim().to_lowercase()))
        })
        .collect())
}

fn verify_sums(sums: &[u8], sig: &[u8], tag: &str) -> Result<()> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let b64 = base64::engine::general_purpose::STANDARD;
    let sig = b64.decode(String::from_utf8_lossy(sig).trim()).context("reading SHA256SUMS.sig")?;
    let sig = Signature::from_slice(&sig).context("reading SHA256SUMS.sig")?;
    let mut message = format!("horizon release {tag}\n").into_bytes();
    message.extend_from_slice(sums);
    for key in RELEASE_KEYS {
        let bytes: [u8; 32] = b64.decode(key).ok().and_then(|k| k.try_into().ok()).ok_or_else(|| anyhow!("bad release key"))?;
        if VerifyingKey::from_bytes(&bytes).is_ok_and(|k| k.verify(&message, &sig).is_ok()) {
            return Ok(());
        }
    }
    bail!("Horizon's checksums aren't signed by Horizon: not installed")
}

/// Downloads, checks and unpacks a build into `horizon/builds/<id>/`.
pub async fn install(client: &reqwest::Client, paths: &Paths, build: &Build, progress: Progress<'_>, log: Log<'_>) -> Result<()> {
    let asset = asset_name().ok_or_else(|| anyhow!("Horizon isn't made for this computer"))?;
    log(&format!("[Tessera] Installing Horizon {}", build.name));
    progress("Checking Horizon's signature", 0, 0);
    let sums = checksums(client, &build.tag).await?;
    let sha = sums.iter().find(|(n, _)| n == asset).map(|(_, h)| h.clone()).ok_or_else(|| anyhow!("{asset} isn't in Horizon's signed checksums"))?;
    let builds = root(paths).join("builds");
    let archive = builds.join(format!(".{}-{asset}", build.id));
    let url = format!("https://github.com/{REPO}/releases/download/{}/{asset}", build.tag);
    fetch_sha256(client, &url, &archive, &sha, "Downloading Horizon", progress).await?;
    progress("Unpacking Horizon", 0, 0);
    let partial = builds.join(format!(".{}.partial", build.id));
    let target = build_dir(paths, &build.id);
    let (a, p, t, commit) = (archive.clone(), partial.clone(), target.clone(), build.commit.clone());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p)?;
        unpack(&a, &p)?;
        // the archive's one folder (horizon-macos-arm64/) is the build
        let top = std::fs::read_dir(&p)?.flatten().map(|e| e.path()).find(|e| e.is_dir() && e.join("research-lock.json").is_file()).unwrap_or(p.clone());
        for name in ["horizon-client", "horizon-prepare", "horizon-server"] {
            if !top.join(exe(name)).is_file() {
                bail!("This Horizon build has no {name}");
            }
        }
        std::fs::write(top.join(".horizon-commit"), &commit)?;
        let _ = std::fs::remove_dir_all(&t);
        std::fs::rename(&top, &t)?;
        std::fs::write(t.join(".installed"), "")?;
        Ok(())
    })
    .await??;
    let _ = std::fs::remove_dir_all(&partial);
    let _ = std::fs::remove_file(&archive);
    prune(paths);
    Ok(())
}

/// Unpacks a .zip or .tar.gz into `to`, keeping only ordinary files and folders inside it.
fn unpack(archive: &Path, to: &Path) -> Result<()> {
    let name = archive.to_string_lossy();
    if name.ends_with(".tar.gz") {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(archive)?));
        tar.set_preserve_permissions(true);
        for entry in tar.entries()? {
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_file() || kind.is_dir() {
                entry.unpack_in(to)?;
            }
        }
        return Ok(());
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let path = to.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        super::paths::ensure_parent(&path)?;
        let mut out = std::fs::File::create(&path)?;
        std::io::copy(&mut entry, &mut out)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

/// Builds and resources no instance or server uses, but the two newest.
pub fn prune(paths: &Paths) {
    let mut used: HashSet<String> = super::instances::list(paths)
        .unwrap_or_default()
        .into_iter()
        .filter(|i| i.loader == super::instances::Loader::Horizon)
        .filter_map(|i| i.loader_version)
        .collect();
    used.extend(super::easyhost::horizon_builds(paths));
    for (i, build) in installed(paths).into_iter().enumerate() {
        if i >= 2 && !used.contains(&build) {
            let _ = std::fs::remove_dir_all(build_dir(paths, &build));
            let _ = std::fs::remove_dir_all(prepared_dir(paths, &build));
        }
    }
}

/// The Horizon commit a build was made from (the short one its release names).
pub fn commit_of(paths: &Paths, build: &str) -> Option<String> {
    std::fs::read_to_string(build_dir(paths, build).join(".horizon-commit")).ok().map(|s| s.trim().to_string())
}

// ── preparing ──

/// What a Horizon program answers to `--launch-api`: the Launch API's version and the
/// options it takes.
#[derive(Deserialize)]
struct LaunchApi {
    launch_api: String,
    #[serde(default)]
    options: Vec<ApiOption>,
}

#[derive(Deserialize)]
struct ApiOption {
    name: String,
}

async fn launch_api(program: &Path) -> Result<HashSet<String>> {
    let out = tokio::process::Command::new(program).arg("--launch-api").stdin(Stdio::null()).output().await.with_context(|| format!("starting {}", program.display()))?;
    let api: LaunchApi = serde_json::from_slice(&out.stdout).map_err(|_| anyhow!("This Horizon build has no Launch API ({}): pick a newer one", program.display()))?;
    if !api.launch_api.starts_with("0.1.") {
        bail!("This Horizon build speaks Launch API {}, which Tessera doesn't know yet: update Tessera Launcher Nightly", api.launch_api);
    }
    Ok(api.options.into_iter().map(|o| o.name).collect())
}

/// Java's home folder from its program (`<home>/bin/java`).
fn java_home(java: &Path) -> Option<PathBuf> {
    java.parent()?.parent().map(Path::to_path_buf)
}

/// The 26.3 version JSON `build` pins (research-lock.json's `minecraft.manifest_sha1`), from
/// Mojang's content-addressed URL. Mojang republishes 26.3 now and then (new translations), and a
/// Horizon build only takes the versions it knows, so the launcher's current copy may not do.
async fn pinned_version(client: &reqwest::Client, paths: &Paths, build: &str) -> Result<(super::versions::VersionJson, PathBuf)> {
    let lock: serde_json::Value = serde_json::from_slice(&std::fs::read(build_dir(paths, build).join("research-lock.json")).context("reading research-lock.json")?)?;
    let sha1 = lock["minecraft"]["manifest_sha1"]
        .as_str()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow!("Horizon {build} pins no Minecraft {MINECRAFT} version file"))?;
    let path = root(paths).join("minecraft").join(format!("{MINECRAFT}-{sha1}.json"));
    let url = format!("https://piston-meta.mojang.com/v1/packages/{sha1}/{MINECRAFT}.json");
    let vanilla = super::download::cached_json(client, &url, path.clone(), Some(sha1)).await?;
    Ok((vanilla, path))
}

/// Makes sure `build` has its resources: from Minecraft 26.3's files (downloaded like vanilla's,
/// for the signed-in owner) and its server jar. Fast when they are current.
pub async fn prepare(client: &reqwest::Client, paths: &Paths, build: &str, progress: Progress<'_>, log: Log<'_>) -> Result<PathBuf> {
    let dir = build_dir(paths, build);
    let preparer = dir.join(exe("horizon-prepare"));
    let output = prepared_dir(paths, build);
    let check = tokio::process::Command::new(&preparer).arg("--check").arg("--output").arg(&output).stdin(Stdio::null()).output().await.context("starting horizon-prepare")?;
    if check.status.success() {
        return Ok(output);
    }
    launch_api(&preparer).await?;
    progress("Reading version", 0, 0);
    let (vanilla, version_json) = pinned_version(client, paths, build).await?;
    let (client_jar, java) = super::launch::vanilla_files(client, paths, &vanilla, progress).await?;
    let server = vanilla.downloads.as_ref().and_then(|d| d.server.clone()).ok_or_else(|| anyhow!("Minecraft {MINECRAFT} has no server jar"))?;
    let server_jar = root(paths).join(format!("server-{MINECRAFT}.jar"));
    download_all(client, vec![Download { url: server.url, path: server_jar.clone(), sha1: server.sha1, size: server.size, executable: false }], |d, t| progress("Downloading the server jar", d, t)).await?;

    log("[Tessera] Preparing Horizon's game files from Minecraft 26.3 (once per Horizon version)");
    progress("Preparing Horizon's game files", 0, 100);
    let mut cmd = tokio::process::Command::new(&preparer);
    cmd.arg("--prepare")
        .arg("--server-jar")
        .arg(&server_jar)
        .arg("--version-json")
        .arg(&version_json)
        .arg("--client-jar")
        .arg(&client_jar)
        .arg("--libraries-dir")
        .arg(paths.libraries())
        .arg("--assets-dir")
        .arg(paths.assets())
        .arg("--output")
        .arg(&output)
        .args(["--complete", "--progress", "json"]);
    if let Some(home) = java_home(&java) {
        cmd.arg("--java-home").arg(home);
    }
    cmd.current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().context("starting horizon-prepare")?;
    let stderr = child.stderr.take();
    let tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let tail2 = tail.clone();
    let err_task = tokio::spawn(async move {
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let mut t = tail2.lock().unwrap();
                t.push(l);
                if t.len() > 8 {
                    t.remove(0);
                }
            }
        }
    });
    let mut error = None;
    if let Some(out) = child.stdout.take() {
        let mut lines = BufReader::new(out).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&l) else { continue };
            if let Some(f) = v["fraction"].as_f64() {
                progress("Preparing Horizon's game files", (f * 100.0) as usize, 100);
            }
            if let Some(m) = v["message"].as_str() {
                log(&format!("[Horizon] {m}"));
            }
            if let Some(e) = v["error"].as_str() {
                error = Some(e.to_string());
            }
        }
    }
    let status = child.wait().await?;
    let _ = err_task.await;
    if !status.success() {
        let detail = error.unwrap_or_else(|| tail.lock().unwrap().join(" / "));
        bail!("Preparing Horizon's game files failed: {detail}");
    }
    Ok(output)
}

// ── playing ──

/// How a Horizon instance starts: its build (with its native mods, when it has some),
/// prepared, and the account's session for the game's standard input.
pub async fn launch_plan(client: &reqwest::Client, paths: &Paths, inst: &Instance, player: &Player, quick_play: Option<&QuickPlay>, progress: Progress<'_>, log: Log<'_>) -> Result<(LaunchPlan, String)> {
    if !player.online {
        bail!("Horizon plays for a Microsoft account that owns Minecraft: Java Edition");
    }
    let build = resolve(client, paths, inst.loader_version.as_deref(), progress, log).await?;
    let resources = prepare(client, paths, &build, progress, log).await?;
    let game_dir = paths.game_dir(&inst.id);
    std::fs::create_dir_all(&game_dir)?;
    registry::check_revoked(client, paths, &game_dir, log).await;
    let program = match native::ensure(client, paths, &game_dir, &build, progress, log).await? {
        Some(built) => built,
        None => build_dir(paths, &build).join(exe("horizon-client")),
    };
    let options = launch_api(&program).await?;
    let has = |o: &str| options.contains(o);
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut args = vec!["--resources".into(), path(&resources)];
    if has("--game-dir") {
        args.extend(["--game-dir".into(), path(&game_dir)]);
    }
    if has("--home") {
        args.extend(["--home".into(), path(&game_dir)]);
    }
    if let (Some(r), true) = (inst.resolution, has("--width")) {
        args.extend(["--width".into(), r.width.to_string(), "--height".into(), r.height.to_string()]);
    }
    match quick_play {
        Some(QuickPlay::World(w)) if has("--quickPlaySingleplayer") => args.extend(["--quickPlaySingleplayer".into(), w.clone()]),
        Some(QuickPlay::Server(s)) if has("--quickPlayMultiplayer") => args.extend(["--quickPlayMultiplayer".into(), s.clone()]),
        _ => {}
    }
    let stdin = if has("--session-stdin") {
        args.push("--session-stdin".into());
        Some(serde_json::json!({ "name": player.name, "uuid": player.uuid, "access_token": player.access_token, "user_type": "msa", "xuid": player.xuid }).to_string())
    } else {
        log("[Tessera] This Horizon build can't take the account: online servers won't let you in");
        None
    };
    Ok((LaunchPlan { java: program, args, game_dir, stdin }, build))
}

/// The newest of `revisions` (40-hex commits of GitHub's `repo`) by commit date; the dates are
/// kept in `horizon/commit-dates.json`, as a commit's date never changes.
pub async fn newest_commit(client: &reqwest::Client, paths: &Paths, repo: &str, revisions: &[String]) -> Result<String> {
    let mut list: Vec<&String> = revisions.iter().collect();
    list.sort();
    list.dedup();
    if list.len() < 2 {
        return list.first().map(|s| s.to_string()).ok_or_else(|| anyhow!("no revision"));
    }
    let file = root(paths).join("commit-dates.json");
    let mut dates: serde_json::Map<String, serde_json::Value> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut best: Option<(String, String)> = None;
    for rev in list {
        let key = format!("{repo}@{rev}");
        let date = match dates.get(&key).and_then(|d| d.as_str()) {
            Some(d) => d.to_string(),
            None => {
                let url = format!("https://api.github.com/repos/{repo}/commits/{rev}");
                let v: serde_json::Value = super::download::fetch_json(client, &url).await.with_context(|| format!("asking GitHub when {repo} {rev} was made"))?;
                let d = v["commit"]["committer"]["date"].as_str().ok_or_else(|| anyhow!("GitHub doesn't know {repo} {rev}"))?.to_string();
                dates.insert(key, d.clone().into());
                d
            }
        };
        if best.as_ref().is_none_or(|(b, _)| date > *b) {
            best = Some((date, rev.clone()));
        }
    }
    let _ = std::fs::write(&file, serde_json::to_vec_pretty(&dates).unwrap_or_default());
    Ok(best.map(|b| b.1).unwrap_or_default())
}

/// Reads a small text file from a zip entry.
pub(crate) fn zip_text(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    if entry.size() > 1 << 20 {
        return None;
    }
    let mut s = String::new();
    entry.read_to_string(&mut s).ok()?;
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nightly_names() {
        assert_eq!(nightly_name("Nightly 2026-11-03 · f42ef42"), Some(("2026-11-03".into(), "f42ef42".into())));
        assert_eq!(nightly_name("Horizon launcher 0.2.37"), None);
        assert_eq!(nightly_name("Nightly 2026-11-03 · ../x"), None);
    }

    #[test]
    fn checks_the_signed_checksums() {
        // Horizon's Nightly 2026-11-03 · f42ef42
        let sums = "27c806b8ef175a3c29cec5244b40999a7b7fd997fbd9f423e1240ff0b297c22b  horizon-linux-x86_64.tar.gz\n710338931e138ad74cffcf52deaf3b67ab0ce54941a886efcbc8f482a2e9e390  horizon-windows-x64.zip\n58432a760b6af8195a543b520daea89be9396a76633753db61eae3553b762593  horizon-macos-arm64.zip\n709fd1c9ca47ba01f90a7dc3e3b9ec9486ae794b322656d86a70c15a0f4c681d  horizon-macos-x64.zip\n34b485754efe23000e7af81dd2fc76e6a6ef7a4e9eb63d3e5d1e0b81907f76a6  horizon-server-linux-x86_64.tar.gz\n571c804d21118b730478dc53eea855d4a09a2c20eca390b28b4d5eb8c061b66a  horizon-source.tar.gz\n";
        let sig = b"iGG5ae6M63DIU8Xeg09C+sqlZLUSX//3iXFJqvF7TQCvkgKagtwGaoe6Yl7oAf1XBjxu/VJgyK9irdCBcugmBQ==\n";
        verify_sums(sums.as_bytes(), sig, "nightly").unwrap();
        assert!(verify_sums(sums.as_bytes(), sig, "nightly-2026-11-01").is_err());
        assert!(verify_sums(sums.replace("27c8", "27c9").as_bytes(), sig, "nightly").is_err());
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// TESSERA_DATA=<launcher data folder> cargo test real_horizon -- --ignored --nocapture
    /// In a temporary folder that borrows the data folder's `meta` (Minecraft's files, Java) and
    /// its Microsoft account: installs the newest Nightly, prepares it, installs Zoom (a native
    /// mod) into a new Horizon instance, builds it (TESSERA_NATIVE=1), starts the game with the
    /// session for 40 s, then makes, starts and stops a Horizon server.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn real_horizon() {
        let data = PathBuf::from(std::env::var("TESSERA_DATA").expect("TESSERA_DATA"));
        let root = std::env::var("TESSERA_SCRATCH").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("tessera-horizon"));
        std::fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        for name in ["meta", "settings.json", "microsoft-accounts.json"] {
            if !root.join(name).exists() && data.join(name).exists() {
                std::os::unix::fs::symlink(data.join(name), root.join(name)).unwrap();
            }
        }
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let progress = |s: &str, d: usize, t: usize| {
            if d == 0 || d == t {
                println!("  {s} {d}/{t}")
            }
        };
        let log = |l: &str| println!("  | {l}");
        let list = builds(&client, &paths).await.unwrap();
        println!("builds: {:?}", list.iter().map(|b| &b.id).collect::<Vec<_>>());
        let build = resolve(&client, &paths, None, &progress, &log).await.unwrap();
        let t0 = std::time::Instant::now();
        let prepared = prepare(&client, &paths, &build, &progress, &log).await.unwrap();
        println!("prepared {build} in {} s: {}", t0.elapsed().as_secs(), prepared.display());

        let inst = match crate::core::instances::get(&paths, "horizon-test") {
            Ok(i) => i,
            Err(_) => crate::core::instances::create(&paths, "Horizon test", MINECRAFT, crate::core::instances::Loader::Horizon, Some(LATEST.into()), None).unwrap(),
        };
        if std::env::var("TESSERA_NATIVE").is_ok() {
            registry::install(&client, &paths, &inst.id, "zoom", None).await.unwrap();
            registry::install(&client, &paths, &inst.id, "menu_panorama", None).await.unwrap();
        }
        let inst = crate::core::instances::get(&paths, &inst.id).unwrap();
        println!("content: {:?}", crate::core::instances::content(&paths, &inst).iter().map(|c| (&c.kind, &c.filename)).collect::<Vec<_>>());

        // the data folder's account, else a stand-in session (fine offline; online servers refuse it)
        let player = match crate::core::settings::load(&paths).microsoft {
            Some(ms) => crate::core::msauth::player(&client, &paths, &ms).await.unwrap().0,
            None => Player { name: "TesseraTest".into(), uuid: "069a79f444e94726a5befca90e38aaf5".into(), access_token: "test".into(), xuid: String::new(), online: true },
        };
        println!("as {}", player.name);
        let t0 = std::time::Instant::now();
        let (plan, b) = launch_plan(&client, &paths, &inst, &player, None, &progress, &log).await.unwrap();
        println!("plan for {b} in {} s: {} {:?}", t0.elapsed().as_secs(), plan.java.display(), plan.args);
        let mut child = tokio::process::Command::new(&plan.java)
            .args(&plan.args)
            .current_dir(&plan.game_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(plan.stdin.unwrap().as_bytes()).await.unwrap();
        drop(stdin);
        let out = child.stderr.take().unwrap();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            let mut seen = Vec::new();
            while let Ok(Some(l)) = lines.next_line().await {
                seen.push(l);
            }
            seen
        });
        tokio::time::sleep(std::time::Duration::from_secs(40)).await;
        assert!(child.try_wait().unwrap().is_none(), "the game stopped by itself");
        child.kill().await.unwrap();
        let seen = reader.await.unwrap();
        for l in seen.iter().filter(|l| l.contains("sign") || l.contains("session") || l.contains("crash") || l.contains("mod")) {
            println!("  game: {l}");
        }
        assert!(!seen.iter().any(|l| l.contains("not signed in")), "the session didn't reach the game");
        println!("the game ran 40 s");

        // a server
        use crate::core::easyhost;
        let new = easyhost::NewServer { name: "Horizon test".into(), kind: easyhost::Kind::Horizon, game_version: String::new(), memory_mb: 2048, instance: None, eula: true };
        let v = easyhost::create(&client, &paths, new, &progress).await.unwrap();
        let id = v.server.id.clone();
        println!("server {id}: {:?} {} {:?} port {}", v.server.kind, v.server.game_version, v.server.loader_version, v.server.port);
        let emit: easyhost::Emit = std::sync::Arc::new(|e| {
            if let easyhost::Event::Line(_, l) = e {
                if l.contains("Done (") || l.contains("ERROR") || l.contains("[Tessera]") || l.contains("players online") || l.contains("error") {
                    println!("  server| {l}");
                }
            }
        });
        easyhost::start(&client, &paths, &id, emit.clone()).await.unwrap();
        let t0 = std::time::Instant::now();
        while easyhost::get_view(&paths, &id).unwrap().state != "running" {
            assert!(t0.elapsed().as_secs() < 120, "not up:\n{}", easyhost::log(&id).join("\n"));
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        println!("server up after {} s", t0.elapsed().as_secs());
        easyhost::send(&id, "list", &emit).await.unwrap();
        easyhost::stop(&id, std::time::Duration::from_secs(30)).await.unwrap();
        while easyhost::get_view(&paths, &id).unwrap().state != "stopped" {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let toml = std::fs::read_to_string(easyhost::data_dir(&paths, &id).join("server.toml")).unwrap();
        assert!(toml.contains(&format!("port = {}", v.server.port)), "the port didn't reach server.toml");
        easyhost::delete(&paths, &id).await.unwrap();
        println!("server stopped and deleted");
    }
}
