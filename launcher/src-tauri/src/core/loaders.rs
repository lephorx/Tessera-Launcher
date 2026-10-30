//! Mod loaders: which versions exist, and the launch profile each one adds on top of vanilla.
//! Fabric and Quilt publish ready profiles; Forge and NeoForge need their installer (see forge.rs).

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::download::{cached_json, fetch_json};
use super::paths::Paths;
use super::versions::{Arguments, Library};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    #[serde(rename = "neoforge")]
    NeoForge,
}

impl Loader {
    /// Modrinth loader names whose mods run on this loader.
    pub fn modrinth_loaders(self) -> &'static [&'static str] {
        match self {
            Loader::Vanilla => &[],
            Loader::Fabric => &["fabric"],
            Loader::Quilt => &["quilt", "fabric"],
            Loader::Forge => &["forge"],
            Loader::NeoForge => &["neoforge"],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

/// What a loader adds to the vanilla version: main class, extra arguments and libraries.
#[derive(Deserialize, Clone, Debug)]
pub struct Profile {
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
}

const FABRIC: &str = "https://meta.fabricmc.net/v2";
const QUILT: &str = "https://meta.quiltmc.org/v3";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FORGE_PROMOS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_VERSIONS: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

#[derive(Deserialize)]
struct MetaEntry {
    loader: MetaLoader,
}

#[derive(Deserialize)]
struct MetaLoader {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

fn looks_stable(v: &str) -> bool {
    let v = v.to_lowercase();
    !["beta", "alpha", "pre", "rc", "snapshot"].iter().any(|t| v.contains(t))
}

/// NeoForge numbers its versions after Minecraft's: 1.21.1 → 21.1.x, 1.21 → 21.0.x, 26.1 → 26.1.x.
fn neoforge_prefix(game: &str) -> String {
    let rest = game.strip_prefix("1.").unwrap_or(game);
    let mut parts: Vec<&str> = rest.split('.').collect();
    if parts.len() == 1 {
        parts.push("0");
    }
    format!("{}.", parts.join("."))
}

fn version_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).collect()
}

/// Versions of `loader` for `game`, newest first.
pub async fn versions(client: &reqwest::Client, loader: Loader, game: &str) -> Result<Vec<LoaderVersion>> {
    match loader {
        Loader::Vanilla => Ok(vec![]),
        Loader::Fabric | Loader::Quilt => {
            let base = if loader == Loader::Fabric { FABRIC } else { QUILT };
            let entries: Vec<MetaEntry> = fetch_json(client, &format!("{base}/versions/loader/{game}")).await.unwrap_or_default();
            Ok(entries
                .into_iter()
                .map(|e| LoaderVersion { stable: e.loader.stable.unwrap_or_else(|| looks_stable(&e.loader.version)), version: e.loader.version })
                .collect())
        }
        Loader::Forge => {
            let xml = client.get(format!("{FORGE_MAVEN}/maven-metadata.xml")).send().await?.text().await?;
            let promos: serde_json::Value = fetch_json(client, FORGE_PROMOS).await.unwrap_or_default();
            let recommended = promos["promos"][format!("{game}-recommended")].as_str().map(String::from);
            let prefix = format!("{game}-");
            let mut list: Vec<LoaderVersion> = xml
                .split("<version>")
                .skip(1)
                .filter_map(|s| s.split("</version>").next())
                .filter_map(|full| full.strip_prefix(&prefix))
                // "1.20.1-47.3.0" → "47.3.0"; skip odd branch builds like "1.7.10-10.13.4.1614-1.7.10"
                .filter(|v| !v.contains('-'))
                .map(|v| LoaderVersion { stable: recommended.as_deref() == Some(v), version: v.to_string() })
                .collect();
            list.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));
            if recommended.is_none() {
                // No recommended build yet: treat the newest as the stable pick.
                if let Some(first) = list.first_mut() {
                    first.stable = true;
                }
            }
            Ok(list)
        }
        Loader::NeoForge => {
            #[derive(Deserialize)]
            struct Versions {
                versions: Vec<String>,
            }
            let all: Versions = fetch_json(client, NEOFORGE_VERSIONS).await?;
            let prefix = neoforge_prefix(game);
            let mut list: Vec<LoaderVersion> = all
                .versions
                .into_iter()
                .filter(|v| v.starts_with(&prefix))
                .map(|v| LoaderVersion { stable: looks_stable(&v), version: v })
                .collect();
            list.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));
            Ok(list)
        }
    }
}

/// Minecraft versions Fabric supports (used to filter the version list).
pub async fn fabric_games(client: &reqwest::Client) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Game {
        version: String,
    }
    let games: Vec<Game> = fetch_json(client, &format!("{FABRIC}/versions/game")).await?;
    Ok(games.into_iter().map(|g| g.version).collect())
}

/// Fabric/Quilt launch profile (cached).
pub async fn meta_profile(client: &reqwest::Client, paths: &Paths, loader: Loader, game: &str, version: &str) -> Result<Profile> {
    let (base, name) = match loader {
        Loader::Fabric => (FABRIC, "fabric-loader"),
        Loader::Quilt => (QUILT, "quilt-loader"),
        _ => return Err(anyhow!("not a meta loader")),
    };
    let id = format!("{name}-{version}-{game}");
    let url = format!("{base}/versions/loader/{game}/{version}/profile/json");
    cached_json(client, &url, paths.version_dir(&id).join(format!("{id}.json")), None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_prefixes() {
        assert_eq!(neoforge_prefix("1.21.1"), "21.1.");
        assert_eq!(neoforge_prefix("1.21"), "21.0.");
        assert_eq!(neoforge_prefix("26.3"), "26.3.");
    }

    #[test]
    fn version_order() {
        assert!(version_key("47.3.10") > version_key("47.3.9"));
    }
}
