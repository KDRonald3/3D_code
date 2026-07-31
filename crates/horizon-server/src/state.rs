//! Shared server state: optional map loaded at startup or via `POST /api/map`.

use horizon_map::Repository;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory map slot shared across handlers.
#[derive(Clone, Default)]
pub struct AppState {
    pub map: Arc<RwLock<Option<Repository>>>,
}

impl AppState {
    pub fn new(initial: Option<Repository>) -> Self {
        Self {
            map: Arc::new(RwLock::new(initial)),
        }
    }
}
