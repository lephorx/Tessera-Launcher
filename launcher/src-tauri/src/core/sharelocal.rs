//! What a copy of a shared pack keeps to itself:
//!
//! - **its own content** (`Instance::local_content`): mods, resource packs and shaders added
//!   on this computer only. Members who can only play add everything this way; editors choose.
//! - **unsynced parts** (`Instance::unsynced`): game settings, the pack's servers or its
//!   resource packs, which this copy doesn't take from the pack.
//!
//! Neither is published, and pulling a new revision leaves both as they are. An editor's
//! upload carries the pack's own version of what this copy unsynced (from the last pack
//! pulled, `share-base.tapack`), so unsyncing never takes anything from the others.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use reqwest::Method;

use super::account::api;
use super::instances::{self, content_dir, Instance};
use super::paths::Paths;
use super::tapack::{self, rel_string, walk, MANIFEST};

/// What an instance can stop taking from its pack.
pub const UNSYNC: [&str; 3] = ["options", "servers", "resourcepacks"];
const OPTIONS: [&str; 3] = ["options.txt", "optionsof.txt", "optionsshaders.txt"];
/// Content folders (the ones files can be this copy's own in).
pub const CONTENT: [&str; 5] = ["mods", "resourcepacks", "shaderpacks", "horizon-mods", "horizon-native-mods"];

fn base_rel(rel: &str) -> &str {
    rel.strip_suffix(".disabled").unwrap_or(rel)
}

fn in_category(key: &str, rel: &str) -> bool {
    match key {
        "options" => OPTIONS.contains(&rel),
        "resourcepacks" => rel.starts_with("resourcepacks/"),
        _ => false,
    }
}

/// In a part of the pack this copy doesn't take.
pub fn unsynced(inst: &Instance, rel: &str) -> bool {
    inst.unsynced.iter().any(|k| in_category(k, base_rel(rel)))
}

/// Added on this computer only.
pub fn is_local(inst: &Instance, rel: &str) -> bool {
    inst.local_content.iter().any(|l| l == base_rel(rel))
}

/// Neither published nor replaced by a pull.
pub fn own(inst: &Instance, rel: &str) -> bool {
    is_local(inst, rel) || unsynced(inst, rel)
}

/// Members who can only play keep what they add to themselves.
pub fn adds_locally(inst: &Instance) -> bool {
    inst.joined.as_ref().is_some_and(|j| j.role != "editor")
}

/// The content files in the game folder, without ".disabled".
pub fn content_files(paths: &Paths, id: &str) -> HashSet<String> {
    let game = paths.game_dir(id);
    let mut files = Vec::new();
    for folder in CONTENT {
        walk(&game, PathBuf::from(folder), &mut files);
    }
    files.iter().map(|f| base_rel(&rel_string(f)).to_string()).collect()
}

/// Content that appeared since `before` becomes this copy's own (`local`), on a shared instance.
pub fn mark_new(paths: &Paths, id: &str, before: &HashSet<String>, local: bool) -> Result<()> {
    if !local {
        return Ok(());
    }
    let mut inst = instances::get(paths, id)?;
    if super::livesync::link(&inst).is_none() {
        return Ok(());
    }
    let mut new: Vec<String> = content_files(paths, id).into_iter().filter(|f| !before.contains(f) && !is_local(&inst, f)).collect();
    if new.is_empty() {
        return Ok(());
    }
    new.sort();
    inst.local_content.extend(new);
    instances::save(paths, &inst)
}

/// A member's content that isn't in the last synced files (put in the folder by hand) is theirs.
pub fn adopt(paths: &Paths, id: &str, synced: &[String]) -> Result<()> {
    let synced: HashSet<String> = synced.iter().map(|f| base_rel(f).to_string()).collect();
    mark_new(paths, id, &synced, true)
}

/// Keeps a content file on this computer only, or puts it (back) in the pack.
pub fn set_local(paths: &Paths, id: &str, kind: &str, filename: &str, local: bool) -> Result<Instance> {
    if filename.contains(['/', '\\']) || filename.starts_with('.') || kind == "datapack" {
        bail!("invalid file name");
    }
    let mut inst = instances::get(paths, id)?;
    let (_, can_write) = super::livesync::link(&inst).ok_or_else(|| anyhow!("This instance isn't shared"))?;
    if !local && !can_write {
        bail!("Only the owner and editors can add content to the pack");
    }
    let rel = format!("{}/{}", content_dir(kind), base_rel(filename));
    inst.local_content.retain(|l| *l != rel);
    if local {
        inst.local_content.push(rel);
    }
    instances::save(paths, &inst)?;
    Ok(inst)
}

