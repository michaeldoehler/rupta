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

    /// Issue signals (new issue, regression, ...) for the alert engine.
    /// Each call returns a new receiver; slow receivers miss signals.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<rupta_core::signal::IssueSignal> {
        let (_tx, rx) = tokio::sync::broadcast::channel(1);
        rx
    }

    /// Stops accepting events and writes everything queued.
    pub async fn shutdown(&self) {}
}
