//! Modrinth API v2: search projects and install them (mods with required dependencies,
//! resource packs, shaders, datapacks; modpacks become new instances).

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use super::download::{download_all, fetch_json, Download};
use super::instances::{self, InstalledMod, Instance, Loader};
use super::mrpack;
use super::paths::Paths;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    #[serde(default)]
    pub follows: u64,
    pub author: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub display_categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    pub date_modified: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
    pub offset: u64,
    pub limit: u64,
}

pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub struct Query<'a> {
    pub text: &'a str,
    /// mod | modpack | resourcepack | shader | datapack
    pub project_type: &'a str,
    pub game_version: Option<&'a str>,
    pub loaders: &'a [String],
    /// relevance | downloads | follows | newest | updated
    pub sort: &'a str,
    pub limit: u32,
    pub offset: u32,
}

pub async fn search(client: &reqwest::Client, q: Query<'_>) -> Result<SearchResult> {
    let mut facets = vec![format!(r#"["project_type:{}"]"#, q.project_type)];
    if let Some(v) = q.game_version {
        facets.push(format!(r#"["versions:{v}"]"#));
    }
    if !q.loaders.is_empty() {
        let any: Vec<String> = q.loaders.iter().map(|l| format!(r#""categories:{l}""#)).collect();
        facets.push(format!("[{}]", any.join(",")));
    }
    let url = format!(
        "{API}/search?query={}&limit={}&offset={}&index={}&facets={}",
        enc(q.text),
        q.limit.clamp(1, 100),
        q.offset,
        enc(q.sort),
        enc(&format!("[{}]", facets.join(",")))
    );
    fetch_json(client, &url).await
}

#[derive(Deserialize)]
struct Project {
    title: String,
    icon_url: Option<String>,
}

#[derive(Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    pub files: Vec<VersionFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub hashes: Hashes,
    pub size: u64,
}

#[derive(Deserialize)]
pub struct Hashes {
    pub sha1: String,
}

impl Version {
    pub fn file_with_sha1(&self, sha1: &str) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.hashes.sha1 == sha1)
    }
}

/// Which Modrinth version each file hash belongs to (unknown hashes are left out).
pub async fn versions_by_sha1(client: &reqwest::Client, hashes: Vec<String>) -> Result<std::collections::HashMap<String, Version>> {
    if hashes.is_empty() {
        return Ok(Default::default());
    }
    super::limits::MODRINTH.take().await;
    let res = client
        .post(format!("{API}/version_files"))
        .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }))
        .send()
        .await?;
    if !res.status().is_success() {
        bail!("Modrinth answered {}", res.status());
    }
    Ok(res.json().await?)
}

#[derive(Deserialize, Clone)]
pub struct ProjectInfo {
    pub id: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub client_side: String,
    pub server_side: String,
}

