//! The server's files: browse, edit text files, add, rename and delete. Only inside the
//! server's folder.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::super::mrpack::safe_relative;
use super::super::paths::Paths;
use super::{data_dir, get};

/// Bigger files open in another program instead.
const MAX_EDIT: u64 = 2 * 1024 * 1024;

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

/// A path inside the server's folder ("" is the folder itself).
pub fn resolve(paths: &Paths, id: &str, rel: &str) -> Result<PathBuf> {
    get(paths, id)?;
    let rel = rel.trim_matches(['/', '\\']);
    let safe = safe_relative(rel).map_err(|_| anyhow::anyhow!("That path isn't in the server's folder"))?;
    Ok(data_dir(paths, id).join(safe))
}

fn plain_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        bail!("Pick a name without / \\ or :");
    }
    Ok(name)
}

pub fn list(paths: &Paths, id: &str, rel: &str) -> Result<Vec<Entry>> {
    let d = resolve(paths, id, rel)?;
    let mut out: Vec<Entry> = std::fs::read_dir(&d)
        .with_context(|| format!("{rel} isn't a folder"))?
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(Entry { name: e.file_name().to_string_lossy().into_owned(), dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, modified: meta.modified().ok().map(DateTime::<Utc>::from) })
        })
        .collect();
    out.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// A text file's contents; an error for big or binary files.
pub fn read_text(paths: &Paths, id: &str, rel: &str) -> Result<String> {
    let p = resolve(paths, id, rel)?;
    let meta = std::fs::metadata(&p)?;
    if meta.is_dir() {
        bail!("That's a folder");
    }
    if meta.len() > MAX_EDIT {
        bail!("This file is too big to edit here; open it in another program");
    }
    let bytes = std::fs::read(&p)?;
    if bytes.contains(&0) {
        bail!("This isn't a text file; open it in another program");
    }
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("This isn't a text file; open it in another program"))
}

pub fn write_text(paths: &Paths, id: &str, rel: &str, text: &str) -> Result<()> {
    let p = resolve(paths, id, rel)?;
    if p.is_dir() || rel.trim_matches(['/', '\\']).is_empty() {
        bail!("That's a folder");
    }
    std::fs::write(p, text)?;
    Ok(())
}

pub fn delete(paths: &Paths, id: &str, rels: &[String]) -> Result<()> {
    for rel in rels {
        if rel.trim_matches(['/', '\\']).is_empty() {
            bail!("The server's folder itself can't be deleted here");
        }
        let p = resolve(paths, id, rel)?;
        if p.is_dir() {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        }
        .with_context(|| format!("deleting {rel}"))?;
    }
    Ok(())
}

pub fn rename(paths: &Paths, id: &str, rel: &str, new_name: &str) -> Result<()> {
    if rel.trim_matches(['/', '\\']).is_empty() {
        bail!("The server's folder itself can't be renamed");
    }
    let p = resolve(paths, id, rel)?;
    let to = p.with_file_name(plain_name(new_name)?);
    if to.exists() {
        bail!("There's already something called {new_name}");
    }
    std::fs::rename(&p, &to)?;
    Ok(())
}

pub fn mkdir(paths: &Paths, id: &str, rel: &str, name: &str) -> Result<()> {
    let p = resolve(paths, id, rel)?.join(plain_name(name)?);
    if p.exists() {
        bail!("There's already something called {name}");
    }
    std::fs::create_dir_all(p)?;
    Ok(())
}

fn copy_into(from: &Path, to_dir: &Path) -> Result<()> {
    let name = from.file_name().context("no file name")?;
    let to = to_dir.join(name);
    if from.is_dir() {
        std::fs::create_dir_all(&to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            copy_into(&e.path(), &to)?;
        }
    } else {
        std::fs::copy(from, &to)?;
    }
    Ok(())
}

/// Copies files or folders from this computer into a folder of the server.
pub fn add(paths: &Paths, id: &str, rel: &str, sources: &[String]) -> Result<usize> {
    let to = resolve(paths, id, rel)?;
    if !to.is_dir() {
        bail!("{rel} isn't a folder");
    }
    for s in sources {
        copy_into(Path::new(s), &to).with_context(|| format!("adding {s}"))?;
    }
    Ok(sources.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_in_the_server_folder() {
        assert!(safe_relative("world/../..").is_err());
        assert!(plain_name("../x").is_err());
        assert!(plain_name("ok.txt").is_ok());
    }
}
