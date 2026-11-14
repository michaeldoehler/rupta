//! Event ingestion: the SDK wire protocol (envelopes and the legacy store
//! endpoint), the processing pipeline (normalization, scrubbing,
//! symbolication, grouping) and the batch writer.
//!
//! ```text
//! HTTP handler ──try_send──► bounded queue ──► N workers ──► batch writer ──► squad
//!  auth, limits,             (429 when full)    normalize,     one transaction
//!  decompress, parse                            scrub, group,  per 50 ms or
//!                                               symbolicate    1,000 events
//! ```
//!
//! See `docs/ingestion.md`.

pub mod auth;
pub mod cron;
pub mod custom_metrics;
pub mod decode;
pub mod envelope;
pub mod escalation;
pub mod filters;
mod http;
pub mod logs;
pub mod metrics;
mod process;
pub mod profiles;
pub mod ratelimit;
pub mod replays;
pub mod retention;
pub mod symbolicate;
mod traces;
mod writer;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use chrono::{DateTime, Utc};
use rupta_core::signal::IssueSignal;
use rupta_store::Store;
use rupta_store::ingest::OutcomeKey;

use crate::auth::{KeyCache, ProjectInfo};
use crate::envelope::Envelope;
use crate::metrics::{Category, Metrics, Outcome};

pub use ipnet::IpNet;

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
    /// Largest event (or transaction) JSON accepted, in bytes.
    pub max_event_bytes: usize,
    /// Largest attachment accepted, in bytes.
    pub max_attachment_bytes: usize,
    /// Proxies whose `X-Forwarded-For` is trusted (addresses or CIDR ranges).
    pub trusted_proxies: Vec<IpNet>,
    /// Writer threads committing batches side by side.
    pub writers: usize,
    /// Most events per batch transaction.
    pub batch_size: usize,
    /// Longest time an event waits for its batch.
    pub batch_interval: Duration,
    /// Days data is kept when a project sets no `retention_days`.
    pub retention_days: u32,
    /// Memory for parsed source maps.
    pub sourcemap_cache_bytes: usize,
    /// Projects with their own entry in the per-project metrics.
    pub metrics_project_labels: usize,
}

impl Default for IngestConfig {
    fn default() -> IngestConfig {
        IngestConfig {
            workers: std::thread::available_parallelism().map_or(4, |n| n.get()),
            queue: 100_000,
            safe: false,
            max_envelope_bytes: 20 << 20,
            max_decompressed_bytes: 100 << 20,
            public_url: "http://localhost:8080".into(),
            max_event_bytes: 1 << 20,
            max_attachment_bytes: 100 << 20,
            trusted_proxies: Vec::new(),
            writers: 1,
            batch_size: 1000,
            batch_interval: Duration::from_millis(50),
            retention_days: 90,
            sourcemap_cache_bytes: 256 << 20,
            metrics_project_labels: 50,
        }
    }
}

/// Parses `RUPTA_TRUSTED_PROXIES` entries: IP addresses or CIDR ranges.
pub fn parse_trusted_proxies<S: AsRef<str>>(items: &[S]) -> Result<Vec<IpNet>, String> {
    items
        .iter()
        .map(|s| {
            let s = s.as_ref().trim();
            s.parse::<IpNet>()
                .or_else(|_| s.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| format!("not an IP address or CIDR range: {s:?}"))
        })
        .collect()
}

/// One accepted envelope, queued for a worker.
pub(crate) struct Job {
    pub project: Arc<ProjectInfo>,
    pub envelope: Envelope,
    pub received: DateTime<Utc>,
    pub client_ip: Option<IpAddr>,
    pub user_agent: Option<String>,
    /// Durability `safe`: answered once the envelope is committed.
    pub ack: Option<tokio::sync::oneshot::Sender<bool>>,
}

pub(crate) enum Msg {
    Job(Box<Job>),
    Stop,
}

pub(crate) struct Inner {
    pub store: Store,
    pub config: IngestConfig,
    pub keys: KeyCache,
    pub limiter: ratelimit::RateLimiter,
    pub metrics: Metrics,
    pub signals: tokio::sync::broadcast::Sender<IssueSignal>,
    pub tx: crossbeam_channel::Sender<Msg>,
    pub closed: AtomicBool,
    /// Envelopes taken by a worker and not yet committed.
    pub in_flight: AtomicUsize,
    /// Envelopes held in a writer's batch (not yet committed).
    pub collected: AtomicUsize,
    pub workers: usize,
    pub threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    /// Outcomes decided outside the pipeline (rate limits, full queue),
    /// written with the next batch.
    pub pending_outcomes: Mutex<HashMap<OutcomeKey, i64>>,
    pub issues: process::IssueCache,
    pub symbolicator: symbolicate::Symbolicator,
    pub writer_state: writer::Shared,
}

impl Inner {
    /// Counts an outcome in the metrics and, for a known project, in the
    /// `outcomes` table (written with the next batch).
    pub fn outcome(
        &self,
        project_id: Option<i64>,
        category: Category,
        outcome: Outcome,
        reason: &str,
        n: u64,
        at: DateTime<Utc>,
    ) {
        self.metrics.count(category, outcome, n);
        if let Some(project_id) = project_id {
            let (outcome_name, reason) = match outcome {
                Outcome::QueueFull => ("rate_limited", "queue_full"),
                o => (o.as_str(), reason),
            };
            let key = writer::outcome_key(project_id, at, category, outcome_name, reason);
            *self.pending_outcomes.lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_default() += n as i64;
        }
    }
}

/// The running pipeline.
#[derive(Clone)]
pub struct Ingest {
    pub store: Store,
    pub config: IngestConfig,
    inner: Arc<Inner>,
}

