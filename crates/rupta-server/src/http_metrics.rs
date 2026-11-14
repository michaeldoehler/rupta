//! Request counts and latency per route template and status class, for
//! `/metrics`: `rupta_http_requests_total` and the histogram
//! `rupta_http_request_duration_seconds`, both labeled `method`, `route`
//! (the route template such as `/api/v1/issues/{id}`, or `unmatched` for
//! the web app and unknown paths) and `status` (`2xx`, `4xx`, …).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;

use crate::expo::Expo;

/// Upper bounds of the latency histogram, in seconds.
pub const BUCKETS: [f64; 12] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];

#[derive(Default, Clone)]
struct Series {
    buckets: [u64; BUCKETS.len()],
    count: u64,
    sum: f64,
}

type Key = (&'static str, String, &'static str);

/// The collected request metrics.
#[derive(Default)]
pub struct HttpMetrics {
    series: Mutex<BTreeMap<Key, Series>>,
}

fn method_label(m: &Method) -> &'static str {
    match *m {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        _ => "other",
    }
}

fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    }
}

impl HttpMetrics {
    pub fn observe(&self, method: &Method, route: &str, status: u16, seconds: f64) {
        let key = (method_label(method), route.to_string(), status_class(status));
        let mut g = self.series.lock().unwrap_or_else(|e| e.into_inner());
        let s = g.entry(key).or_default();
        s.count += 1;
        s.sum += seconds;
        if let Some(i) = BUCKETS.iter().position(|b| seconds <= *b) {
            s.buckets[i] += 1;
        }
    }

    pub fn render(&self, out: &mut Expo) {
        let g = self.series.lock().unwrap_or_else(|e| e.into_inner());
        out.family("rupta_http_requests_total", "counter", "HTTP requests by method, route template and status class.");
        for ((m, r, c), s) in g.iter() {
            out.sample("rupta_http_requests_total", &[("method", m), ("route", r), ("status", c)], s.count as f64);
        }
        out.family(
            "rupta_http_request_duration_seconds",
            "histogram",
            "Time from receiving a request to sending the response headers.",
        );
        for ((m, r, c), s) in g.iter() {
            let mut cumulative = 0;
            for (i, b) in BUCKETS.iter().enumerate() {
                cumulative += s.buckets[i];
                let le = b.to_string();
                out.sample(
                    "rupta_http_request_duration_seconds_bucket",
                    &[("method", m), ("route", r), ("status", c), ("le", &le)],
                    cumulative as f64,
                );
            }
            let labels = [("method", *m), ("route", r.as_str()), ("status", *c)];
            out.sample(
                "rupta_http_request_duration_seconds_bucket",
                &[labels[0], labels[1], labels[2], ("le", "+Inf")],
                s.count as f64,
            );
            out.sample("rupta_http_request_duration_seconds_sum", &labels, s.sum);
            out.sample("rupta_http_request_duration_seconds_count", &labels, s.count as f64);
        }
    }
}

/// The middleware: times every request.
pub async fn track(State(m): State<Arc<HttpMetrics>>, req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let route = req.extensions().get::<MatchedPath>().map(|p| p.as_str().to_string());
    let resp = next.run(req).await;
    let route = route.unwrap_or_else(|| "unmatched".to_string());
    m.observe(&method, &route, resp.status().as_u16(), start.elapsed().as_secs_f64());
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::routing::get;
    use tower::ServiceExt;

    #[tokio::test]
    async fn routes_are_labeled_by_template() {
        let m = Arc::new(HttpMetrics::default());
        let app = Router::new()
            .route("/items/{id}", get(|| async { "ok" }))
            .fallback(|| async { (axum::http::StatusCode::NOT_FOUND, "no") })
            .layer(axum::middleware::from_fn_with_state(m.clone(), track));
        for uri in ["/items/1", "/items/2", "/nothing"] {
            app.clone().oneshot(axum::http::Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        }
        let mut e = Expo::default();
        m.render(&mut e);
        let s = e.finish();
        assert!(
            s.contains("rupta_http_requests_total{method=\"GET\",route=\"/items/{id}\",status=\"2xx\"} 2\n"),
            "{s}"
        );
        assert!(s.contains("rupta_http_requests_total{method=\"GET\",route=\"unmatched\",status=\"4xx\"} 1\n"), "{s}");
        assert!(s.contains("rupta_http_request_duration_seconds_bucket{method=\"GET\",route=\"/items/{id}\",status=\"2xx\",le=\"+Inf\"} 2\n"), "{s}");
        assert!(
            s.contains(
                "rupta_http_request_duration_seconds_count{method=\"GET\",route=\"/items/{id}\",status=\"2xx\"} 2\n"
            ),
            "{s}"
        );
    }
}
