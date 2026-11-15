//! `rupta`: the error-tracking server.

mod config;
mod expo;
mod http_metrics;
mod ops;
mod web;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use rupta_store::{Store, StoreOptions};
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::config::{Config, LogFormat, ProcessEnv};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => {}
        Some("version" | "--version" | "-V") => {
            println!("rupta {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("healthcheck") => return ops::healthcheck_cli(),
        Some("help" | "--help" | "-h") => {
            println!(
                "usage: rupta [serve|healthcheck|version]\n\nConfiguration comes from RUPTA_* environment variables; see docs/configuration.md."
            );
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown command {other:?}; try `rupta help`"),
    }
    let config = Config::from_env(&ProcessEnv)?;
    init_tracing(&config);
    tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("rupta").build()?.block_on(serve(config))
}

fn init_tracing(config: &Config) {
    let filter = tracing_subscriber::EnvFilter::try_new(&config.log)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match config.log_format {
        LogFormat::Json => builder.json().with_current_span(false).init(),
        LogFormat::Text => builder.init(),
    }
}

/// Everything the server runs, shared by the handlers.
pub struct App {
    pub config: Config,
    pub store: Store,
    pub ingest: rupta_ingest::Ingest,
    pub api: rupta_api::ApiState,
    pub http_metrics: Arc<http_metrics::HttpMetrics>,
    pub metrics_cache: ops::MetricsCache,
    pub started: std::time::Instant,
    /// Start time in Unix seconds.
    pub started_unix: f64,
}

impl App {
    pub fn new(config: Config, store: Store, ingest: rupta_ingest::Ingest, api: rupta_api::ApiState) -> App {
        App {
            config,
            store,
            ingest,
            api,
            http_metrics: Arc::default(),
            metrics_cache: ops::MetricsCache::default(),
            started: std::time::Instant::now(),
            started_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or_default(),
        }
    }
}

async fn serve(config: Config) -> Result<()> {
    tracing::info!(version = env!("CARGO_PKG_VERSION"), ?config, "starting rupta");
    let opts = StoreOptions {
        data_dir: config.data_dir.clone(),
        remote_url: config.storage_url.clone(),
        remote_sync: config.remote_sync,
        cache_bytes: config.cache_bytes,
    };
    let store = tokio::task::spawn_blocking(move || Store::open(&opts)).await?.context("opening the database")?;

    let trusted_proxies = rupta_ingest::parse_trusted_proxies(&config.trusted_proxies)
        .map_err(|e| anyhow::anyhow!("RUPTA_TRUSTED_PROXIES: {e}"))?;
    let ingest = rupta_ingest::Ingest::start(
        store.clone(),
        rupta_ingest::IngestConfig {
            workers: config.ingest_workers,
            queue: config.ingest_queue,
            safe: config.durability == config::Durability::Safe,
            max_envelope_bytes: config.max_envelope_bytes,
            max_decompressed_bytes: config.max_decompressed_bytes,
            public_url: config.public_url.clone(),
            trusted_proxies,
            retention_days: config.retention_days,
            metrics_project_labels: config.metrics_project_labels,
            ..rupta_ingest::IngestConfig::default()
        },
    );
    let retention = rupta_ingest::retention::spawn(store.clone(), config.retention_days);
    let api_state = rupta_api::ApiState::new(store.clone(), api_config(&config));
    if let (Some(email), Some(password)) = (&config.admin_email, &config.admin_password) {
        rupta_api::bootstrap::admin_from_env(&api_state, email, password, &config.default_org)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    let alerts = rupta_api::alerts::spawn(api_state.clone(), ingest.subscribe());
    let maintenance = rupta_api::maintenance::spawn(api_state.clone());
    let escalation = ingest.spawn_escalation();
    let app = Arc::new(App::new(config.clone(), store.clone(), ingest.clone(), api_state.clone()));

    let mcp = rupta_mcp::router(api_state.clone(), config.mcp);
    let api = rupta_api::router(api_state);
    let router = Router::new()
        .merge(ops::router(app.clone()))
        .merge(ingest.router())
        .merge(api)
        .merge(mcp)
        .fallback(web::serve)
        .layer(axum::middleware::from_fn_with_state(app.http_metrics.clone(), http_metrics::track))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));

    let listener = TcpListener::bind(config.listen).await.with_context(|| format!("binding {}", config.listen))?;
    tracing::info!(listen = %config.listen, public_url = %config.public_url, "listening");
    axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    retention.abort();
    tracing::info!("shutting down: writing queued events");
    if tokio::time::timeout(config.shutdown_timeout, ingest.shutdown()).await.is_err() {
        tracing::warn!("ingest did not drain within RUPTA_SHUTDOWN_TIMEOUT_SECS");
    }
    escalation.abort();
    alerts.abort();
    maintenance.abort();
    drop(app);
    // Closing the database uploads anything not yet in the object store.
    tokio::task::spawn_blocking(move || drop(store)).await?;
    tracing::info!("stopped");
    Ok(())
}

/// Settings of the API from the server configuration.
fn api_config(config: &Config) -> rupta_api::ApiConfig {
    use rupta_core::role::Role;
    let oidc = config.oidc.as_ref().map(|o| rupta_auth::oidc::OidcSettings {
        id: rupta_auth::oidc::INSTANCE_PROVIDER_ID.to_string(),
        name: o.name.clone(),
        issuer: o.issuer.clone(),
        client_id: o.client_id.clone(),
        client_secret: Some(o.client_secret.clone()),
        scopes: o.scopes.clone(),
        role_claim: Some(o.role_claim.clone()).filter(|c| !c.is_empty()),
        role_mapping: o.role_mapping.iter().filter_map(|(idp, r)| Some((idp.clone(), Role::parse(r)?))).collect(),
        // Validated when the configuration is read.
        default_role: rupta_auth::oidc::parse_default_role(&o.default_role).unwrap_or(Some(Role::Member)),
        jit_provisioning: o.jit_provisioning,
        allowed_audiences: o.allowed_audiences.clone(),
    });
    rupta_api::ApiConfig {
        oidc,
        local_login: config.local_login,
        default_org: config.default_org.clone(),
        smtp_url: config.smtp_url.clone(),
        smtp_from: config.smtp_from.clone(),
        camelmailer_api_key: config.camelmailer_api_key.clone(),
        camelmailer_url: config.camelmailer_url.clone(),
        cors_allowed_origins: config.cors_allowed_origins.clone(),
        trusted_proxies: config.trusted_proxies.clone(),
        sso_allow_private_issuers: config.sso_allow_private_issuers,
        integrations_allow_private_hosts: config.integrations_allow_private_hosts,
        ..rupta_api::ApiConfig::new(&config.public_url, &config.secret_key)
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
