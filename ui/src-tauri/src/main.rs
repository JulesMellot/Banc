//! Banc window. Started when the user opens Banc or clicks a `banc://`
//! link, and ended when the window closes.
//!
//! It holds no state of its own besides the admin key: everything about the
//! workstation is asked to the agent.

// Without a console window on Windows (ignored elsewhere).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod admin;
mod agent;
mod error;
mod links;
mod server;

use tauri::Manager;

fn main() {
    let result = tauri::Builder::default()
        // A second launch (a link clicked while the window is open) hands
        // its link over and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .manage(admin::PendingKey::default())
        .manage(admin::PendingManifest::default())
        .manage(links::Links::default())
        .setup(|app| {
            links::watch(app);
            agent::start_session();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent::agent_status,
            agent::enroll,
            agent::confirm_manual,
            agent::install_agent,
            agent::hostname,
            agent::read_link,
            admin::admin_key_status,
            admin::admin_key_generate,
            admin::admin_key_confirm,
            admin::admin_key_restore,
            admin::admin_key_register,
            admin::manifest_prepare,
            admin::manifest_publish,
            admin::manifest_cancel,
            links::pending_link,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        tracing::error!("window stopped: {error}");
    }
}
