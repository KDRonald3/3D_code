//! Shared server state: optional map loaded at startup or via `POST /api/map`,
//! plus the in-process analysis job slot for `POST /api/analyse`.

use horizon_map::Repository;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Lifecycle of a single in-process analysis job.
#[derive(Debug, Clone)]
pub enum AnalyseStatus {
    Idle,
    Running { path: PathBuf, started: Instant },
    Done { path: PathBuf, finished: Instant },
    Failed {
        path: PathBuf,
        error: String,
        finished: Instant,
    },
}

impl Default for AnalyseStatus {
    fn default() -> Self {
        Self::Idle
    }
}

/// In-memory map + analyse slots shared across handlers.
#[derive(Clone, Default)]
pub struct AppState {
    pub map: Arc<RwLock<Option<Repository>>>,
    pub analyse: Arc<RwLock<AnalyseStatus>>,
}

impl AppState {
    pub fn new(initial: Option<Repository>) -> Self {
        Self {
            map: Arc::new(RwLock::new(initial)),
            analyse: Arc::new(RwLock::new(AnalyseStatus::Idle)),
        }
    }
}
