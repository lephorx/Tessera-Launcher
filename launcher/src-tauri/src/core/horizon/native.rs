//! Native mods: horizon-mixins bundles that change Horizon's own Rust source before it is
//! compiled (docs/native-mods.md there). The launcher compiles the game with an instance's
//! turned-on mods, the way Horizon's launcher does: its pinned Rust (toolchain.rs), horizon-mixins
//! at the commit the mods name, the source of the Horizon commit they pin (or the build's own
//! when they only need its hook points), `horizon-mixins apply`, then `cargo build`. The game
//! then plays the built program with the build's resources. A set of mods built once is never
//! built again; `native/target/` stays, so a change only recompiles what it touches.
//!
//! Not built here yet: trait mods (an `[api]` crate, `trait_method` injections), which need
//! Horizon's generated API; they are refused with a clear message.
//!
//! ```text
//! horizon/native/rust/, cargo/        the toolchain and its Cargo home
//! horizon/native/tools/<commit>/      horizon-mixins
//! horizon/native/source/<commit>/     Horizon's source at a commit
//! horizon/native/stage/, target/      the modded source and Cargo's build
//! horizon/native/builds/<key>/        built games
//! horizon/native/build.log            the last build's whole output
//! ```

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::super::launch::Progress;
use super::super::paths::Paths;
use super::toolchain::{self, Toolchain};
use super::{exe, Log};

const MIXINS_GIT: &str = "https://github.com/horizon-rust/horizon-mixins";
/// The horizon-mixins Horizon's launcher builds with (native_mods::MIXINS_REV there).
const MIXINS_REV: &str = "ddd9700b21997c95d95ac6ee5ecafe3830cf622a";
const MIXINS_REPO: &str = "horizon-rust/horizon-mixins";
/// Where an instance's native mods are, in its game folder (Horizon's own name).
pub const FOLDER: &str = "horizon-native-mods";
const MANIFEST: &str = "horizon-mixin.toml";
const NATIVE_MANIFEST: &str = "horizon-native.json";
const HOOKS_FILE: &str = "crates/horizon-client/src/game/native_hooks.rs";
const HOOKS_LINE: &str = "pub(crate) const NATIVE_HOOKS_API: u32 = ";

/// One build at a time: they share `stage/` and `target/`.
static BUILDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug)]
struct Bundle {
    id: String,
    version: String,
    /// Its unpacked `mixins/<id>/`
    dir: PathBuf,
    horizon: Option<String>,
    mixins: Option<String>,
    hooks: Option<u32>,
    /// It has something for horizon-mixins to apply (more than [mod] and [compatibility]).
    mixins_content: bool,
    /// Its files, for the build's key
    digest: String,
}

#[derive(serde::Deserialize)]
struct NativeJson {
    id: String,
    version: String,
    #[serde(default)]
    horizon: Option<String>,
    #[serde(default)]
    mixins: Option<String>,
    #[serde(default)]
    hooks: Option<u32>,
}

pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_revision(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The turned-on native mods of a game folder (`.hnmod` files; `.disabled` ones are off).
pub fn enabled(game_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(game_dir.join(FOLDER))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "hnmod"))
        .collect();
    out.sort();
    out
}

/// The id and version a `.hnmod` says it is, checked as `read` checks it.
pub fn identify(file: &Path) -> Result<(String, String)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file)?).context("not a native mod")?;
    let json: NativeJson = serde_json::from_str(&super::zip_text(&mut zip, NATIVE_MANIFEST).ok_or_else(|| anyhow!("no {NATIVE_MANIFEST}"))?)?;
    if !valid_id(&json.id) {
        bail!("invalid mod id {:?}", json.id);
    }
    Ok((json.id, json.version))
}

