//! Banc server: API, admin, iroh relay, cache, render queue.
//!
//! Phase 1: enrollment, invitations, machines, admin accounts, iroh endpoint
//! with the control channel, relay.

pub mod api;
pub mod auth;
pub mod config;
pub mod control;
pub mod db;
pub mod relay;
pub mod server;
pub mod state;
pub mod store;
