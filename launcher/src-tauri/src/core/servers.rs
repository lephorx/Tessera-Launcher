//! CSCM machines the launcher talks to directly: on this computer, on the LAN, or over
//! Tailscale (e.g. http://100.101.102.103:3000). Requests go to the dashboard's `/api` proxy
//! with a CSCM login token, like the dashboard itself does.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::paths::Paths;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Machine {
    pub id: String,
    pub name: String,
    /// Dashboard address, e.g. http://localhost:3000 or http://my-pc.tailnet.ts.net:3000
    pub url: String,
    pub username: String,
    /// CSCM login token, in machines.json in the launcher's data folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub added_at: DateTime<Utc>,
}

/// What the UI gets: never the token.
#[derive(Serialize, Clone, Debug)]
pub struct MachineView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    pub signed_in: bool,
}

impl From<&Machine> for MachineView {
    fn from(m: &Machine) -> Self {
        Self { id: m.id.clone(), name: m.name.clone(), url: m.url.clone(), username: m.username.clone(), signed_in: m.token.is_some() }
    }
}

fn load(paths: &Paths) -> Vec<Machine> {
    std::fs::read(paths.file("servers.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn store(paths: &Paths, machines: &[Machine]) -> Result<()> {
    std::fs::create_dir_all(&paths.root)?;
    let file = paths.file("servers.json");
    std::fs::write(&file, serde_json::to_vec_pretty(machines)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn list(paths: &Paths) -> Vec<MachineView> {
    load(paths).iter().map(MachineView::from).collect()
}

/// Accepts "100.64.0.5:3000", "my-pc.local", "http://…/"; returns "http://host:port".
pub fn normalise_url(input: &str) -> Result<String> {
    let mut url = input.trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        bail!("Enter the address of the CSCM dashboard");
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{url}");
    }
    let parsed = reqwest::Url::parse(&url).map_err(|_| anyhow!("That address doesn't look right"))?;
    if parsed.host_str().is_none() {
        bail!("That address doesn't look right");
    }
    Ok(url)
}

async fn login(client: &reqwest::Client, url: &str, username: &str, password: &str, otp: &str) -> Result<String> {
    if !super::limits::login_allowed(url) {
        bail!("Too many sign-in attempts. Wait a minute and try again.");
    }
    let res = client
        .post(format!("{url}/api/auth/login"))
        .json(&serde_json::json!({ "username": username, "password": password, "otp": otp }))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach {url}: {}", e.without_url()))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = body.get("error").and_then(Value::as_str).unwrap_or("Sign-in failed");
        bail!("{msg}");
    }
    body.get("token").and_then(Value::as_str).map(String::from).ok_or_else(|| anyhow!("{url} is not a CSCM dashboard"))
}

pub async fn add(client: &reqwest::Client, paths: &Paths, name: &str, url: &str, username: &str, password: &str, otp: &str) -> Result<MachineView> {
    let url = normalise_url(url)?;
    let token = login(client, &url, username, password, otp).await?;
    let mut machines = load(paths);
    // Same dashboard and user again: just refresh the login.
    if let Some(m) = machines.iter_mut().find(|m| m.url == url && m.username == username) {
        m.token = Some(token);
        let view = MachineView::from(&*m);
        store(paths, &machines)?;
        return Ok(view);
    }
    let host = reqwest::Url::parse(&url)?.host_str().unwrap_or("CSCM").to_string();
    let machine = Machine {
        id: format!("m{}", Utc::now().timestamp_millis()),
        name: if name.trim().is_empty() { host } else { name.trim().into() },
        url,
        username: username.into(),
        token: Some(token),
        added_at: Utc::now(),
    };
    let view = MachineView::from(&machine);
    machines.push(machine);
    store(paths, &machines)?;
    Ok(view)
}

pub async fn sign_in(client: &reqwest::Client, paths: &Paths, id: &str, password: &str, otp: &str) -> Result<MachineView> {
    let mut machines = load(paths);
    let m = machines.iter_mut().find(|m| m.id == id).ok_or_else(|| anyhow!("Unknown server"))?;
    m.token = Some(login(client, &m.url, &m.username, password, otp).await?);
    let view = MachineView::from(&*m);
    store(paths, &machines)?;
    Ok(view)
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let mut machines = load(paths);
    machines.retain(|m| m.id != id);
    store(paths, &machines)
}

#[derive(Serialize)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Value,
}

/// Calls the machine's CSCM API (`path` below `/api`). A 401 forgets the token so the UI asks to sign in again.
pub async fn request(client: &reqwest::Client, paths: &Paths, id: &str, method: &str, path: &str, body: Option<Value>) -> Result<ApiResponse> {
    if !path.starts_with('/') || path.contains("..") {
        bail!("invalid path");
    }
    let mut machines = load(paths);
    let m = machines.iter_mut().find(|m| m.id == id).ok_or_else(|| anyhow!("Unknown server"))?;
    let token = m.token.clone().ok_or_else(|| anyhow!("Sign in to {} first", m.name))?;
    let method = reqwest::Method::from_bytes(method.as_bytes())?;
    let mut req = client
        .request(method, format!("{}/api{path}", m.url))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(30));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await.map_err(|e| anyhow!("Can't reach {}: {}", m.name, e.without_url()))?;
    let status = res.status().as_u16();
    let body = res.json().await.unwrap_or(Value::Null);
    if status == 401 {
        m.token = None;
        store(paths, &machines)?;
    }
    Ok(ApiResponse { status, body })
}

