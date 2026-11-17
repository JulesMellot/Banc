//! Messages and types shared by the server, the agent and the session helper.
//!
//! Phase 1: invitation links, enrollment, the control channel between an
//! agent and the server, machine descriptions.

pub mod control;
pub mod enroll;
pub mod invite;
pub mod keys;
pub mod machine;
mod text;
