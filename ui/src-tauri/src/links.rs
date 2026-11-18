//! `banc://` links: the one that opened the window, and those clicked while
//! it is open.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

/// The last link received, until the interface takes it.
#[derive(Default)]
pub struct Links(Mutex<Option<String>>);

/// Event sent to the interface when a link arrives while it runs.
pub const EVENT: &str = "banc-link";
/// Event sent when the web admin asks for the draft to be signed.
pub const PUBLISH_EVENT: &str = "banc-publish";
/// The link the web admin opens after saving a draft. It carries nothing: the
/// window reads the draft from the server and shows it before any signature.
const PUBLISH_LINK: &str = "banc://publish";

pub fn watch(app: &tauri::App) {
    let handle = app.handle().clone();
    // Development builds are not installed: register the scheme at run
    // time where the OS allows it (Linux, Windows).
    if cfg!(debug_assertions) {
        let _ = app.deep_link().register_all();
    }
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        if let Some(url) = urls.first() {
            remember(&handle, url.to_string());
        }
    }
    app.deep_link().on_open_url(move |event| {
        if let Some(url) = event.urls().first() {
            if url.as_str().starts_with(PUBLISH_LINK) {
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.set_focus();
                }
                let _ = handle.emit(PUBLISH_EVENT, ());
                return;
            }
            remember(&handle, url.to_string());
            let _ = handle.emit(EVENT, url.to_string());
        }
    });
}

fn remember(app: &AppHandle, url: String) {
    if !url.starts_with(banc_proto::invite::LINK_PREFIX) {
        return;
    }
    let links = app.state::<Links>();
    *links.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(url);
}

/// The link not yet handled by the interface, if any.
#[tauri::command]
pub fn pending_link(links: tauri::State<'_, Links>) -> Option<String> {
    links.0.lock().unwrap_or_else(|p| p.into_inner()).take()
}