/// Unpacks a `.hnmod`'s `mixins/<id>/` into `into/<id>/`, as Horizon reads one: only regular
/// files at plain relative paths, within its limits, nothing run.
fn read(file: &Path, into: &Path) -> Result<Bundle> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file)?).with_context(|| format!("{name} isn't a native mod"))?;
    let json: NativeJson = serde_json::from_str(&super::zip_text(&mut zip, NATIVE_MANIFEST).ok_or_else(|| anyhow!("{name} has no {NATIVE_MANIFEST}"))?).with_context(|| format!("{name}: {NATIVE_MANIFEST}"))?;
    if !valid_id(&json.id) {
        bail!("{name}: invalid mod id {:?}", json.id);
    }
    for pin in [&json.horizon, &json.mixins].into_iter().flatten() {
        if !is_revision(pin) {
            bail!("{name}: {pin} isn't a commit");
        }
    }
    if zip.len() > 512 {
        bail!("{name} has too many files for a native mod");
    }
    let prefix = format!("mixins/{}/", json.id);
    let dir = into.join(&json.id);
    let _ = std::fs::remove_dir_all(&dir);
    let mut total = 0u64;
    let mut hash = Sha256::new();
    let mut names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|e| e.name().to_string())).collect();
    names.sort();
    for entry_name in names {
        let mut entry = zip.by_name(&entry_name)?;
        let Some(rel) = entry_name.strip_prefix(&prefix) else { continue };
        if entry.is_dir() || rel.is_empty() {
            continue;
        }
        // links and devices: their Unix mode isn't a regular file's
        if entry.unix_mode().is_some_and(|m| m & 0o170000 != 0o100000 && m & 0o170000 != 0) {
            bail!("{name}: {entry_name} isn't a regular file");
        }
        let ok = rel.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(['\\', ':']));
        if !ok {
            bail!("{name}: {entry_name} isn't a plain path");
        }
        total += entry.size();
        if total > 64 << 20 {
            bail!("{name} is too big for a native mod");
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        hash.update(rel.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
        let to = dir.join(rel);
        super::super::paths::ensure_parent(&to)?;
        std::fs::write(&to, &bytes)?;
    }
    let path = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&path).map_err(|_| anyhow!("{name} has no {prefix}{MANIFEST}"))?;
    let table: toml::Table = text.parse().with_context(|| format!("{name}: {MANIFEST}"))?;
    let field = |t: &str, k: &str| table.get(t).and_then(|v| v.get(k)).and_then(|v| v.as_str()).map(str::to_string);
    if field("mod", "id").as_deref() != Some(json.id.as_str()) {
        bail!("{name}: its {MANIFEST} names another mod");
    }
    let trait_mod = table.contains_key("api")
        || table.get("injection").and_then(|v| v.as_array()).is_some_and(|a| a.iter().any(|i| i.get("trait_method").is_some()));
    if trait_mod {
        bail!("{} {} is a trait mod (it adds its own API), which Tessera can't build yet: play it in the Horizon launcher for now", json.id, json.version);
    }
    let mixins_content = table.keys().any(|k| !["mod", "compatibility", "api", "dependencies"].contains(&k.as_str()));
    Ok(Bundle {
        horizon: field("compatibility", "horizon_revision").or(json.horizon),
        id: json.id,
        version: json.version,
        dir,
        mixins: json.mixins,
        hooks: json.hooks,
        mixins_content,
        digest: hex::encode(hash.finalize()),
    })
}

/// The full Horizon commit a build was made from (its release names the short one).
async fn base_revision(client: &reqwest::Client, paths: &Paths, build: &str) -> Result<String> {
    let file = super::build_dir(paths, build).join(".horizon-revision");
    if let Some(r) = std::fs::read_to_string(&file).ok().map(|s| s.trim().to_string()).filter(|r| is_revision(r)) {
        return Ok(r);
    }
    let short = super::commit_of(paths, build).ok_or_else(|| anyhow!("Horizon {build} doesn't say its commit"))?;
    let url = format!("https://api.github.com/repos/{}/commits/{short}", super::REPO);
    let v: serde_json::Value = super::super::download::fetch_json(client, &url).await.context("asking GitHub for Horizon's commit")?;
    let sha = v["sha"].as_str().filter(|s| is_revision(s)).ok_or_else(|| anyhow!("GitHub doesn't know Horizon {short}"))?.to_string();
    let _ = std::fs::write(&file, &sha);
    Ok(sha)
}

/// The native hook points' version of Horizon at `revision` (None: it has none), kept once read.
async fn hooks_at(client: &reqwest::Client, paths: &Paths, revision: &str) -> Option<u32> {
    let file = toolchain::build_root(paths).join("hooks.json");
    let mut known: BTreeMap<String, Option<u32>> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if let Some(v) = known.get(revision) {
        return *v;
    }
    let url = format!("https://raw.githubusercontent.com/{}/{revision}/{HOOKS_FILE}", super::REPO);
    let res = client.get(&url).send().await.ok()?;
    let found = if res.status().is_success() {
        let text = res.text().await.ok()?;
        text.lines().find_map(|l| l.strip_prefix(HOOKS_LINE)).and_then(|l| l.trim_end().strip_suffix(';')).and_then(|n| n.parse().ok())
    } else if res.status() == reqwest::StatusCode::NOT_FOUND {
        None
    } else {
        return None;
    };
    known.insert(revision.to_string(), found);
    let _ = std::fs::write(&file, serde_json::to_vec(&known).unwrap_or_default());
    found
}

