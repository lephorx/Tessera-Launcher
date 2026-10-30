use std::path::{Path, PathBuf};

/// Where everything lives, below the app data dir:
///
/// ```text
/// meta/versions/<id>/<id>.json, <id>.jar   game versions
/// meta/libraries/…                         maven layout, shared by all instances
/// meta/assets/{indexes,objects}/…          sounds, languages
/// meta/java/<component>/…                  Java runtimes from Mojang
/// instances/<id>/instance.json             one folder per instance
/// instances/<id>/game/                     the game directory (mods, saves, options)
/// settings.json, servers.json
/// ```
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.meta().join("versions").join(id)
    }
    pub fn libraries(&self) -> PathBuf {
        self.meta().join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.meta().join("assets")
    }
    pub fn java(&self, component: &str) -> PathBuf {
        self.meta().join("java").join(component)
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn instance(&self, id: &str) -> PathBuf {
        self.instances().join(id)
    }
    pub fn game_dir(&self, id: &str) -> PathBuf {
        self.instance(id).join("game")
    }
    pub fn natives(&self, instance: &str) -> PathBuf {
        self.instance(instance).join("natives")
    }
    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

/// `group:artifact:version[:classifier]` → `group/path/artifact/version/artifact-version[-classifier].jar`
pub fn maven_path(coords: &str) -> Option<PathBuf> {
    // "group:artifact:version[:classifier][@ext]", ext defaults to jar
    let (coords, ext) = coords.split_once('@').unwrap_or((coords, "jar"));
    let mut parts = coords.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();
    let file = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    let mut path = PathBuf::new();
    for g in group.split('.') {
        path.push(g);
    }
    Some(path.join(artifact).join(version).join(file))
}

pub fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.16.9").unwrap(),
            PathBuf::from("net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar")
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-macos-arm64").unwrap(),
            PathBuf::from("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-macos-arm64.jar")
        );
        assert_eq!(
            maven_path("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412:mappings@txt").unwrap(),
            PathBuf::from("de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412-mappings.txt")
        );
    }
}
