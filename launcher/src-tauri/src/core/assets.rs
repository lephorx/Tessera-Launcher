use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;

use super::download::{cached_json, Download};
use super::paths::Paths;
use super::versions::AssetIndexRef;

#[derive(Deserialize)]
struct AssetIndex {
    objects: HashMap<String, AssetObject>,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

/// The asset index (cached) and the list of objects to download.
pub async fn asset_downloads(client: &reqwest::Client, paths: &Paths, index: &AssetIndexRef) -> Result<Vec<Download>> {
    let index_path = paths.assets().join("indexes").join(format!("{}.json", index.id));
    let parsed: AssetIndex = cached_json(client, &index.url, index_path, Some(&index.sha1)).await?;
    let objects = paths.assets().join("objects");
    let mut seen = std::collections::HashSet::new();
    Ok(parsed
        .objects
        .into_values()
        .filter(|o| seen.insert(o.hash.clone()))
        .map(|o| {
            let prefix = &o.hash[..2];
            Download {
                url: format!("https://resources.download.minecraft.net/{prefix}/{}", o.hash),
                path: objects.join(prefix).join(&o.hash),
                sha1: Some(o.hash),
                size: Some(o.size),
                executable: false,
            }
        })
        .collect())
}