/// Sets a bundle's `[compatibility] horizon_revision` (which horizon-mixins checks) to the
/// build's: a mod on the hook points builds on any Horizon that has them.
fn repin(bundle: &Bundle, revision: &str) -> Result<()> {
    let path = bundle.dir.join(MANIFEST);
    let mut table: toml::Table = std::fs::read_to_string(&path)?.parse()?;
    let compat = table.entry("compatibility").or_insert_with(|| toml::Value::Table(Default::default()));
    if let Some(t) = compat.as_table_mut() {
        t.insert("horizon_revision".into(), revision.into());
    }
    std::fs::write(&path, toml::to_string(&table)?)?;
    Ok(())
}

/// Builds the game with the instance's turned-on native mods, or uses the build of them made
/// before; None when it has none (the build's own game plays).
pub async fn ensure(client: &reqwest::Client, paths: &Paths, game_dir: &Path, build: &str, progress: Progress<'_>, log: Log<'_>) -> Result<Option<PathBuf>> {
    let files = enabled(game_dir);
    if files.is_empty() {
        return Ok(None);
    }
    let _one = match BUILDING.try_lock() {
        Ok(g) => g,
        Err(_) => {
            progress("Waiting for another native build", 0, 0);
            BUILDING.lock().await
        }
    };
    let root = toolchain::build_root(paths);
    let incoming = root.join("mods.incoming");
    let _ = std::fs::remove_dir_all(&incoming);
    std::fs::create_dir_all(&incoming)?;
    let mut mods = Vec::new();
    let mut ids = HashSet::new();
    for f in &files {
        let b = read(f, &incoming)?;
        if !ids.insert(b.id.clone()) {
            bail!("Two native mods are {}: turn one off", b.id);
        }
        mods.push(b);
    }
    mods.sort_by(|a, b| a.id.cmp(&b.id));

    // the Horizon to build on: the mods' pin, else the build's; mods on the hook points follow
    progress("Choosing the Horizon to build on", 0, 0);
    let base = base_revision(client, paths, build).await?;
    let pins: Vec<String> = mods.iter().filter(|b| b.hooks.is_none()).filter_map(|b| b.horizon.clone()).collect();
    let mut revision = if pins.is_empty() { base.clone() } else { super::newest_commit(client, paths, super::REPO, &pins).await? };
    let hooked: Vec<&Bundle> = mods.iter().filter(|b| b.hooks.is_some()).collect();
    if !hooked.is_empty() {
        let fits = |found: Option<u32>| hooked.iter().all(|b| found.is_some() && b.hooks <= found);
        if !fits(hooks_at(client, paths, &revision).await) {
            if revision != base && fits(hooks_at(client, paths, &base).await) {
                revision = base.clone();
            } else {
                let names: Vec<String> = hooked.iter().map(|b| format!("{} (hook points v{})", b.id, b.hooks.unwrap_or(0))).collect();
                bail!("Horizon {} doesn't have the hook points these mods use: {}. Update Horizon or the mods.", &revision[..7], names.join(", "));
            }
        }
    }
    let other: Vec<String> = mods.iter().filter(|b| b.hooks.is_none()).filter_map(|b| b.horizon.as_ref().filter(|r| **r != revision).map(|r| format!("{} → {}", b.id, &r[..7]))).collect();
    if !other.is_empty() {
        log(&format!("[Tessera] Made for other Horizon versions: {}; building them on {} anyway", other.join(", "), &revision[..7]));
    }
    let mut mixin_pins: Vec<String> = mods.iter().filter_map(|b| b.mixins.clone()).collect();
    mixin_pins.push(MIXINS_REV.into());
    let mixins = super::newest_commit(client, paths, MIXINS_REPO, &mixin_pins).await?;

    let mut key = Sha256::new();
    for part in [revision.as_str(), mixins.as_str(), toolchain::VERSION, "fast", std::env::consts::OS] {
        key.update(part.as_bytes());
        key.update([0]);
    }
    for b in &mods {
        key.update(format!("{}\0{}\0{}\0", b.id, b.version, b.digest).as_bytes());
    }
    let key = hex::encode(key.finalize())[..20].to_string();
    let out = root.join("builds").join(&key);
    let binary = out.join(exe("horizon-client"));
    let names: Vec<String> = mods.iter().map(|b| format!("{} {}", b.id, b.version)).collect();
    if out.join(".installed").is_file() && binary.is_file() {
        let _ = std::fs::write(out.join(".installed"), chrono::Utc::now().to_rfc3339());
        log(&format!("[Tessera] Native mods: {} (built before)", names.join(", ")));
        let _ = std::fs::remove_dir_all(&incoming);
        return Ok(Some(binary));
    }

    log(&format!("[Tessera] Building Horizon with native mods: {}. They run with full access to this computer; the first build downloads Rust and compiles the whole game (several minutes).", names.join(", ")));
    let started = std::time::Instant::now();
    let log_file = root.join("build.log");
    let _ = std::fs::write(&log_file, format!("Horizon {revision}, horizon-mixins {mixins}, mods: {}\n", names.join(", ")));
    toolchain::check_linker(true)?;
    let rust = toolchain::install(client, paths, progress).await?;
    let tool = mixins_tool(&rust, &root, &mixins, &log_file, progress).await?;
    progress("Downloading Horizon's source", 0, 0);
    let source = source(client, &root, &revision).await?;
    for b in mods.iter().filter(|b| b.hooks.is_some() || b.horizon.as_deref() != Some(revision.as_str())) {
        repin(b, &revision)?;
    }

    let fresh = root.join("stage.new");
    let _ = std::fs::remove_dir_all(&fresh);
    let staged = root.join("mixins.incoming");
    let _ = std::fs::remove_dir_all(&staged);
    std::fs::create_dir_all(&staged)?;
    for b in mods.iter().filter(|b| b.mixins_content) {
        copy_dir(&b.dir, &staged.join(&b.id))?;
    }
    if mods.iter().any(|b| b.mixins_content) {
        progress("Applying the mods to Horizon's source", 0, 0);
        let mut cmd = rust.command(&tool);
        cmd.arg("apply").arg("--workspace").arg(&source).arg("--mods").arg(&staged).arg("--out").arg(&fresh);
        run(cmd, "horizon-mixins", &log_file, |_| {}).await.map_err(|e| {
            if other.is_empty() {
                e
            } else {
                anyhow!("{e:#}\n\nSome mods were made for other Horizon versions ({}), which may be why: install versions made for the same Nightly.", other.join(", "))
            }
        })?;
    } else {
        copy_dir(&source, &fresh)?;
    }
    let _ = std::fs::remove_dir_all(&staged);
    restore_attributes(&source, &fresh)?;
    let stage = root.join("stage");
    let (f, s) = (fresh.clone(), stage.clone());
    tokio::task::spawn_blocking(move || sync(&f, &s)).await??;
    let _ = std::fs::remove_dir_all(&fresh);

    let toml_of = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|t| t.parse::<toml::Table>().ok()).unwrap_or_default();
    let profile = if toml_of(stage.join("Cargo.toml")).get("profile").and_then(|p| p.get("fast")).is_some() { "fast" } else { "release" };
    let mods_feature = toml_of(stage.join("crates/horizon-client/Cargo.toml")).get("features").and_then(|f| f.get("mods")).is_some();
    let target = root.join("target");
    let mut cmd = rust.command(&rust.cargo());
    cmd.arg("build");
    let config = stage.join(".cargo").join("config.toml");
    if config.is_file() {
        cmd.arg("--config").arg(&config);
    }
    cmd.arg("--manifest-path").arg(stage.join("Cargo.toml")).args(["--locked", "--profile", profile]);
    if mods_feature {
        cmd.args(["--features", "horizon-client/mods"]);
    }
    cmd.args(["-p", "horizon-client", "--bin", "horizon-client"]).env("CARGO_TARGET_DIR", &target).env("CARGO_TERM_PROGRESS_WHEN", "always").env("CARGO_TERM_PROGRESS_WIDTH", "100");
    progress("Compiling the game with its mods", 0, 0);
    let (mut done, mut total, mut name) = (0usize, 0usize, String::new());
    let compiled = run(cmd, "Compiling the game", &log_file, |line| {
        if let Some(rest) = line.trim_start().strip_prefix("Compiling ") {
            done += 1;
            name = rest.split_whitespace().next().unwrap_or_default().to_string();
        } else if let Some((d, t)) = cargo_progress(line) {
            done = done.max(d);
            total = t;
        } else {
            return;
        }
        progress(&format!("Compiling the game with its mods: {name}"), done, total.max(done));
    })
    .await;
    if let Err(e) = compiled {
        let output = std::fs::read_to_string(&log_file).unwrap_or_default();
        let failed = culprits(&stage, &output, &mods);
        return Err(match explain(&failed, &mods, &revision) {
            Some(why) => anyhow!("{why} The whole output is in {}", log_file.display()),
            None => e,
        });
    }

    let partial = root.join("builds").join(format!(".{key}.partial"));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial)?;
    std::fs::copy(target.join(profile).join(exe("horizon-client")), partial.join(exe("horizon-client"))).context("the built game")?;
    let about = serde_json::json!({
        "base": build, "horizon": revision, "horizon_mixins": mixins, "rust": toolchain::VERSION, "profile": profile,
        "mods": mods.iter().map(|b| serde_json::json!({ "id": b.id, "version": b.version })).collect::<Vec<_>>(),
    });
    std::fs::write(partial.join("native-mods.json"), serde_json::to_vec_pretty(&about)?)?;
    let _ = std::fs::remove_dir_all(&out);
    std::fs::rename(&partial, &out)?;
    std::fs::write(out.join(".installed"), chrono::Utc::now().to_rfc3339())?;
    let _ = std::fs::remove_dir_all(&incoming);
    prune(&root, &key);
    log(&format!("[Tessera] Built in {} s", started.elapsed().as_secs()));
    Ok(Some(binary))
}

