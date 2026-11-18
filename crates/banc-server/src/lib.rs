//! Banc server: API, admin, iroh relay, cache, render queue.
//!
//! Phase 1: enrollment, invitations, machines, admin accounts, iroh endpoint
//! with the control channel, relay.

pub mod api;
pub mod auth;
pub mod config;
pub mod control;
pub mod db;
pub mod packages;
pub mod relay;
pub mod relay_proxy;
pub mod server;
pub mod state;
pub mod store;
