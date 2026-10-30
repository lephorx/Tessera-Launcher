//! "Export debug info" (Settings → Troubleshooting): a zip with the launcher and system
//! versions, the instances, and the latest game logs, to send along with a bug report.
//! Sign-ins (Tessera session, Microsoft accounts) are never included.

use std::io::Write;
use std::path::Path;

use anyhow::Result;

use super::instances;
use super::paths::Paths;
use super::settings;

/// The last `max` lines of a text file.
fn tail(path: &Path, max: usize) -> Option<String> {
    let text = String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    Some(lines[lines.len().saturating_sub(max)..].join("\n"))
}

pub fn export(paths: &Paths, out: &Path, launcher_logs: &[(String, Vec<String>)]) -> Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out)?);
    let opts = zip::write::SimpleFileOptions::default();

    let s = settings::load(paths);
    let mut info = String::new();
    info += &format!("Tessera Launcher {}\n", env!("CARGO_PKG_VERSION"));
    info += &format!("{} ({})\n", sysinfo::System::long_os_version().unwrap_or_default(), std::env::consts::ARCH);
    let memory = sysinfo::System::new().map(|mut s| {
        s.refresh_memory();
        s.total_memory() / 1024 / 1024
    });
    info += &format!("Memory: {} MB, {} CPUs\n", memory.unwrap_or(0), std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));
    info += &format!("Data folder: {}\n", paths.root.display());
    info += &format!("Default memory: {} MB, downloads at once: {}\n", s.memory_mb, s.max_downloads);
    info += &format!("Minecraft account signed in: {}\n\nInstances:\n", s.microsoft.is_some());
    for i in instances::list(paths).unwrap_or_default() {
        info += &format!(
            "- {} ({}): Minecraft {}, {:?} {}, {} files, memory {:?}, java {:?}\n",
            i.name,
            i.id,
            i.game_version,
            i.loader,
            i.loader_version.as_deref().unwrap_or(""),
            i.mods.len(),
            i.memory_mb,
            i.java_path,
        );
    }
    zip.start_file("tessera-debug.txt", opts)?;
    zip.write_all(info.as_bytes())?;

    for (id, lines) in launcher_logs {
        zip.start_file(format!("launcher-log/{id}.log"), opts)?;
        zip.write_all(lines.join("\n").as_bytes())?;
    }
    for i in instances::list(paths).unwrap_or_default() {
        let logs = paths.game_dir(&i.id).join("logs");
        if let Some(text) = tail(&logs.join("latest.log"), 3000) {
            zip.start_file(format!("game-logs/{}/latest.log", i.id), opts)?;
            zip.write_all(text.as_bytes())?;
        }
        // the newest crash report, if any
        let crash = std::fs::read_dir(paths.game_dir(&i.id).join("crash-reports"))
            .into_iter()
            .flatten()
            .flatten()
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        if let Some(c) = crash.and_then(|c| tail(&c.path(), 3000).map(|t| (c.file_name(), t))) {
            zip.start_file(format!("game-logs/{}/{}", i.id, c.0.to_string_lossy()), opts)?;
            zip.write_all(c.1.as_bytes())?;
        }
    }
    zip.finish()?;
    Ok(())
}