/// The mods whose code the compiler's errors point into. horizon-mixins puts each `[[module]]`
/// of a mod into its file as `mod <name> { … }`, so an error's `--> file:line` inside one of
/// those names its mod.
fn culprits<'a>(stage: &Path, output: &str, mods: &'a [Bundle]) -> Vec<&'a Bundle> {
    let modules: Vec<(String, &Bundle)> = mods
        .iter()
        .flat_map(|b| {
            let table: toml::Table = std::fs::read_to_string(b.dir.join(MANIFEST)).ok().and_then(|t| t.parse().ok()).unwrap_or_default();
            let names: Vec<String> = table.get("module").and_then(|m| m.as_array()).into_iter().flatten().filter_map(|m| m.get("name")?.as_str().map(str::to_string)).collect();
            names.into_iter().map(move |n| (n, b))
        })
        .collect();
    let mut files: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut found: Vec<&Bundle> = Vec::new();
    for at in output.lines().filter_map(|l| l.trim_start().strip_prefix("--> ")) {
        let mut parts = at.trim().rsplitn(3, ':');
        let (Some(_col), Some(line), Some(file)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let Ok(line) = line.parse::<usize>() else { continue };
        let lines = files.entry(file.to_string()).or_insert_with(|| std::fs::read_to_string(stage.join(file)).unwrap_or_default().lines().map(str::to_string).collect());
        if let Some(b) = enclosing(lines, line, &modules) {
            if !found.iter().any(|f| f.id == b.id) {
                found.push(b);
            }
        }
    }
    found
}

