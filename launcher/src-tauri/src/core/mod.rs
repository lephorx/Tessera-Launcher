//! The launcher core: everything needed to get from "an instance" to a running game.
//! Written for Tessera from the public Mojang, Fabric and Modrinth APIs.

pub mod account;
pub mod assets;
pub mod debug;
pub mod download;
pub mod easyhost;
pub mod forge;
pub mod hostlocal;
pub mod instances;
pub mod import;
pub mod java;
pub mod launch;
pub mod limits;
pub mod livesync;
pub mod loaders;
pub mod mrpack;
pub mod msauth;
pub mod moddeps;
pub mod modrinth;
pub mod paths;
pub mod rcon;
pub mod serverclient;
pub mod serverlist;
pub mod serverpack;
pub mod servers;
pub mod settings;
pub mod share;
pub mod sharetrust;
pub mod skins;
pub mod storage;
pub mod tapack;
mod smoke;
pub mod versions;

/// Tessera Launcher Nightly: the experimental build from main (built with TESSERA_NIGHTLY set, see
/// .github/workflows/nightly.yml). It installs next to the released launcher with its own
/// name, data folder and update feed (tauri.nightly.conf.json).
pub const NIGHTLY: bool = option_env!("TESSERA_NIGHTLY").is_some();
pub const APP_NAME: &str = if NIGHTLY { "Tessera Launcher Nightly" } else { "Tessera Launcher" };

/// Identifies us to Mojang, Fabric and Modrinth (Modrinth requires a descriptive User-Agent).
pub const USER_AGENT: &str = concat!("tessera-launcher/", env!("CARGO_PKG_VERSION"), " (github.com/lephorx/Tessera-Launcher)");

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("http client")
}
