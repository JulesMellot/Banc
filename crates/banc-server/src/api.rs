//! HTTP API: enrollment, server information and the admin.
//!
//! Errors are JSON, `{"error": "<code>", "message": "<English text>"}`. The
//! web admin shows the French text matching the code.
//!
//! Admins authenticate with a session cookie (`HttpOnly`, `SameSite=Strict`).
//! Every admin request that changes something takes a JSON body, which a
//! cross-site form cannot send.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use banc_proto::enroll::{self, EnrollRequest, EnrollResponse, ServerInfo};
use banc_proto::invite::{Invite, InviteToken};
use banc_proto::keys::{AdminKeyRegistration, Fingerprint, PublicKey};
use banc_proto::machine::{MachineRole, PersonRole};
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::store::{self, EnrollError, EnrollRefusal, StoreError};
use crate::{auth, relay_proxy};

/// Session cookie name.
const SESSION_COOKIE: &str = "banc_session";
/// An admin session lasts 12 hours.
const SESSION_LIFETIME_SECS: i64 = 12 * 3600;
/// Longest admin or user name.
const MAX_NAME_LEN: usize = 64;

type AppResult<T> = Result<T, ApiError>;

pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route(enroll::INFO_PATH, get(info))
        .route(enroll::ENROLL_PATH, post(enroll_machine))
        .route("/api/v1/admin/setup", post(setup))
        .route("/api/v1/admin/login", post(login))
        .route("/api/v1/admin/logout", post(logout))
        .route("/api/v1/admin/me", get(me))
        .route(
            "/api/v1/admin/admin-key",
            get(get_admin_key).put(put_admin_key),
        )
        .route(
            "/api/v1/admin/invites",
            get(list_invites).post(create_invite),
        )
        .route("/api/v1/admin/invites/{id}/revoke", post(revoke_invite))
        .route("/api/v1/admin/machines", get(list_machines))
        .route("/api/v1/admin/machines/{id}/revoke", post(revoke_machine))
        .route(relay_proxy::RELAY_PATH, any(relay))
        .route(relay_proxy::PROBE_PATH, any(relay))
        .route(relay_proxy::CAPTIVE_PORTAL_PATH, any(relay));
    api.fallback(fallback).with_state(state)
}

// ---------------------------------------------------------------- errors

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    fn internal(error: &dyn std::fmt::Display) -> Self {
        tracing::error!("internal error: {error:#}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "server error",
        )
    }

    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "not found")
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self::internal(&anyhow::Error::from(error))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body<'a> {
            error: &'a str,
            message: &'a str,
        }
        let body = Body {
            error: self.code,
            message: &self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

/// JSON body, with JSON errors when it does not parse.
struct Body<T>(T);

impl<S, T> axum::extract::FromRequest<S> for Body<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: axum::extract::Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::bad_request("bad_request", rejection.body_text())),
        }
    }
}

// ---------------------------------------------------------------- public

#[derive(Serialize)]
struct InfoResponse<'a> {
    #[serde(flatten)]
    server: &'a ServerInfo,
    /// No admin yet: the web admin shows the first-admin form.
    setup_required: bool,
}

async fn info(State(state): State<Arc<AppState>>) -> AppResult<Response> {
    let setup_required = state.setup().is_some();
    let server = state.server_info();
    let body = InfoResponse {
        server: &server,
        setup_required,
    };
    Ok(Json(body).into_response())
}

