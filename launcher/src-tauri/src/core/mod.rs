//! The launcher core: everything needed to get from "an instance" to a running game.
//! Written for Tessera from the public Mojang, Fabric and Modrinth APIs.

pub mod account;
pub mod assets;
pub mod download;
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
pub mod modrinth;
pub mod paths;
pub mod serverlist;
pub mod serverpack;
pub mod servers;
pub mod settings;
pub mod share;
pub mod skins;
pub mod tapack;
mod smoke;
pub mod versions;

/// Identifies us to Mojang, Fabric and Modrinth (Modrinth requires a descriptive User-Agent).
pub const USER_AGENT: &str = concat!("tessera-launcher/", env!("CARGO_PKG_VERSION"), " (github.com/lephorx/Tessera-Launcher)");

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("http client")
}
