//! Shared state of the API handlers.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rupta_auth::crypto::Keys;
use rupta_auth::oidc::{OidcClient, OidcSettings};
use rupta_auth::ratelimit::LoginGuard;
use rupta_store::Store;
use rupta_store::squad::Connection;

use crate::error::{ApiError, ApiResult};
use crate::mail::Mailer;

/// Settings of the API, from the server configuration.
#[derive(Clone, Debug)]
pub struct ApiConfig {
    /// Base URL of the instance, without a trailing slash.
    pub public_url: String,
    /// `RUPTA_SECRET_KEY`.
    pub secret_key: Vec<u8>,
    /// Whether email and password login is allowed.
    pub local_login: bool,
    /// The instance-wide single sign-on provider.
    pub oidc: Option<OidcSettings>,
    /// Organization that single sign-on users join.
    pub default_org: String,
    /// `RUPTA_SMTP_URL`.
    pub smtp_url: Option<String>,
    /// `RUPTA_SMTP_FROM`.
    pub smtp_from: Option<String>,
    /// Origins allowed to call the API from browsers (CORS).
    pub cors_allowed_origins: Vec<String>,
    /// Proxies whose `X-Forwarded-For` is trusted (IPs or CIDR ranges).
    pub trusted_proxies: Vec<String>,
    /// Largest JSON request body.
    pub max_body_bytes: usize,
    /// The SSO connection test may fetch issuers on loopback, link-local
    /// and private addresses (`RUPTA_SSO_ALLOW_PRIVATE_ISSUERS`).
    pub sso_allow_private_issuers: bool,
    /// Issue trackers and paging services may be reached on loopback,
    /// link-local and private addresses
    /// (`RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS`).
    pub integrations_allow_private_hosts: bool,
}

impl ApiConfig {
    /// Settings for tests and local use.
    pub fn new(public_url: &str, secret_key: &[u8]) -> ApiConfig {
        ApiConfig {
            public_url: public_url.trim_end_matches('/').to_string(),
            secret_key: secret_key.to_vec(),
            local_login: true,
            oidc: None,
            default_org: "default".into(),
            smtp_url: None,
            smtp_from: None,
            cors_allowed_origins: Vec::new(),
            trusted_proxies: Vec::new(),
            max_body_bytes: 1 << 20,
            sso_allow_private_issuers: false,
            integrations_allow_private_hosts: false,
        }
    }
}

/// Counters exported on `/metrics`.
#[derive(Default, Debug)]
pub struct ApiMetrics {
    pub logins: AtomicU64,
    pub login_failures: AtomicU64,
    pub login_lockouts: AtomicU64,
    pub sso_logins: AtomicU64,
    pub sso_failures: AtomicU64,
    pub alerts_fired: AtomicU64,
    pub notifications_sent: AtomicU64,
    pub notification_failures: AtomicU64,
    pub artifact_bundles: AtomicU64,
    /// Counters with labels: `(family, labels) -> value`.
    labeled: Mutex<BTreeMap<SeriesKey, u64>>,
}

/// A labeled series: family name and label pairs.
type SeriesKey = (&'static str, Vec<(&'static str, String)>);

/// Counter families with labels, rendered even before their first sample.
pub const LABELED_COUNTERS: &[(&str, &str)] = &[
    ("rupta_alert_notifications_total", "Alert notifications by channel kind and result (success, failure)."),
    (
        "rupta_integration_requests_total",
        "Calls to issue trackers by kind, operation and result (success or the failure reason).",
    ),
    ("rupta_integration_webhooks_total", "Incoming tracker webhooks by kind and result."),
    (
        "rupta_integration_sync_total",
        "Status synchronisation between issues and tickets by kind, direction and result.",
    ),
];

impl ApiMetrics {
    pub fn inc(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds one to a labeled counter of [`LABELED_COUNTERS`].
    pub fn count(&self, family: &'static str, labels: &[(&'static str, &str)]) {
        self.add(family, labels, 1);
    }

    /// Adds `n` to a labeled counter of [`LABELED_COUNTERS`].
    pub fn add(&self, family: &'static str, labels: &[(&'static str, &str)], n: u64) {
        debug_assert!(LABELED_COUNTERS.iter().any(|(f, _)| *f == family), "{family} is not declared");
        if n == 0 {
            return;
        }
        let key = (family, labels.iter().map(|(k, v)| (*k, v.to_string())).collect());
        *self.labeled.lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_default() += n;
    }

    /// The value of a labeled counter (tests).
    pub fn get(&self, family: &'static str, labels: &[(&'static str, &str)]) -> u64 {
        let key = (family, labels.iter().map(|(k, v)| (*k, v.to_string())).collect());
        self.labeled.lock().unwrap_or_else(|e| e.into_inner()).get(&key).copied().unwrap_or(0)
    }

    /// Prometheus text format.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (name, help, v) in [
            ("rupta_logins_total", "Successful password logins.", &self.logins),
            ("rupta_login_failures_total", "Failed password logins.", &self.login_failures),
            ("rupta_login_lockouts_total", "Accounts or IPs locked after failed logins.", &self.login_lockouts),
            ("rupta_sso_logins_total", "Successful single sign-on logins.", &self.sso_logins),
            ("rupta_sso_failures_total", "Failed single sign-on logins.", &self.sso_failures),
            ("rupta_alerts_fired_total", "Alert rules that fired.", &self.alerts_fired),
            ("rupta_notifications_sent_total", "Notifications delivered.", &self.notifications_sent),
            ("rupta_notification_failures_total", "Notifications that failed.", &self.notification_failures),
            ("rupta_artifact_bundles_total", "Artifact bundles assembled.", &self.artifact_bundles),
        ] {
            out.push_str(&format!(
                "# HELP {name} {help}\n# TYPE {name} counter\n{name} {}\n",
                v.load(Ordering::Relaxed)
            ));
        }
        let labeled = self.labeled.lock().unwrap_or_else(|e| e.into_inner());
        for (family, help) in LABELED_COUNTERS {
            let _ = write!(out, "# HELP {family} {help}\n# TYPE {family} counter\n");
            for ((f, labels), v) in labeled.iter() {
                if f != family {
                    continue;
                }
                let l: Vec<String> = labels.iter().map(|(k, v)| format!("{k}=\"{}\"", escape_label(v))).collect();
                let _ = writeln!(out, "{family}{{{}}} {v}", l.join(","));
            }
        }
        out
    }
}

