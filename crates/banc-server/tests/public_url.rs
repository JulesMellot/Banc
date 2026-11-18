//! A server deployed with no address in its configuration learns it from the
//! request that creates the first admin, and keeps it.

#![cfg(test)]

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use banc_proto::keys::{AdminKeyRegistration, SecretKey};
use banc_server::config::Config;
use banc_server::server::{self, Running};
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery staple";

async fn start(dir: &Path) -> Running {
    let text = format!(
        "data_dir = '{}'\n[http]\nlisten = \"127.0.0.1:0\"\n\
         [iroh]\nlisten = \"127.0.0.1:0\"\n[relay]\nlisten = \"127.0.0.1:0\"\n",
        dir.join("data").display()
    );
    let config = Config::parse(&text, dir).unwrap();
    server::start(&config, Arc::new(|| 1_800_000_000))
        .await
        .unwrap()
}

/// A request as the platform's HTTPS proxy forwards it.
async fn call(
    server: &Running,
    method: Method,
    path: &str,
    proto: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Option<String>, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "banc:8080")
        .header("x-forwarded-host", "banc.example.com")
        .header("x-forwarded-proto", proto);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
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
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, cookie, json)
}

fn setup_body(dir: &Path) -> Value {
    let token = std::fs::read_to_string(dir.join("data/setup-token")).unwrap();
    json!({ "token": token.trim(), "name": "jules", "password": PASSWORD })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn learned_from_the_first_admin_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let running = start(dir.path()).await;
    assert_eq!(running.state.public_url(), None);
    assert_eq!(running.state.server_info().relay_url, None);

    // Over plain HTTP nothing is created: the admin can try again in HTTPS.
    let body = setup_body(dir.path());
    let (status, _, json) = call(
        &running,
        Method::POST,
        "/api/v1/admin/setup",
        "http",
        None,
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(json["error"], "public_url_unknown");
    assert_eq!(running.state.public_url(), None);

    // A wrong token teaches the server nothing.
    let mut wrong = body.clone();
    wrong["token"] = json!("wrong");
    let path = "/api/v1/admin/setup";
    let (status, _, _) = call(&running, Method::POST, path, "https", None, Some(wrong)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(running.state.public_url(), None);

    let (status, cookie, json) =
        call(&running, Method::POST, path, "https", None, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let cookie = cookie.unwrap();
    assert_eq!(
        running.state.public_url().as_deref(),
        Some("https://banc.example.com")
    );

    // The relay and the invitations use it at once.
    let (_, _, info) = call(&running, Method::GET, "/api/v1/info", "https", None, None).await;
    assert_eq!(info["relay_url"], "https://banc.example.com");
    let key = serde_json::to_value(AdminKeyRegistration::signed(&SecretKey::generate())).unwrap();
    let path = "/api/v1/admin/admin-key";
    let (status, _, json) = call(
        &running,
        Method::PUT,
        path,
        "https",
        Some(&cookie),
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let invite = json!({ "user_name": "Léa", "person_role": "editor" });
    let path = "/api/v1/admin/invites";
    let (status, _, json) = call(
        &running,
        Method::POST,
        path,
        "https",
        Some(&cookie),
        Some(invite),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["server"], "https://banc.example.com");

    // Still known after a restart.
    running.shutdown().await.unwrap();
    let running = start(dir.path()).await;
    assert_eq!(
        running.state.public_url().as_deref(),
        Some("https://banc.example.com")
    );
    assert_eq!(
        running.state.server_info().relay_url.as_deref(),
        Some("https://banc.example.com")
    );
    running.shutdown().await.unwrap();
}
