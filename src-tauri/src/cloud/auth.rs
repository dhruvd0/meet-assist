//! CASE accounts on Supabase Auth. The session lives in the OS keychain, not
//! in the settings store or SQLite.

use super::{ANON_KEY, SUPABASE_URL};
use serde::{Deserialize, Serialize};

const KEYCHAIN_SERVICE: &str = "com.case.app.cloud";
const KEYCHAIN_ACCOUNT: &str = "session";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub user_id: String,
    pub email: String,
}

impl Session {
    pub fn expires_soon(&self) -> bool {
        self.expires_at - chrono::Utc::now().timestamp() < 60
    }
}

#[derive(Debug)]
pub enum AuthError {
    /// The server refused: bad credentials, a taken email, a revoked refresh token.
    Rejected(String),
    /// The server could not be reached.
    Network(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Rejected(m) | AuthError::Network(m) => f.write_str(m),
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    expires_in: Option<i64>,
    user: Option<User>,
}

#[derive(Deserialize)]
struct User {
    id: String,
    email: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    msg: Option<String>,
    message: Option<String>,
    error_description: Option<String>,
}

pub async fn sign_up(
    http: &reqwest::Client,
    email: &str,
    password: &str,
) -> Result<Session, AuthError> {
    token_request(
        http,
        "signup",
        serde_json::json!({ "email": email, "password": password }),
    )
    .await
}

pub async fn sign_in(
    http: &reqwest::Client,
    email: &str,
    password: &str,
) -> Result<Session, AuthError> {
    token_request(
        http,
        "token?grant_type=password",
        serde_json::json!({ "email": email, "password": password }),
    )
    .await
}

pub async fn refresh(http: &reqwest::Client, session: &Session) -> Result<Session, AuthError> {
    token_request(
        http,
        "token?grant_type=refresh_token",
        serde_json::json!({ "refresh_token": session.refresh_token }),
    )
    .await
}

/// Best effort: revokes the refresh token server-side. Local sign-out does not
/// depend on it succeeding.
pub async fn sign_out(http: &reqwest::Client, session: &Session) {
    let res = http
        .post(format!("{SUPABASE_URL}/auth/v1/logout"))
        .header("apikey", ANON_KEY)
        .bearer_auth(&session.access_token)
        .send()
        .await;
    if let Err(e) = res {
        log::warn!("cloud sign-out request failed: {e}");
    }
}

async fn token_request(
    http: &reqwest::Client,
    path: &str,
    body: serde_json::Value,
) -> Result<Session, AuthError> {
    let res = http
        .post(format!("{SUPABASE_URL}/auth/v1/{path}"))
        .header("apikey", ANON_KEY)
        .json(&body)
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;
    let status = res.status();
    let text = res
        .text()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;
    if !status.is_success() {
        let msg = serde_json::from_str::<ErrorResponse>(&text)
            .ok()
            .and_then(|e| e.msg.or(e.message).or(e.error_description))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(if status.is_server_error() {
            AuthError::Network(msg)
        } else {
            AuthError::Rejected(msg)
        });
    }
    let parsed: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| AuthError::Rejected(format!("unexpected auth response: {e}")))?;
    let (Some(access_token), Some(refresh_token), Some(user)) =
        (parsed.access_token, parsed.refresh_token, parsed.user)
    else {
        return Err(AuthError::Rejected("auth response had no session".into()));
    };
    let expires_at = parsed
        .expires_at
        .unwrap_or_else(|| chrono::Utc::now().timestamp() + parsed.expires_in.unwrap_or(3600));
    Ok(Session {
        access_token,
        refresh_token,
        expires_at,
        email: user.email.unwrap_or_default(),
        user_id: user.id,
    })
}

fn entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
}

pub fn load() -> Option<Session> {
    let secret = match entry().and_then(|e| e.get_password()) {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return None,
        Err(e) => {
            log::warn!("cloud session not readable from keychain: {e}");
            return None;
        }
    };
    serde_json::from_str(&secret).ok()
}

pub fn save(session: &Session) -> anyhow::Result<()> {
    entry()?.set_password(&serde_json::to_string(session)?)?;
    Ok(())
}

pub fn clear() {
    match entry().and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => log::warn!("could not remove cloud session from keychain: {e}"),
    }
}
