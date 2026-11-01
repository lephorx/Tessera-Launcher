//! Mojang's version manifest and version JSON (piston-meta), and the platform rules in them.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};

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

/// Minecraft and loader versions become folder names (meta/versions/<id>/) and parts of
/// URLs. A pack names them, so only plain names: "../../instances/x/game/v" would make the
/// launcher read a version file the pack brought (its own main class, arguments, libraries).
pub fn is_safe_version(v: &str) -> bool {
    (1..=64).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)) && !v.contains("..")
}

pub fn check_version(v: &str) -> Result<()> {
    if !is_safe_version(v) {
        bail!("\"{}\" isn't a version Tessera can use", v.chars().take(64).collect::<String>());
    }
    Ok(())
}

pub async fn manifest(client: &reqwest::Client) -> Result<Manifest> {
    fetch_json(client, MANIFEST_URL).await
}

/// The last manifest we fetched, kept to check cached version files offline.
fn manifest_file(paths: &Paths) -> std::path::PathBuf {
    paths.meta().join("version_manifest_v2.json")
}

fn saved_manifest(paths: &Paths) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(manifest_file(paths)).ok()?).ok()
}

/// The version JSON for a game version, cached in meta/versions/<id>/<id>.json. The cached
/// copy is only used while its hash is the one Mojang's manifest names for it.
pub async fn version_json(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<VersionJson> {
    check_version(id)?;
    let path = paths.version_dir(id).join(format!("{id}.json"));
    // Offline: the cached copy, checked against the manifest saved last time.
    if let (Ok(bytes), Some(saved)) = (tokio::fs::read(&path).await, saved_manifest(paths)) {
        let known = saved.versions.iter().find(|v| v.id == id).map(|v| v.sha1.as_str());
        if known == Some(hex::encode(Sha1::digest(&bytes)).as_str()) {
            if let Ok(v) = serde_json::from_slice(&bytes) {
                return Ok(v);
            }
        }
    }
    let manifest = manifest(client).await?;
    if let Ok(bytes) = serde_json::to_vec(&manifest) {
        let _ = super::paths::ensure_parent(&manifest_file(paths));
        let _ = std::fs::write(manifest_file(paths), bytes);
    }
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| anyhow!("Minecraft {id} doesn't exist"))?;
    // a cached file with another hash is downloaded again
    cached_json(client, &entry.url, path, Some(&entry.sha1)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_are_plain_names() {
        for ok in ["1.21.1", "24w14a", "b1.7.3", "rd-132211", "c0.30_01c", "0.16.9", "47.3.0", "21.1.77", "20.2.3-beta", "0.26.4-beta.1", "1.20.1+build.10"] {
            assert!(is_safe_version(ok), "{ok}");
        }
        for bad in ["", "../../instances/x/game/v", "..", "1..2", "a/b", "a\\b", "C:x", "1.21 ", "%2e%2e", &"9".repeat(65)] {
            assert!(!is_safe_version(bad), "{bad}");
        }
        assert!(check_version("../x").is_err());
    }

    /// A version file that isn't Mojang's (planted or broken) is never used, even offline.
    #[tokio::test]
    async fn cached_version_needs_the_manifest_hash() {
        let root = std::env::temp_dir().join(format!("tessera-versions-{}", std::process::id()));
        let paths = Paths::new(root.clone());
        let dir = paths.version_dir("1.21.1");
        std::fs::create_dir_all(&dir).unwrap();
        let fake = br#"{"id":"1.21.1","mainClass":"Evil","libraries":[]}"#;
        std::fs::write(dir.join("1.21.1.json"), fake).unwrap();
        let manifest = serde_json::json!({ "latest": { "release": "1.21.1", "snapshot": "1.21.1" }, "versions": [
            { "id": "1.21.1", "type": "release", "url": "https://invalid.invalid/1.21.1.json", "sha1": "0000000000000000000000000000000000000000", "releaseTime": "" }
        ]});
        std::fs::write(manifest_file(&paths), manifest.to_string()).unwrap();
        // no network on .invalid: the wrong hash means it isn't read from the cache
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap();
        assert!(version_json(&client, &paths, "1.21.1").await.map(|v| v.main_class).unwrap_or_default() != "Evil");
        // with the right hash it is (offline); the call above may have fetched the real one
        std::fs::write(dir.join("1.21.1.json"), fake).unwrap();
        let manifest = serde_json::json!({ "latest": { "release": "1.21.1", "snapshot": "1.21.1" }, "versions": [
            { "id": "1.21.1", "type": "release", "url": "https://invalid.invalid/1.21.1.json", "sha1": hex::encode(Sha1::digest(fake)), "releaseTime": "" }
        ]});
        std::fs::write(manifest_file(&paths), manifest.to_string()).unwrap();
        assert_eq!(version_json(&client, &paths, "1.21.1").await.unwrap().main_class, "Evil");
        assert!(version_json(&client, &paths, "../../x").await.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

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
