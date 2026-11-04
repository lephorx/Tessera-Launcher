//! More skins than your own: MineSkin's public gallery (api.mineskin.org, skins people
//! uploaded there, searchable by name) and any Minecraft player's skin by their name
//! (Mojang's profile API). The pictures come from textures.minecraft.net and are kept in
//! `skins/gallery/<hash>.png`, so paging back and saving one doesn't download it again.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::paths::Paths;
use super::skins::{validate, Model};

const MINESKIN: &str = "https://api.mineskin.org/v2/skins";
const PAGE: usize = 24;

#[derive(Serialize, Clone, Debug)]
pub struct GallerySkin {
    /// The texture's hash on textures.minecraft.net
    pub id: String,
    pub name: String,
    /// data: URL of the PNG
    pub texture: String,
    /// Known for players' skins; gallery skins leave it to the picture (None)
    pub model: Option<Model>,
}

#[derive(Serialize, Debug)]
pub struct Gallery {
    pub skins: Vec<GallerySkin>,
    /// Pass back as `after` for the next page
    pub next: Option<String>,
}

/// A texture hash as textures.minecraft.net names them (hex).
fn check_hash(hash: &str) -> Result<()> {
    if (32..=80).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        bail!("Unknown skin")
    }
}

fn data_url(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// A skin picture by its texture hash: from the cache, or textures.minecraft.net.
pub async fn texture(client: &reqwest::Client, paths: &Paths, hash: &str) -> Result<Vec<u8>> {
    check_hash(hash)?;
    let hash = hash.to_ascii_lowercase();
    let file = paths.root.join("skins").join("gallery").join(format!("{hash}.png"));
    if let Ok(bytes) = std::fs::read(&file) {
        if validate(&bytes).is_ok() {
            return Ok(bytes);
        }
    }
    let res = client.get(format!("https://textures.minecraft.net/texture/{hash}")).send().await.context("downloading the skin")?;
    if !res.status().is_success() {
        bail!("The skin isn't on Minecraft's servers anymore");
    }
    let bytes = res.bytes().await?.to_vec();
    validate(&bytes)?;
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, &bytes);
    Ok(bytes)
}

/// One page of MineSkin's public gallery, newest first, or those whose name contains `query`.
pub async fn browse(client: &reqwest::Client, paths: &Paths, query: &str, after: Option<&str>) -> Result<Gallery> {
    let mut req = client.get(MINESKIN).query(&[("size", PAGE.to_string())]);
    let query = query.trim();
    if !query.is_empty() {
        req = req.query(&[("filter", query.chars().take(40).collect::<String>())]);
    }
    if let Some(after) = after.filter(|a| a.len() <= 64 && a.bytes().all(|b| b.is_ascii_alphanumeric())) {
        req = req.query(&[("after", after)]);
    }
    let res = req.send().await.context("Couldn't reach MineSkin's gallery")?;
    if !res.status().is_success() {
        bail!("MineSkin's gallery answered {}", res.status());
    }
    let body: Value = res.json().await?;
    let next = body["pagination"]["next"]["after"].as_str().map(String::from);
    let mut seen = std::collections::HashSet::new();
    let listed: Vec<(String, String)> = body["skins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let hash = s["texture"].as_str()?.to_ascii_lowercase();
            check_hash(&hash).ok()?;
            let name = s["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("none") && !n.eq_ignore_ascii_case("null")).unwrap_or("Untitled");
            seen.insert(hash.clone()).then(|| (hash, name.chars().take(40).collect()))
        })
        .collect();
    let skins = futures::stream::iter(listed)
        .map(|(hash, name)| async move {
            let bytes = texture(client, paths, &hash).await.ok()?;
            Some(GallerySkin { id: hash, name, texture: data_url(&bytes), model: None })
        })
        .buffered(8)
        .filter_map(|s| async move { s })
        .collect()
        .await;
    Ok(Gallery { skins, next })
}

