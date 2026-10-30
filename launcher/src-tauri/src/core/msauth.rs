//! Microsoft sign-in for Minecraft: Microsoft account → Xbox Live → XSTS → Minecraft
//! services token, then the Minecraft profile (name, skins, capes).
//!
//! Sign-in uses the authorization code flow with PKCE in a launcher window that ends on
//! Microsoft's `nativeclient` page, so nothing listens on a local port. Accounts (with the
//! refresh token) are kept in `microsoft-accounts.json`, readable only by the user.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::account::write_private;
use super::launch::Player;
use super::paths::Paths;

/// The launcher's Azure app (public client, approved by Mojang for the Minecraft API).
pub const CLIENT_ID: &str = match option_env!("TESSERA_MS_CLIENT_ID") {
    Some(id) => id,
    None => "",
};
const AUTHORIZE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const REDIRECT: &str = "https://login.microsoftonline.com/common/oauth2/nativeclient";
const SCOPE: &str = "XboxLive.signin offline_access";
const MC: &str = "https://api.minecraftservices.com";

#[derive(Serialize, Deserialize, Clone)]
pub struct MsAccount {
    /// Minecraft profile id (UUID without dashes)
    pub id: String,
    pub name: String,
    refresh_token: String,
    mc_token: String,
    /// Unix seconds
    expires_at: i64,
    xuid: String,
}

#[derive(Serialize, Clone)]
pub struct AccountView {
    pub id: String,
    pub name: String,
}

impl From<&MsAccount> for AccountView {
    fn from(a: &MsAccount) -> Self {
        Self { id: a.id.clone(), name: a.name.clone() }
    }
}

// ── stored accounts ──

#[derive(Serialize, Deserialize, Default)]
struct Store {
    accounts: Vec<MsAccount>,
}

fn file(paths: &Paths) -> std::path::PathBuf {
    paths.file("microsoft-accounts.json")
}

