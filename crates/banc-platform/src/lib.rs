//! Tout le code de Banc qui dépend de l'OS.
//!
//! Règle du dépôt : aucun `cfg` d'OS en dehors de cette crate
//! (`cargo xtask check architecture` le vérifie). Chaque module regroupe ses
//! variantes par OS dans des sous-modules `linux`, `macos`, `windows`.

pub mod logging;
pub mod paths;
pub mod signal;
