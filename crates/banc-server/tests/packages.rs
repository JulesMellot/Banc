//! The package store and the publication of manifests, through the HTTP API.

#![cfg(test)]

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use banc_proto::keys::{AdminKeyRegistration, SecretKey};
use banc_proto::machine::Os;
use banc_proto::manifest::{
    Manifest, Package, PackageBuild, PackageFile, PackageKind, Place, SignedManifest,
};
use banc_server::config::Config;
use banc_server::server::{self, Running};
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery staple";

struct Harness {
    _dir: tempfile::TempDir,
    server: Running,
    cookie: String,
    admin_key: SecretKey,
}

async fn send(
    server: &Running,
    request: axum::http::request::Builder,
    body: Body,
) -> (StatusCode, Option<String>, Value) {
    let router = banc_server::api::router(server.state.clone());
    let response = router.oneshot(request.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let cookie = response.headers().get(header::SET_COOKIE).map(|value| {
        value
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    });
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .unwrap();
    (
        status,
        cookie,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

impl Harness {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let text = format!(
            "data_dir = '{}'\npublic_url = \"http://localhost:8080\"\n[http]\nlisten = \"127.0.0.1:0\"\n\
             [iroh]\nlisten = \"127.0.0.1:0\"\n[relay]\nenabled = false\n",
            dir.path().join("data").display()
        );
        let config = Config::parse(&text, dir.path()).unwrap();
        let server = server::start(&config, Arc::new(|| 1_800_000_000))
            .await
            .unwrap();
        let token = read_token(dir.path());
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/admin/setup")
            .header(header::CONTENT_TYPE, "application/json");
        let body = json!({ "token": token, "name": "jules", "password": PASSWORD });
        let (status, cookie, _) = send(&server, request, Body::from(body.to_string())).await;
        assert_eq!(status, StatusCode::OK);
        let mut harness = Self {
            _dir: dir,
            server,
            cookie: cookie.unwrap(),
            admin_key: SecretKey::from_bytes(&[3; 32]),
        };
        harness.register_key().await;
        harness
    }

    async fn register_key(&mut self) {
        let registration = AdminKeyRegistration::signed(&self.admin_key);
        let body = serde_json::to_value(registration).unwrap();
        let (status, _) = self
            .json(Method::PUT, "/api/v1/admin/admin-key", Some(body))
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn json(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, &self.cookie);
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let (status, _, json) = send(&self.server, request, body).await;
        (status, json)
    }

    async fn upload(&self, name: &str, content: &'static [u8]) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/admin/packages/files?name={name}"))
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/octet-stream");
        let (status, _, json) = send(&self.server, request, Body::from(content)).await;
        (status, json)
    }

    async fn stored_file(&self) -> PackageFile {
        let (status, row) = self.upload("Team.cube", b"lut").await;
        assert_eq!(status, StatusCode::OK, "{row}");
        PackageFile {
            blake3: row["blake3"].as_str().unwrap().to_owned(),
            size: row["size"].as_u64().unwrap(),
            name: "Team.cube".to_owned(),
        }
    }

    async fn publish(&self, signed: &SignedManifest) -> (StatusCode, Value) {
        let body = serde_json::to_value(signed).unwrap();
        self.json(Method::POST, "/api/v1/admin/manifest", Some(body))
            .await
    }
}

fn read_token(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("data/setup-token"))
        .unwrap()
        .trim()
        .to_owned()
}

fn lut(file: PackageFile) -> Package {
    Package {
        id: "team-lut".to_owned(),
        version: "1.0".to_owned(),
        required: true,
        builds: vec![PackageBuild {
            os: Os::Macos,
            arch: None,
            kind: PackageKind::Copy,
            file: Some(file),
            place: Some(Place::Lut),
            args: Vec::new(),
            instructions: None,
        }],
    }
}

fn manifest(version: u64, packages: Vec<Package>) -> Manifest {
    Manifest {
        version,
        resolve: None,
        packages,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_uploaded_file_is_kept_under_its_hash() {
    let harness = Harness::start().await;
    let file = harness.stored_file().await;
    assert_eq!(file.blake3, blake3::hash(b"lut").to_hex().as_str());
    let stored = harness.server.state.packages_dir.join(&file.blake3);
    assert_eq!(std::fs::read(stored).unwrap(), b"lut");

    let (status, files) = harness
        .json(Method::GET, "/api/v1/admin/packages/files", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(files[0]["name"], "Team.cube");

    // The same content again keeps one line.
    harness.stored_file().await;
    let (_, files) = harness
        .json(Method::GET, "/api/v1/admin/packages/files", None)
        .await;
    assert_eq!(files.as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_needs_a_session_a_binary_body_and_a_plain_name() {
    let harness = Harness::start().await;
    let path = "/api/v1/admin/packages/files?name=a.cube";

    let anonymous = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/octet-stream");
    let (status, _, _) = send(&harness.server, anonymous, Body::from("x")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let form = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::COOKIE, &harness.cookie)
        .header(header::CONTENT_TYPE, "text/plain");
    let (status, _, _) = send(&harness.server, form, Body::from("x")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = harness.upload("..%2F..%2Fevil", b"x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "bad_file_name");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_is_checked_while_editing() {
    let harness = Harness::start().await;
    let path = "/api/v1/admin/manifest/draft";

    let unknown = PackageFile {
        blake3: "ab".repeat(32),
        size: 3,
        name: "Team.cube".to_owned(),
    };
    let draft = json!({ "packages": [lut(unknown)] });
    let (status, body) = harness.json(Method::PUT, path, Some(draft)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "package_file_missing");

    let mut bad = lut(harness.stored_file().await);
    bad.id = "Not Valid".to_owned();
    let (status, body) = harness
        .json(Method::PUT, path, Some(json!({ "packages": [bad] })))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "bad_manifest");

    let good = lut(harness.stored_file().await);
    let (status, _) = harness
        .json(Method::PUT, path, Some(json!({ "packages": [good] })))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, view) = harness
        .json(Method::GET, "/api/v1/admin/manifest", None)
        .await;
    assert_eq!(view["changed"], true);
    assert_eq!(view["next_version"], 1);
    assert_eq!(view["draft"]["packages"][0]["id"], "team-lut");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_admin_key_can_publish() {
    let harness = Harness::start().await;
    let file = harness.stored_file().await;
    let wanted = manifest(1, vec![lut(file)]);

    let forged = SignedManifest::sign(&SecretKey::from_bytes(&[9; 32]), &wanted).unwrap();
    let (status, body) = harness.publish(&forged).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "bad_signature");

    let mut changed = SignedManifest::sign(&harness.admin_key, &wanted).unwrap();
    changed.payload = changed.payload.replace("team-lut", "evil-lut");
    let (status, _) = harness.publish(&changed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, view) = harness
        .json(Method::GET, "/api/v1/admin/manifest", None)
        .await;
    assert_eq!(view["published"], Value::Null);

    let signed = SignedManifest::sign(&harness.admin_key, &wanted).unwrap();
    let (status, _) = harness.publish(&signed).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(*harness.server.state.manifest_version.borrow(), 1);

    let (_, view) = harness
        .json(Method::GET, "/api/v1/admin/manifest", None)
        .await;
    assert_eq!(view["published"]["version"], 1);
    assert_eq!(view["next_version"], 2);
    assert_eq!(view["changed"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn versions_follow_each_other_and_files_must_be_stored() {
    let harness = Harness::start().await;
    let file = harness.stored_file().await;

    let skipped = SignedManifest::sign(&harness.admin_key, &manifest(5, vec![])).unwrap();
    let (status, body) = harness.publish(&skipped).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "manifest_stale");

    let first = SignedManifest::sign(&harness.admin_key, &manifest(1, vec![])).unwrap();
    assert_eq!(harness.publish(&first).await.0, StatusCode::NO_CONTENT);
    let (status, _) = harness.publish(&first).await;
    assert_eq!(status, StatusCode::CONFLICT, "the same version again");

    let missing = PackageFile {
        blake3: "cd".repeat(32),
        ..file
    };
    let unknown =
        SignedManifest::sign(&harness.admin_key, &manifest(2, vec![lut(missing)])).unwrap();
    let (status, body) = harness.publish(&unknown).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "package_file_missing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_package_routes_need_a_session() {
    let harness = Harness::start().await;
    for (method, path) in [
        (Method::GET, "/api/v1/admin/packages/files"),
        (Method::GET, "/api/v1/admin/packages/grid"),
        (Method::GET, "/api/v1/admin/manifest"),
    ] {
        let request = Request::builder().method(method).uri(path);
        let (status, _, _) = send(&harness.server, request, Body::empty()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
}