/// The mod whose `mod <name> {` holds 1-based `line`: the nearest one above it whose braces
/// haven't closed by then.
fn enclosing<'a>(lines: &[String], line: usize, modules: &[(String, &'a Bundle)]) -> Option<&'a Bundle> {
    let upto = line.min(lines.len());
    for start in (0..upto).rev() {
        let l = lines[start].trim_start();
        let l = l.strip_prefix("pub(crate) ").or_else(|| l.strip_prefix("pub ")).unwrap_or(l);
        let Some(name) = l.strip_prefix("mod ").and_then(|r| r.split(|c: char| !(c.is_alphanumeric() || c == '_')).next()) else { continue };
        let Some((_, b)) = modules.iter().find(|(n, _)| n == name) else { continue };
        let mut depth = 0i64;
        for l in &lines[start..upto - 1] {
            depth += l.matches('{').count() as i64 - l.matches('}').count() as i64;
        }
        return (depth > 0).then_some(*b);
    }
    None
}

/// Why the build failed, in words, when it's a mod made for another Horizon than the one built.
fn explain(failed: &[&Bundle], mods: &[Bundle], revision: &str) -> Option<String> {
    let short = |r: &str| r.chars().take(7).collect::<String>();
    let names = |list: &[&Bundle]| list.iter().map(|b| format!("{} {}", b.id, b.version)).collect::<Vec<_>>().join(", ");
    let pinners: Vec<&Bundle> = mods.iter().filter(|b| b.hooks.is_none() && b.horizon.as_deref() == Some(revision)).collect();
    let by = if pinners.is_empty() { String::new() } else { format!(" (the version {} needs)", names(&pinners)) };
    let older: Vec<&Bundle> = failed.iter().copied().filter(|b| b.hooks.is_none() && b.horizon.as_deref().is_some_and(|r| r != revision)).collect();
    if !older.is_empty() {
        let made = older.iter().map(|b| format!("{} for {}", b.id, short(b.horizon.as_deref().unwrap_or_default()))).collect::<Vec<_>>().join(", ");
        let them = if older.len() == 1 { "it" } else { "them" };
        return Some(format!(
            "{} doesn't compile on Horizon {}{by}: it was made for another Horizon version ({made}). These mods can't be built together until {them} get an update: turn {} off in the instance's mods to play the others.",
            names(&older), short(revision), names(&older)
        ));
    }
    if !failed.is_empty() {
        return Some(format!("The code of {} doesn't compile on Horizon {}. Turn it off in the instance's mods, or wait for an update.", names(failed), short(revision)));
    }
    None
}