fn load(paths: &Paths) -> Store {
    std::fs::read(file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(paths: &Paths, store: &Store) -> Result<()> {
    std::fs::create_dir_all(&paths.root)?;
    write_private(&file(paths), &serde_json::to_vec(store)?).context("saving the Microsoft account")
}

fn upsert(paths: &Paths, account: &MsAccount) -> Result<()> {
    let mut store = load(paths);
    store.accounts.retain(|a| a.id != account.id);
    store.accounts.push(account.clone());
    save(paths, &store)
}

pub fn list(paths: &Paths) -> Vec<AccountView> {
    load(paths).accounts.iter().map(AccountView::from).collect()
}

pub fn get(paths: &Paths, id: &str) -> Option<AccountView> {
    load(paths).accounts.iter().find(|a| a.id == id).map(AccountView::from)
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let mut store = load(paths);
    store.accounts.retain(|a| a.id != id);
    save(paths, &store)
}

// ── sign-in window ──

pub struct Pkce {
    pub verifier: String,
    pub state: String,
    pub url: String,
}

fn random_text() -> Result<String> {
    use base64::Engine;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// The Microsoft sign-in page to open, with a fresh PKCE verifier and state.
pub fn start() -> Result<Pkce> {
    use base64::Engine;
    if CLIENT_ID.is_empty() {
        bail!("Microsoft sign-in isn't set up in this build of the launcher");
    }
    let verifier = random_text()?;
    let state = random_text()?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE,
        [
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT),
            ("scope", SCOPE),
            ("prompt", "select_account"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
        ],
    )?;
    Ok(Pkce { verifier, state, url: url.into() })
}

/// What the sign-in window ended on: the code, or Microsoft's error.
pub fn parse_redirect(url: &str, state: &str) -> Result<String> {
    let url = reqwest::Url::parse(url)?;
    let get = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    if let Some(e) = get("error") {
        bail!("{}", get("error_description").unwrap_or(e));
    }
    if get("state").as_deref() != Some(state) {
        bail!("The sign-in answer didn't match; try again");
    }
    get("code").ok_or_else(|| anyhow!("Microsoft didn't send a sign-in code"))
}

// ── tokens ──

async fn ms_token(client: &reqwest::Client, form: &[(&str, &str)]) -> Result<(String, String)> {
    let res = client.post(TOKEN).form(form).send().await.context("reaching Microsoft")?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or_default();
    if !status.is_success() {
        let why = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("unknown error");
        bail!("Microsoft sign-in failed: {}", why.lines().next().unwrap_or(why));
    }
    let access = body["access_token"].as_str().ok_or_else(|| anyhow!("Microsoft sent no access token"))?;
    let refresh = body["refresh_token"].as_str().unwrap_or_default();
    Ok((access.to_string(), refresh.to_string()))
}

async fn post_json(client: &reqwest::Client, url: &str, body: Value) -> Result<(reqwest::StatusCode, Value)> {
    let res = client.post(url).header("Accept", "application/json").json(&body).send().await.with_context(|| format!("reaching {}", url.split('/').nth(2).unwrap_or(url)))?;
    let status = res.status();
    Ok((status, res.json().await.unwrap_or_default()))
}

/// Xbox Live and XSTS, then the Minecraft token: (token, seconds valid, xuid).
async fn minecraft_token(client: &reqwest::Client, ms_access: &str) -> Result<(String, i64, String)> {
    let (status, xbl) = post_json(
        client,
        "https://user.auth.xboxlive.com/user/authenticate",
        json!({
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={ms_access}") },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }),
    )
    .await?;
    let xbl_token = xbl["Token"].as_str().filter(|_| status.is_success()).ok_or_else(|| anyhow!("Xbox Live sign-in failed ({status})"))?;

    let (status, xsts) = post_json(
        client,
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }),
    )
    .await?;
    if !status.is_success() {
        bail!(match xsts["XErr"].as_i64() {
            Some(2148916233) => "This Microsoft account has no Xbox profile yet. Sign in once at minecraft.net to create one, then try again.",
            Some(2148916235) => "Xbox Live isn't available in this account's country.",
            Some(2148916236 | 2148916237) => "This account needs adult verification on xbox.com first.",
            Some(2148916238) => "This is a child account. An adult needs to add it to a Microsoft family first.",
            _ => "Xbox sign-in failed",
        });
    }
    let xsts_token = xsts["Token"].as_str().ok_or_else(|| anyhow!("Xbox sent no token"))?;
    let claims = &xsts["DisplayClaims"]["xui"][0];
    let uhs = claims["uhs"].as_str().ok_or_else(|| anyhow!("Xbox sent no user hash"))?;
    let xuid = claims["xid"].as_str().unwrap_or_default().to_string();

    let (status, mc) = post_json(client, &format!("{MC}/authentications/login_with_xbox"), json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts_token}") })).await?;
    if !status.is_success() {
        if status == reqwest::StatusCode::FORBIDDEN {
            bail!("Minecraft refused the sign-in (the launcher's app may not be approved yet)");
        }
        bail!("Minecraft sign-in failed ({status})");
    }
    let token = mc["access_token"].as_str().ok_or_else(|| anyhow!("Minecraft sent no token"))?;
    Ok((token.to_string(), mc["expires_in"].as_i64().unwrap_or(86400), xuid))
}

// ── profile ──

#[derive(Deserialize, Clone, Debug)]
pub struct Texture {
    pub id: String,
    pub state: String,
    pub url: String,
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub capes: Vec<Texture>,
}

async fn check(res: reqwest::Response, what: &str) -> Result<reqwest::Response> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    let body: Value = res.json().await.unwrap_or_default();
    let why = body["errorMessage"].as_str().or(body["error"].as_str()).map(|s| format!(": {s}")).unwrap_or_default();
    bail!("{what} failed ({status}){why}")
}

pub async fn profile(client: &reqwest::Client, token: &str) -> Result<Profile> {
    let res = client.get(format!("{MC}/minecraft/profile")).bearer_auth(token).send().await.context("reaching Minecraft")?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("This Microsoft account doesn't own Minecraft: Java Edition");
    }
    Ok(check(res, "Loading the Minecraft profile").await?.json().await?)
}

/// Finishes sign-in with the code from the window and saves the account.
pub async fn sign_in(client: &reqwest::Client, paths: &Paths, code: &str, verifier: &str) -> Result<AccountView> {
    let (access, refresh) = ms_token(
        client,
        &[("client_id", CLIENT_ID), ("grant_type", "authorization_code"), ("code", code), ("redirect_uri", REDIRECT), ("code_verifier", verifier), ("scope", SCOPE)],
    )
    .await?;
    let (mc_token, expires_in, xuid) = minecraft_token(client, &access).await?;
    let p = profile(client, &mc_token).await?;
    let account = MsAccount { id: p.id, name: p.name, refresh_token: refresh, mc_token, expires_at: chrono::Utc::now().timestamp() + expires_in, xuid };
    upsert(paths, &account)?;
    Ok(AccountView::from(&account))
}