/// How players reach a CSCM server, best first.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Addresses {
    /// The custom domain (Cloudflare) once its SRV record exists, else the Playit address.
    pub public: Option<String>,
    /// The Playit address, when `public` is a custom domain.
    pub playit: Option<String>,
    /// The machine's host and the server's port, for players on the same network.
    pub lan: Option<String>,
}

impl Addresses {
    /// The one to put in the game's server list.
    pub fn best(&self) -> Option<&str> {
        self.public.as_deref().or(self.lan.as_deref())
    }
}

/// Same rules as the CSCM dashboard (serverConnectAddress): a CNAME counts once its SRV record
/// carries the port (Java); Bedrock has the port in the Playit address.
pub fn addresses(server: &Value, machine_url: &str) -> Addresses {
    let bedrock = server["type"] == "bedrock";
    let records = server["dns_records"].as_array().cloned().unwrap_or_default();
    let cname = records.iter().find(|r| r["type"] == "CNAME").and_then(|r| r["name"].as_str());
    let srv_ready = bedrock || records.iter().any(|r| r["type"] == "SRV");
    let tunnel = server["tunnels"].as_array().and_then(|t| t.iter().find(|t| t["address"].is_string()));
    let playit = tunnel.and_then(|t| {
        let addr = t["address"].as_str()?;
        // Bedrock always needs the port; Java's raw tunnel host (*.ply.gg) too, only
        // joinmc.link names carry it in an SRV record
        let needs_port = bedrock || !addr.ends_with("joinmc.link");
        Some(match t["external_port"].as_i64() {
            Some(port) if needs_port && !addr.contains(':') => format!("{addr}:{port}"),
            _ => addr.to_string(),
        })
    });
    let custom = cname.filter(|_| srv_ready).map(String::from);
    let host = reqwest::Url::parse(machine_url).ok().and_then(|u| u.host_str().map(String::from));
    let lan = host.zip(server["port"].as_i64()).map(|(h, p)| if p == 25565 { h } else { format!("{h}:{p}") });
    Addresses { playit: custom.as_ref().and(playit.clone()), public: custom.or(playit), lan }
}

pub fn machine_url(paths: &Paths, id: &str) -> Option<String> {
    load(paths).into_iter().find(|m| m.id == id).map(|m| m.url)
}

