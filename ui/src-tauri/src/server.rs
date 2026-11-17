//! The few server calls the application makes itself: logging in as an
//! admin to register the admin key, then logging out.

use std::time::Duration;

use banc_proto::keys::SecretKey;
use reqwest::header::{COOKIE, SET_COOKIE};

use crate::error::{CommandError, CommandResult};

fn client() -> CommandResult<reqwest::Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| CommandError::new("internal", e))
}

/// The server's error code, or `fallback`.
async fn refusal(response: reqwest::Response, fallback: &str) -> CommandError {
    #[derive(serde::Deserialize)]
    struct Body {
        error: String,
        message: String,
    }
    let status = response.status();
    match response.json::<Body>().await {
        Ok(body) => CommandError {
            code: body.error,
            message: body.message,
        },
        Err(_) => CommandError::new(fallback, format!("HTTP {status}")),
    }
}

pub async fn register_admin_key(
    server: &str,
    name: &str,
    password: &str,
    key: &SecretKey,
    replace: bool,
) -> CommandResult<()> {
    let client = client()?;
    let unreachable = |e: reqwest::Error| CommandError::new("network", e);
    let response = client
        .post(format!("{server}/api/v1/admin/login"))
        .json(&serde_json::json!({ "name": name, "password": password }))
        .send()
        .await
        .map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(refusal(response, "bad_credentials").await);
    }
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::to_owned)
        .ok_or_else(|| CommandError::new("internal", "no session cookie"))?;

    let response = client
        .put(format!("{server}/api/v1/admin/admin-key"))
        .header(COOKIE, &cookie)
        .json(&crate::admin::registration(key, replace))
        .send()
        .await
        .map_err(unreachable)?;
    let result = if response.status().is_success() {
        Ok(())
    } else {
        Err(refusal(response, "internal").await)
    };
    // The session is not needed any more.
    let _ = client
        .post(format!("{server}/api/v1/admin/logout"))
        .header(COOKIE, &cookie)
        .send()
        .await;
    result
}
