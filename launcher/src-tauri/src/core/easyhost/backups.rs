//! Backups: the server folder (world, mods, settings) as a zip in
//! `easy-servers/<id>/backups/`, by hand or on a schedule. A running server stops writing
//! its world while it's copied (`save-off`, `save-all flush`). The server software isn't in
//! it (it's downloaded again); restoring puts the folder back as it was, and installs the
//! type and version it had then if that's changed since.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::super::mrpack::safe_relative;
use super::super::paths::{ensure_parent, Paths};
use super::{data_dir, dir, get, install, run, save, software_files, EasyServer};

#[derive(Serialize, Clone, Debug)]
pub struct Backup {
    /// the file name
    pub name: String,
    pub size: u64,
    pub created_at: DateTime<Utc>,
    /// made by the schedule (old ones are deleted), not by hand
    pub automatic: bool,
}

fn backups_dir(paths: &Paths, id: &str) -> PathBuf {
    dir(paths, id).join("backups")
}

fn valid_name(name: &str) -> bool {
    name.ends_with(".zip") && !name.contains(['/', '\\']) && !name.starts_with('.')
}

pub fn list(paths: &Paths, id: &str) -> Result<Vec<Backup>> {
    get(paths, id)?;
    let mut all: Vec<Backup> = std::fs::read_dir(backups_dir(paths, id))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !valid_name(&name) {
                return None;
            }
            let meta = e.metadata().ok()?;
            Some(Backup { automatic: name.starts_with("auto-"), size: meta.len(), created_at: meta.modified().ok().map(DateTime::<Utc>::from).unwrap_or_default(), name })
        })
        .collect();
    all.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(all)
}

pub fn path_of(paths: &Paths, id: &str, name: &str) -> Result<PathBuf> {
    get(paths, id)?;
    if !valid_name(name) {
        bail!("Unknown backup");
    }
    let p = backups_dir(paths, id).join(name);
    if !p.exists() {
        bail!("Unknown backup");
    }
    Ok(p)
}

/// Logs, the lock a running server holds, and the server software (downloaded again).
fn skip(rel: &Path, software: &[String]) -> bool {
    let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    first == "logs" || first == "crash-reports" || software.contains(&first) || rel.file_name().is_some_and(|n| n == "session.lock")
}

fn zip_dir(data: &Path, server: &EasyServer, out: &Path) -> Result<()> {
    let tmp = out.with_extension("zip.part");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    // what the server was then, so a restore brings back its type and version too
    zip.start_file(".tessera/server.json", opts)?;
    zip.write_all(&serde_json::to_vec_pretty(server)?)?;
    let software = software_files(data);
    let mut stack = vec![data.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(data)?.to_path_buf();
            if skip(&rel, &software) {
                continue;
            }
            let name = rel.to_string_lossy().replace('\\', "/");
            if e.file_type()?.is_dir() {
                zip.add_directory(format!("{name}/"), opts)?;
                stack.push(p);
            } else {
                // a file the running server holds open (Windows) is left out, not fatal
                let Ok(mut f) = std::fs::File::open(&p) else { continue };
                zip.start_file(name, opts)?;
                std::io::copy(&mut f, &mut zip)?;
            }
        }
    }
    zip.finish()?;
    std::fs::rename(&tmp, out)?;
    Ok(())
}

