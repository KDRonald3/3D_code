//! Horizon web UI server skeleton.
//!
//! Binds to an ephemeral loopback port and serves a placeholder response so the
//! crate layout and `axum` dependency are proven before the real UI lands.

use anyhow::{Context, Result};
use axum::{routing::get, Router};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let app = Router::new().route("/", get(placeholder));

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .context("failed to bind loopback ephemeral port")?;
    let addr = listener
        .local_addr()
        .context("failed to read bound address")?;

    println!("http://{addr}");

    axum::serve(listener, app)
        .await
        .context("horizon-server exited with error")?;
    Ok(())
}

async fn placeholder() -> &'static str {
    "Horizon server placeholder"
}