/// "Building [=====>   ] 123/456: crate…" → (123, 456).
fn cargo_progress(line: &str) -> Option<(usize, usize)> {
    let rest = line.trim_start().strip_prefix("Building")?;
    let after = rest.split(']').nth(1)?.trim_start();
    let nums = after.split(':').next()?;
    let (d, t) = nums.trim().split_once('/')?;
    Some((d.parse().ok()?, t.parse().ok()?))
}

/// Runs `cmd`, all its output into `log_file`, each line (split at `\r` too: Cargo's progress
/// bar) to `line`. A failure says what failed with the first errors and the end of the output.
async fn run(mut cmd: tokio::process::Command, what: &str, log_file: &Path, mut line: impl FnMut(&str)) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(log_file).await?;
    file.write_all(format!("$ {cmd:?}\n").as_bytes()).await?;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().with_context(|| format!("starting {what}"))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|p| Box::new(p) as _)].into_iter().flatten() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut pipe = pipe;
            let mut buf = vec![0u8; 8192];
            while let Ok(n) = pipe.read(&mut buf).await {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let (mut pending, mut tail, mut errors) = (Vec::<u8>::new(), std::collections::VecDeque::new(), Vec::new());
    let mut text = String::new();
    while let Some(chunk) = rx.recv().await {
        for b in chunk {
            if b == b'\n' || b == b'\r' {
                let l = String::from_utf8_lossy(&pending).into_owned();
                pending.clear();
                if l.trim().is_empty() {
                    continue;
                }
                line(&l);
                if !l.trim_start().starts_with("Building") {
                    let _ = file.write_all(format!("{l}\n").as_bytes()).await;
                    text.push_str(&l);
                    text.push('\n');
                    if l.starts_with("error") && errors.len() < 4 {
                        errors.push(l.clone());
                    }
                    tail.push_back(l);
                    if tail.len() > 12 {
                        tail.pop_front();
                    }
                }
            } else {
                pending.push(b);
            }
        }
    }
    let status = child.wait().await?;
    let _ = file.flush().await;
    if status.success() {
        return Ok(());
    }
    // a horizon-mixins diagnostic: {"code", "message", "file"}
    if let Some(start) = text.find("{\n") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text[start..].split("\n}").next().map(|s| format!("{s}\n}}")).as_deref().unwrap_or("")) {
            if let (Some(c), Some(m)) = (v["code"].as_str(), v["message"].as_str()) {
                let mod_id = v["mod_id"].as_str().map(|m| format!(" in {m}")).unwrap_or_default();
                bail!("{what} failed{mod_id}: {c}: {m}. The whole output is in {}", log_file.display());
            }
        }
    }
    let tail: Vec<String> = tail.into();
    let first: Vec<&String> = errors.iter().filter(|e| !tail.contains(e)).collect();
    let first = if first.is_empty() { String::new() } else { format!("{}\n…\n", first.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")) };
    bail!("{what} failed ({status}); the whole output is in {}:\n{first}{}", log_file.display(), tail.join("\n"))
}

/// horizon-mixins at `rev`, built once with the toolchain's cargo.
async fn mixins_tool(rust: &Toolchain, root: &Path, rev: &str, log_file: &Path, progress: Progress<'_>) -> Result<PathBuf> {
    let dir = root.join("tools").join(&rev[..12]);
    let bin = dir.join("bin").join(exe("horizon-mixins"));
    if !bin.is_file() {
        progress("Building horizon-mixins", 0, 0);
        let mut cmd = rust.command(&rust.cargo());
        cmd.args(["install", "--locked", "--git", MIXINS_GIT, "--rev", rev, "--root"]).arg(&dir).arg("horizon-mixins-cli");
        run(cmd, "Building horizon-mixins", log_file, |_| {}).await?;
    }
    Ok(bin)
}

/// Horizon's source at `revision`: GitHub's archive, once per commit, without its links
/// (horizon-mixins refuses them; nothing needs them to build).
async fn source(client: &reqwest::Client, root: &Path, revision: &str) -> Result<PathBuf> {
    let dir = root.join("source").join(revision);
    if dir.join(".horizon-revision").is_file() {
        return Ok(dir);
    }
    let url = format!("https://codeload.github.com/{}/tar.gz/{revision}", super::REPO);
    let res = client.get(&url).send().await.with_context(|| format!("GET {url}"))?;
    if !res.status().is_success() {
        bail!("GitHub has no Horizon source for {revision} ({})", res.status());
    }
    let bytes = res.bytes().await.context("downloading Horizon's source")?;
    let partial = root.join("source").join(format!(".{revision}.partial"));
    let (p, d, r) = (partial.clone(), dir.clone(), revision.to_string());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p)?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
        for entry in tar.entries()? {
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_file() || kind.is_dir() {
                entry.unpack_in(&p)?;
            }
        }
        let top = std::fs::read_dir(&p)?.flatten().map(|e| e.path()).find(|e| e.is_dir()).ok_or_else(|| anyhow!("The source archive is empty"))?;
        strip_attributes(&top)?;
        std::fs::write(top.join(".horizon-revision"), &r)?;
        let _ = std::fs::remove_dir_all(&d);
        std::fs::rename(&top, &d)?;
        let _ = std::fs::remove_dir_all(&p);
        Ok(())
    })
    .await??;
    Ok(dir)
}

