//! Horizon web UI server — serves the adapted function-map viewer.
//!
//! Binds to an ephemeral loopback port, optionally loads a saved map from
//! `--map`, prints the URL, and opens the default browser.

use anyhow::{bail, Context, Result};
use clap::Parser;
use horizon_map::map_from_slice;
use horizon_server::{app, AppState};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::TcpListener;

#[derive(Debug, Parser)]
#[command(
    name = "horizon-server",
    about = "Serve the Horizon function-map review UI on loopback",
    long_about = "Start a local web UI that loads a Horizon Repository JSON and \
lets you audit call sites, conflicts, unresolved edges, and drop counters.\n\n\
Binds 127.0.0.1 on an OS-assigned ephemeral port, prints the URL, and opens \
the default browser. Pass --map to preload a saved map JSON so the page shows \
it immediately; otherwise use Open JSON… in the browser."
)]
struct Cli {
    /// Path to a saved Horizon map JSON to load at startup.
    #[arg(short, long, value_name = "FILE")]
    map: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let initial = match cli.map {
        Some(path) => {
            if !path.is_file() {
                bail!("map file does not exist: {}", path.display());
            }
            let bytes = std::fs::read(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let repo = map_from_slice(&bytes)
                .with_context(|| format!("failed to parse map {}", path.display()))?;
            Some(repo)
        }
        None => None,
    };

    let state = AppState::new(initial);
    let router = app(state);

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .context("failed to bind loopback ephemeral port")?;
    let addr = listener
        .local_addr()
        .context("failed to read bound address")?;

    let url = format!("http://{addr}/");
    println!("{url}");
    open_browser(&url);

    axum::serve(listener, router)
        .await
        .context("horizon-server exited with error")?;
    Ok(())
}

/// Open `url` in the default browser. Best-effort — failures are ignored so
/// the server still runs if no browser is available.
fn open_browser(url: &str) {
    let result = {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn()
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(url).spawn()
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            std::process::Command::new("xdg-open").arg(url).spawn()
        }
    };
    if let Err(err) = result {
        eprintln!("horizon-server: could not open browser: {err}");
    }
}
