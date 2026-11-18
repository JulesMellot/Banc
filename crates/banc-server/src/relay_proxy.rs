//! Carries the relay's paths on the API's HTTP port.
//!
//! A hosting platform (Coolify and the like) gives a container one domain
//! and routes it to one port. The relay keeps its own local listener; these
//! paths are forwarded to it, WebSocket upgrade included. The relay still
//! decides who may use it.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::Response;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

/// Where relay clients connect.
pub const RELAY_PATH: &str = "/relay";
/// Latency probe of the relay clients.
pub const PROBE_PATH: &str = "/ping";
/// Captive portal check of the relay clients.
pub const CAPTIVE_PORTAL_PATH: &str = "/generate_204";

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("cannot reach the relay")]
    Connect(#[from] std::io::Error),
    #[error("relay connection")]
    Http(#[from] hyper::Error),
}

/// Sends the request to the relay and returns its answer. After a protocol
/// upgrade, bytes are copied both ways until one side closes.
pub async fn forward(relay: SocketAddr, mut request: Request) -> Result<Response, ProxyError> {
    let stream = TcpStream::connect(relay).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.with_upgrades().await {
            tracing::debug!("relay proxy connection: {error}");
        }
    });

    let client_upgrade = hyper::upgrade::on(&mut request);
    let mut response = sender.send_request(request).await?;
    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let relay_upgrade = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            match tokio::try_join!(client_upgrade, relay_upgrade) {
                Ok((client, relay)) => {
                    let (mut client, mut relay) = (TokioIo::new(client), TokioIo::new(relay));
                    if let Err(error) = tokio::io::copy_bidirectional(&mut client, &mut relay).await
                    {
                        tracing::debug!("relay proxy stream: {error}");
                    }
                }
                Err(error) => tracing::debug!("relay proxy upgrade: {error}"),
            }
        });
    }
    Ok(response.map(Body::new))
}
