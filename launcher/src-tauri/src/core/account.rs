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
static TEST_SESSION: Mutex<Option<StoredSession>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Clone, Default)]
struct StoredSession {
    cookie: String,
    /// Proves this computer passed two-step sign-in (the API trusts it for 30 days).
    /// Files from before two-step sign-in have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device: Option<String>,
}

fn stored() -> Option<StoredSession> {
    #[cfg(test)]
    return TEST_SESSION.lock().unwrap().clone();
    #[cfg(not(test))]
    {
        let bytes = std::fs::read(SESSION_FILE.get()?).ok()?;
        serde_json::from_slice::<StoredSession>(&bytes).ok()
    }
}

fn session() -> Option<String> {
    stored().map(|s| s.cookie).filter(|c| !c.is_empty())
}

/// sha256 hex of this sign-in's session token, so the API can tell which signed-in device is
/// this one. The cookie holds "<token>.<signature>", URL-encoded.
fn session_hash() -> Option<String> {
    use sha2::Digest;
    let cookie = session()?;
    let value = cookie.split(';').map(str::trim).find_map(|c| c.split_once('=').filter(|(k, _)| k.ends_with("session_token")).map(|(_, v)| v.to_string()))?;
    let token = percent_decode(&value).split('.').next()?.to_string();
    Some(hex::encode(sha2::Sha256::digest(token.as_bytes())))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Sign-in requests say which OS this is, for the list of signed-in devices.
fn auth_user_agent() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("tessera-launcher/{} ({os}; github.com/lephorx/Tessera-Launcher)", env!("CARGO_PKG_VERSION"))
}

fn device() -> Option<String> {
    stored()?.device.filter(|d| !d.is_empty())
}

fn save(s: &StoredSession) -> Result<()> {
    #[cfg(test)]
    {
        *TEST_SESSION.lock().unwrap() = Some(s.clone());
    }
    #[cfg(not(test))]
    {
        let file = SESSION_FILE.get().ok_or_else(|| anyhow!("The launcher isn't ready yet"))?;
        let bytes = serde_json::to_vec(s)?;
        anyhow::Context::context(write_private(file, &bytes), "saving the sign-in")?;
    }
    *TOKEN.lock().unwrap() = None;
    Ok(())
}

/// A new sign-in cookie; the device token stays, so this computer stays trusted.
fn store_session(cookie: &str) -> Result<()> {
    let device = stored().and_then(|s| s.device);
    save(&StoredSession { cookie: cookie.to_string(), device })
}

/// Remembers (or forgets, with None) that this computer passed two-step sign-in.
fn store_device(device: Option<String>) -> Result<()> {
    let s = stored().ok_or_else(|| anyhow!("Sign in to your Tessera account first."))?;
    save(&StoredSession { device, ..s })
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
    anyhow!(auth_message(status, &body))
}

fn auth_message(status: reqwest::StatusCode, body: &Value) -> String {
    let code = body["code"].as_str().unwrap_or_default();
    match code {
        "INVALID_EMAIL_OR_PASSWORD" => "Wrong email or password.".to_string(),
        "USER_ALREADY_EXISTS" | "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL" => "There is already an account with that email.".to_string(),
        "PASSWORD_TOO_SHORT" => "Use at least 8 characters for your password.".to_string(),
        "INVALID_EMAIL" => "That email address doesn't look right.".to_string(),
        "EMAIL_NOT_VERIFIED" => "Verify your email first.".to_string(),
        "INVALID_OTP" => "That code isn't right.".to_string(),
        "OTP_EXPIRED" => "That code expired. Send a new one.".to_string(),
        "TOO_MANY_ATTEMPTS" => "Too many wrong codes. Send a new one.".to_string(),
        _ if status == reqwest::StatusCode::TOO_MANY_REQUESTS => "Too many attempts. Wait a minute and try again.".to_string(),
        _ => body["message"].as_str().map(String::from).unwrap_or_else(|| format!("Sign-in failed ({status})")),
    }
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
    #[serde(default)]
    pub email_verified: bool,
    /// Two-step sign-in by email is on for this account.
    #[serde(default)]
    pub two_factor: bool,
    /// It's on and this computer hasn't entered a code yet: until it does, the API only
    /// answers /me and the two-step endpoints.
    #[serde(default)]
    pub two_factor_required: bool,
}

