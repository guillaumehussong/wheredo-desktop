//! xAI OAuth 2.0 device-code flow with file-based token storage.
//! Mirrors macOS `OAuth.swift`: same endpoints, same oauth.json format
//! (tokens are interchangeable between the two apps).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{config, http};

const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const DEVICE_AUTH_ENDPOINT: &str = "https://auth.x.ai/oauth2/device/code";
const TOKEN_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Same field names as the Swift `XAICredentials` Codable struct.
/// `expiresAt` uses Swift's default Date encoding (seconds since 2001-01-01).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<f64>,
}

/// Swift Date reference epoch (2001-01-01) as a Unix timestamp.
const APPLE_EPOCH_UNIX: f64 = 978_307_200.0;

impl Credentials {
    fn expires_at_unix(&self) -> Option<f64> {
        self.expires_at.map(|v| v + APPLE_EPOCH_UNIX)
    }

    fn from_token_response(tok: &TokenResponse, fallback_refresh: Option<String>) -> Self {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Credentials {
            access_token: tok.access_token.clone(),
            refresh_token: tok.refresh_token.clone().or(fallback_refresh),
            expires_at: tok
                .expires_in
                .map(|s| now_unix + s as f64 - APPLE_EPOCH_UNIX),
        }
    }
}

#[derive(Debug)]
pub enum OAuthError {
    NotLoggedIn,
    Denied,
    Expired,
    NoRefreshToken,
    RequestFailed(u16, String),
    Network(String),
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthError::NotLoggedIn => write!(f, "Not logged in — run with --login first"),
            OAuthError::Denied => write!(f, "Authorization denied"),
            OAuthError::Expired => write!(f, "Login expired — run --login again"),
            OAuthError::NoRefreshToken => write!(f, "No refresh token stored"),
            OAuthError::RequestFailed(code, body) => write!(f, "OAuth HTTP {code}: {body}"),
            OAuthError::Network(e) => write!(f, "Network error: {e}"),
        }
    }
}

impl std::error::Error for OAuthError {}

impl From<reqwest::Error> for OAuthError {
    fn from(e: reqwest::Error) -> Self {
        OAuthError::Network(e.to_string())
    }
}

fn token_file() -> PathBuf {
    config::app_data_dir().join("oauth.json")
}

pub fn load() -> Option<Credentials> {
    let data = std::fs::read(token_file()).ok()?;
    serde_json::from_slice(&data).ok()
}

pub fn save(creds: &Credentials) {
    let Ok(data) = serde_json::to_vec_pretty(creds) else { return };
    let path = token_file();
    if std::fs::write(&path, data).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

pub fn clear() {
    let _ = std::fs::remove_file(token_file());
}

/// Resolve a valid access token, refreshing if expired. Errors if not logged in.
pub async fn access_token() -> Result<String, OAuthError> {
    let creds = load().ok_or(OAuthError::NotLoggedIn)?;
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    if let Some(exp) = creds.expires_at_unix() {
        if exp > now_unix + 60.0 {
            return Ok(creds.access_token);
        }
    }
    match refresh(&creds).await {
        Ok(refreshed) => {
            save(&refreshed);
            Ok(refreshed.access_token)
        }
        Err(_) => Ok(creds.access_token),
    }
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct TokenError {
    error: Option<String>,
}

/// Interactive device-code login: opens the browser, polls, stores tokens.
pub async fn login() -> Result<Credentials, OAuthError> {
    let mut fields = HashMap::new();
    fields.insert("client_id", CLIENT_ID);
    fields.insert("scope", SCOPE);
    let resp = http::post_form(DEVICE_AUTH_ENDPOINT, &fields).await?;
    if resp.status != 200 {
        return Err(OAuthError::RequestFailed(resp.status, resp.text()));
    }
    let dev: DeviceCodeResponse =
        serde_json::from_slice(&resp.body).map_err(|e| OAuthError::Network(e.to_string()))?;

    println!("\n━━━ xAI sign-in (SuperGrok / X Premium) ━━━");
    let url_to_open = if let Some(complete) = &dev.verification_uri_complete {
        println!("Open this link in your browser:\n  {complete}");
        complete.clone()
    } else {
        println!("Go to {} and enter code: {}", dev.verification_uri, dev.user_code);
        dev.verification_uri.clone()
    };
    let _ = open::that(url_to_open);
    println!("Waiting for authorization…\n");

    let deadline =
        std::time::Instant::now() + Duration::from_secs(dev.expires_in.unwrap_or(300));
    let mut interval = dev.interval.unwrap_or(5);

    while std::time::Instant::now() < deadline {
        let mut tfields = HashMap::new();
        tfields.insert("grant_type", DEVICE_GRANT_TYPE);
        tfields.insert("client_id", CLIENT_ID);
        let device_code = dev.device_code.clone();
        tfields.insert("device_code", device_code.as_str());
        let tresp = http::post_form(TOKEN_ENDPOINT, &tfields).await?;

        if tresp.status == 200 {
            let tok: TokenResponse = serde_json::from_slice(&tresp.body)
                .map_err(|e| OAuthError::Network(e.to_string()))?;
            let creds = Credentials::from_token_response(&tok, None);
            save(&creds);
            println!("✓ Connected.\n");
            return Ok(creds);
        }

        let err: Option<TokenError> = serde_json::from_slice(&tresp.body).ok();
        match err.and_then(|e| e.error).as_deref() {
            Some("authorization_pending") => {
                tokio::time::sleep(Duration::from_secs(interval)).await;
            }
            Some("slow_down") => {
                interval += 5;
                tokio::time::sleep(Duration::from_secs(interval)).await;
            }
            Some("access_denied") | Some("authorization_denied") => {
                return Err(OAuthError::Denied)
            }
            Some("expired_token") => return Err(OAuthError::Expired),
            _ => return Err(OAuthError::RequestFailed(tresp.status, tresp.text())),
        }
    }
    Err(OAuthError::Expired)
}

pub async fn refresh(creds: &Credentials) -> Result<Credentials, OAuthError> {
    let refresh_token = creds
        .refresh_token
        .clone()
        .ok_or(OAuthError::NoRefreshToken)?;
    let mut fields = HashMap::new();
    fields.insert("grant_type", "refresh_token");
    fields.insert("client_id", CLIENT_ID);
    fields.insert("refresh_token", refresh_token.as_str());
    let resp = http::post_form(TOKEN_ENDPOINT, &fields).await?;
    if resp.status != 200 {
        return Err(OAuthError::RequestFailed(resp.status, resp.text()));
    }
    let tok: TokenResponse =
        serde_json::from_slice(&resp.body).map_err(|e| OAuthError::Network(e.to_string()))?;
    Ok(Credentials::from_token_response(&tok, Some(refresh_token)))
}
