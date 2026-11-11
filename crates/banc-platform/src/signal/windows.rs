pub(super) async fn shutdown() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}