/// How a password sign-in ended: signed in (maybe still owing a two-step code, see
/// `Me::two_factor_required`), or the email isn't verified yet and Neon Auth emailed a code.
#[derive(Serialize, Debug)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum SignIn {
    SignedIn { me: Me },
    VerifyEmail { email: String },
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

pub async fn sign_in(client: &reqwest::Client, email: &str, password: &str) -> Result<SignIn> {
    if !super::limits::login_allowed("tessera-account") {
        bail!("Too many sign-in attempts. Wait a minute and try again.");
    }
    let res = client
        .post(format!("{AUTH_URL}/sign-in/email"))
        .header("origin", ORIGIN)
        .header("user-agent", auth_user_agent())
        .json(&serde_json::json!({ "email": email.trim(), "password": password }))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    let status = res.status();
    if !status.is_success() {
        let body: Value = res.json().await.unwrap_or(Value::Null);
        // Neon Auth has already emailed a code
        if status == reqwest::StatusCode::FORBIDDEN && body["code"] == "EMAIL_NOT_VERIFIED" {
            return Ok(SignIn::VerifyEmail { email: email.trim().to_string() });
        }
        bail!(auth_message(status, &body));
    }
    store_session(&cookies_from(&res))?;
    Ok(SignIn::SignedIn { me: me(client).await? })
}

/// Emails a 6-digit code that verifies the address.
pub async fn send_verification(client: &reqwest::Client, email: &str) -> Result<()> {
    let res = client
        .post(format!("{AUTH_URL}/email-otp/send-verification-otp"))
        .header("origin", ORIGIN)
        .json(&serde_json::json!({ "email": email.trim(), "type": "email-verification" }))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err(auth_error(res).await);
    }
    Ok(())
}

/// Fixes the address of an account that isn't verified yet (a typo, a throwaway address):
/// the API checks the password, then Neon Auth emails a code to the new address. Returns it.
pub async fn change_email(client: &reqwest::Client, email: &str, password: &str, new_email: &str) -> Result<String> {
    if !super::limits::login_allowed("tessera-account") {
        bail!("Too many attempts. Wait a minute and try again.");
    }
    let res = client
        .post(format!("{API_URL}/email/change"))
        .json(&serde_json::json!({ "email": email.trim(), "password": password, "new_email": new_email.trim() }))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        bail!("{}", body["message"].as_str().unwrap_or("Changing the email didn't work"));
    }
    let next = body["email"].as_str().unwrap_or(new_email.trim()).to_string();
    *TOKEN.lock().unwrap() = None;
    send_verification(client, &next).await?;
    Ok(next)
}

/// Verifies the address with the emailed code. Neon Auth signs in with it, so a new
/// session cookie replaces the old one when it sends one.
pub async fn verify_email(client: &reqwest::Client, email: &str, code: &str) -> Result<Me> {
    let mut req = client
        .post(format!("{AUTH_URL}/email-otp/verify-email"))
        .header("origin", ORIGIN)
        .header("user-agent", auth_user_agent())
        .json(&serde_json::json!({ "email": email.trim(), "otp": code.trim() }));
    if let Some(cookie) = session() {
        req = req.header("cookie", cookie);
    }
    let res = req.send().await.map_err(|e| anyhow!("Can't reach Tessera: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err(auth_error(res).await);
    }
    let cookies = cookies_from(&res);
    if !cookies.is_empty() {
        store_session(&cookies)?;
    } else {
        // the cached token may still say "not verified"
        *TOKEN.lock().unwrap() = None;
    }
    if session().is_none() {
        bail!("Your email is verified. Sign in with your password.");
    }
    two_factor_auto(client).await;
    me(client).await
}

/// A verified email turns two-step sign-in on (unless the member chose before), and this
/// computer stays trusted. Best effort: without it the account just stays as it was.
async fn two_factor_auto(client: &reqwest::Client) {
    let body = api(client, reqwest::Method::POST, "/me/2fa/auto", Some(serde_json::json!({ "label": device_label() })), None).await;
    if let Some(token) = body.ok().and_then(|b| b["device_token"].as_str().map(String::from)) {
        let _ = store_device(Some(token));
    }
}

/// What this computer calls itself in the list of trusted devices.
fn device_label() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("Tessera Launcher on {os}")
}

