//! Ingest counters in Prometheus text format. Plain atomics, no locks.

use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// Data categories, as used in outcomes, rate limits and metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Error,
    Default,
    Security,
    Transaction,
    Attachment,
    Session,
    Feedback,
    Monitor,
    Internal,
    Replay,
    Profile,
    Span,
    LogItem,
    MetricBucket,
    Unknown,
}

pub const CATEGORIES: [Category; 15] = [
    Category::Error,
    Category::Default,
    Category::Security,
    Category::Transaction,
    Category::Attachment,
    Category::Session,
    Category::Feedback,
    Category::Monitor,
    Category::Internal,
    Category::Replay,
    Category::Profile,
    Category::Span,
    Category::LogItem,
    Category::MetricBucket,
    Category::Unknown,
];

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Error => "error",
            Category::Default => "default",
            Category::Security => "security",
            Category::Transaction => "transaction",
            Category::Attachment => "attachment",
            Category::Session => "session",
            Category::Feedback => "feedback",
            Category::Monitor => "monitor",
            Category::Internal => "internal",
            Category::Replay => "replay",
            Category::Profile => "profile",
            Category::Span => "span",
            Category::LogItem => "log_item",
            Category::MetricBucket => "metric_bucket",
            Category::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Category {
        CATEGORIES.iter().copied().find(|c| c.as_str() == s).unwrap_or(match s {
            "user_report_v2" => Category::Feedback,
            "replay_video" => Category::Replay,
            "profile_chunk" => Category::Profile,
            "statsd" | "metric" => Category::MetricBucket,
            _ => Category::Unknown,
        })
    }

    /// The category of an envelope item type.
    pub fn of_item(ty: &str) -> Category {
        match ty {
            "event" => Category::Error,
            "security" => Category::Security,
            "transaction" => Category::Transaction,
            "attachment" => Category::Attachment,
            "session" | "sessions" => Category::Session,
            "user_report" | "feedback" => Category::Feedback,
            "check_in" => Category::Monitor,
            "client_report" => Category::Internal,
            "replay_event" | "replay_recording" | "replay_video" => Category::Replay,
            "profile" | "profile_chunk" => Category::Profile,
            "span" | "otel_span" => Category::Span,
            "log" | "otel_log" => Category::LogItem,
            "statsd" | "metric_buckets" | "metric_meta" => Category::MetricBucket,
            _ => Category::Unknown,
        }
    }

    fn index(self) -> usize {
        CATEGORIES.iter().position(|c| *c == self).unwrap_or(CATEGORIES.len() - 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Accepted,
    Filtered,
    RateLimited,
    Invalid,
    DroppedUnsupported,
    QueueFull,
    ClientDiscard,
}

const OUTCOMES: [Outcome; 7] = [
    Outcome::Accepted,
    Outcome::Filtered,
    Outcome::RateLimited,
    Outcome::Invalid,
    Outcome::DroppedUnsupported,
    Outcome::QueueFull,
    Outcome::ClientDiscard,
];

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Accepted => "accepted",
            Outcome::Filtered => "filtered",
            Outcome::RateLimited => "rate_limited",
            Outcome::Invalid => "invalid",
            Outcome::DroppedUnsupported => "dropped_unsupported",
            Outcome::QueueFull => "queue_full",
            Outcome::ClientDiscard => "client_discard",
        }
    }

    fn index(self) -> usize {
        OUTCOMES.iter().position(|o| *o == self).unwrap_or(0)
    }
}

/// Upper bounds of the batch commit time histogram, in seconds.
const BATCH_BUCKETS: [f64; 10] = [0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0];

#[derive(Default)]
pub struct Metrics {
    events: [[AtomicU64; 7]; 15],
    batches: AtomicU64,
    batch_buckets: [AtomicU64; 10],
    batch_count_over: AtomicU64,
    batch_micros_sum: AtomicU64,
    batch_errors: AtomicU64,
    pub(crate) http_requests: AtomicU64,
}

impl Metrics {
    pub fn count(&self, category: Category, outcome: Outcome, n: u64) {
        if n > 0 {
            self.events[category.index()][outcome.index()].fetch_add(n, Ordering::Relaxed);
        }
    }

    pub fn get(&self, category: Category, outcome: Outcome) -> u64 {
        self.events[category.index()][outcome.index()].load(Ordering::Relaxed)
    }