impl Ingest {
    /// Starts the pipeline's background workers.
    pub fn start(store: Store, config: IngestConfig) -> Ingest {
        let workers = config.workers.max(1);
        let (tx, rx) = crossbeam_channel::bounded::<Msg>(config.queue.max(1));
        let (wtx, wrx) = crossbeam_channel::bounded::<process::Processed>(config.batch_size.max(1) * 4);
        let (signals, _) = tokio::sync::broadcast::channel(4096);
        let inner = Arc::new(Inner {
            keys: KeyCache::new(store.clone()),
            store: store.clone(),
            limiter: Default::default(),
            metrics: Metrics::with_project_cap(config.metrics_project_labels),
            signals,
            tx,
            closed: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            collected: AtomicUsize::new(0),
            workers,
            threads: Mutex::new(Vec::new()),
            pending_outcomes: Mutex::new(HashMap::new()),
            issues: process::IssueCache::default(),
            symbolicator: symbolicate::Symbolicator::new(config.sourcemap_cache_bytes),
            writer_state: writer::Shared::default(),
            config: config.clone(),
        });
        let mut threads = Vec::new();
        for i in 0..workers {
            let (inner, rx, wtx) = (inner.clone(), rx.clone(), wtx.clone());
            threads.push(
                std::thread::Builder::new()
                    .name(format!("ingest-worker-{i}"))
                    .spawn(move || process::worker(inner, rx, wtx))
                    .expect("spawning an ingest worker"),
            );
        }
        drop(wtx);
        for i in 0..config.writers.max(1) {
            let (winner, wrx) = (inner.clone(), wrx.clone());
            threads.push(
                std::thread::Builder::new()
                    .name(format!("ingest-writer-{i}"))
                    .spawn(move || writer::run(winner, wrx))
                    .expect("spawning an ingest writer"),
            );
        }
        *inner.threads.lock().unwrap_or_else(|e| e.into_inner()) = threads;
        Ingest { store, config, inner }
    }

    /// Routes `/api/{project_id}/envelope/`, `/store/` and `/security/`.
    pub fn router(&self) -> Router {
        http::router(self.inner.clone())
    }

    /// Issue signals (new issue, regression, ...) for the alert engine.
    /// Each call returns a new receiver; slow receivers miss signals.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<IssueSignal> {
        self.inner.signals.subscribe()
    }

    /// Runs escalation detection once and publishes its signals (blocking;
    /// see [`escalation`]).
    pub fn check_escalation(&self, now: DateTime<Utc>) -> rupta_store::Result<escalation::EscalationReport> {
        let report = escalation::run_once(&self.store, now)?;
        for s in &report.signals {
            let _ = self.inner.signals.send(s.clone());
        }
        Ok(report)
    }

    /// Runs escalation detection every five minutes.
    pub fn spawn_escalation(&self) -> tokio::task::JoinHandle<()> {
        let ingest = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(300));
            tick.tick().await;
            loop {
                tick.tick().await;
                let i = ingest.clone();
                match tokio::task::spawn_blocking(move || i.check_escalation(Utc::now())).await {
                    Ok(Ok(r)) if !r.escalated.is_empty() || !r.calmed.is_empty() => {
                        tracing::info!(escalated = r.escalated.len(), calmed = r.calmed.len(), "escalation check")
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => tracing::warn!(error = %e, "escalation check failed"),
                    Err(e) => tracing::error!(error = %e, "escalation task panicked"),
                }
            }
        })
    }

    /// Stops accepting events and writes everything queued.
    pub async fn shutdown(&self) {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = self.inner.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let threads = std::mem::take(&mut *inner.threads.lock().unwrap_or_else(|e| e.into_inner()));
            // One stop message per worker, queued behind everything accepted;
            // the writers stop when the workers are gone.
            for _ in 0..inner.workers {
                let _ = inner.tx.send(Msg::Stop);
            }
            for t in threads {
                let _ = t.join();
            }
        })
        .await;
    }

    /// Appends the ingest metrics in Prometheus text format.
    pub fn render_metrics(&self, out: &mut String) {
        self.inner.metrics.render(out, self.inner.tx.len(), self.config.queue);
    }

    /// Error events and new issues per project (see
    /// [`metrics::ProjectCounters`]).
    pub fn project_counts(&self) -> Vec<metrics::ProjectCount> {
        self.inner.metrics.projects.snapshot()
    }

    /// Envelopes waiting for a worker.
    pub fn queue_depth(&self) -> usize {
        self.inner.tx.len()
    }

    /// Items counted with an outcome since start (tests, diagnostics).
    pub fn outcome_count(&self, category: Category, outcome: Outcome) -> u64 {
        self.inner.metrics.get(category, outcome)
    }

    /// Batches committed since start.
    pub fn batches(&self) -> u64 {
        self.inner.metrics.batches()
    }

    /// Waits until everything accepted so far is committed (tests,
    /// benchmarks). Returns false on timeout.
    pub async fn flush(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        loop {
            if self.inner.tx.is_empty() && self.inner.in_flight.load(Ordering::Acquire) == 0 {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_proxy_lists() {
        let p = parse_trusted_proxies(&["10.0.0.0/8", " 192.168.1.5 ", "::1"]).unwrap();
        assert!(p[0].contains(&"10.1.2.3".parse::<IpAddr>().unwrap()));
        assert!(p[1].contains(&"192.168.1.5".parse::<IpAddr>().unwrap()));
        assert!(!p[1].contains(&"192.168.1.6".parse::<IpAddr>().unwrap()));
        assert!(parse_trusted_proxies(&["nope"]).is_err());
    }
}
