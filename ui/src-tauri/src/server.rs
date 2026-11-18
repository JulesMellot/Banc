//! The few server calls the application makes itself: registering the admin
//! key with an admin account, and publishing a manifest with the key alone.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use banc_proto::keys::SecretKey;
use banc_proto::manifest::{
    DraftRequest, Manifest, Package, RequiredResolve, SignedManifest, DRAFT_PATH, PUBLISH_PATH,
};
use reqwest::header::{COOKIE, SET_COOKIE};
use serde::Deserialize;

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

fn unreachable(error: reqwest::Error) -> CommandError {
    CommandError::new("network", error)
}

/// Opens an admin session and returns its cookie.
async fn login(
    client: &reqwest::Client,
    server: &str,
    name: &str,
    password: &str,
) -> CommandResult<String> {
    let response = client
        .post(format!("{server}/api/v1/admin/login"))
        .json(&serde_json::json!({ "name": name, "password": password }))
        .send()
        .await
        .map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(refusal(response, "bad_credentials").await);
    }
    response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::to_owned)
        .ok_or_else(|| CommandError::new("internal", "no session cookie"))
}

/// The session is not needed any more.
async fn logout(client: &reqwest::Client, server: &str, cookie: &str) {
    let _ = client
        .post(format!("{server}/api/v1/admin/logout"))
        .header(COOKIE, cookie)
        .send()
        .await;
}

pub async fn register_admin_key(
    server: &str,
    name: &str,
    password: &str,
    key: &SecretKey,
    replace: bool,
) -> CommandResult<()> {
    let client = client()?;
    let cookie = login(&client, server, name, password).await?;

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
    logout(&client, server, &cookie).await;
    result
}

/// What the server proposes to publish.
#[derive(Debug, Deserialize)]
pub struct ManifestView {
    pub draft: Draft,
    pub changed: bool,
    pub next_version: u64,
}

#[derive(Debug, Deserialize)]
pub struct Draft {
    #[serde(default)]
    pub resolve: Option<RequiredResolve>,
    #[serde(default)]
    pub packages: Vec<Package>,
}

/// The manifest to sign, from what the server proposes. It is only a
/// proposal: the admin reads this exact manifest before signing it.
pub fn manifest_to_sign(view: ManifestView) -> CommandResult<Manifest> {
    let manifest = Manifest {
        version: view.next_version,
        resolve: view.draft.resolve,
        packages: view.draft.packages,
    };
    manifest
        .check()
        .map_err(|e| CommandError::new("bad_manifest", e))?;
    Ok(manifest)
}

/// The manifest the admin is reading, and where it goes once signed.
pub struct PublishSession {
    pub server: String,
    pub manifest: Manifest,
}

/// Reads the manifest to publish, and whether it differs from what is
/// published. The server refuses a key it does not know: the workstations
/// would refuse the manifest too.
pub async fn prepare_publication(
    server: &str,
    key: &SecretKey,
) -> CommandResult<(PublishSession, bool)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| CommandError::new("internal", e))?
        .as_secs();
    let now = i64::try_from(now).map_err(|e| CommandError::new("internal", e))?;
    let response = client()?
        .post(format!("{server}{DRAFT_PATH}"))
        .json(&DraftRequest::signed(key, now))
        .send()
        .await
        .map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(refusal(response, "internal").await);
    }
    let view: ManifestView = response
        .json()
        .await
        .map_err(|e| CommandError::new("internal", e))?;
    let changed = view.changed;
    let session = PublishSession {
        server: server.to_owned(),
        manifest: manifest_to_sign(view)?,
    };
    Ok((session, changed))
}

/// Sends the signed manifest. Its signature is all the server asks for.
pub async fn publish(session: &PublishSession, signed: &SignedManifest) -> CommandResult<()> {
    let response = client()?
        .post(format!("{}{PUBLISH_PATH}", session.server))
        .json(signed)
        .send()
        .await
        .map_err(unreachable)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(refusal(response, "internal").await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(packages: serde_json::Value) -> ManifestView {
        serde_json::from_value(serde_json::json!({
            "published": null,
            "draft": { "resolve": { "version": "21.1" }, "packages": packages },
            "changed": true,
            "next_version": 4,
        }))
        .unwrap()
    }

    #[test]
    fn the_manifest_takes_the_next_version() {
        let manifest = manifest_to_sign(view(serde_json::json!([]))).unwrap();
        assert_eq!(manifest.version, 4);
        assert_eq!(manifest.resolve.unwrap().version, "21.1");
    }

    #[test]
    fn a_malformed_proposal_is_never_signed() {
        let packages = serde_json::json!([
            { "id": "Not Valid", "version": "1", "required": true, "builds": [] }
        ]);
        let error = manifest_to_sign(view(packages)).unwrap_err();
        assert_eq!(error.code, "bad_manifest");
    }
}
