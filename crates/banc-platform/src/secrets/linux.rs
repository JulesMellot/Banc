use super::{Backend, SecretError};
use crate::paths::{self, Scope};

/// Files in the data folder: `/var/lib/banc/secrets` for the service,
/// readable by root only.
pub(super) fn backend(scope: Scope) -> Result<Backend, SecretError> {
    Ok(Backend::Files(paths::state_dir(scope)?.join("secrets")))
}
