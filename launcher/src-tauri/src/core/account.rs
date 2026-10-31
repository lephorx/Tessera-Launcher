//! Tessera account: sign-in with Neon Auth and calls to the Tessera API (a Neon Function).
//!
//! Only public values live in the launcher: the Auth URL and the API URL. The session is a
//! cookie from Neon Auth, kept in `session.json` in the launcher's data folder, readable
//! only by your user account (like the Modrinth App keeps its sign-in in its database).
//! Not the OS keychain: macOS asks for the login password whenever the app's signature
//! changes, which for unsigned builds is every update. For API calls the session is traded
//! for a 15-minute signed token, which the API verifies before doing anything; signing out
//! deletes the file and ends the session at Neon Auth.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Override for development: TESSERA_AUTH_URL / TESSERA_API_URL at build time.
pub const AUTH_URL: &str = match option_env!("TESSERA_AUTH_URL") {
    Some(u) => u,
    None => "https://ep-aged-brook-b9nb312t.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/auth",
};
pub const API_URL: &str = match option_env!("TESSERA_API_URL") {
    Some(u) => u,
    None => "https://br-rapid-thunder-b9spxx3v-api.compute.c-7.eu-central-1.aws.neon.tech/v1",
};
/// Neon Auth only accepts requests from trusted origins; local origins are trusted by default.
const ORIGIN: &str = "http://localhost";

/// Cached API token and when it stops being usable.
static TOKEN: LazyLock<Mutex<Option<(String, Instant)>>> = LazyLock::new(|| Mutex::new(None));

/// Where the session is kept; set once at startup (tests keep it in memory).
static SESSION_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn init(data_dir: &std::path::Path) {
    let _ = SESSION_FILE.set(data_dir.join("session.json"));
}

#[cfg(test)]
#[test]
fn session_file_is_private() {
    let dir = std::env::temp_dir().join(format!("tessera-session-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("session.json");
    write_private(&file, b"{}").unwrap();
    write_private(&file, b"{\"cookie\":\"x\"}").unwrap(); // replacing works too
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(std::fs::read(&file).unwrap(), b"{\"cookie\":\"x\"}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(test)]
static TEST_SESSION: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize, Deserialize)]
#[cfg_attr(test, allow(dead_code))]
struct StoredSession {
    cookie: String,
}

fn session() -> Option<String> {
    #[cfg(test)]
    return TEST_SESSION.lock().unwrap().clone();
    #[cfg(not(test))]
    {
        let bytes = std::fs::read(SESSION_FILE.get()?).ok()?;
        serde_json::from_slice::<StoredSession>(&bytes).ok().map(|s| s.cookie).filter(|c| !c.is_empty())
    }
}

fn store_session(cookie: &str) -> Result<()> {
    #[cfg(test)]
    {
        *TEST_SESSION.lock().unwrap() = Some(cookie.to_string());
    }
    #[cfg(not(test))]
    {
        let file = SESSION_FILE.get().ok_or_else(|| anyhow!("The launcher isn't ready yet"))?;
        let bytes = serde_json::to_vec(&StoredSession { cookie: cookie.to_string() })?;
        anyhow::Context::context(write_private(file, &bytes), "saving the sign-in")?;
    }
    *TOKEN.lock().unwrap() = None;
    Ok(())
}

/// Writes a file only the current user can read (0600 on macOS and Linux; Windows keeps
/// files in the user's AppData private already).
pub(crate) fn write_private(file: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = file.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, file)
}

fn forget_session() {
    #[cfg(test)]
    {
        *TEST_SESSION.lock().unwrap() = None;
    }
    #[cfg(not(test))]
    if let Some(file) = SESSION_FILE.get() {
        let _ = std::fs::remove_file(file);
    }
    *TOKEN.lock().unwrap() = None;
}

/// "name=value; name2=value2" from the Set-Cookie headers of a sign-in response.
fn cookies_from(res: &reqwest::Response) -> String {
    res.headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::trim).map(String::from))
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

async fn auth_error(res: reqwest::Response) -> anyhow::Error {
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    let code = body["code"].as_str().unwrap_or_default();
    let message = match code {
        "INVALID_EMAIL_OR_PASSWORD" => "Wrong email or password.".to_string(),
        "USER_ALREADY_EXISTS" | "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL" => "There is already an account with that email.".to_string(),
        "PASSWORD_TOO_SHORT" => "Use at least 8 characters for your password.".to_string(),
        "INVALID_EMAIL" => "That email address doesn't look right.".to_string(),
        _ if status == reqwest::StatusCode::TOO_MANY_REQUESTS => "Too many attempts. Wait a minute and try again.".to_string(),
        _ => body["message"].as_str().map(String::from).unwrap_or_else(|| format!("Sign-in failed ({status})")),
    };
    anyhow!(message)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub username: Option<String>,
    /// Joined with an invite (closed beta); without it, only joining works.
    #[serde(default)]
    pub member: bool,
    #[serde(default)]
    pub is_admin: bool,
    /// The Terms version this account accepted; asked again when it isn't the current one.
    #[serde(default)]
    pub terms_version: Option<String>,
    /// "16+" or "13-15"
    #[serde(default)]
    pub age_declaration: Option<String>,
}

