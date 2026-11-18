//! The few server calls the application makes itself, with an admin account:
//! registering the admin key, and publishing a signed manifest.

use std::time::Duration;

use banc_proto::keys::{PublicKey, SecretKey};
use banc_proto::manifest::{Manifest, Package, RequiredResolve, SignedManifest};
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

/// A session open between the preview and the signature.
pub struct PublishSession {
    pub server: String,
    pub cookie: String,
    pub manifest: Manifest,
}

/// Logs in and reads the manifest to publish. Refuses a server that does not
/// know this workstation's admin key: the workstations would refuse the
/// manifest.
pub async fn prepare_publication(
    server: &str,
    name: &str,
    password: &str,
    key: &PublicKey,
) -> CommandResult<PublishSession> {
    #[derive(Deserialize)]
    struct AdminKey {
        public_key: Option<PublicKey>,
    }

    let client = client()?;
    let cookie = login(&client, server, name, password).await?;
    let prepared = async {
        let registered: AdminKey = read(&client, server, &cookie, "admin-key").await?;
        if registered.public_key.as_ref() != Some(key) {
            return Err(CommandError::new(
                "admin_key_not_registered",
                "the server knows another admin key",
            ));
        }
        let view: ManifestView = read(&client, server, &cookie, "manifest").await?;
        manifest_to_sign(view)
    }
    .await;
    match prepared {
        Ok(manifest) => Ok(PublishSession {
            server: server.to_owned(),
            cookie,
            manifest,
        }),
        Err(error) => {
            logout(&client, server, &cookie).await;
            Err(error)
        }
    }
}

async fn read<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    server: &str,
    cookie: &str,
    path: &str,
) -> CommandResult<T> {
    let response = client
        .get(format!("{server}/api/v1/admin/{path}"))
        .header(COOKIE, cookie)
        .send()
        .await
        .map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(refusal(response, "internal").await);
    }
    response
        .json()
        .await
        .map_err(|e| CommandError::new("internal", e))
}

/// Sends the signed manifest and closes the session.
pub async fn publish(session: &PublishSession, signed: &SignedManifest) -> CommandResult<()> {
    let client = client()?;
    let response = client
        .post(format!("{}/api/v1/admin/manifest", session.server))
        .header(COOKIE, &session.cookie)
        .json(signed)
        .send()
        .await
        .map_err(unreachable)?;
    let result = if response.status().is_success() {
        Ok(())
    } else {
        Err(refusal(response, "internal").await)
    };
    logout(&client, &session.server, &session.cookie).await;
    result
}

/// Closes a session that will not publish.
pub async fn cancel(session: &PublishSession) {
    if let Ok(client) = client() {
        logout(&client, &session.server, &session.cookie).await;
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
