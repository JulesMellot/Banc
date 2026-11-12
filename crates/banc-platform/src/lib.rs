//! All of Banc's OS-dependent code.
//!
//! Repository rule: no OS `cfg` outside this crate
//! (`cargo xtask check architecture` checks it). Each module groups its
//! per-OS variants in `linux`, `macos` and `windows` submodules.

pub mod logging;
pub mod paths;
pub mod signal;
