//! Mojang's version manifest and version JSON (piston-meta), and the platform rules in them.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::download::{cached_json, fetch_json};
use super::paths::Paths;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Manifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub sha1: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// Pre-1.13 versions: one space-separated string of game arguments.
    #[serde(rename = "minecraftArguments", default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndexRef>,
    pub downloads: Option<ClientDownloads>,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Value>,
    #[serde(default)]
    pub jvm: Vec<Value>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ClientDownloads {
    pub client: Artifact,
    /// The dedicated server jar (1.2.5 and newer).
    #[serde(default)]
    pub server: Option<Artifact>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct JavaVersion {
    pub component: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Artifact {
    pub path: Option<String>,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    /// Maven repository base for loader libraries that have no `downloads` (Fabric).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    /// Old-style natives: os name → classifier, e.g. {"osx": "natives-osx"}.
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

/// Mojang's OS names.
pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

fn rule_matches(rule: &Rule) -> bool {
    if let Some(os) = &rule.os {
        if os.name.as_deref().is_some_and(|n| n != os_name()) {
            return false;
        }
        if let Some(arch) = &os.arch {
            let ours = if cfg!(target_arch = "x86") { "x86" } else if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" };
            if arch != ours {
                return false;
            }
        }
    }
    // We don't enable demo mode, custom resolution, quick play etc., so feature rules never match.
    if let Some(features) = &rule.features {
        if features.values().any(|v| *v) {
            return false;
        }
    }
    true
}

/// Mojang semantics: no rules = allowed; otherwise the last matching rule decides.
pub fn rules_allow(rules: Option<&Vec<Rule>>) -> bool {
    let Some(rules) = rules else { return true };
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

/// Turns an `arguments.game`/`arguments.jvm` list into plain strings, applying rules.
pub fn flatten_arguments(values: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for v in values {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Object(obj) => {
                let rules: Option<Vec<Rule>> = obj.get("rules").and_then(|r| serde_json::from_value(r.clone()).ok());
                if !rules_allow(rules.as_ref()) {
                    continue;
                }
                match obj.get("value") {
                    Some(Value::String(s)) => out.push(s.clone()),
                    Some(Value::Array(items)) => out.extend(items.iter().filter_map(|i| i.as_str().map(String::from))),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

pub async fn manifest(client: &reqwest::Client) -> Result<Manifest> {
    fetch_json(client, MANIFEST_URL).await
}

/// The version JSON for a game version, cached in meta/versions/<id>/<id>.json.
pub async fn version_json(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<VersionJson> {
    let path = paths.version_dir(id).join(format!("{id}.json"));
    // Offline: use the cached copy without asking for the manifest.
    if let Ok(bytes) = tokio::fs::read(&path).await {
        if let Ok(v) = serde_json::from_slice(&bytes) {
            return Ok(v);
        }
    }
    let manifest = manifest(client).await?;
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| anyhow!("Minecraft {id} doesn't exist"))?;
    cached_json(client, &entry.url, path, Some(&entry.sha1)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn last_matching_rule_wins() {
        let rules: Vec<Rule> = serde_json::from_value(json!([
            { "action": "allow" },
            { "action": "disallow", "os": { "name": os_name() } }
        ]))
        .unwrap();
        assert!(!rules_allow(Some(&rules)));
        assert!(rules_allow(None));
    }

    #[test]
    fn feature_arguments_are_skipped() {
        let args = flatten_arguments(&[
            json!("--username"),
            json!("${auth_player_name}"),
            json!({ "rules": [{ "action": "allow", "features": { "is_demo_user": true } }], "value": "--demo" }),
            json!({ "rules": [{ "action": "allow", "os": { "name": os_name() } }], "value": ["-Dfoo", "-Dbar"] }),
        ]);
        assert_eq!(args, vec!["--username", "${auth_player_name}", "-Dfoo", "-Dbar"]);
    }
}
