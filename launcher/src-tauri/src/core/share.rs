//! Sharing instances through the Tessera API: the instance is exported as a .tapack
//! (content from Modrinth linked, not copied), uploaded, and reachable by invite link or by
//! friends added by username. Joining downloads the pack and imports it.

use anyhow::{anyhow, bail, Result};
use reqwest::Method;
use serde_json::{json, Value};

use super::account::{api, share_code};
use super::instances::{self, Instance, JoinedShare};
use super::paths::Paths;
use super::tapack::{self, ExportOptions};

pub const LINK_BASE: &str = "https://tesseralauncher.com/share/";

pub(crate) fn query(inst: &Instance) -> String {
    let loader = serde_json::to_value(inst.loader).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    format!("name={}&game_version={}&loader={}", enc(&inst.name), enc(&inst.game_version), loader)
}

/// What a shared pack carries, both ways: sharing uploads only these, and an update from the
/// share can only write these. Everything else (worlds, logs, screenshots, server lists,
/// mods' account files) stays on each computer.
pub const SHARED: &[&str] = &[
    "mods",
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts", // CraftTweaker
    "resourcepacks",
    "shaderpacks",
    "options.txt",
    "optionsof.txt",      // OptiFine
    "optionsshaders.txt", // OptiFine and Iris shader settings
];

/// A path in the game folder that belongs in a shared pack.
pub fn shared_path(rel: &str) -> bool {
    let top = rel.split('/').next().unwrap_or("");
    SHARED.contains(&top) && !tapack::private_file(rel)
}

/// The instance as a .tapack in memory: the shared folders, plus the share's jars this copy
/// holds back (so not taking a jar here doesn't remove it for everyone).
pub(crate) async fn pack_bytes(
    client: &reqwest::Client,
    paths: &Paths,
    inst: &Instance,
    progress: &(dyn Fn(&str, usize, usize) + Send + Sync),
) -> Result<Vec<u8>> {
    let include: Vec<String> = tapack::entries(paths, &inst.id)?.into_iter().filter(|e| SHARED.contains(&e.name.as_str())).map(|e| e.name).collect();
    let tmp = std::env::temp_dir().join(format!("tessera-share-{}.tapack", inst.id));
    let opts = ExportOptions { name: inst.name.clone(), version: "1".into(), author: String::new(), summary: String::new(), include, format: Default::default() };
    let made = async {
        tapack::export(client, paths, &inst.id, &tmp, opts, progress).await?;
        super::sharetrust::carry_held(paths, &inst.id, &tmp)?;
        // the last server joined is this computer's business
        super::sharetrust::strip_last_server(&tmp)?;
        Ok::<_, anyhow::Error>(std::fs::read(&tmp)?)
    }
    .await;
    let _ = std::fs::remove_file(&tmp);
    made
}

/// What sharing this instance uploads, for the check before the first upload: each shared
/// file or folder with its size and file count, and what stays on this computer.
pub fn preview(paths: &Paths, id: &str) -> Result<Value> {
    let game = paths.game_dir(id);
    let mut shared = Vec::new();
    let mut left_out = Vec::new();
    for e in tapack::entries(paths, id)? {
        if !SHARED.contains(&e.name.as_str()) {
            left_out.push(e.name);
            continue;
        }
        let mut files = Vec::new();
        tapack::walk(&game, std::path::PathBuf::from(&e.name), &mut files);
        let (kept, private): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| !tapack::private_file(&tapack::rel_string(f)));
        let size: u64 = kept.iter().filter_map(|f| std::fs::metadata(game.join(f)).ok()).map(|m| m.len()).sum();
        if kept.is_empty() {
            continue;
        }
        shared.push(json!({ "name": e.name, "is_dir": e.is_dir, "size": size, "files": kept.len(), "private": private.len() }));
    }
    Ok(json!({ "shared": shared, "left_out": left_out }))
}

