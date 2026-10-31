//! "Get local pack" on a server: makes an instance that can join it. Same Minecraft version
//! and loader; for a modpack server its pack, for a Fabric or Forge server the exact mod files
//! in its mods folder (without the ones Modrinth lists as server-only), plus what those need
//! that the server can do without: a client-only mod's dependencies (Fabric doesn't load
//! client-only mods on a server, so it doesn't check what they need there). The server goes
//! into the instance's multiplayer list.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use sha1::{Digest, Sha1};

use super::instances::{self, Icon, InstalledMod, Instance, Loader};
use super::paths::Paths;
use super::{loaders, moddeps, modrinth, mrpack, serverlist, servers, versions};

type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Send + Sync);

/// CSCM's loader version as the launcher writes it (Forge's may carry the game version).
fn loader_version(raw: Option<&str>, game: &str) -> Option<String> {
    let v = raw?.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("latest") || v.eq_ignore_ascii_case("recommended") {
        return None;
    }
    Some(v.strip_prefix(&format!("{game}-")).unwrap_or(v).to_string())
}

/// The instance, and mods it needs that couldn't be found (by mod id).
#[derive(serde::Serialize)]
pub struct Made {
    pub instance: Instance,
    pub missing: Vec<String>,
}

pub async fn make(client: &reqwest::Client, paths: &Paths, machine: &str, server_id: i64, progress: Progress<'_>) -> Result<Made> {
    progress("Asking the server", 0, 0);
    let res = servers::request(client, paths, machine, "GET", &format!("/servers/{server_id}"), None).await?;
    if res.status != 200 {
        bail!("{}", res.body["message"].as_str().unwrap_or("Couldn't load the server"));
    }
    let server = &res.body["server"];
    let name = server["name"].as_str().unwrap_or("Server").to_string();
    let kind = server["type"].as_str().unwrap_or_default();
    if kind == "bedrock" {
        bail!("{name} is a Bedrock server; it's played with Minecraft Bedrock Edition, not the launcher");
    }

    let mut missing = Vec::new();
    let mut inst = if kind == "modpack" {
        progress("Downloading the server's pack", 0, 0);
        let pack = servers::download_file(client, paths, machine, server_id, ".tessera/pack.mrpack")
            .await?
            .ok_or_else(|| anyhow!("{name} has no pack yet; start it once and try again"))?;
        let dir = std::env::temp_dir().join(format!("tessera-server-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("pack.mrpack");
        std::fs::write(&file, pack)?;
        let origin = mrpack::Origin { project_id: None, version_id: None, icon_url: None };
        let made = mrpack::import(client, paths, &file, origin, progress).await;
        let _ = std::fs::remove_dir_all(&dir);
        made?
    } else {
        let mut game = server["version"].as_str().unwrap_or_default().to_string();
        if game.is_empty() || game.eq_ignore_ascii_case("latest") {
            game = versions::manifest(client).await?.latest.release;
        }
        let loader = match kind {
            "fabric" => Loader::Fabric,
            "forge" => Loader::Forge,
            // Paper, Purpur and vanilla servers take a vanilla game; their plugins are server-side
            _ => Loader::Vanilla,
        };
        let lv = match loader {
            Loader::Vanilla => None,
            _ => match loader_version(server["loader_version"].as_str(), &game) {
                Some(v) => Some(v),
                None => {
                    let list = loaders::versions(client, loader, &game).await?;
                    Some(list.iter().find(|l| l.stable).or(list.first()).ok_or_else(|| anyhow!("No {loader:?} for Minecraft {game}"))?.version.clone())
                }
            },
        };
        let mut inst = instances::create(paths, &name, &game, loader, lv, None)?;
        if loader != Loader::Vanilla {
            match copy_mods(client, paths, machine, server_id, &mut inst, progress).await {
                Ok(m) => missing = m,
                Err(e) => {
                    let _ = instances::delete(paths, &inst.id);
                    return Err(e);
                }
            }
        }
        inst
    };

    inst.name = name.clone();
    if let Ok(Some(icon)) = servers::download_file(client, paths, machine, server_id, "server-icon.png").await {
        let file = std::env::temp_dir().join(format!("tessera-server-icon-{}.png", std::process::id()));
        if std::fs::write(&file, icon).is_ok() && instances::set_icon_file(paths, &inst.id, &file).is_ok() {
            inst.icon = Some(Icon::File);
        }
        let _ = std::fs::remove_file(file);
    }
    instances::save(paths, &inst)?;

    let machine_url = servers::machine_url(paths, machine).unwrap_or_default();
    if let Some(address) = servers::addresses(server, &machine_url).best() {
        serverlist::upsert(&paths.game_dir(&inst.id), None, &name, address)?;
    }
    Ok(Made { instance: inst, missing })
}

/// The server's mods, except server-only ones (Modrinth's names and icons where it knows
/// them), and what they need. Returns the mod ids still missing.
async fn copy_mods(client: &reqwest::Client, paths: &Paths, machine: &str, server_id: i64, inst: &mut Instance, progress: Progress<'_>) -> Result<Vec<String>> {
    let res = servers::request(client, paths, machine, "GET", &format!("/servers/{server_id}/files?path=mods"), None).await?;
    if res.status == 404 {
        return Ok(Vec::new()); // no mods folder: plain Fabric / Forge
    }
    let jars: Vec<String> = res.body["entries"]
        .as_array()
        .map(|e| e.iter().filter(|e| e["type"] == "file").filter_map(|e| e["name"].as_str()).filter(|n| n.ends_with(".jar")).map(String::from).collect())
        .unwrap_or_default();

    let mut files = Vec::new();
    for (i, jar) in jars.iter().enumerate() {
        progress("Downloading the server's mods", i, jars.len());
        let bytes = servers::download_file(client, paths, machine, server_id, &format!("mods/{jar}"))
            .await?
            .with_context(|| format!("{jar} disappeared from the server"))?;
        files.push((jar.clone(), hex::encode(Sha1::digest(&bytes)), bytes));
    }
    progress("Checking which mods players need", jars.len(), jars.len());
    let known = modrinth::versions_by_sha1(client, files.iter().map(|f| f.1.clone()).collect()).await.unwrap_or_default();
    let ids: Vec<String> = known.values().map(|v| v.project_id.clone()).collect();
    let projects = modrinth::projects(client, &ids).await.unwrap_or_default();

    let mods = paths.game_dir(&inst.id).join("mods");
    std::fs::create_dir_all(&mods)?;
    let mut copied = Vec::new();
    for (jar, sha1, bytes) in files {
        let version = known.get(&sha1);
        let project = version.and_then(|v| projects.get(&v.project_id));
        if project.is_some_and(|p| p.client_side == "unsupported") {
            continue;
        }
        if let Some(v) = version {
            copied.push(v);
        }
        write_new(&mods.join(&jar), &bytes)?;
        if let (Some(v), Some(p)) = (version, project) {
            inst.mods.push(InstalledMod {
                kind: "mod".into(),
                world: None,
                project_id: v.project_id.clone(),
                version_id: v.id.clone(),
                title: p.title.clone(),
                version: v.version_number.clone(),
                filename: jar,
                icon_url: p.icon_url.clone(),
            });
        }
    }
    complete(client, paths, inst, &copied, progress).await
}

/// Adds what the mods need: the dependencies Modrinth lists (the exact versions they name),
/// then anything their own metadata asks for that still isn't there (by what Modrinth knows
/// about the jars in the folder, then by mod id). Returns the mod ids that couldn't be found.
pub async fn complete(client: &reqwest::Client, paths: &Paths, inst: &mut Instance, versions: &[&modrinth::Version], progress: Progress<'_>) -> Result<Vec<String>> {
    progress("Adding what the mods need", 0, 0);
    let mods = paths.game_dir(&inst.id).join("mods");
    let before = inst.mods.len();
    let there = moddeps::provided(&mods);
    modrinth::add_dependencies(client, paths, inst, versions, &[]).await?;
    let result = complete_more(client, paths, inst, &mods).await;
    // a mod that was there already as a jar Modrinth doesn't know: the copy just added goes
    // (two of the same mod stop the game too)
    let mut i = before;
    while i < inst.mods.len() {
        let file = mods.join(&inst.mods[i].filename);
        let ids = moddeps::ids(&file);
        if !ids.is_empty() && ids.iter().all(|id| there.contains(id)) {
            let _ = std::fs::remove_file(&file);
            inst.mods.remove(i);
        } else {
            i += 1;
        }
    }
    result
}

async fn complete_more(client: &reqwest::Client, paths: &Paths, inst: &mut Instance, mods: &Path) -> Result<Vec<String>> {
    let mods = mods.to_path_buf();
    if moddeps::missing(&mods).is_empty() {
        return Ok(Vec::new());
    }
    // the jars there, as Modrinth knows them: their dependency lists name what's missing
    let hashes: Vec<String> = std::fs::read_dir(&mods)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jar"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .map(|b| hex::encode(Sha1::digest(&b)))
        .collect();
    let known = modrinth::versions_by_sha1(client, hashes).await.unwrap_or_default();
    let all: Vec<&modrinth::Version> = known.values().collect();
    modrinth::add_dependencies(client, paths, inst, &all, &[]).await?;
    let missing = moddeps::missing(&mods);
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let loaders = inst.loader.modrinth_loaders().to_vec();
    let mut wanted = Vec::new();
    let mut not_found = Vec::new();
    for (id, _) in &missing {
        match modrinth::project_for_mod_id(client, id, &inst.game_version, &loaders).await {
            Some(p) => wanted.push(p),
            None => not_found.push(id.clone()),
        }
    }
    modrinth::add_dependencies(client, paths, inst, &[], &wanted).await?;
    // what's still missing after that (a project Modrinth has, but not for this version)
    let still: Vec<String> = moddeps::missing(&mods).into_iter().map(|(id, by)| format!("{id} (for {by})")).collect();
    Ok(if still.is_empty() { not_found } else { still })
}

/// Server file names come from the server: only plain names, never paths.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        bail!("The server has a mod file with an odd name: {name}");
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_loader_versions() {
        assert_eq!(loader_version(Some("0.16.9"), "1.21.1").as_deref(), Some("0.16.9"));
        assert_eq!(loader_version(Some("1.20.1-47.4.0"), "1.20.1").as_deref(), Some("47.4.0"));
        assert_eq!(loader_version(Some("RECOMMENDED"), "1.20.1"), None);
        assert_eq!(loader_version(None, "1.20.1"), None);
    }
}

#[cfg(test)]
mod real_machine {
    use super::*;

    /// TESSERA_DATA=<launcher data folder> cargo test real_local_pack -- --ignored --nocapture
    /// Only reads from the machine; the instance is made in a temporary copy of the folder.
    #[tokio::test]
    #[ignore]
    async fn real_local_pack() {
        let data = std::path::PathBuf::from(std::env::var("TESSERA_DATA").expect("TESSERA_DATA"));
        let root = std::env::temp_dir().join(format!("tessera-local-pack-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::copy(data.join("servers.json"), root.join("servers.json")).unwrap();
        let paths = Paths::new(root.clone());
        let client = crate::core::http_client();
        let machine = servers::list(&paths)[0].id.clone();
        let pick: i64 = match std::env::var("TESSERA_SERVER").ok().and_then(|s| s.parse().ok()) {
            Some(id) => id,
            None => {
                let list = servers::request(&client, &paths, &machine, "GET", "/servers", None).await.unwrap();
                let all = list.body["servers"].as_array().expect("signed in to the machine?").clone();
                for s in &all {
                    println!("server {} {} {} {}", s["id"], s["name"], s["type"], s["version"]);
                }
                all.iter().find(|s| s["type"] != "bedrock").unwrap()["id"].as_i64().unwrap()
            }
        };
        let made = make(&client, &paths, &machine, pick, &|s, d, t| println!("  {s} {d}/{t}")).await.unwrap();
        println!("missing: {:?}", made.missing);
        let inst = made.instance;
        println!("instance: {} {} {:?} {:?}, {} mods recorded", inst.name, inst.game_version, inst.loader, inst.loader_version, inst.mods.len());
        println!("mods folder: {:?}", std::fs::read_dir(paths.game_dir(&inst.id).join("mods")).map(|r| r.count()).ok());
        println!("server list: {:?}", serverlist::list(&paths.game_dir(&inst.id)));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod real_dependencies {
    use super::*;

    /// cargo test real_missing_dependencies -- --ignored --nocapture
    /// A Fabric 26.2 instance with only Roughly Enough Professions (it needs Roughly Enough
    /// Items): once through Modrinth's dependency list, once from the jar's own metadata.
    #[tokio::test]
    #[ignore]
    async fn real_missing_dependencies() {
        let client = crate::core::http_client();
        let rep = modrinth::versions_by_sha1(&client, vec![]).await.unwrap();
        assert!(rep.is_empty());
        let v: serde_json::Value = crate::core::download::fetch_json(&client, "https://api.modrinth.com/v2/project/V8XJ8f5f/version?loaders=%5B%22fabric%22%5D&game_versions=%5B%2226.2%22%5D").await.unwrap();
        let file = &v[0]["files"][0];
        let jar = client.get(file["url"].as_str().unwrap()).send().await.unwrap().bytes().await.unwrap();
        let sha1 = file["hashes"]["sha1"].as_str().unwrap().to_string();
        for through_modrinth in [true, false] {
            let root = std::env::temp_dir().join(format!("tessera-deps-{}-{through_modrinth}", std::process::id()));
            let paths = Paths::new(root.clone());
            let mut inst = instances::create(&paths, "Deps", "26.2", Loader::Fabric, Some("0.20.1".into()), None).unwrap();
            let mods = paths.game_dir(&inst.id).join("mods");
            std::fs::create_dir_all(&mods).unwrap();
            std::fs::write(mods.join(file["filename"].as_str().unwrap()), &jar).unwrap();
            println!("missing before: {:?}", moddeps::missing(&mods));
            let known = if through_modrinth { modrinth::versions_by_sha1(&client, vec![sha1.clone()]).await.unwrap() } else { Default::default() };
            let versions: Vec<&modrinth::Version> = known.values().collect();
            let still = complete(&client, &paths, &mut inst, &versions, &|_, _, _| {}).await.unwrap();
            let titles: Vec<&str> = inst.mods.iter().map(|m| m.title.as_str()).collect();
            println!("through Modrinth's list: {through_modrinth}: added {titles:?}, still missing {still:?}");
            println!("  files: {:?}", std::fs::read_dir(&mods).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>());
            assert!(titles.iter().any(|t| t.contains("Roughly Enough Items")), "REI wasn't added");
            assert!(moddeps::missing(&mods).is_empty(), "still missing: {:?}", moddeps::missing(&mods));
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
