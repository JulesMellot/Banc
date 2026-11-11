use std::future::poll_fn;
use std::task::Poll;

use tokio::signal::unix::{signal, SignalKind};

pub(super) async fn shutdown() -> std::io::Result<()> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    poll_fn(|cx| {
        if terminate.poll_recv(cx).is_ready() || interrupt.poll_recv(cx).is_ready() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    Ok(())
}
