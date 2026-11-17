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
mod text;
