//! What the mods in a folder need that isn't there, from their own `fabric.mod.json` /
//! `quilt.mod.json` (and the jars bundled inside them). Catches what Modrinth's dependency
//! lists miss: a server can run without a client-only mod's dependencies (Fabric skips
//! client-only mods there), but a game made from it can't.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

/// Provided by the game and loader themselves.
const BUILT_IN: [&str; 6] = ["minecraft", "java", "fabricloader", "fabric-loader", "quilt_loader", "quilted_fabric_api"];

#[derive(Default, Debug)]
struct Found {
    provided: HashSet<String>,
    /// (mod id, the mod that needs it)
    needed: Vec<(String, String)>,
}

fn read_meta<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, found: &mut Found, depth: u32) {
    let mut nested = Vec::new();
    for name in ["fabric.mod.json", "quilt.mod.json"] {
        let Ok(mut f) = zip.by_name(name) else { continue };
        let mut text = String::new();
        if f.read_to_string(&mut text).is_err() {
            continue;
        }
        drop(f);
        // some mods put comments or control characters in theirs; serde is strict
        let Ok(v) = serde_json::from_str::<Value>(&text.replace(['\n', '\r', '\t'], " ")) else { continue };
        let (id, depends, provides, jars) = if name == "fabric.mod.json" {
            (v["id"].as_str(), v["depends"].clone(), v["provides"].clone(), v["jars"].clone())
        } else {
            let q = &v["quilt_loader"];
            let deps: serde_json::Map<String, Value> = q["depends"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d.as_str().map(String::from).or_else(|| (d["optional"] != true).then(|| d["id"].as_str().map(String::from)).flatten()))
                .map(|id| (id, Value::Null))
                .collect();
            let provides: Vec<Value> = q["provides"].as_array().into_iter().flatten().filter_map(|p| p.as_str().or(p["id"].as_str()).map(|s| Value::String(s.into()))).collect();
            (q["id"].as_str(), Value::Object(deps), Value::Array(provides), q["jars"].clone())
        };
        let Some(id) = id else { continue };
        found.provided.insert(id.to_string());
        for p in provides.as_array().into_iter().flatten().filter_map(Value::as_str) {
            found.provided.insert(p.to_string());
        }
        for dep in depends.as_object().into_iter().flatten().map(|(k, _)| k) {
            found.needed.push((dep.clone(), id.to_string()));
        }
        for j in jars.as_array().into_iter().flatten() {
            if let Some(file) = j["file"].as_str().or(j.as_str()) {
                nested.push(file.to_string());
            }
        }
    }
    if depth < 3 {
        for file in nested {
            let Ok(mut f) = zip.by_name(&file) else { continue };
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                drop(f);
                if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(buf)) {
                    read_meta(&mut inner, found, depth + 1);
                }
            }
        }
    }
}

fn scan_jar(path: &Path) -> Found {
    let mut found = Found::default();
    // only the metadata is read, not the whole jar
    if let Ok(file) = std::fs::File::open(path) {
        if let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(file)) {
            read_meta(&mut zip, &mut found, 0);
        }
    }
    found
}

/// The mod ids one jar provides (with the jars inside it).
pub fn ids(jar: &Path) -> HashSet<String> {
    scan_jar(jar).provided
}

/// Every mod id the jars in `mods` provide.
pub fn provided(mods: &Path) -> HashSet<String> {
    std::fs::read_dir(mods).into_iter().flatten().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "jar")).flat_map(|e| ids(&e.path())).collect()
}

/// Mod ids something in `mods` requires that no jar there provides, each with a mod that
/// needs it. Empty for Forge and NeoForge mods (they say it elsewhere).
pub fn missing(mods: &Path) -> Vec<(String, String)> {
    let mut found = Found::default();
    for e in std::fs::read_dir(mods).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "jar") {
            let f = scan_jar(&p);
            found.provided.extend(f.provided);
            found.needed.extend(f.needed);
        }
    }
    let mut seen = HashSet::new();
    found
        .needed
        .into_iter()
        .filter(|(id, _)| !BUILT_IN.contains(&id.as_str()) && !found.provided.contains(id) && !(id == "fabric" && found.provided.contains("fabric-api")))
        .filter(|(id, _)| seen.insert(id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(path: &Path, meta: &str, nested: Option<(&str, Vec<u8>)>) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("fabric.mod.json", o).unwrap();
        z.write_all(meta.as_bytes()).unwrap();
        if let Some((name, bytes)) = nested {
            z.start_file(name, o).unwrap();
            z.write_all(&bytes).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn finds_what_mods_need() {
        let dir = std::env::temp_dir().join(format!("tessera-moddeps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // a library inside another jar counts as there
        let inner = dir.join("inner.jar");
        jar(&inner, r#"{"id":"fabric-api-base","version":"1"}"#, None);
        jar(&dir.join("api.jar"), r#"{"id":"fabric-api","version":"1","jars":[{"file":"META-INF/jars/inner.jar"}]}"#, Some(("META-INF/jars/inner.jar", std::fs::read(&inner).unwrap())));
        std::fs::remove_file(inner).unwrap();
        jar(&dir.join("rep.jar"), r#"{"id":"roughlyenoughprofessions","version":"9","depends":{"minecraft":">=26.2","fabricloader":"*","roughlyenoughitems":">=26.2","fabric-api-base":"*"}}"#, None);
        assert_eq!(missing(&dir), vec![("roughlyenoughitems".to_string(), "roughlyenoughprofessions".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