/// The share for this instance (with friends), or null when it isn't shared.
pub async fn status(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<Value> {
    let inst = instances::get(paths, id)?;
    let Some(code) = inst.share_code.clone() else { return Ok(Value::Null) };
    let mine = api(client, Method::GET, "/shares/mine", None, None).await?;
    let share = mine["shares"].as_array().and_then(|list| list.iter().find(|s| s["code"] == code.as_str()).cloned());
    match share {
        Some(mut s) => {
            s["link"] = json!(format!("{LINK_BASE}{code}"));
            Ok(s)
        }
        None => {
            // deleted elsewhere (website, another computer)
            let mut inst = inst;
            inst.share_code = None;
            instances::save(paths, &inst)?;
            Ok(Value::Null)
        }
    }
}

pub async fn create(client: &reqwest::Client, paths: &Paths, id: &str, progress: &(dyn Fn(&str, usize, usize) + Send + Sync)) -> Result<Value> {
    let mut inst = instances::get(paths, id)?;
    let bytes = pack_bytes(client, paths, &inst, progress).await?;
    progress("Uploading", 0, 0);
    let res = api(client, Method::POST, &format!("/shares?{}", query(&inst)), None, Some(bytes)).await?;
    inst.share_code = res["share"]["code"].as_str().map(String::from);
    instances::save(paths, &inst)?;
    super::livesync::mark_synced(paths, id, res["share"]["revision"].as_i64().unwrap_or(1))?;
    status(client, paths, id).await
}

/// Uploads the instance as it is now, so friends get the new mods and configs.
pub async fn update(client: &reqwest::Client, paths: &Paths, id: &str, progress: &(dyn Fn(&str, usize, usize) + Send + Sync)) -> Result<Value> {
    let inst = instances::get(paths, id)?;
    let code = inst.share_code.clone().ok_or_else(|| anyhow!("This instance isn't shared"))?;
    let bytes = pack_bytes(client, paths, &inst, progress).await?;
    progress("Uploading", 0, 0);
    let res = api(client, Method::PUT, &format!("/shares/{code}?{}", query(&inst)), None, Some(bytes)).await?;
    if let Some(rev) = res["revision"].as_i64() {
        super::livesync::mark_synced(paths, id, rev)?;
    }
    status(client, paths, id).await
}

pub async fn stop(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<()> {
    let mut inst = instances::get(paths, id)?;
    if let Some(code) = inst.share_code.take() {
        api(client, Method::DELETE, &format!("/shares/{code}"), None, None).await?;
        instances::save(paths, &inst)?;
    }
    Ok(())
}

/// Downloads a shared pack (invite link, code, or one shared with you) and installs it.
pub async fn join(client: &reqwest::Client, paths: &Paths, link: &str, progress: &(dyn Fn(&str, usize, usize) + Send + Sync)) -> Result<Instance> {
    let code = share_code(link)?;
    progress("Finding the pack", 0, 0);
    let dl = api(client, Method::GET, &format!("/shares/{code}/download"), None, None).await?;
    let url = dl["url"].as_str().ok_or_else(|| anyhow!("No download link"))?;
    progress("Downloading pack", 0, 0);
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let tmp = std::env::temp_dir().join(format!("tessera-join-{code}.tapack"));
    std::fs::write(&tmp, &bytes)?;
    let info = api(client, Method::GET, &format!("/shares/{code}"), None, None).await.ok();
    // who the popup names: the pack's owner (the share answer doesn't say who changed it last)
    let owner = info.as_ref().and_then(|i| i["share"]["owner"].as_str().map(String::from));
    // checked like any update: jars Tessera doesn't know wait for the member's answer
    let result = super::sharetrust::install_new(client, paths, &tmp, owner.as_deref(), progress).await;
    let _ = std::fs::remove_file(&tmp);
    let mut inst = result?;
    // remember where it came from, for updates (and edits, for editors)
    if let Some(info) = info {
        let share = &info["share"];
        if share["role"] == "viewer" || share["role"] == "editor" {
            inst.joined = Some(JoinedShare {
                code: code.clone(),
                role: share["role"].as_str().unwrap_or("viewer").into(),
                owner: share["owner"].as_str().map(String::from),
                updated_at: share["updated_at"].as_str().map(String::from),
            });
            instances::save(paths, &inst)?;
            super::livesync::mark_synced(paths, &inst.id, share["revision"].as_i64().unwrap_or(0))?;
        }
    }
    Ok(inst)
}

/// A joined instance's share: role, owner, and whether a newer version is up. Follows
/// changes made elsewhere: a new role, being made the owner, or being removed.
pub async fn member_status(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<Value> {
    let mut inst = instances::get(paths, id)?;
    let Some(joined) = inst.joined.clone() else { return Ok(Value::Null) };
    let info = match api(client, Method::GET, &format!("/shares/{}", joined.code), None, None).await {
        Ok(v) => v,
        Err(e) if e.to_string().contains("not_found") || e.to_string().contains("doesn't exist") => json!({ "share": null }),
        Err(e) => return Err(e),
    };
    let share = info["share"].clone();
    let role = share["role"].as_str().map(String::from);
    match role.as_deref() {
        // made the owner: it's your share now
        Some("owner") => {
            inst.joined = None;
            inst.share_code = Some(joined.code.clone());
            instances::save(paths, &inst)?;
            return Ok(json!({ "became_owner": true }));
        }
        Some(r) => {
            let mut changed = false;
            if r != joined.role || share["owner"].as_str() != joined.owner.as_deref() {
                inst.joined = Some(JoinedShare { role: r.into(), owner: share["owner"].as_str().map(String::from), ..joined.clone() });
                changed = true;
            }
            if changed {
                instances::save(paths, &inst)?;
            }
        }
        None => {}
    }
    let update_available = share["updated_at"].as_str().is_some_and(|u| Some(u) != joined.updated_at.as_deref());
    Ok(json!({
        "code": joined.code,
        "role": role,
        "owner": share["owner"],
        "name": share["name"],
        "updated_at": share["updated_at"],
        "update_available": role.is_some() && update_available,
        // removed from the share, or it was deleted
        "removed": role.is_none(),
    }))
}

/// Installs the share's newest version over the instance (worlds stay).
pub async fn pull(client: &reqwest::Client, paths: &Paths, id: &str, progress: &(dyn Fn(&str, usize, usize) + Send + Sync)) -> Result<Instance> {
    let inst = instances::get(paths, id)?;
    let joined = inst.joined.clone().ok_or_else(|| anyhow!("This instance doesn't come from a share"))?;
    progress("Finding the pack", 0, 0);
    let info = api(client, Method::GET, &format!("/shares/{}", joined.code), None, None).await?;
    let dl = api(client, Method::GET, &format!("/shares/{}/download", joined.code), None, None).await?;
    let url = dl["url"].as_str().ok_or_else(|| anyhow!("No download link"))?;
    progress("Downloading pack", 0, 0);
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let tmp = std::env::temp_dir().join(format!("tessera-pull-{}.tapack", joined.code));
    std::fs::write(&tmp, &bytes)?;
    // the same checks as live sync: unknown jars and new versions wait for an answer
    let checked = super::sharetrust::prepare(client, paths, id, &tmp, info["share"]["revision"].as_i64().unwrap_or(0), info["share"]["updated_by"].as_str()).await;
    let _ = std::fs::remove_file(&tmp);
    let safe = match checked? {
        super::sharetrust::Prepared::Ready { pack, .. } => pack,
        super::sharetrust::Prepared::Version(vc) => bail!("The pack moved to Minecraft {}. Accept the change in the Share tab first.", vc.to.minecraft),
    };
    let result = tapack::apply_update(client, paths, id, &safe, progress).await;
    let _ = std::fs::remove_file(&safe);
    let mut inst = result?;
    inst.joined = Some(JoinedShare { updated_at: info["share"]["updated_at"].as_str().map(String::from), ..joined });
    instances::save(paths, &inst)?;
    super::livesync::mark_synced(paths, id, info["share"]["revision"].as_i64().unwrap_or(0))?;
    instances::get(paths, id)
}

/// Editors: upload this copy as the share's new version (same Minecraft and loader).
pub async fn publish(client: &reqwest::Client, paths: &Paths, id: &str, progress: &(dyn Fn(&str, usize, usize) + Send + Sync)) -> Result<()> {
    let inst = instances::get(paths, id)?;
    let joined = inst.joined.clone().ok_or_else(|| anyhow!("This instance doesn't come from a share"))?;
    if joined.role != "editor" {
        bail!("You can install this pack but not change it. Ask the owner to make you an editor.");
    }
    let bytes = pack_bytes(client, paths, &inst, progress).await?;
    progress("Uploading", 0, 0);
    let res = api(client, Method::PUT, &format!("/shares/{}?{}", joined.code, query(&inst)), None, Some(bytes)).await?;
    if let Some(rev) = res["revision"].as_i64() {
        super::livesync::mark_synced(paths, id, rev)?;
    }
    // what we uploaded is the newest version now
    let info = api(client, Method::GET, &format!("/shares/{}", joined.code), None, None).await?;
    let mut inst = instances::get(paths, id)?;
    inst.joined = Some(JoinedShare { updated_at: info["share"]["updated_at"].as_str().map(String::from), ..joined });
    instances::save(paths, &inst)?;
    Ok(())
}

/// Leave a share you were added to; the installed copy stays as a normal instance.
pub async fn leave(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<()> {
    let mut inst = instances::get(paths, id)?;
    let Some(joined) = inst.joined.clone() else { return Ok(()) };
    let me = super::account::current(client).await.ok_or_else(|| anyhow!("Sign in first"))?;
    match api(client, Method::DELETE, &format!("/shares/{}/members/{}", joined.code, me.id), None, None).await {
        Ok(_) => {}
        // already off it, or the pack was deleted: only this copy is left to unlink
        Err(e) if e.to_string().contains("not_found") => {}
        Err(e) => return Err(e.context("Couldn't leave the pack")),
    }
    inst.joined = None;
    inst.share_sync = None;
    super::shareservers::clear(paths, &mut inst)?;
    instances::save(paths, &inst)?;
    Ok(())
}

/// Before a joined instance is deleted: off the pack too, so its owner doesn't keep a member
/// who has nothing to sync. Offline or signed out, the instance goes anyway.
pub async fn leave_quietly(client: &reqwest::Client, paths: &Paths, id: &str) {
    let Ok(inst) = instances::get(paths, id) else { return };
    let Some(joined) = inst.joined else { return };
    if let Some(me) = super::account::current(client).await {
        let _ = api(client, Method::DELETE, &format!("/shares/{}/members/{}", joined.code, me.id), None, None).await;
    }
}

/// Hands your share to a friend on it; your instance stays linked as an editor.
pub async fn transfer(client: &reqwest::Client, paths: &Paths, id: &str, user_id: &str) -> Result<()> {
    let mut inst = instances::get(paths, id)?;
    let code = inst.share_code.clone().ok_or_else(|| anyhow!("This instance isn't shared"))?;
    if !user_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        bail!("Unknown friend");
    }
    api(client, Method::POST, &format!("/shares/{code}/transfer"), Some(json!({ "user_id": user_id })), None).await?;
    let info = api(client, Method::GET, &format!("/shares/{code}"), None, None).await?;
    inst.share_code = None;
    inst.joined = Some(JoinedShare {
        code,
        role: "editor".into(),
        owner: info["share"]["owner"].as_str().map(String::from),
        updated_at: info["share"]["updated_at"].as_str().map(String::from),
    });
    instances::save(paths, &inst)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_allowlist_is_shared() {
        for yes in ["mods/sodium.jar", "config/sodium-options.json", "kubejs/server_scripts/a.js", "scripts/recipes.zs", "defaultconfigs/x.toml", "options.txt", "shaderpacks/bsl.zip"] {
            assert!(shared_path(yes), "{yes}");
        }
        for no in ["saves/World/level.dat", "logs/latest.log", "crash-reports/x.txt", "screenshots/a.png", "servers.dat", "essential/microsoft_accounts.json", "config/x/accounts.json", "journeymap/data.dat", "usercache.json", "options.txt.bak/x"] {
            assert!(!shared_path(no), "{no}");
        }
    }
}