/// Stops (or starts again) taking a part of the pack. Taking it again brings the pack's
/// version at the next sync; resource packs this copy has keep as its own.
pub fn set_unsynced(paths: &Paths, id: &str, key: &str, unsynced: bool) -> Result<Instance> {
    if !UNSYNC.contains(&key) {
        bail!("unknown setting");
    }
    let mut inst = instances::get(paths, id)?;
    if inst.joined.is_none() {
        bail!("Only a copy of someone else's shared pack can keep parts of it to itself");
    }
    if inst.unsynced.iter().any(|k| k == key) == unsynced {
        return Ok(inst);
    }
    if unsynced {
        inst.unsynced.push(key.to_string());
        if key == "servers" {
            super::shareservers::clear(paths, &mut inst)?;
        }
    } else {
        inst.unsynced.retain(|k| k != key);
        if key == "resourcepacks" {
            let mut files = Vec::new();
            walk(&paths.game_dir(id), PathBuf::from("resourcepacks"), &mut files);
            for f in files {
                let rel = base_rel(&rel_string(&f)).to_string();
                if !inst.local_content.contains(&rel) {
                    inst.local_content.push(rel);
                }
            }
        }
        // the pack's version comes in at the next sync
        if key != "servers" {
            if let Some(s) = inst.share_sync.as_mut() {
                s.revision = 0;
            }
        }
    }
    instances::save(paths, &inst)?;
    Ok(inst)
}

/// This copy's own files, set aside while a pull replaces the shared folders.
pub struct Kept {
    dir: PathBuf,
    files: Vec<String>,
}

pub fn stash(paths: &Paths, inst: &Instance) -> Result<Kept> {
    let game = paths.game_dir(&inst.id);
    let dir = std::env::temp_dir().join(format!("tessera-own-{}", inst.id));
    let _ = std::fs::remove_dir_all(&dir);
    let mut found = Vec::new();
    for folder in CONTENT {
        walk(&game, PathBuf::from(folder), &mut found);
    }
    let mut files: Vec<String> = found.iter().map(|f| rel_string(f)).collect();
    files.extend(OPTIONS.iter().filter(|o| game.join(o).is_file()).map(|o| o.to_string()));
    files.retain(|f| own(inst, f));
    for rel in &files {
        let to = dir.join(rel);
        std::fs::create_dir_all(to.parent().unwrap_or(&dir))?;
        std::fs::copy(game.join(rel), &to)?;
    }
    Ok(Kept { dir, files })
}