    pub fn batch(&self, seconds: f64, ok: bool) {
        self.batches.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.batch_errors.fetch_add(1, Ordering::Relaxed);
        }
        self.batch_micros_sum.fetch_add((seconds * 1e6) as u64, Ordering::Relaxed);
        match BATCH_BUCKETS.iter().position(|b| seconds <= *b) {
            Some(i) => self.batch_buckets[i].fetch_add(1, Ordering::Relaxed),
            None => self.batch_count_over.fetch_add(1, Ordering::Relaxed),
        };
    }

    pub fn batches(&self) -> u64 {
        self.batches.load(Ordering::Relaxed)
    }

    /// Appends the metrics in Prometheus text exposition format.
    pub fn render(&self, out: &mut String, queue_depth: usize, queue_capacity: usize) {
        let _ = writeln!(out, "# HELP rupta_ingest_queue_depth Envelopes waiting for an ingest worker.");
        let _ = writeln!(out, "# TYPE rupta_ingest_queue_depth gauge");
        let _ = writeln!(out, "rupta_ingest_queue_depth {queue_depth}");
        let _ = writeln!(out, "# HELP rupta_ingest_queue_capacity Capacity of the ingest queue.");
        let _ = writeln!(out, "# TYPE rupta_ingest_queue_capacity gauge");
        let _ = writeln!(out, "rupta_ingest_queue_capacity {queue_capacity}");
        let _ = writeln!(out, "# HELP rupta_ingest_events_total Envelope items by data category and outcome.");
        let _ = writeln!(out, "# TYPE rupta_ingest_events_total counter");
        for c in CATEGORIES {
            for o in OUTCOMES {
                let v = self.get(c, o);
                if v > 0 {
                    let _ = writeln!(
                        out,
                        "rupta_ingest_events_total{{category=\"{}\",outcome=\"{}\"}} {v}",
                        c.as_str(),
                        o.as_str()
                    );
                }
            }
        }
        let _ = writeln!(out, "# HELP rupta_ingest_batches_total Batches committed by the ingest writer.");
        let _ = writeln!(out, "# TYPE rupta_ingest_batches_total counter");
        let _ = writeln!(out, "rupta_ingest_batches_total {}", self.batches.load(Ordering::Relaxed));
        let _ = writeln!(out, "# HELP rupta_ingest_batch_errors_total Batches that failed to commit.");
        let _ = writeln!(out, "# TYPE rupta_ingest_batch_errors_total counter");
        let _ = writeln!(out, "rupta_ingest_batch_errors_total {}", self.batch_errors.load(Ordering::Relaxed));
        let _ = writeln!(out, "# HELP rupta_ingest_batch_seconds Time to write and commit one batch.");
        let _ = writeln!(out, "# TYPE rupta_ingest_batch_seconds histogram");
        let mut cumulative = 0;
        for (i, b) in BATCH_BUCKETS.iter().enumerate() {
            cumulative += self.batch_buckets[i].load(Ordering::Relaxed);
            let _ = writeln!(out, "rupta_ingest_batch_seconds_bucket{{le=\"{b}\"}} {cumulative}");
        }
        cumulative += self.batch_count_over.load(Ordering::Relaxed);
        let _ = writeln!(out, "rupta_ingest_batch_seconds_bucket{{le=\"+Inf\"}} {cumulative}");
        let _ = writeln!(
            out,
            "rupta_ingest_batch_seconds_sum {}",
            self.batch_micros_sum.load(Ordering::Relaxed) as f64 / 1e6
        );
        let _ = writeln!(out, "rupta_ingest_batch_seconds_count {cumulative}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_prometheus_text() {
        let m = Metrics::default();
        m.count(Category::Error, Outcome::Accepted, 3);
        m.count(Category::Replay, Outcome::DroppedUnsupported, 1);
        m.batch(0.004, true);
        m.batch(2.0, false);
        let mut s = String::new();
        m.render(&mut s, 5, 100);
        assert!(s.contains("rupta_ingest_queue_depth 5\n"));
        assert!(s.contains("rupta_ingest_events_total{category=\"error\",outcome=\"accepted\"} 3\n"));
        assert!(s.contains("rupta_ingest_events_total{category=\"replay\",outcome=\"dropped_unsupported\"} 1\n"));
        assert!(s.contains("rupta_ingest_batches_total 2\n"));
        assert!(s.contains("rupta_ingest_batch_seconds_bucket{le=\"0.005\"} 1\n"));
        assert!(s.contains("rupta_ingest_batch_seconds_bucket{le=\"+Inf\"} 2\n"));
        assert_eq!(Category::of_item("replay_recording"), Category::Replay);
        assert_eq!(Category::parse("user_report_v2"), Category::Feedback);
    }
}