async fn enroll_machine(
    State(state): State<Arc<AppState>>,
    Body(request): Body<EnrollRequest>,
) -> AppResult<Json<EnrollResponse>> {
    if !request.is_signed() {
        return Err(ApiError::bad_request(
            "bad_signature",
            "the request is not signed with the machine key",
        ));
    }
    let name = enroll::clean_name(&request.name)
        .ok_or_else(|| ApiError::bad_request("bad_name", "invalid machine name"))?;
    if !short_ascii(&request.arch) || !short_ascii(&request.agent_version) {
        return Err(ApiError::bad_request(
            "bad_request",
            "invalid arch or version",
        ));
    }

    let machine = store::NewMachine {
        public_key: &request.machine_key,
        name,
        os: request.os,
        arch: &request.arch,
        agent_version: &request.agent_version,
    };
    let token_hash = auth::token_hash(request.token.as_bytes());
    let enrolled = store::enroll(&mut state.db(), &token_hash, &machine, state.now());
    let enrolled = match enrolled {
        Ok(enrolled) => enrolled,
        Err(EnrollError::Refused(refusal)) => {
            tracing::info!(key = %request.machine_key.fmt_short(), "enrollment refused: {refusal}");
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                refusal_code(refusal),
                refusal.to_string(),
            ));
        }
        Err(EnrollError::Store(error)) => return Err(error.into()),
    };
    tracing::info!(
        machine = enrolled.machine_id,
        name,
        os = %request.os,
        key = %request.machine_key.fmt_short(),
        "machine enrolled"
    );
    Ok(Json(EnrollResponse {
        machine_id: enrolled.machine_id,
        machine_role: enrolled.machine_role,
        person_role: enrolled.person_role,
        admin_key: enrolled.admin_key,
        server: state.server_info(),
    }))
}

fn refusal_code(refusal: EnrollRefusal) -> &'static str {
    match refusal {
        EnrollRefusal::UnknownInvite => "invite_unknown",
        EnrollRefusal::InviteUsed => "invite_used",
        EnrollRefusal::InviteRevoked => "invite_revoked",
        EnrollRefusal::InviteExpired => "invite_expired",
        EnrollRefusal::AdminKeyChanged => "invite_obsolete",
        EnrollRefusal::KeyAlreadyEnrolled => "machine_key_known",
    }
}

fn short_ascii(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

// ---------------------------------------------------------------- sessions

/// The admin behind the request's session cookie.
pub struct AdminSession(store::Admin);

impl FromRequestParts<Arc<AppState>> for AdminSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        let unauthorized =
            || ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "log in first");
        let token = session_token(&parts.headers).ok_or_else(unauthorized)?;
        let admin = store::session_admin(&state.db(), &auth::token_hash(&token), state.now())?;
        admin.map(Self).ok_or_else(unauthorized)
    }
}

/// The session token of the cookie, decoded.
fn session_token(headers: &HeaderMap) -> Option<Vec<u8>> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .and_then(|(_, value)| data_encoding::BASE64URL_NOPAD.decode(value.as_bytes()).ok())
}

fn session_cookie(state: &AppState, value: &str, max_age: i64) -> AppResult<HeaderValue> {
    let https = state
        .public_url()
        .is_some_and(|url| url.starts_with("https://"));
    let secure = if https { "; Secure" } else { "" };
    let cookie = format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    );
    HeaderValue::from_str(&cookie).map_err(|e| ApiError::internal(&e))
}

/// Opens a session and returns the cookie that carries it.
fn open_session(state: &AppState, admin_id: i64) -> AppResult<HeaderValue> {
    let token: [u8; 32] = auth::random_bytes().map_err(|e| ApiError::internal(&e))?;
    let now = state.now();
    store::create_session(
        &state.db(),
        &auth::token_hash(&token),
        admin_id,
        now,
        now + SESSION_LIFETIME_SECS,
    )?;
    let value = data_encoding::BASE64URL_NOPAD.encode(&token);
    session_cookie(state, &value, SESSION_LIFETIME_SECS)
}

fn clean_admin_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME_LEN
        && !name.chars().any(char::is_control);
    if valid {
        Ok(name)
    } else {
        Err(ApiError::bad_request("bad_name", "invalid name"))
    }
}

/// Runs password hashing off the async threads.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> AppResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::internal(&e))
}

/// The address the browser used to reach the server, as the proxy in front
/// reports it. `None` for anything but HTTPS, or `localhost` in development.
fn detect_public_url(headers: &HeaderMap) -> Option<String> {
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let host = first("x-forwarded-host").or_else(|| first("host"))?;
    let scheme = first("x-forwarded-proto").unwrap_or("http");
    banc_proto::invite::normalize_server(&format!("{scheme}://{host}")).ok()
}

