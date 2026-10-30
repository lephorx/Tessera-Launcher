//! "Get local pack" on a server: makes an instance that can join it. Same Minecraft version
//! and loader; for a modpack server its pack, for a Fabric or Forge server the mods in its
//! mods folder (without the ones Modrinth lists as server-only). The server goes into the
//! instance's multiplayer list.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use sha1::{Digest, Sha1};

use super::instances::{self, Icon, InstalledMod, Instance, Loader};
use super::paths::Paths;
use super::{loaders, modrinth, mrpack, serverlist, servers, versions};

type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Send + Sync);

/// CSCM's loader version as the launcher writes it (Forge's may carry the game version).
fn loader_version(raw: Option<&str>, game: &str) -> Option<String> {
    let v = raw?.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("latest") || v.eq_ignore_ascii_case("recommended") {
        return None;
    }
    Some(v.strip_prefix(&format!("{game}-")).unwrap_or(v).to_string())
}

pub async fn make(client: &reqwest::Client, paths: &Paths, machine: &str, server_id: i64, progress: Progress<'_>) -> Result<Instance> {
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
            if let Err(e) = copy_mods(client, paths, machine, server_id, &mut inst, progress).await {
                let _ = instances::delete(paths, &inst.id);
                return Err(e);
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
    Ok(inst)
}

/// The server's mods, except server-only ones; Modrinth's names and icons where it knows them.
async fn copy_mods(client: &reqwest::Client, paths: &Paths, machine: &str, server_id: i64, inst: &mut Instance, progress: Progress<'_>) -> Result<()> {
    let res = servers::request(client, paths, machine, "GET", &format!("/servers/{server_id}/files?path=mods"), None).await?;
    if res.status == 404 {
        return Ok(()); // no mods folder: plain Fabric / Forge
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
    for (jar, sha1, bytes) in files {
        let version = known.get(&sha1);
        let project = version.and_then(|v| projects.get(&v.project_id));
        if project.is_some_and(|p| p.client_side == "unsupported") {
            continue;
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
    Ok(())
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
        let inst = make(&client, &paths, &machine, pick, &|s, d, t| println!("  {s} {d}/{t}")).await.unwrap();
        println!("instance: {} {} {:?} {:?}, {} mods recorded", inst.name, inst.game_version, inst.loader, inst.loader_version, inst.mods.len());
        println!("mods folder: {:?}", std::fs::read_dir(paths.game_dir(&inst.id).join("mods")).map(|r| r.count()).ok());
        println!("server list: {:?}", serverlist::list(&paths.game_dir(&inst.id)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