/// The crate roots whose inner attributes horizon-mixins can't read.
const ROOTS: [&str; 2] = ["crates/horizon-client/src/lib.rs", "crates/horizon-client/src/main.rs"];
const STRIPPED: &str = ".tessera-stripped.json";

/// Attributes horizon-mixins reads (its `macro_attributes`); any other inner attribute of a
/// crate root makes it refuse every target in the crate.
fn readable(attr: &str) -> bool {
    let name = attr.trim_start_matches("#![").split(|c: char| !c.is_ascii_alphanumeric() && c != '_').next().unwrap_or_default();
    ["doc", "cfg", "cfg_attr", "path", "allow", "warn", "deny", "forbid", "expect", "inline", "cold", "must_use", "deprecated", "track_caller"].contains(&name)
}

/// Works around horizon-rust/horizon#8169: `#![recursion_limit]` in horizon-client's lib.rs makes
/// horizon-mixins refuse every target. Such lines leave the source for horizon-mixins and are
/// put back into its output (`restore_attributes`), so the game compiles as Horizon wrote it.
fn strip_attributes(source: &Path) -> Result<()> {
    let mut stripped: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for rel in ROOTS {
        let path = source.join(rel);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut kept = Vec::new();
        for (i, l) in text.lines().enumerate() {
            if l.starts_with("#![") && !readable(l) {
                stripped.entry(rel.to_string()).or_default().push((i, l.to_string()));
            } else {
                kept.push(l);
            }
        }
        if stripped.contains_key(rel) {
            std::fs::write(&path, kept.join("\n") + "\n")?;
        }
    }
    std::fs::write(source.join(STRIPPED), serde_json::to_vec(&stripped)?)?;
    Ok(())
}