/// Several projects at once, by id.
pub async fn projects(client: &reqwest::Client, ids: &[String]) -> Result<std::collections::HashMap<String, ProjectInfo>> {
    let mut out = std::collections::HashMap::new();
    for chunk in ids.chunks(100) {
        let list: Vec<String> = chunk.iter().map(|i| format!(r#""{i}""#)).collect();
        let found: Vec<ProjectInfo> = fetch_json(client, &format!("{API}/projects?ids={}", enc(&format!("[{}]", list.join(","))))).await?;
        out.extend(found.into_iter().map(|p| (p.id.clone(), p)));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct Dependency {
    project_id: Option<String>,
    dependency_type: String,
}

async fn versions(client: &reqwest::Client, project_id: &str, loaders: &[&str], game_version: Option<&str>) -> Result<Vec<Version>> {
    let mut url = format!("{API}/project/{}/version?", enc(project_id));
    if !loaders.is_empty() {
        let list: Vec<String> = loaders.iter().map(|l| format!(r#""{l}""#)).collect();
        url.push_str(&format!("loaders={}&", enc(&format!("[{}]", list.join(",")))));
    }
    if let Some(v) = game_version {
        url.push_str(&format!("game_versions={}", enc(&format!(r#"["{v}"]"#))));
    }
    fetch_json(client, &url).await
}

fn primary(version: &Version) -> Result<&VersionFile> {
    version.files.iter().find(|f| f.primary).or(version.files.first()).ok_or_else(|| anyhow!("This project has no files"))
}

/// Installs the newest compatible version of a project into an instance. Mods bring their
/// required dependencies; datapacks go into `world`.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    instance_id: &str,
    project_id: &str,
    kind: &str,
    world: Option<&str>,
) -> Result<Instance> {
    let mut inst = instances::get(paths, instance_id)?;
    let game = paths.game_dir(&inst.id);
    let (dir, loaders): (std::path::PathBuf, Vec<&str>) = match kind {
        "mod" => {
            if inst.loader == Loader::Vanilla {
                bail!("Vanilla can't run mods. Pick an instance with a mod loader.");
            }
            (game.join("mods"), inst.loader.modrinth_loaders().to_vec())
        }
        "resourcepack" => (game.join("resourcepacks"), vec![]),
        "shader" => (game.join("shaderpacks"), vec![]),
        "datapack" => {
            let w = world.ok_or_else(|| anyhow!("Pick a world for the datapack"))?;
            if !instances::worlds(paths, &inst.id).iter().any(|x| x == w) {
                bail!("That world doesn't exist");
            }
            (game.join("saves").join(w).join("datapacks"), vec!["datapack"])
        }
        other => bail!("Can't install {other} into an instance"),
    };

    let mut queue = vec![project_id.to_string()];
    let mut downloads = Vec::new();
    while let Some(pid) = queue.pop() {
        if inst.mods.iter().any(|m| m.project_id == pid && m.kind == kind && m.world.as_deref() == world) {
            continue;
        }
        let version = versions(client, &pid, &loaders, Some(&inst.game_version))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No version for Minecraft {} on this loader", inst.game_version))?;
        let file = primary(&version)?;
        let project: Project = fetch_json(client, &format!("{API}/project/{}", enc(&pid))).await?;
        downloads.push(Download {
            url: file.url.clone(),
            path: dir.join(&file.filename),
            sha1: Some(file.hashes.sha1.clone()),
            size: Some(file.size),
            executable: false,
        });
        inst.mods.push(InstalledMod {
            kind: kind.into(),
            world: world.map(String::from),
            project_id: version.project_id.clone(),
            version_id: version.id.clone(),
            title: project.title,
            version: version.version_number.clone(),
            filename: file.filename.clone(),
            icon_url: project.icon_url,
        });
        if kind == "mod" {
            for dep in &version.dependencies {
                if dep.dependency_type == "required" {
                    if let Some(p) = &dep.project_id {
                        queue.push(p.clone());
                    }
                }
            }
        }
    }
    download_all(client, downloads, |_, _| {}).await?;
    instances::save(paths, &inst)?;
    Ok(inst)
}

/// Downloads the newest version of a modpack project and creates an instance from it.
pub async fn install_modpack(
    client: &reqwest::Client,
    paths: &Paths,
    project_id: &str,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Instance> {
    progress("Finding the newest version", 0, 0);
    let version = versions(client, project_id, &[], None).await?.into_iter().next().ok_or_else(|| anyhow!("This modpack has no versions"))?;
    let file = primary(&version)?;
    let project: Project = fetch_json(client, &format!("{API}/project/{}", enc(project_id))).await?;
    let tmp = std::env::temp_dir().join(format!("tessera-{}.mrpack", version.id));
    download_all(
        client,
        vec![Download { url: file.url.clone(), path: tmp.clone(), sha1: Some(file.hashes.sha1.clone()), size: Some(file.size), executable: false }],
        |d, t| progress("Downloading modpack", d, t),
    )
    .await?;
    let origin = mrpack::Origin { project_id: Some(project_id.into()), version_id: Some(version.id.clone()), icon_url: project.icon_url };
    let result = mrpack::import(client, paths, &tmp, origin, progress).await;
    let _ = std::fs::remove_file(&tmp);
    result
}