/// Sends a newer server pack to a modpack server (CSCM 1.15+). CSCM swaps it in and
/// restarts the server if it's running; the server then installs the new mods and removes
/// the ones that left the pack. Returns whether the server restarted.
pub async fn update_pack(client: &reqwest::Client, paths: &Paths, id: &str, server_id: i64, pack: Vec<u8>) -> Result<bool> {
    let machines = load(paths);
    let m = machines.iter().find(|m| m.id == id).ok_or_else(|| anyhow!("Unknown server"))?;
    let token = m.token.clone().ok_or_else(|| anyhow!("Sign in to {} first", m.name))?;
    let form = reqwest::multipart::Form::new()
        .part("pack", reqwest::multipart::Part::bytes(pack).file_name("pack.mrpack").mime_str("application/zip")?);
    let res = client
        .put(format!("{}/api/servers/{server_id}/pack", m.url))
        .bearer_auth(token)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach {}: {}", m.name, e.without_url()))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    match status.as_u16() {
        200..=299 => Ok(body["restarted"].as_bool().unwrap_or(false)),
        // the route itself is missing on older CSCM; a missing server says so in its message
        404 if body["message"].is_null() && body["error"].is_null() => bail!("{} runs an older CSCM. Update it to keep the server's mods in sync.", m.name),
        _ => bail!("{}", body["error"].as_str().or(body["message"].as_str()).unwrap_or("CSCM refused the pack")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_prefer_the_custom_domain() {
        let s = serde_json::json!({
            "type": "modpack", "port": 25570,
            "tunnels": [{ "address": "abc.joinmc.link", "external_port": 41234 }],
            "dns_records": [{ "type": "CNAME", "name": "smp.example.com" }, { "type": "SRV", "name": "_minecraft._tcp.smp.example.com" }]
        });
        let a = addresses(&s, "http://192.168.1.20:3000");
        assert_eq!(a.public.as_deref(), Some("smp.example.com"));
        assert_eq!(a.playit.as_deref(), Some("abc.joinmc.link"));
        assert_eq!(a.lan.as_deref(), Some("192.168.1.20:25570"));
        // no SRV yet: the domain doesn't carry the port, so Playit is the address
        let s = serde_json::json!({ "type": "fabric", "port": 25565, "tunnels": [{ "address": "abc.joinmc.link", "external_port": 41234 }], "dns_records": [{ "type": "CNAME", "name": "smp.example.com" }] });
        let a = addresses(&s, "http://my-pc:3000");
        assert_eq!((a.public.as_deref(), a.playit, a.lan.as_deref()), (Some("abc.joinmc.link"), None, Some("my-pc")));
        // a raw Playit host needs its port
        let s = serde_json::json!({ "type": "modpack", "port": 25567, "tunnels": [{ "address": "domination-trifle.tun.ply.gg", "external_port": 44596 }], "dns_records": [] });
        assert_eq!(addresses(&s, "http://192.168.20.27:3000").public.as_deref(), Some("domination-trifle.tun.ply.gg:44596"));
        // local only
        let a = addresses(&serde_json::json!({ "type": "modpack", "port": 25566 }), "http://my-pc:3000");
        assert_eq!(a.best(), Some("my-pc:25566"));
    }

    #[test]
    fn urls() {
        assert_eq!(normalise_url("100.64.0.5:3000/").unwrap(), "http://100.64.0.5:3000");
        assert_eq!(normalise_url("https://cscm.example.com").unwrap(), "https://cscm.example.com");
        assert!(normalise_url("  ").is_err());
    }
}

/// Uploads a server pack to a machine's CSCM (`POST /api/servers/from-pack`); returns the new server's id.
/// Reports upload progress: ("Uploading", KB sent, KB total).
pub type UploadProgress = std::sync::Arc<dyn Fn(&str, usize, usize) + Send + Sync>;

/// The pack as a streamed upload, so the launcher can show how much has been sent.
fn upload_part(pack: Vec<u8>, progress: UploadProgress) -> reqwest::multipart::Part {
    const CHUNK: usize = 64 * 1024;
    let total = pack.len();
    progress("Uploading", 0, total / 1024);
    let chunks: Vec<Vec<u8>> = pack.chunks(CHUNK).map(<[u8]>::to_vec).collect();
    let stream = futures::stream::iter(chunks.into_iter().scan(0usize, move |sent, chunk| {
        *sent += chunk.len();
        progress("Uploading", *sent / 1024, total / 1024);
        Some(Ok::<_, std::io::Error>(chunk))
    }));
    reqwest::multipart::Part::stream_with_length(reqwest::Body::wrap_stream(stream), total as u64)
}

pub async fn create_from_pack(
    client: &reqwest::Client,
    paths: &Paths,
    id: &str,
    name: &str,
    mem_gb: u32,
    pack: Vec<u8>,
    progress: UploadProgress,
) -> Result<i64> {
    let machines = load(paths);
    let m = machines.iter().find(|m| m.id == id).ok_or_else(|| anyhow!("Unknown server"))?;
    let token = m.token.clone().ok_or_else(|| anyhow!("Sign in to {} first", m.name))?;
    let form = reqwest::multipart::Form::new()
        .text("name", name.to_string())
        .text("mem_min", "1")
        .text("mem_max", mem_gb.max(1).to_string())
        .part("pack", upload_part(pack, progress).file_name("pack.mrpack").mime_str("application/zip")?);
    let res = client
        .post(format!("{}/api/servers/from-pack", m.url))
        .bearer_auth(token)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach {}: {}", m.name, e.without_url()))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = body["error"].as_str().or(body["message"].as_str()).unwrap_or("CSCM refused the pack");
        if status == reqwest::StatusCode::NOT_FOUND {
            bail!("{} runs an older CSCM without modpack servers. Update it first.", m.name);
        }
        bail!("{msg}");
    }
    body["server_id"].as_i64().ok_or_else(|| anyhow!("CSCM sent no server id"))
}