/// The account with a Minecraft token that's good for a while longer (refreshing it if
/// needed, which also picks up a changed name).
pub async fn fresh(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<MsAccount> {
    let mut account = load(paths).accounts.into_iter().find(|a| a.id == id).ok_or_else(|| anyhow!("That Microsoft account was removed from the launcher"))?;
    if account.expires_at - chrono::Utc::now().timestamp() > 10 * 60 {
        return Ok(account);
    }
    let (access, refresh) = ms_token(client, &[("client_id", CLIENT_ID), ("grant_type", "refresh_token"), ("refresh_token", &account.refresh_token), ("scope", SCOPE)])
        .await
        .context("Your Microsoft sign-in expired; sign in again")?;
    let (mc_token, expires_in, xuid) = minecraft_token(client, &access).await?;
    if let Ok(p) = profile(client, &mc_token).await {
        account.name = p.name;
    }
    if !refresh.is_empty() {
        account.refresh_token = refresh;
    }
    account.mc_token = mc_token;
    account.expires_at = chrono::Utc::now().timestamp() + expires_in;
    account.xuid = xuid;
    upsert(paths, &account)?;
    Ok(account)
}

/// The player to launch as. Without internet the last token is used, so singleplayer still
/// works; `Err` only when the account is gone.
pub async fn player(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<(Player, Option<String>)> {
    let (account, warning) = match fresh(client, paths, id).await {
        Ok(a) => (a, None),
        Err(e) => {
            let a = load(paths).accounts.into_iter().find(|a| a.id == id).ok_or(e)?;
            (a, Some("Couldn't refresh the Microsoft sign-in; online servers may not let you in".to_string()))
        }
    };
    Ok((Player { name: account.name, uuid: account.id, access_token: account.mc_token, xuid: account.xuid, online: true }, warning))
}

// ── skins and capes ──

/// A cape the account owns, with its texture as a data URL for the 3D preview.
#[derive(Serialize)]
pub struct Cape {
    pub id: String,
    pub name: String,
    pub texture: String,
    pub active: bool,
}

pub async fn capes(client: &reqwest::Client, paths: &Paths, id: &str) -> Result<Vec<Cape>> {
    use base64::Engine;
    let account = fresh(client, paths, id).await?;
    let p = profile(client, &account.mc_token).await?;
    let mut out = Vec::new();
    for c in p.capes {
        let url = c.url.replacen("http://", "https://", 1);
        let Ok(res) = client.get(&url).send().await.and_then(|r| r.error_for_status()) else { continue };
        let Ok(bytes) = res.bytes().await else { continue };
        out.push(Cape {
            name: c.alias.clone().unwrap_or_else(|| "Cape".into()).replace('_', " "),
            texture: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)),
            active: c.state == "ACTIVE",
            id: c.id,
        });
    }
    Ok(out)
}

/// Makes the skin (and cape, or no cape) the account's skin on Mojang's servers.
pub async fn apply_skin(client: &reqwest::Client, paths: &Paths, id: &str, png: Vec<u8>, slim: bool, cape: Option<&str>) -> Result<()> {
    let account = fresh(client, paths, id).await?;
    let form = reqwest::multipart::Form::new()
        .text("variant", if slim { "slim" } else { "classic" })
        .part("file", reqwest::multipart::Part::bytes(png).file_name("skin.png").mime_str("image/png")?);
    let res = client.post(format!("{MC}/minecraft/profile/skins")).bearer_auth(&account.mc_token).multipart(form).send().await.context("reaching Minecraft")?;
    check(res, "Changing your skin").await?;
    let cape_url = format!("{MC}/minecraft/profile/capes/active");
    let res = match cape {
        Some(c) => client.put(&cape_url).bearer_auth(&account.mc_token).json(&json!({ "capeId": c })).send().await,
        None => client.delete(&cape_url).bearer_auth(&account.mc_token).send().await,
    }
    .context("reaching Minecraft")?;
    check(res, "Changing your cape").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_redirect() {
        let ok = format!("{REDIRECT}?code=M.abc&state=xyz");
        assert_eq!(parse_redirect(&ok, "xyz").unwrap(), "M.abc");
        assert!(parse_redirect(&ok, "other").is_err());
        let denied = format!("{REDIRECT}?error=access_denied&error_description=The+user+said+no&state=xyz");
        assert_eq!(parse_redirect(&denied, "xyz").unwrap_err().to_string(), "The user said no");
    }
}
