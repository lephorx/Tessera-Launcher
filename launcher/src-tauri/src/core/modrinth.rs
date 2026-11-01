//! Modrinth API v2: search projects and install them (mods with required dependencies,
//! resource packs, shaders, datapacks; modpacks become new instances).

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    /// release | beta | alpha
    #[serde(default)]
    pub version_type: String,
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
    /// the exact version the mod asks for, if it names one
    version_id: Option<String>,
    dependency_type: String,
}

async fn versions(client: &reqwest::Client, project_id: &str, loaders: &[&str], game_version: Option<&str>) -> Result<Vec<Version>> {
    fetch_json(client, &versions_url(project_id, loaders, game_version)).await
}

fn versions_url(project_id: &str, loaders: &[&str], game_version: Option<&str>) -> String {
    let mut url = format!("{API}/project/{}/version?", enc(project_id));
    if !loaders.is_empty() {
        let list: Vec<String> = loaders.iter().map(|l| format!(r#""{l}""#)).collect();
        url.push_str(&format!("loaders={}&", enc(&format!("[{}]", list.join(",")))));
    }
    if let Some(v) = game_version {
        url.push_str(&format!("game_versions={}", enc(&format!(r#"["{v}"]"#))));
    }
    url
}

/// One version of a project, for the project page (newest first from Modrinth).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct VersionInfo {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    /// release | beta | alpha
    pub version_type: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    pub date_published: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub changelog: Option<String>,
}

pub async fn project_versions(client: &reqwest::Client, project_id: &str, loaders: &[&str], game_version: Option<&str>) -> Result<Vec<VersionInfo>> {
    fetch_json(client, &versions_url(project_id, loaders, game_version)).await
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct GalleryImage {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

/// What the project page shows about a project.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ProjectDetail {
    pub id: String,
    pub slug: String,
    pub project_type: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub followers: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    pub published: Option<String>,
    pub updated: Option<String>,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
    /// Filled in from the project's members: the owner's username.
    #[serde(default)]
    pub author: Option<String>,
}

/// A project by id or slug, with its owner's name.
pub async fn project_detail(client: &reqwest::Client, id: &str) -> Result<ProjectDetail> {
    let mut p: ProjectDetail = fetch_json(client, &format!("{API}/project/{}", enc(id))).await?;
    // projects owned by an organization have no owner among the members; that's fine
    let members: Vec<Value> = fetch_json(client, &format!("{API}/project/{}/members", enc(&p.id))).await.unwrap_or_default();
    let owner = members.iter().find(|m| m["role"] == "Owner").or(members.first());
    p.author = owner.and_then(|m| m["user"]["username"].as_str()).map(String::from);
    Ok(p)
}

/// A file Modrinth knows by its hash.
pub struct Known {
    pub project_id: String,
    pub version_id: String,
    pub version: String,
    pub title: String,
    pub icon_url: Option<String>,
}

/// Which Modrinth project and version each file is (by sha1). Best effort: unknown files and
/// a failed lookup are simply left out.
pub async fn identify(client: &reqwest::Client, hashes: Vec<String>) -> std::collections::HashMap<String, Known> {
    let Ok(found) = versions_by_sha1(client, hashes).await else { return Default::default() };
    let ids: Vec<String> = found.values().map(|v| v.project_id.clone()).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let projects = projects(client, &ids).await.unwrap_or_default();
    found
        .into_iter()
        .filter_map(|(sha1, v)| {
            let p = projects.get(&v.project_id)?;
            Some((sha1, Known { project_id: v.project_id, version_id: v.id, version: v.version_number, title: p.title.clone(), icon_url: p.icon_url.clone() }))
        })
        .collect()
}

fn primary(version: &Version) -> Result<&VersionFile> {
    version.files.iter().find(|f| f.primary).or(version.files.first()).ok_or_else(|| anyhow!("This project has no files"))
}

/// Installs the newest compatible version of a project into an instance, or `version_id`
/// (replacing the version the instance has). Mods bring their required dependencies;
/// datapacks go into `world`.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    instance_id: &str,
    project_id: &str,
    kind: &str,
    world: Option<&str>,
    version_id: Option<&str>,
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

    let mut chosen = None;
    if let Some(vid) = version_id {
        let v: Version = fetch_json(client, &format!("{API}/version/{}", enc(vid))).await?;
        if !v.game_versions.is_empty() && !v.game_versions.contains(&inst.game_version) {
            bail!("This version is for Minecraft {}; the instance is {}", v.game_versions.join(", "), inst.game_version);
        }
        if !loaders.is_empty() && !v.loaders.iter().any(|l| loaders.contains(&l.as_str())) {
            bail!("This version is for {}, not this instance's loader", v.loaders.join(", "));
        }
        // the version picked replaces the one installed
        let same = |m: &InstalledMod| m.project_id == v.project_id && m.kind == kind && m.world.as_deref() == world;
        for m in inst.mods.iter().filter(|m| same(m)) {
            let file = dir.join(&m.filename);
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(dir.join(format!("{}.disabled", m.filename)));
        }
        inst.mods.retain(|m| !same(m));
        chosen = Some(v);
    }

    let mut queue = vec![project_id.to_string()];
    let mut downloads = Vec::new();
    while let Some(pid) = queue.pop() {
        if inst.mods.iter().any(|m| m.project_id == pid && m.kind == kind && m.world.as_deref() == world) {
            continue;
        }
        let version = match chosen.take() {
            Some(v) => v,
            None => versions(client, &pid, &loaders, Some(&inst.game_version))
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("No version for Minecraft {} on this loader", inst.game_version))?,
        };
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

/// A project found for a mod id only by its name or slug, not by an id a mod or Modrinth
/// named. Anyone can publish a look-alike, so these are installed only after a yes.
#[derive(Serialize, Clone, Debug)]
pub struct NameMatch {
    /// The mod id that is missing, and the mod that needs it.
    pub mod_id: String,
    pub needed_by: String,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub author: Option<String>,
    pub downloads: Option<u64>,
}

/// The Modrinth project of a mod id from a mod's own metadata: a search hit whose name or
/// slug is the id without spaces or dashes, for this Minecraft and loader, or a project
/// with that slug.
pub async fn project_for_mod_id(client: &reqwest::Client, mod_id: &str, game: &str, loaders: &[&str]) -> Option<NameMatch> {
    // "Roughly Enough Items (REI)" is roughlyenoughitems
    let norm = |s: &str| s.split('(').next().unwrap_or(s).to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let want = norm(mod_id);
    let loaders: Vec<String> = loaders.iter().map(|l| l.to_string()).collect();
    let text = mod_id.replace(['_', '-'], " ");
    let found = search(client, Query { text: &text, project_type: "mod", game_version: Some(game), loaders: &loaders, sort: "relevance", limit: 20, offset: 0 }).await.ok()?;
    if let Some(h) = found.hits.iter().find(|h| norm(&h.slug) == want || norm(&h.title) == want) {
        return Some(NameMatch {
            mod_id: mod_id.into(),
            needed_by: String::new(),
            project_id: h.project_id.clone(),
            slug: h.slug.clone(),
            title: h.title.clone(),
            author: Some(h.author.clone()),
            downloads: Some(h.downloads),
        });
    }
    let p: Value = fetch_json(client, &format!("{API}/project/{}", enc(mod_id))).await.ok()?;
    Some(NameMatch {
        mod_id: mod_id.into(),
        needed_by: String::new(),
        project_id: p["id"].as_str()?.into(),
        slug: p["slug"].as_str().unwrap_or(mod_id).into(),
        title: p["title"].as_str().unwrap_or(mod_id).into(),
        author: None,
        downloads: p["downloads"].as_u64(),
    })
}

/// Adds the required dependencies of `needed_by` (and theirs) that the instance doesn't have:
/// the exact version a mod names, otherwise the newest for the instance's Minecraft and
/// loader. Also projects asked for by id or slug (`wanted`). Returns the titles added, and
/// the ids nothing was found for.
pub async fn add_dependencies(client: &reqwest::Client, paths: &Paths, inst: &mut Instance, needed_by: &[&Version], wanted: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let dir = paths.game_dir(&inst.id).join("mods");
    let loaders = inst.loader.modrinth_loaders().to_vec();
    let mut have: std::collections::HashSet<String> = inst.mods.iter().map(|m| m.project_id.clone()).collect();
    have.extend(needed_by.iter().map(|v| v.project_id.clone()));
    let required = |v: &Version| -> Vec<(Option<String>, Option<String>)> {
        v.dependencies.iter().filter(|d| d.dependency_type == "required").map(|d| (d.project_id.clone(), d.version_id.clone())).collect()
    };
    let mut queue: Vec<(Option<String>, Option<String>)> = needed_by.iter().flat_map(|v| required(v)).collect();
    queue.extend(wanted.iter().map(|w| (Some(w.clone()), None)));
    let mut downloads = Vec::new();
    let (mut added, mut missing) = (Vec::new(), Vec::new());
    while let Some((pid, vid)) = queue.pop() {
        if pid.as_ref().is_some_and(|p| have.contains(p)) {
            continue;
        }
        let exact: Option<Version> = match &vid {
            Some(v) => fetch_json(client, &format!("{API}/version/{}", enc(v))).await.ok(),
            None => None,
        };
        let version = match (exact, &pid) {
            (Some(v), _) => v,
            (None, Some(p)) => match versions(client, p, &loaders, Some(&inst.game_version)).await.ok().and_then(|l| l.into_iter().next()) {
                Some(v) => v,
                None => {
                    missing.push(p.clone());
                    continue;
                }
            },
            (None, None) => continue,
        };
        // a slug asked for resolves to the project's id
        if !have.insert(version.project_id.clone()) {
            continue;
        }
        if let Some(p) = &pid {
            have.insert(p.clone());
        }
        let file = primary(&version)?;
        let project: Project = fetch_json(client, &format!("{API}/project/{}", enc(&version.project_id))).await?;
        downloads.push(Download { url: file.url.clone(), path: dir.join(&file.filename), sha1: Some(file.hashes.sha1.clone()), size: Some(file.size), executable: false });
        added.push(project.title.clone());
        inst.mods.push(InstalledMod {
            kind: "mod".into(),
            world: None,
            project_id: version.project_id.clone(),
            version_id: version.id.clone(),
            title: project.title,
            version: version.version_number.clone(),
            filename: file.filename.clone(),
            icon_url: project.icon_url,
        });
        queue.extend(required(&version));
    }
    download_all(client, downloads, |_, _| {}).await?;
    Ok((added, missing))
}

/// A modpack version: `version_id`, or the newest release (the newest beta or alpha when
/// there's no release).
pub async fn modpack_version(client: &reqwest::Client, project_id: &str, version_id: Option<&str>) -> Result<Version> {
    if let Some(v) = version_id {
        return fetch_json(client, &format!("{API}/version/{}", enc(v))).await;
    }
    let all = versions(client, project_id, &[], None).await?;
    let pick = all.iter().position(|v| v.version_type == "release").unwrap_or(0);
    all.into_iter().nth(pick).ok_or_else(|| anyhow!("This modpack has no versions"))
}

/// The .mrpack of a modpack version, downloaded once into meta/modpacks/<version id>.mrpack
/// (looking inside, installing and updating all use it).
pub async fn cached_mrpack(
    client: &reqwest::Client,
    paths: &Paths,
    version: &Version,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<std::path::PathBuf> {
    let file = version.files.iter().find(|f| f.primary && f.filename.ends_with(".mrpack")).or_else(|| version.files.iter().find(|f| f.filename.ends_with(".mrpack"))).ok_or_else(|| anyhow!("This version has no .mrpack file"))?;
    if version.id.contains(['/', '\\', '.']) {
        bail!("invalid version id");
    }
    let path = paths.modpacks().join(format!("{}.mrpack", version.id));
    download_all(
        client,
        vec![Download { url: file.url.clone(), path: path.clone(), sha1: Some(file.hashes.sha1.clone()), size: Some(file.size), executable: false }],
        |d, t| progress("Downloading modpack", d, t),
    )
    .await?;
    Ok(path)
}

/// Downloads a modpack project (the newest version, or `version_id`) and creates an instance from it.
pub async fn install_modpack(
    client: &reqwest::Client,
    paths: &Paths,
    project_id: &str,
    version_id: Option<&str>,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Instance> {
    progress("Finding the modpack version", 0, 0);
    let version = modpack_version(client, project_id, version_id).await?;
    let project: Project = fetch_json(client, &format!("{API}/project/{}", enc(&version.project_id))).await?;
    let file = cached_mrpack(client, paths, &version, progress).await?;
    let origin = mrpack::Origin { project_id: Some(version.project_id.clone()), version_id: Some(version.id.clone()), icon_url: project.icon_url };
    mrpack::import(client, paths, &file, origin, progress).await
}

/// Updates a modpack instance to another version of its pack (the newest when `version_id`
/// is None), or with `reset`, puts the pack's own version back as installed.
pub async fn modpack_apply(
    client: &reqwest::Client,
    paths: &Paths,
    instance_id: &str,
    version_id: Option<&str>,
    reset: bool,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Instance> {
    let inst = instances::get(paths, instance_id)?;
    let pack = inst.pack.clone().ok_or_else(|| anyhow!("This instance isn't from a modpack"))?;
    let project_id = pack.project_id.clone().ok_or_else(|| anyhow!("This modpack was imported from a file, so Tessera can't fetch its versions"))?;
    progress("Finding the modpack version", 0, 0);
    let target = match (reset, version_id) {
        (true, _) => pack.version_id.as_deref().ok_or_else(|| anyhow!("Tessera doesn't know which version of the modpack this is"))?,
        (false, v) => v.unwrap_or_default(),
    };
    let version = modpack_version(client, &project_id, (!target.is_empty()).then_some(target)).await?;
    if version.project_id != project_id {
        bail!("That version belongs to another project");
    }
    // instances from before the launcher remembered the pack's files: read them from the old version
    let mut old_files = pack.files.clone();
    if old_files.is_empty() {
        if let Some(old) = &pack.version_id {
            let old = modpack_version(client, &project_id, Some(old)).await?;
            let file = cached_mrpack(client, paths, &old, progress).await?;
            old_files = mrpack::open(&file)?.all_paths()?;
        }
    }
    let file = cached_mrpack(client, paths, &version, progress).await?;
    let origin = mrpack::Origin { project_id: Some(project_id), version_id: Some(version.id.clone()), icon_url: None };
    let mode = if reset { mrpack::Mode::Reset } else { mrpack::Mode::Update };
    mrpack::apply(client, paths, inst, &file, origin, mode, &old_files, progress).await
}