/// Without a configured address, the server keeps the one the admin used.
/// Only called once the request has proved it comes from an admin.
fn remember_public_url(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    if state.public_url().is_some() {
        return Ok(());
    }
    let url = detect_public_url(headers).ok_or_else(|| {
        ApiError::bad_request(
            "public_url_unknown",
            "open the server through its https:// address",
        )
    })?;
    store::set_public_url(&state.db(), &url, state.now())?;
    tracing::info!(url, "public address learned");
    state.adopt_public_url(url);
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupRequest {
    token: String,
    name: String,
    password: String,
}

/// Creates the first admin, with the token the server wrote at its first
/// start.
async fn setup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Body(request): Body<SetupRequest>,
) -> AppResult<Response> {
    let token_ok = |setup: &Option<crate::state::SetupToken>| {
        setup
            .as_ref()
            .is_some_and(|setup| setup.hash == auth::token_hash(request.token.trim().as_bytes()))
    };
    if !token_ok(&state.setup()) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "bad_setup_token",
            "invalid setup token",
        ));
    }
    let name = clean_admin_name(&request.name)?.to_owned();
    if !auth::password_acceptable(&request.password) {
        return Err(ApiError::bad_request("weak_password", "password too short"));
    }
    let password = request.password;
    let hash = blocking(move || auth::hash_password(&password))
        .await?
        .map_err(|e| ApiError::internal(&e))?;

    // Checked again under the lock: two simultaneous requests create one
    // admin.
    let mut setup = state.setup();
    if !token_ok(&setup) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "bad_setup_token",
            "invalid setup token",
        ));
    }
    remember_public_url(&state, &headers)?;
    let id = {
        let db = state.db();
        if store::admin_count(&db)? > 0 {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "already_set_up",
                "an admin already exists",
            ));
        }
        store::create_admin(&db, &name, &hash, state.now())?
            .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "name_taken", "name taken"))?
    };
    if let Some(file) = setup.take().and_then(|setup| setup.file) {
        if let Err(error) = std::fs::remove_file(&file) {
            tracing::warn!(file = %file.display(), "cannot delete the setup token file: {error}");
        }
    }
    drop(setup);
    tracing::info!(admin = id, "first admin created");
    let cookie = open_session(&state, id)?;
    let body = Json(store::Admin { id, name });
    Ok(([(SET_COOKIE, cookie)], body).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    name: String,
    password: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Body(request): Body<LoginRequest>,
) -> AppResult<Response> {
    let found = store::admin_by_name(&state.db(), request.name.trim())?;
    let password = request.password;
    let admin = blocking(move || match found {
        Some((admin, hash)) if auth::verify_password(&hash, &password) => Some(admin),
        Some(_) => None,
        None => {
            auth::waste_a_check(&password);
            None
        }
    })
    .await?;
    let Some(admin) = admin else {
        tracing::info!("admin login refused");
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "bad_credentials",
            "wrong name or password",
        ));
    };
    tracing::info!(admin = admin.id, "admin logged in");
    if let Err(error) = remember_public_url(&state, &headers) {
        tracing::warn!("public address not learned at login: {}", error.message);
    }
    let cookie = open_session(&state, admin.id)?;
    Ok(([(SET_COOKIE, cookie)], Json(admin)).into_response())
}