/// Agreeing to the Terms and Privacy Policy at sign-up (or when they change): which version,
/// the age band ("16+" or "13-15"), and for 13–15 that a parent or guardian agreed.
/// The API checks all of it again.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Consent {
    pub terms_version: String,
    pub age: String,
    #[serde(default)]
    pub parental_consent: bool,
}

pub async fn sign_in(client: &reqwest::Client, email: &str, password: &str) -> Result<Me> {
    if !super::limits::login_allowed("tessera-account") {
        bail!("Too many sign-in attempts. Wait a minute and try again.");
    }
    let res = client
        .post(format!("{AUTH_URL}/sign-in/email"))
        .header("origin", ORIGIN)
        .json(&serde_json::json!({ "email": email.trim(), "password": password }))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err(auth_error(res).await);
    }
    store_session(&cookies_from(&res))?;
    me(client).await
}

/// The code from an invite link (https://tesseralauncher.com/invite/AbCd234567) or the code itself.
pub fn invite_code(input: &str) -> Result<String> {
    let code = input.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
    if (6..=32).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric()) {
        Ok(code)
    } else {
        bail!("That doesn't look like an invite link")
    }
}

/// Tessera is invite-only: checks the invite first, so nobody makes an account they can't use.
/// Sign-up happens on an invite's page on tesseralauncher.com; this is for the tests.
#[cfg(test)]
pub async fn sign_up(client: &reqwest::Client, email: &str, invite: &str, username: &str, password: &str, consent: &Consent) -> Result<Me> {
    if !super::limits::login_allowed("tessera-account") {
        bail!("Too many attempts. Wait a minute and try again.");
    }
    if !(3..=24).contains(&username.len()) || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("Usernames are 3–24 letters, digits or underscores.");
    }
    if consent.age == "13-15" && !consent.parental_consent {
        bail!("A parent or guardian has to agree to your account.");
    }
    let code = invite_code(invite)?;
    let check: Value = client
        .get(format!("{API_URL}/invites/{code}"))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?
        .json()
        .await
        .unwrap_or(Value::Null);
    if check["valid"] != true {
        bail!("{}", check["reason"].as_str().unwrap_or("That invite doesn't work."));
    }
    let res = client
        .post(format!("{AUTH_URL}/sign-up/email"))
        .header("origin", ORIGIN)
        .json(&serde_json::json!({ "email": email.trim(), "password": password, "name": username }))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err(auth_error(res).await);
    }
    store_session(&cookies_from(&res))?;
    redeem(client, &code, username, Some(consent)).await.map_err(|e| anyhow!("Your account is made, but joining failed: {e}"))?;
    me(client).await
}

/// Joins Tessera with an invite: picks the username friends add you by.
pub async fn redeem(client: &reqwest::Client, invite: &str, username: &str, consent: Option<&Consent>) -> Result<()> {
    let code = invite_code(invite)?;
    let mut body = serde_json::json!({ "username": username.trim() });
    if let Some(c) = consent {
        body["terms_version"] = c.terms_version.clone().into();
        body["age"] = c.age.clone().into();
        body["parental_consent"] = c.parental_consent.into();
    }
    api(client, reqwest::Method::POST, &format!("/invites/{code}/redeem"), Some(body), None).await?;
    Ok(())
}

/// Accepts the current Terms for an account made before them (or before they changed).
pub async fn consent(client: &reqwest::Client, consent: &Consent) -> Result<()> {
    api(client, reqwest::Method::POST, "/me/consent", Some(serde_json::to_value(consent)?), None).await?;
    Ok(())
}

/// Everything Tessera stores about this account, as the API sends it.
pub async fn export(client: &reqwest::Client) -> Result<Value> {
    api(client, reqwest::Method::GET, "/me/export", None, None).await
}

/// Deletes the account with its shares and their packs, then forgets the sign-in here.
pub async fn delete(client: &reqwest::Client) -> Result<()> {
    api(client, reqwest::Method::DELETE, "/me", None, None).await?;
    forget_session();
    Ok(())
}

pub async fn sign_out(client: &reqwest::Client) {
    if let Some(cookie) = session() {
        let _ = client.post(format!("{AUTH_URL}/sign-out")).header("origin", ORIGIN).header("cookie", cookie).send().await;
    }
    forget_session();
}