/// Escapes a label value for the Prometheus text format.
pub fn escape_label(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Everything the API handlers need.
#[derive(Clone)]
pub struct ApiState {
    pub store: Store,
    pub config: Arc<ApiConfig>,
    pub keys: Keys,
    pub oidc: OidcClient,
    pub mailer: Option<Mailer>,
    pub login_guard: Arc<LoginGuard>,
    pub metrics: Arc<ApiMetrics>,
    /// For notifications (webhooks, Slack, Teams).
    pub http: reqwest::Client,
    /// For issue trackers and paging services (SSRF guard, retries).
    pub outbound: crate::integrations::http::Outbound,
    /// Tickets being created by alert rules (issue, integration).
    pub ticket_in_flight: Arc<crate::integrations::sync::InFlight>,
    trusted_proxies: Arc<Vec<(IpAddr, u8)>>,
}

impl ApiState {
    pub fn new(store: Store, config: ApiConfig) -> ApiState {
        let mailer = match (&config.smtp_url, &config.smtp_from) {
            (Some(url), Some(from)) => match Mailer::from_url(url, from) {
                Ok(m) => Some(m),
                Err(e) => {
                    tracing::error!(error = %e, "invalid RUPTA_SMTP_URL or RUPTA_SMTP_FROM; email is disabled");
                    None
                }
            },
            (Some(_), None) => {
                tracing::warn!("RUPTA_SMTP_URL is set without RUPTA_SMTP_FROM; email is disabled");
                None
            }
            _ => None,
        };
        let trusted_proxies = config.trusted_proxies.iter().filter_map(|p| parse_cidr(p)).collect();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(concat!("rupta/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        ApiState {
            store,
            outbound: crate::integrations::http::Outbound::new(config.integrations_allow_private_hosts),
            ticket_in_flight: Arc::default(),
            keys: Keys::derive(&config.secret_key),
            config: Arc::new(config),
            oidc: OidcClient::new(),
            mailer,
            login_guard: Arc::default(),
            metrics: Arc::default(),
            http,
            trusted_proxies: Arc::new(trusted_proxies),
        }
    }

    /// Runs database work on the blocking pool.
    pub async fn db<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> ApiResult<T> + Send + 'static,
    {
        let db = self.store.database().clone();
        tokio::task::spawn_blocking(move || f(&db.connect())).await.map_err(ApiError::internal)?
    }

    /// True if cookies must carry `Secure`.
    pub fn secure_cookies(&self) -> bool {
        self.config.public_url.starts_with("https://")
    }

    /// The client address: the peer, or the last untrusted hop of
    /// `X-Forwarded-For` when the peer is a trusted proxy.
    pub fn client_ip(&self, peer: Option<IpAddr>, forwarded_for: Option<&str>) -> Option<IpAddr> {
        let peer = peer?;
        if !self.is_trusted(peer) {
            return Some(peer);
        }
        let Some(xff) = forwarded_for else { return Some(peer) };
        let hops: Vec<IpAddr> = xff.split(',').filter_map(|h| h.trim().parse().ok()).collect();
        for hop in hops.iter().rev() {
            if !self.is_trusted(*hop) {
                return Some(*hop);
            }
        }
        hops.first().copied().or(Some(peer))
    }

    fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted_proxies.iter().any(|(net, bits)| in_cidr(ip, *net, *bits))
    }
}

fn parse_cidr(s: &str) -> Option<(IpAddr, u8)> {
    let (ip, bits) = match s.split_once('/') {
        Some((ip, bits)) => (ip.parse::<IpAddr>().ok()?, bits.parse::<u8>().ok()?),
        None => {
            let ip: IpAddr = s.parse().ok()?;
            (ip, if ip.is_ipv4() { 32 } else { 128 })
        }
    };
    Some((ip, bits))
}

fn in_cidr(ip: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let bits = bits.min(32);
            if bits == 0 {
                return true;
            }
            let mask = u32::MAX << (32 - bits);
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let bits = bits.min(128);
            if bits == 0 {
                return true;
            }
            let mask = u128::MAX << (128 - bits);
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_for_is_used_only_behind_trusted_proxies() {
        let mut config = ApiConfig::new("http://localhost", b"0123456789abcdef0123456789abcdef");
        config.trusted_proxies = vec!["10.0.0.0/8".into()];
        let state = ApiState::new(Store::open_in_memory().unwrap(), config);
        let proxy: IpAddr = "10.1.2.3".parse().unwrap();
        let outsider: IpAddr = "203.0.113.9".parse().unwrap();
        assert_eq!(state.client_ip(Some(proxy), Some("198.51.100.1, 10.0.0.5")), Some("198.51.100.1".parse().unwrap()));
        assert_eq!(state.client_ip(Some(outsider), Some("198.51.100.1")), Some(outsider));
        assert_eq!(state.client_ip(Some(proxy), None), Some(proxy));
    }
}