async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(token) = session_token(&headers) {
        store::delete_session(&state.db(), &auth::token_hash(&token))?;
    }
    let cookie = session_cookie(&state, "", 0)?;
    Ok(([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

async fn me(AdminSession(admin): AdminSession) -> Json<store::Admin> {
    Json(admin)
}

// ---------------------------------------------------------------- admin key

#[derive(Serialize)]
struct AdminKeyResponse {
    public_key: Option<PublicKey>,
    fingerprint: Option<Fingerprint>,
}

async fn get_admin_key(
    State(state): State<Arc<AppState>>,
    _admin: AdminSession,
) -> AppResult<Json<AdminKeyResponse>> {
    let key = store::admin_key(&state.db())?;
    Ok(Json(AdminKeyResponse {
        public_key: key,
        fingerprint: key.as_ref().map(Fingerprint::of),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminKeyRequest {
    #[serde(flatten)]
    registration: AdminKeyRegistration,
    /// Replacing an existing key cuts every workstation off from future
    /// manifests: it must be asked for explicitly.
    #[serde(default)]
    replace: bool,
}

/// Registers the admin key, sent by the admin's Banc application.
async fn put_admin_key(
    State(state): State<Arc<AppState>>,
    AdminSession(admin): AdminSession,
    Body(request): Body<AdminKeyRequest>,
) -> AppResult<Json<AdminKeyResponse>> {
    let registration = request.registration;
    if !registration.is_valid() {
        return Err(ApiError::bad_request(
            "bad_signature",
            "the registration is not signed with the key",
        ));
    }
    let key = registration.public_key;
    let mut db = state.db();
    match store::admin_key(&db)? {
        Some(current) if current == key => {}
        Some(_) if !request.replace => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "admin_key_exists",
                "an admin key is already registered",
            ));
        }
        _ => {
            store::set_admin_key(&mut db, &key, admin.id, state.now())?;
            tracing::warn!(admin = admin.id, key = %key.fmt_short(), "admin key registered");
        }
    }
    Ok(Json(AdminKeyResponse {
        public_key: Some(key),
        fingerprint: Some(Fingerprint::of(&key)),
    }))
}

// ---------------------------------------------------------------- invitations

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteRequest {
    user_name: String,
    person_role: PersonRole,
    #[serde(default = "default_machine_role")]
    machine_role: MachineRole,
}

fn default_machine_role() -> MachineRole {
    MachineRole::Workstation
}

#[derive(Serialize)]
struct InviteCreated {
    invite: store::InviteRow,
    /// Shown once: the server only keeps the token's hash.
    link: String,
    key: String,
    server: String,
}

async fn create_invite(
    State(state): State<Arc<AppState>>,
    AdminSession(admin): AdminSession,
    Body(request): Body<InviteRequest>,
) -> AppResult<Json<InviteCreated>> {
    let user_name = clean_admin_name(&request.user_name)?;
    let server = state.public_url().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "public_url_unknown",
            "open the server through its https:// address",
        )
    })?;
    let db = state.db();
    let admin_key = store::admin_key(&db)?.ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "admin_key_missing",
            "register the admin key from the Banc application first",
        )
    })?;
    let token = InviteToken::from_bytes(auth::random_bytes().map_err(|e| ApiError::internal(&e))?);
    let new = store::NewInvite {
        token_hash: &auth::token_hash(token.as_bytes()),
        user_name,
        person_role: request.person_role,
        machine_role: request.machine_role,
        admin_key: &admin_key,
        created_by: admin.id,
    };
    let row = store::create_invite(&db, &new, state.now())?;
    tracing::info!(admin = admin.id, invite = row.id, "invitation created");
    let invite = Invite {
        server,
        token,
        fingerprint: Fingerprint::of(&admin_key),
    };
    Ok(Json(InviteCreated {
        invite: row,
        link: invite.link(),
        key: invite.key(),
        server: invite.server,
    }))
}

async fn list_invites(
    State(state): State<Arc<AppState>>,
    _admin: AdminSession,
) -> AppResult<Json<Vec<store::InviteRow>>> {
    Ok(Json(store::list_invites(&state.db(), state.now())?))
}

async fn revoke_invite(
    State(state): State<Arc<AppState>>,
    AdminSession(admin): AdminSession,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if store::revoke_invite(&state.db(), id, admin.id, state.now())? {
        tracing::info!(admin = admin.id, invite = id, "invitation revoked");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found())
    }
}

// ---------------------------------------------------------------- machines

#[derive(Serialize)]
struct MachineView {
    #[serde(flatten)]
    machine: store::MachineRow,
    /// Its control connection is open right now.
    online: bool,
}

async fn list_machines(
    State(state): State<Arc<AppState>>,
    _admin: AdminSession,
) -> AppResult<Json<Vec<MachineView>>> {
    let machines = store::list_machines(&state.db())?;
    let views = machines
        .into_iter()
        .map(|machine| {
            let online = machine
                .public_key
                .parse::<PublicKey>()
                .is_ok_and(|key| state.connections.is_connected(&key));
            MachineView { machine, online }
        })
        .collect();
    Ok(Json(views))
}

async fn revoke_machine(
    State(state): State<Arc<AppState>>,
    AdminSession(admin): AdminSession,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let key = store::revoke_machine(&state.db(), id, admin.id, state.now())?;
    let Some(key) = key else {
        return Err(ApiError::not_found());
    };
    state.connections.disconnect(&key);
    tracing::warn!(admin = admin.id, machine = id, "machine revoked");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- relay

/// The relay's paths, on the same port as the API: a hosting platform gives
/// the server one domain and one HTTP port.
async fn relay(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
) -> AppResult<Response> {
    let Some(relay) = state.relay_addr.get().copied() else {
        return Err(ApiError::not_found());
    };
    relay_proxy::forward(relay, request).await.map_err(|error| {
        tracing::warn!("relay proxy: {error}");
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "relay_unavailable",
            "relay unavailable",
        )
    })
}