/// A valid API token, refreshed from the session when it's about to expire.
async fn token(client: &reqwest::Client) -> Result<String> {
    if let Some((t, until)) = TOKEN.lock().unwrap().clone() {
        if Instant::now() < until {
            return Ok(t);
        }
    }
    let cookie = session().ok_or_else(|| anyhow!("Sign in to your Tessera account first."))?;
    let res = client
        .get(format!("{AUTH_URL}/token"))
        .header("origin", ORIGIN)
        .header("cookie", cookie)
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        forget_session();
        bail!("Your sign-in expired. Sign in again.");
    }
    let body: Value = res.error_for_status()?.json().await?;
    let jwt = body["token"].as_str().ok_or_else(|| anyhow!("Tessera sent no token"))?.to_string();
    // use it until a minute before its own expiry
    let exp = jwt
        .split('.')
        .nth(1)
        .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["exp"].as_u64());
    let ttl = exp
        .map(|e| e.saturating_sub(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()))
        .unwrap_or(600)
        .saturating_sub(60);
    *TOKEN.lock().unwrap() = Some((jwt.clone(), Instant::now() + Duration::from_secs(ttl)));
    Ok(jwt)
}

/// Calls the Tessera API with the caller's token. `body` is JSON or raw bytes.
pub async fn api(client: &reqwest::Client, method: reqwest::Method, path: &str, json: Option<Value>, bytes: Option<Vec<u8>>) -> Result<Value> {
    let jwt = token(client).await?;
    let mut req = client.request(method, format!("{API_URL}{path}")).bearer_auth(jwt).timeout(Duration::from_secs(120));
    if let Some(j) = json {
        req = req.json(&j);
    } else if let Some(b) = bytes {
        req = req.header("content-type", "application/zip").body(b);
    }
    let res = req.send().await.map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            *TOKEN.lock().unwrap() = None;
        }
        bail!("{}", body["message"].as_str().or(body["error"].as_str()).unwrap_or("Something went wrong"));
    }
    Ok(body)
}

/// Who is signed in, or None.
pub async fn current(client: &reqwest::Client) -> Option<Me> {
    session()?;
    me(client).await.ok()
}

async fn me(client: &reqwest::Client) -> Result<Me> {
    let body = api(client, reqwest::Method::GET, "/me", None, None).await?;
    Ok(serde_json::from_value(body["user"].clone())?)
}

pub async fn set_username(client: &reqwest::Client, username: &str) -> Result<()> {
    api(client, reqwest::Method::PUT, "/me/username", Some(serde_json::json!({ "username": username })), None).await?;
    Ok(())
}

/// Accepts an invite link (https://tesseralauncher.com/share/AbCd2345) or the bare code.
pub fn share_code(input: &str) -> Result<String> {
    let code = input.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
    if code.len() == 8 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
        Ok(code)
    } else {
        bail!("That isn't a Tessera invite link")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_links() {
        assert_eq!(share_code("https://tesseralauncher.com/share/AbCd2345").unwrap(), "AbCd2345");
        assert_eq!(share_code(" AbCd2345/ ").unwrap(), "AbCd2345");
        assert!(share_code("https://tesseralauncher.com/share/../../x").is_err());
    }

    /// Real round trip against the Tessera API on Neon (needs network; leaves an
    /// `e2e-launcher-…@example.com` account behind: cloud/functions/api/e2e.py cleans those up).
    #[tokio::test]
    #[ignore]
    async fn sign_up_share_join_stop() {
        use crate::core::{instances, paths::Paths, share};
        let client = crate::core::http_client();
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 100_000;
        let me = sign_up(&client, &format!("e2e-launcher-{n}@example.com"), &std::env::var("TESSERA_TEST_INVITE").expect("TESSERA_TEST_INVITE"), &format!("launcher_{n}"), "correct horse battery", &Consent { terms_version: "2026-10-31".into(), age: "16+".into(), parental_consent: false }).await.unwrap();
        assert_eq!(me.username.as_deref(), Some(format!("launcher_{n}").as_str()));

        let root = std::env::temp_dir().join(format!("tessera-share-e2e-{n}"));
        let paths = Paths::new(root.clone());
        let inst = instances::create(&paths, "Shared Pack", "1.21.1", instances::Loader::Fabric, Some("0.16.9".into()), None).unwrap();
        std::fs::create_dir_all(paths.game_dir(&inst.id).join("config")).unwrap();
        std::fs::write(paths.game_dir(&inst.id).join("config/a.toml"), b"a = 1").unwrap();

        let s = share::create(&client, &paths, &inst.id, &|_, _, _| {}).await.unwrap();
        let link = s["link"].as_str().unwrap().to_string();
        assert!(link.starts_with(share::LINK_BASE));
        let joined = share::join(&client, &paths, &link, &|_, _, _| {}).await.unwrap();
        assert_eq!(joined.name, "Shared Pack");
        assert_eq!(std::fs::read(paths.game_dir(&joined.id).join("config/a.toml")).unwrap(), b"a = 1");

        share::stop(&client, &paths, &inst.id).await.unwrap();
        assert!(share::status(&client, &paths, &inst.id).await.unwrap().is_null());
        sign_out(&client).await;
        assert!(current(&client).await.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
