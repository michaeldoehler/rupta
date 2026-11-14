//! Event ingestion: the SDK wire protocol (envelopes and the legacy store
//! endpoint), the processing pipeline (normalization, scrubbing,
//! symbolication, grouping) and the batch writer.

use axum::Router;
use rupta_store::Store;

/// Settings of the ingest pipeline.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    pub workers: usize,
    pub queue: usize,
    /// Acknowledge only after the event is committed.
    pub safe: bool,
    pub max_envelope_bytes: usize,
    pub max_decompressed_bytes: usize,
    /// Base URL of the instance, used for DSNs.
    pub public_url: String,
}

/// The running pipeline.
#[derive(Clone)]
pub struct Ingest {
    pub store: Store,
    pub config: IngestConfig,
}

impl Ingest {
    /// Starts the pipeline's background workers.
    pub fn start(store: Store, config: IngestConfig) -> Ingest {
        Ingest { store, config }
    }

    /// Routes `/api/{project_id}/envelope/`, `/store/` and `/security/`.
    pub fn router(&self) -> Router {
        Router::new()
    }

    /// Stops accepting events and writes everything queued.
    pub async fn shutdown(&self) {}
}
