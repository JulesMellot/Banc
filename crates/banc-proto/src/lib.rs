//! Messages and types shared by the server, the agent and the session helper.
//!
//! Phase 1: invitation links, enrollment, the control channel between an
//! agent and the server, the local channel between the interface and the
//! agent, machine descriptions, the admin key.

pub mod admin_key;
pub mod control;
pub mod enroll;
pub mod invite;
pub mod keys;
pub mod local;
pub mod machine;
pub mod manifest;
mod text;
pub mod update;

/// Version of the workstation programs: the one given by the release
/// workflow, `0.0.0` for a development build.
pub const VERSION: &str = match option_env!("BANC_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
