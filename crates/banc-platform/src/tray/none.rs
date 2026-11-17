use std::future::Future;
use std::time::Duration;

use super::{Tray, TrayError};

pub(super) fn run<S, F>(
    _: Tray,
    _: Duration,
    _: S,
    _: impl FnMut() + 'static,
) -> Result<(), TrayError>
where
    S: FnMut() -> F + Send + 'static,
    F: Future<Output = String>,
{
    Err(TrayError::Unsupported)
}