#[derive(Deserialize)]
struct Profile {
    id: String,
    name: String,
}

/// The skin a Minecraft player wears now, by their name.
pub async fn player(client: &reqwest::Client, paths: &Paths, name: &str) -> Result<GallerySkin> {
    let name = name.trim();
    if name.is_empty() || name.len() > 16 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        bail!("That isn't a Minecraft name");
    }
    let res = client.get(format!("https://api.mojang.com/users/profiles/minecraft/{name}")).send().await.context("Couldn't reach Mojang")?;
    if res.status() == reqwest::StatusCode::NOT_FOUND || res.status() == reqwest::StatusCode::NO_CONTENT {
        bail!("No Minecraft player is called {name}");
    }
    if !res.status().is_success() {
        bail!("Mojang answered {}", res.status());
    }
    let profile: Profile = res.json().await?;
    let session: Value = client
        .get(format!("https://sessionserver.mojang.com/session/minecraft/profile/{}", profile.id))
        .send()
        .await
        .context("Couldn't reach Mojang")?
        .json()
        .await?;
    let encoded = session["properties"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["name"] == "textures"))
        .and_then(|p| p["value"].as_str())
        .ok_or_else(|| anyhow!("{} has no skin", profile.name))?;
    let textures: Value = serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(encoded)?)?;
    let skin = &textures["textures"]["SKIN"];
    let url = skin["url"].as_str().ok_or_else(|| anyhow!("{} wears Minecraft's default skin", profile.name))?;
    let hash = url.rsplit('/').next().unwrap_or_default().to_string();
    let bytes = texture(client, paths, &hash).await?;
    let model = if skin["metadata"]["model"] == "slim" { Model::Slim } else { Model::Classic };
    Ok(GallerySkin { id: hash, name: profile.name, texture: data_url(&bytes), model: Some(model) })
}

/// Saves a gallery or player skin as one of your skins.
pub async fn save(client: &reqwest::Client, paths: &Paths, hash: &str, name: &str, model: Model) -> Result<super::skins::SavedSkin> {
    let bytes = texture(client, paths, hash).await?;
    super::skins::add_bytes(paths, bytes, name, model, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_texture_hashes() {
        assert!(check_hash("fdd6526f91eac3f36ad04a5acefed2968e6da5dec818920aa6b4217f6b977475").is_ok());
        for bad in ["", "../x", "fdd6526f91eac3f36ad04a5acefed2968e6da5dec818920aa6b4217f6b97747/", "zz"] {
            assert!(check_hash(bad).is_err(), "{bad}");
        }
    }

    /// cargo test real_skin_gallery -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn real_skin_gallery() {
        let root = std::env::temp_dir().join(format!("tessera-skinlib-{}", std::process::id()));
        let paths = Paths::new(root.clone());
        let client = reqwest::Client::builder().user_agent("Tessera-Launcher-test").build().unwrap();
        let page = browse(&client, &paths, "", None).await.unwrap();
        println!("newest: {} skins, next {:?}", page.skins.len(), page.next);
        assert!(page.skins.len() > 10);
        let more = browse(&client, &paths, "", page.next.as_deref()).await.unwrap();
        // the same texture is uploaded more than once: the next page brings mostly new ones
        assert!(more.skins.iter().filter(|s| !page.skins.iter().any(|p| p.id == s.id)).count() > 10);
        let knights = browse(&client, &paths, "knight", None).await.unwrap();
        println!("knight: {:?}", knights.skins.iter().map(|s| &s.name).collect::<Vec<_>>());
        let p = player(&client, &paths, "Notch").await.unwrap();
        println!("Notch: {} {:?}", p.id, p.model);
        let saved = save(&client, &paths, &p.id, "Notch", p.model.clone().unwrap()).await.unwrap();
        assert_eq!(saved.name, "Notch");
        assert!(player(&client, &paths, "../x").await.is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