fn restore_attributes(source: &Path, stage: &Path) -> Result<()> {
    let stripped: BTreeMap<String, Vec<(usize, String)>> = std::fs::read(source.join(STRIPPED)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    for (rel, lines) in stripped {
        let path = stage.join(&rel);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut all: Vec<String> = text.lines().map(str::to_string).collect();
        for (i, l) in lines {
            if !all.contains(&l) {
                // inner attributes stay above the first item: after the leading //! lines
                let at = all.iter().position(|x| !x.starts_with("//!") && !x.starts_with("#![")).unwrap_or(all.len()).min(i);
                all.insert(at, l);
            }
        }
        std::fs::write(&path, all.join("\n") + "\n")?;
    }
    let _ = std::fs::remove_file(stage.join(STRIPPED));
    Ok(())
}

fn files(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        let t = e.file_type()?;
        if t.is_dir() {
            files(&p, base, out)?;
        } else if t.is_file() {
            out.push((p.strip_prefix(base)?.to_string_lossy().replace('\\', "/"), p));
        }
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    let mut list = Vec::new();
    files(from, from, &mut list)?;
    for (rel, p) in list {
        let target = to.join(&rel);
        super::super::paths::ensure_parent(&target)?;
        std::fs::copy(&p, &target)?;
    }
    Ok(())
}

/// Makes `to` hold `from`'s files, writing only those that changed: Cargo goes by file times,
/// so a build after a mod changes recompiles only what it touched.
fn sync(from: &Path, to: &Path) -> Result<()> {
    let mut new = Vec::new();
    files(from, from, &mut new)?;
    let names: HashSet<&str> = new.iter().map(|(n, _)| n.as_str()).collect();
    for (rel, p) in &new {
        let target = to.join(rel);
        if std::fs::read(&target).ok() != std::fs::read(p).ok() {
            super::super::paths::ensure_parent(&target)?;
            std::fs::copy(p, &target)?;
        }
    }
    if to.is_dir() {
        let mut old = Vec::new();
        files(to, to, &mut old)?;
        for (rel, p) in old {
            if !names.contains(rel.as_str()) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(())
}

/// Keeps the four most recently used builds besides `keep` (switching a mod off and on again
/// plays one again without compiling).
fn prune(root: &Path, keep: &str) {
    let mut builds: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(root.join("builds"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.path().join(".installed").metadata().ok()?.modified().ok()?)))
        .collect();
    builds.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, _) in builds.into_iter().filter(|(n, _)| n != keep).skip(4) {
        let _ = std::fs::remove_dir_all(root.join("builds").join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_compile_error_names_the_mod_made_for_another_horizon() {
        let dir = std::env::temp_dir().join(format!("tessera-culprit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bundle = |id: &str, horizon: &str, module: &str| {
            let d = dir.join("mixins").join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(MANIFEST), format!("[mod]\nid = \"{id}\"\n[[module]]\nid = \"m\"\nfile = \"crates/a/src/game.rs\"\nname = \"{module}\"\nsource = \"m.rs\"\n")).unwrap();
            Bundle { id: id.into(), version: "1.0".into(), dir: d, horizon: Some(horizon.into()), mixins: None, hooks: None, mixins_content: true, digest: String::new() }
        };
        let new = "9d979caa48a3bb58203319b9ea044db58c38e0d8";
        let mods = vec![bundle("carryblock", "142388e1c136e622652ec7505af15d8ad195e041", "carryblock_native"), bundle("mod_menu", new, "mod_menu")];
        let stage = dir.join("stage");
        std::fs::create_dir_all(stage.join("crates/a/src")).unwrap();
        // line 1..: game code, then carryblock's module (lines 4-8), then game code again (10)
        std::fs::write(stage.join("crates/a/src/game.rs"), "fn a() {}\n\n\npub(crate) mod carryblock_native {\n    fn b() {\n        broken();\n    }\n}\n\nfn c() { other(); }\n").unwrap();
        let output = "error[E0599]: no method\n --> crates/a/src/game.rs:6:9\nerror: x\n --> crates/a/src/game.rs:10:10\n";
        let failed = culprits(&stage, output, &mods);
        assert_eq!(failed.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["carryblock"]);
        let why = explain(&failed, &mods, new).unwrap();
        assert!(why.starts_with("carryblock 1.0 doesn't compile on Horizon 9d979ca (the version mod_menu 1.0 needs): it was made for another Horizon version (carryblock for 142388e)"), "{why}");
        // an error outside every mod's module blames none
        assert!(culprits(&stage, " --> crates/a/src/game.rs:10:10\n", &mods).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The last real failed build on this computer: cargo test real_build_failure -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_build_failure() {
        let root = PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/dev.tessera.launcher.nightly/horizon/native");
        let output = std::fs::read_to_string(root.join("build.log")).unwrap();
        let revision = output.split_whitespace().nth(1).unwrap().trim_end_matches(',').to_string();
        let mut mods = Vec::new();
        for d in std::fs::read_dir(root.join("mods.incoming")).into_iter().flatten().flatten() {
            let table: toml::Table = std::fs::read_to_string(d.path().join(MANIFEST)).unwrap().parse().unwrap();
            let horizon = table.get("compatibility").and_then(|c| c.get("horizon_revision")).and_then(|v| v.as_str()).map(String::from);
            mods.push(Bundle { id: d.file_name().to_string_lossy().into(), version: "?".into(), dir: d.path(), horizon, mixins: None, hooks: None, mixins_content: true, digest: String::new() });
        }
        let failed = culprits(&root.join("stage"), &output, &mods);
        println!("{:?}", explain(&failed, &mods, &revision));
    }

    #[test]
    fn reads_cargo_progress() {
        assert_eq!(cargo_progress("    Building [=====>      ] 123/456: serde, tokio"), Some((123, 456)));
        assert_eq!(cargo_progress("   Compiling serde v1.0.0"), None);
    }

    #[test]
    fn takes_out_and_puts_back_unreadable_attributes() {
        let dir = std::env::temp_dir().join(format!("tessera-attrs-{}", std::process::id()));
        let lib = dir.join(ROOTS[0]);
        std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
        let original = "//! Gameplay\n#![recursion_limit = \"256\"]\n#![forbid(unsafe_code)]\nextern crate self as horizon_client;\n";
        std::fs::write(&lib, original).unwrap();
        strip_attributes(&dir).unwrap();
        assert!(!std::fs::read_to_string(&lib).unwrap().contains("recursion_limit"));
        assert!(std::fs::read_to_string(&lib).unwrap().contains("#![forbid(unsafe_code)]"));
        let stage = dir.join("stage");
        std::fs::create_dir_all(stage.join(ROOTS[0]).parent().unwrap()).unwrap();
        std::fs::copy(&lib, stage.join(ROOTS[0])).unwrap();
        restore_attributes(&dir, &stage).unwrap();
        assert_eq!(std::fs::read_to_string(stage.join(ROOTS[0])).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
