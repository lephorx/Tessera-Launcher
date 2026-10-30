use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::paths::Paths;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// Offline player name until Microsoft sign-in is approved for our app.
    pub player_name: String,
    pub memory_mb: u32,
    /// "modern" | "classic", same values as the dashboard's localStorage["cscm-design"].
    pub design: String,
    /// Tessera backend, for accounts and syncing packs and servers.
    pub backend_url: String,
    /// Library groups, in the order they were made (also empty ones).
    pub groups: Vec<String>,
    /// Minecraft accounts added to the launcher (offline names for now); `player_name` is the
    /// one playing.
    pub players: Vec<String>,
    /// The Microsoft account playing (its profile id); None = the offline name in
    /// `player_name`. While one plays, `player_name` is its Minecraft name.
    pub microsoft: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            player_name: "Player".into(),
            memory_mb: 4096,
            design: "modern".into(),
            backend_url: "http://localhost:8080".into(),
            groups: Vec::new(),
            players: Vec::new(),
            microsoft: None,
        }
    }
}

pub fn load(paths: &Paths) -> Settings {
    let mut s: Settings = std::fs::read(paths.file("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    // older settings only had the one name
    if s.microsoft.is_none() && !s.players.iter().any(|p| p == &s.player_name) {
        s.players.insert(0, s.player_name.clone());
    }
    s
}

pub fn save(paths: &Paths, settings: &Settings) -> Result<()> {
    std::fs::create_dir_all(&paths.root)?;
    std::fs::write(paths.file("settings.json"), serde_json::to_vec_pretty(settings)?)?;
    Ok(())
}
