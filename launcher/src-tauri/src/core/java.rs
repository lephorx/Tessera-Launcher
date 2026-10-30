//! Java runtimes from Mojang's own runtime manifest, so players never install Java themselves.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::Deserialize;

use super::download::{download_all, fetch_json, Download};
use super::paths::Paths;

const RUNTIMES_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
}

#[derive(Deserialize)]
struct ManifestRef {
    url: String,
}

#[derive(Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    executable: bool,
    downloads: Option<RuntimeDownloads>,
    target: Option<String>,
}

#[derive(Deserialize)]
struct RuntimeDownloads {
    raw: RawDownload,
}

#[derive(Deserialize)]
struct RawDownload {
    url: String,
    sha1: String,
    size: u64,
}

/// Mojang's platform keys, best match first (Apple silicon falls back to Intel builds under Rosetta).
fn platforms() -> &'static [&'static str] {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["mac-os-arm64", "mac-os"]
    } else if cfg!(target_os = "macos") {
        &["mac-os"]
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        &["windows-arm64", "windows-x64"]
    } else if cfg!(target_os = "windows") {
        &["windows-x64"]
    } else {
        &["linux"]
    }
}

pub fn java_binary(paths: &Paths, component: &str) -> PathBuf {
    let base = paths.java(component);
    if cfg!(target_os = "macos") {
        base.join("jre.bundle/Contents/Home/bin/java")
    } else if cfg!(target_os = "windows") {
        base.join("bin/javaw.exe")
    } else {
        base.join("bin/java")
    }
}

/// Makes sure the runtime `component` (e.g. "java-runtime-delta") is installed; returns the java binary.
pub async fn ensure_java(
    client: &reqwest::Client,
    paths: &Paths,
    component: &str,
    progress: impl Fn(usize, usize) + Send + Sync,
) -> Result<PathBuf> {
    let bin = java_binary(paths, component);
    let marker = paths.java(component).join(".complete");
    if bin.exists() && marker.exists() {
        return Ok(bin);
    }

    let all: HashMap<String, HashMap<String, Vec<RuntimeEntry>>> = fetch_json(client, RUNTIMES_URL).await?;
    let entry = platforms()
        .iter()
        .filter_map(|p| all.get(*p)?.get(component)?.first())
        .next()
        .ok_or_else(|| anyhow!("No Java runtime \"{component}\" for this computer"))?;
    let manifest: RuntimeManifest = fetch_json(client, &entry.manifest.url).await?;

    let root = paths.java(component);
    let mut downloads = Vec::new();
    let mut links = Vec::new();
    for (name, file) in manifest.files {
        let path = root.join(&name);
        match file.kind.as_str() {
            "directory" => std::fs::create_dir_all(&path)?,
            "file" => {
                if let Some(d) = file.downloads {
                    downloads.push(Download {
                        url: d.raw.url,
                        path,
                        sha1: Some(d.raw.sha1),
                        size: Some(d.raw.size),
                        executable: file.executable,
                    });
                }
            }
            "link" => {
                if let Some(target) = file.target {
                    links.push((path, target));
                }
            }
            _ => {}
        }
    }
    download_all(client, downloads, progress).await?;
    #[cfg(unix)]
    for (path, target) in links {
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, &path)?;
    }
    std::fs::write(marker, b"")?;
    Ok(bin)
}