// ---------------------------------------------------------------- web admin

/// Outside the API: files of the web admin, `index.html` for the paths of
/// the single-page application.
async fn fallback(State(state): State<Arc<AppState>>, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") {
        return ApiError::not_found().into_response();
    }
    let Some(root) = &state.admin_ui else {
        return ApiError::not_found().into_response();
    };
    let Some(relative) = safe_relative_path(path) else {
        return ApiError::not_found().into_response();
    };
    let file = root.join(&relative);
    let (file, content) = match tokio::fs::read(&file).await {
        Ok(content) => (file, content),
        // An unknown path without an extension is a route of the
        // application.
        Err(_) if !relative.contains('.') => {
            let index = root.join("index.html");
            match tokio::fs::read(&index).await {
                Ok(content) => (index, content),
                Err(_) => return ApiError::not_found().into_response(),
            }
        }
        Err(_) => return ApiError::not_found().into_response(),
    };
    let content_type = content_type(&file);
    let cache = if relative.starts_with("assets/") {
        // Vite puts a hash in these names.
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static(cache),
            ),
            (
                axum::http::header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
        ],
        content,
    )
        .into_response()
}

/// The request path as a relative file path, `index.html` for `/`. `None`
/// for anything that could leave the folder.
fn safe_relative_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    if trimmed.is_empty() {
        return Some("index.html".to_owned());
    }
    let safe = trimmed.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.contains(['\\', ':', '%'])
            && !part.starts_with('.')
    });
    safe.then(|| trimmed.to_owned())
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_paths() {
        assert_eq!(safe_relative_path("/").as_deref(), Some("index.html"));
        assert_eq!(
            safe_relative_path("/assets/index-abc.js").as_deref(),
            Some("assets/index-abc.js")
        );
        assert_eq!(safe_relative_path("/machines").as_deref(), Some("machines"));
        for path in [
            "/../etc/passwd",
            "/assets/../../x",
            "/./x",
            "//x",
            "/a\\b",
            "/c:/x",
            "/%2e%2e/x",
            "/.env",
        ] {
            assert_eq!(safe_relative_path(path), None, "{path}");
        }
    }

    #[test]
    fn cookie_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("theme=dark; banc_session=AQID"),
        );
        assert_eq!(session_token(&headers), Some(vec![1, 2, 3]));
        headers.insert(COOKIE, HeaderValue::from_static("banc_session=***"));
        assert_eq!(session_token(&headers), None);
        assert_eq!(session_token(&HeaderMap::new()), None);
    }

    #[test]
    fn public_address_from_the_proxy() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, HeaderValue::from_static(value));
            }
            map
        };
        let behind_proxy = headers(&[
            ("host", "banc:8080"),
            ("x-forwarded-host", "banc.example.com"),
            ("x-forwarded-proto", "https"),
        ]);
        assert_eq!(
            detect_public_url(&behind_proxy).as_deref(),
            Some("https://banc.example.com")
        );
        let chain = headers(&[
            ("host", "banc.example.com"),
            ("x-forwarded-proto", "https, http"),
        ]);
        assert_eq!(
            detect_public_url(&chain).as_deref(),
            Some("https://banc.example.com")
        );
        let local = headers(&[("host", "localhost:8080")]);
        assert_eq!(
            detect_public_url(&local).as_deref(),
            Some("http://localhost:8080")
        );
        // Plain HTTP on a real address would put an insecure link in invitations.
        for refused in [
            headers(&[("host", "banc.example.com")]),
            headers(&[("host", "203.0.113.5:8080"), ("x-forwarded-proto", "http")]),
            headers(&[("x-forwarded-proto", "https")]),
        ] {
            assert_eq!(detect_public_url(&refused), None);
        }
    }

    #[test]
    fn version_and_arch_text() {
        assert!(short_ascii("aarch64"));
        assert!(short_ascii("0.1.0-dev+abc"));
        assert!(!short_ascii(""));
        assert!(!short_ascii("x86 64"));
        assert!(!short_ascii(&"x".repeat(33)));
    }
}
