//! Horizon web UI server library — router and handlers for the map viewer.
//!
//! The binary (`main.rs`) binds loopback, optionally loads `--map`, and opens
//! a browser. Tests exercise the router in-process.

pub mod app;
pub mod host_guard;
pub mod routes;
pub mod source;
pub mod state;
pub mod static_files;

pub use app::app;
pub use state::AppState;