/// Emails a two-step code: "sign_in", "enable" or "disable".
pub async fn two_factor_send(client: &reqwest::Client, purpose: &str) -> Result<()> {
    if !matches!(purpose, "sign_in" | "enable" | "disable") {
        bail!("Unknown code purpose");
    }
    api(client, reqwest::Method::POST, "/me/2fa/code", Some(serde_json::json!({ "purpose": purpose })), None).await?;
    Ok(())
}

/// Finishes two-step sign-in; this computer is then trusted for 30 days.
pub async fn two_factor_verify(client: &reqwest::Client, code: &str) -> Result<Me> {
    let body = api(client, reqwest::Method::POST, "/me/2fa/verify", Some(serde_json::json!({ "code": code.trim(), "label": device_label() })), None).await?;
    store_device(body["device_token"].as_str().map(String::from))?;
    me(client).await
}

/// Turns two-step sign-in on; this computer stays trusted.
pub async fn two_factor_enable(client: &reqwest::Client, code: &str) -> Result<Me> {
    let body = api(client, reqwest::Method::POST, "/me/2fa/enable", Some(serde_json::json!({ "code": code.trim(), "label": device_label() })), None).await?;
    store_device(body["device_token"].as_str().map(String::from))?;
    me(client).await
}

/// Turns two-step sign-in off; every trusted device is forgotten.
pub async fn two_factor_disable(client: &reqwest::Client, code: &str) -> Result<Me> {
    api(client, reqwest::Method::POST, "/me/2fa/disable", Some(serde_json::json!({ "code": code.trim() })), None).await?;
    store_device(None)?;
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
    two_factor_auto(client).await;
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

/// Deletes the account with its shares and their packs, then forgets the sign-in here. Takes
/// the password and the username (or email) typed out; the API checks both.
pub async fn delete(client: &reqwest::Client, password: &str, confirm: &str) -> Result<()> {
    api(client, reqwest::Method::DELETE, "/me", Some(serde_json::json!({ "password": password, "confirm": confirm.trim() })), None).await?;
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
    if let Some(d) = device() {
        req = req.header("x-tessera-device", d);
    }
    if let Some(h) = session_hash() {
        req = req.header("x-tessera-session", h);
    }
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
    fn old_session_files_still_load() {
        let s: StoredSession = serde_json::from_str(r#"{"cookie":"a=b"}"#).unwrap();
        assert_eq!(s.cookie, "a=b");
        assert!(s.device.is_none());
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"cookie":"a=b"}"#);
    }

    #[test]
    fn new_cookie_keeps_the_device_and_sign_out_drops_it() {
        store_session("a=1").unwrap();
        store_device(Some("dev".into())).unwrap();
        store_session("a=2").unwrap();
        assert_eq!(session().as_deref(), Some("a=2"));
        assert_eq!(device().as_deref(), Some("dev"));
        // the API is told which session this is: the hash of the token before the signature
        store_session("other=1; __Secure-neon-auth.session_token=abc%2Bdef.sig%3D%3D").unwrap();
        use sha2::Digest;
        assert_eq!(session_hash().unwrap(), hex::encode(sha2::Sha256::digest(b"abc+def")));
        forget_session();
        assert!(session().is_none() && device().is_none());
    }

    #[test]
    fn me_without_the_new_fields() {
        let me: Me = serde_json::from_str(r#"{"id":"1","email":"a@b.c","name":null,"username":"x"}"#).unwrap();
        assert!(!me.email_verified && !me.two_factor && !me.two_factor_required);
    }

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