/// Puts the kept files back over what a pull installed (`inst` is the instance after it,
/// `before` the one before), with what was known about them.
pub fn restore(paths: &Paths, before: &Instance, kept: Kept, inst: &mut Instance) -> Result<()> {
    let game = paths.game_dir(&inst.id);
    // what the pack brought for the parts this copy doesn't take goes
    if before.unsynced.iter().any(|k| k == "resourcepacks") {
        let _ = std::fs::remove_dir_all(game.join("resourcepacks"));
        inst.mods.retain(|m| m.kind != "resourcepack");
    }
    if before.unsynced.iter().any(|k| k == "options") {
        for o in OPTIONS {
            let _ = std::fs::remove_file(game.join(o));
        }
    }
    for rel in &kept.files {
        let to = game.join(rel);
        let plain = game.join(base_rel(rel));
        let _ = std::fs::remove_file(&plain);
        let _ = std::fs::remove_file(PathBuf::from(format!("{}.disabled", plain.display())));
        std::fs::create_dir_all(to.parent().unwrap_or(&game))?;
        std::fs::copy(kept.dir.join(rel), &to)?;
        let name = Path::new(base_rel(rel)).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(m) = before.mods.iter().find(|m| m.filename == name && rel.starts_with(&format!("{}/", content_dir(&m.kind)))) {
            if !inst.mods.iter().any(|x| x.filename == m.filename && x.kind == m.kind) {
                inst.mods.push(m.clone());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&kept.dir);
    Ok(())
}

/// The last pack pulled: the pack's own version of what this copy unsynced.
pub fn base_path(paths: &Paths, id: &str) -> PathBuf {
    paths.instance(id).join("share-base.tapack")
}

fn needs_base(inst: &Instance) -> bool {
    inst.unsynced.iter().any(|k| k == "options" || k == "resourcepacks")
}

/// The pack's version for an upload, fetched once when no pull saved it yet. An upload
/// without it would take the unsynced parts away from everyone, so it's needed.
pub async fn base(client: &reqwest::Client, paths: &Paths, inst: &Instance) -> Result<Option<PathBuf>> {
    if !needs_base(inst) {
        return Ok(None);
    }
    let file = base_path(paths, &inst.id);
    if file.is_file() {
        return Ok(Some(file));
    }
    let (code, _) = super::livesync::link(inst).ok_or_else(|| anyhow!("This instance isn't shared"))?;
    let dl = api(client, Method::GET, &format!("/shares/{code}/download"), None, None).await?;
    let url = dl["url"].as_str().ok_or_else(|| anyhow!("No download link"))?;
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    std::fs::write(&file, &bytes)?;
    Ok(Some(file))
}

/// Takes this copy's own files out of a pack about to be uploaded, and puts the pack's
/// version of the unsynced parts (from `base`) in their place.
pub fn filter_upload(pack: &Path, inst: &Instance, base: Option<&Path>) -> Result<()> {
    if inst.local_content.is_empty() && !needs_base(inst) {
        return Ok(());
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack)?)?;
    let mut m = tapack::read_manifest(&mut zip)?;
    m.content.retain(|c| !own(inst, &c.path));
    let mut from_base = None;
    if let Some(base) = base {
        let mut bz = zip::ZipArchive::new(std::fs::File::open(base)?)?;
        let bm = tapack::read_manifest(&mut bz)?;
        m.content.extend(bm.content.into_iter().filter(|c| unsynced(inst, &c.path)));
        from_base = Some(bz);
    }
    let tmp = pack.with_extension("tapack.own");
    let written = (|| -> Result<()> {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file(MANIFEST, deflated)?;
        w.write_all(&serde_json::to_vec_pretty(&m)?)?;
        let overridden = |name: &str| name.strip_prefix("overrides/").is_some_and(|rel| own(inst, rel));
        for i in 0..zip.len() {
            let e = zip.by_index_raw(i)?;
            if e.name() != MANIFEST && !overridden(e.name()) {
                w.raw_copy_file(e)?;
            }
        }
        if let Some(bz) = from_base.as_mut() {
            for i in 0..bz.len() {
                let e = bz.by_index_raw(i)?;
                if e.name().strip_prefix("overrides/").is_some_and(|rel| unsynced(inst, rel)) {
                    w.raw_copy_file(e)?;
                }
            }
        }
        w.finish()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, pack)?;
    Ok(())
}

/// Copies files picked on the computer into the instance: jars into mods/, zips into
/// shaderpacks/ when they hold shaders, else resourcepacks/.
pub fn add_files(paths: &Paths, id: &str, files: &[String], local: Option<bool>) -> Result<Instance> {
    let inst = instances::get(paths, id)?;
    let before = content_files(paths, id);
    let game = paths.game_dir(id);
    for f in files {
        let from = Path::new(f);
        let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| anyhow!("{f} isn't a file"))?;
        let lower = name.to_lowercase();
        let folder = if lower.ends_with(".jar") {
            if inst.loader == super::loaders::Loader::Vanilla {
                bail!("{name} is a mod, and vanilla Minecraft can't load mods");
            }
            "mods"
        } else if lower.ends_with(".zip") {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(from)?).map_err(|_| anyhow!("{name} isn't a zip file"))?;
            let shaders = (0..zip.len()).any(|i| zip.by_index_raw(i).is_ok_and(|e| e.name().split('/').take(2).any(|p| p == "shaders")));
            let mut mcmeta = String::new();
            let is_pack = zip.by_name("pack.mcmeta").is_ok_and(|mut e| e.read_to_string(&mut mcmeta).is_ok());
            if shaders && !is_pack {
                "shaderpacks"
            } else {
                "resourcepacks"
            }
        } else {
            bail!("{name}: only mods (.jar), resource packs and shaders (.zip) can be added");
        };
        let dir = game.join(folder);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(from, dir.join(&name))?;
    }
    mark_new(paths, id, &before, local.unwrap_or_else(|| adds_locally(&inst)))?;
    instances::get(paths, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{instances::JoinedShare, loaders::Loader};

    fn member(paths: &Paths, role: &str) -> Instance {
        let mut inst = instances::create(paths, "Shared", "1.21.1", Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        inst.joined = Some(JoinedShare { code: "AbCd2345".into(), role: role.into(), owner: None, updated_at: None });
        instances::save(paths, &inst).unwrap();
        inst
    }

    fn write(paths: &Paths, id: &str, rel: &str, text: &str) {
        let f = paths.game_dir(id).join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }

    #[test]
    fn own_files_survive_a_pull() {
        let base = std::env::temp_dir().join(format!("tessera-own-test-{}", std::process::id()));
        let paths = Paths::new(base.clone());
        let mut inst = member(&paths, "viewer");
        write(&paths, &inst.id, "mods/shared.jar", "s");
        write(&paths, &inst.id, "mods/mine.jar.disabled", "m");
        write(&paths, &inst.id, "options.txt", "fov:1\n");
        inst.local_content = vec!["mods/mine.jar".into()];
        inst.unsynced = vec!["options".into()];
        instances::save(&paths, &inst).unwrap();

        let kept = stash(&paths, &inst).unwrap();
        // the pull: the pack's folders and options replace this copy's
        std::fs::remove_dir_all(paths.game_dir(&inst.id).join("mods")).unwrap();
        write(&paths, &inst.id, "mods/shared.jar", "s2");
        write(&paths, &inst.id, "options.txt", "fov:0.5\n");
        let mut after = instances::get(&paths, &inst.id).unwrap();
        restore(&paths, &inst, kept, &mut after).unwrap();

        let game = paths.game_dir(&inst.id);
        assert_eq!(std::fs::read_to_string(game.join("mods/shared.jar")).unwrap(), "s2");
        assert_eq!(std::fs::read_to_string(game.join("mods/mine.jar.disabled")).unwrap(), "m");
        assert_eq!(std::fs::read_to_string(game.join("options.txt")).unwrap(), "fov:1\n");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn new_content_is_a_viewers_own() {
        let base = std::env::temp_dir().join(format!("tessera-own-new-{}", std::process::id()));
        let paths = Paths::new(base.clone());
        let inst = member(&paths, "viewer");
        write(&paths, &inst.id, "mods/shared.jar", "s");
        let before = content_files(&paths, &inst.id);
        write(&paths, &inst.id, "mods/mine.jar", "m");
        mark_new(&paths, &inst.id, &before, adds_locally(&inst)).unwrap();
        let inst = instances::get(&paths, &inst.id).unwrap();
        assert_eq!(inst.local_content, vec!["mods/mine.jar".to_string()]);
        assert!(own(&inst, "mods/mine.jar.disabled") && !own(&inst, "mods/shared.jar"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn uploads_leave_own_files_out_and_keep_the_packs() {
        let base = std::env::temp_dir().join(format!("tessera-own-up-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let pack = |path: &Path, files: &[(&str, &str)]| {
            let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
            let m = serde_json::json!({ "format": 1, "name": "P", "version": "1", "minecraft": "1.21.1", "loader": { "type": "fabric", "version": "0.16.9" }, "content": [] });
            w.start_file(MANIFEST, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(m.to_string().as_bytes()).unwrap();
            for (name, text) in files {
                w.start_file(format!("overrides/{name}"), zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(text.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        };
        let up = base.join("up.tapack");
        let shared = base.join("base.tapack");
        pack(&up, &[("mods/a.jar", "a"), ("mods/mine.jar", "m"), ("options.txt", "fov:1"), ("config/x.txt", "x")]);
        pack(&shared, &[("mods/old.jar", "o"), ("options.txt", "fov:0.5")]);
        let paths = Paths::new(base.join("data"));
        let mut inst = member(&paths, "editor");
        inst.local_content = vec!["mods/mine.jar".into()];
        inst.unsynced = vec!["options".into()];
        filter_upload(&up, &inst, Some(&shared)).unwrap();

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&up).unwrap()).unwrap();
        let mut names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["overrides/config/x.txt", "overrides/mods/a.jar", "overrides/options.txt", MANIFEST]);
        let mut options = String::new();
        zip.by_name("overrides/options.txt").unwrap().read_to_string(&mut options).unwrap();
        assert_eq!(options, "fov:0.5");
        let _ = std::fs::remove_dir_all(base);
    }
}
