use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Context, Result};
use futures::{StreamExt, TryStreamExt};
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use super::paths::ensure_parent;

#[derive(Clone, Debug)]
pub struct Download {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub executable: bool,
}

/// How long a 429 asks us to wait (Retry-After or Modrinth's X-Ratelimit-Reset), capped at a minute.
pub fn retry_after(res: &reqwest::Response) -> std::time::Duration {
    let secs = ["retry-after", "x-ratelimit-reset"]
        .iter()
        .find_map(|h| res.headers().get(*h)?.to_str().ok()?.trim().parse::<u64>().ok())
        .unwrap_or(5);
    std::time::Duration::from_secs(secs.clamp(1, 60))
}

pub async fn fetch_json<T: DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T> {
    if url.starts_with("https://api.modrinth.com/") {
        super::limits::MODRINTH.take().await;
    }
    let mut res = client.get(url).send().await.with_context(|| format!("GET {url}"))?;
    if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        tokio::time::sleep(retry_after(&res)).await;
        res = client.get(url).send().await.with_context(|| format!("GET {url}"))?;
    }
    if !res.status().is_success() {
        bail!("GET {url} → {}", res.status());
    }
    res.json().await.with_context(|| format!("parsing {url}"))
}

/// Reads a cached JSON file, or downloads it (checking the hash when given) and caches it.
pub async fn cached_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    path: PathBuf,
    sha1: Option<&str>,
) -> Result<T> {
    if let Ok(bytes) = tokio::fs::read(&path).await {
        if sha1.is_none_or(|s| hex::encode(Sha1::digest(&bytes)) == s) {
            if let Ok(v) = serde_json::from_slice(&bytes) {
                return Ok(v);
            }
        }
    }
    download_one(
        client,
        &Download { url: url.into(), path: path.clone(), sha1: sha1.map(Into::into), size: None, executable: false },
    )
    .await?;
    let bytes = tokio::fs::read(&path).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// A file counts as present when it exists with the expected size (hashing thousands of
/// asset files on every launch would be slow). Fresh downloads are always hash-checked.
async fn present(d: &Download) -> bool {
    match tokio::fs::metadata(&d.path).await {
        Ok(m) => d.size.is_none_or(|s| m.len() == s),
        Err(_) => false,
    }
}

async fn download_one(client: &reqwest::Client, d: &Download) -> Result<()> {
    ensure_parent(&d.path).with_context(|| format!("creating the folder for {}", d.path.display()))?;
    // "jvm.part" for both lib/jvm.cfg and lib/jvm.lib (Windows' Java) would have the two
    // downloads write and move one temp file: keep the whole name
    let mut tmp = d.path.clone().into_os_string();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500 * attempt)).await;
        }
        match try_download(client, d, &tmp).await {
            Ok(()) => {
                rename_retrying(&tmp, &d.path).await.with_context(|| format!("saving {}", d.path.display()))?;
                #[cfg(unix)]
                if d.executable {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&d.path, std::fs::Permissions::from_mode(0o755))?;
                }
                return Ok(());
            }
            Err(e) => last_err = Some(e),
        }
    }
    let _ = tokio::fs::remove_file(&tmp).await;
    Err(last_err.unwrap()).with_context(|| format!("downloading {}", d.url))
}

/// Antivirus scanners on Windows hold a fresh file for a moment; moving it then fails.
async fn rename_retrying(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match tokio::fs::rename(from, to).await {
            Err(_) if tries < 5 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(200 * tries)).await;
            }
            done => return done,
        }
    }
}

async fn try_download(client: &reqwest::Client, d: &Download, tmp: &PathBuf) -> Result<()> {
    let res = client.get(&d.url).send().await?;
    if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        tokio::time::sleep(retry_after(&res)).await;
        bail!("{} → rate limited, retrying", d.url);
    }
    if !res.status().is_success() {
        bail!("{} → {}", d.url, res.status());
    }
    let mut file = tokio::fs::File::create(tmp).await?;
    let mut hasher = Sha1::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    if let Some(expected) = &d.sha1 {
        let got = hex::encode(hasher.finalize());
        if &got != expected {
            bail!("hash mismatch for {}: expected {expected}, got {got}", d.url);
        }
    }
    Ok(())
}

/// Downloads everything that is missing, 16 at a time. `progress(done, total)` after each file.
/// Files downloaded at once (Settings → Resources).
static MAX_DOWNLOADS: AtomicUsize = AtomicUsize::new(16);

pub fn set_max_downloads(n: u32) {
    MAX_DOWNLOADS.store(n.clamp(1, 16) as usize, Ordering::Relaxed);
}

pub async fn download_all(
    client: &reqwest::Client,
    items: Vec<Download>,
    progress: impl Fn(usize, usize) + Send + Sync,
) -> Result<()> {
    let mut missing = Vec::new();
    // the same file listed twice (e.g. a library both vanilla and the loader name) would be
    // downloaded into the same .part file twice at once
    let mut seen = std::collections::HashSet::new();
    for d in items {
        if !seen.insert(d.path.clone()) {
            continue;
        }
        if !present(&d).await {
            missing.push(d);
        }
    }
    let total = missing.len();
    if total == 0 {
        return Ok(());
    }
    let done = AtomicUsize::new(0);
    progress(0, total);
    futures::stream::iter(missing.into_iter().map(Ok::<_, anyhow::Error>))
        .try_for_each_concurrent(MAX_DOWNLOADS.load(Ordering::Relaxed), |d| {
            let done = &done;
            let progress = &progress;
            async move {
                download_one(client, &d).await?;
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
                Ok(())
            }
        })
        .await
}