pub async fn create(paths: &Paths, id: &str, automatic: bool) -> Result<Backup> {
    let server = get(paths, id)?;
    let out_dir = backups_dir(paths, id);
    std::fs::create_dir_all(&out_dir)?;
    let name = format!("{}{}.zip", if automatic { "auto-" } else { "backup-" }, chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let out = out_dir.join(&name);
    let live = run::running().lock().unwrap().get(id).is_some_and(|r| r.state == "running");
    if live {
        let _ = run::command_quiet(id, "save-off").await;
        let _ = run::command_quiet(id, "save-all flush").await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let data = data_dir(paths, id);
    let zipped = {
        let out = out.clone();
        tokio::task::spawn_blocking(move || zip_dir(&data, &server, &out)).await.map_err(|e| anyhow!("{e}"))?
    };
    if live {
        let _ = run::command_quiet(id, "save-on").await;
    }
    if let Err(e) = zipped {
        let _ = std::fs::remove_file(out.with_extension("zip.part"));
        return Err(e);
    }
    list(paths, id)?.into_iter().find(|b| b.name == name).ok_or_else(|| anyhow!("The backup disappeared"))
}

pub fn delete(paths: &Paths, id: &str, name: &str) -> Result<()> {
    std::fs::remove_file(path_of(paths, id, name)?)?;
    Ok(())
}

/// Puts the server folder back as it was. The server has to be stopped.
pub async fn restore(client: &reqwest::Client, paths: &Paths, id: &str, name: &str, progress: super::Progress<'_>) -> Result<EasyServer> {
    let file = path_of(paths, id, name)?;
    if run::is_running(id) {
        bail!("Stop the server first");
    }
    let current = get(paths, id)?;
    let data = data_dir(paths, id);
    let old = dir(paths, id).join("data.before-restore");
    run::remove_dir_retrying(&old).await?;
    std::fs::rename(&data, &old)?;
    progress("Restoring the backup", 0, 0);
    let unpacked = {
        let (file, data) = (file.clone(), data.clone());
        tokio::task::spawn_blocking(move || unzip(&file, &data)).await.map_err(|e| anyhow!("{e}"))?
    };
    let undo = |e: anyhow::Error| {
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::rename(&old, &data);
        e
    };
    let then = unpacked.map_err(undo)?;
    // its type and version then; the name, port and schedule now
    let mut s = current.clone();
    let same = then.as_ref().is_none_or(|t| t.kind == current.kind && t.game_version == current.game_version && t.loader_version == current.loader_version);
    if same {
        // the software it has now is the right one
        for n in software_files(&old) {
            let _ = std::fs::rename(old.join(&n), data.join(&n));
        }
    } else if let Some(t) = then {
        match install(client, paths, &data, t.kind, &t.game_version, t.loader_version.clone(), progress).await {
            Ok(i) => {
                s.kind = t.kind;
                s.game_version = t.game_version;
                s.loader_version = i.loader_version;
                s.java_component = i.java_component;
                s.start = i.start;
            }
            Err(e) => return Err(undo(e)),
        }
    }
    save(paths, &s)?;
    let _ = run::remove_dir_retrying(&old).await;
    Ok(s)
}

fn unzip(file: &Path, data: &Path) -> Result<Option<EasyServer>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file)?)?;
    std::fs::create_dir_all(data)?;
    let mut then = None;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i)?;
        let name = e.name().to_string();
        if name == ".tessera/server.json" {
            let mut buf = Vec::new();
            e.read_to_end(&mut buf)?;
            then = serde_json::from_slice(&buf).ok();
            continue;
        }
        let target = data.join(safe_relative(name.trim_end_matches('/'))?);
        if e.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            ensure_parent(&target)?;
            std::io::copy(&mut e, &mut std::fs::File::create(&target)?)?;
        }
    }
    Ok(then)
}

/// Makes the scheduled backups that are due and deletes the oldest automatic ones. The
/// launcher checks every few minutes while it's open.
pub async fn run_schedules(paths: &Paths) {
    for v in super::list(paths) {
        let s = v.server;
        let Some(hours) = s.backup_every_hours.filter(|h| *h > 0) else { continue };
        let Ok(all) = list(paths, &s.id) else { continue };
        let last_auto = all.iter().filter(|b| b.automatic).map(|b| b.created_at).max();
        let due = last_auto.is_none_or(|t| Utc::now() - t >= chrono::Duration::hours(hours as i64));
        // a stopped server doesn't change: one backup of how it was is enough
        if !due || (v.state == "stopped" && last_auto.is_some()) {
            continue;
        }
        if create(paths, &s.id, true).await.is_ok() {
            if let Ok(all) = list(paths, &s.id) {
                for b in all.iter().filter(|b| b.automatic).skip(s.backup_keep.max(1) as usize) {
                    let _ = delete(paths, &s.id, &b.name);
                }
            }
        }
    }
}
