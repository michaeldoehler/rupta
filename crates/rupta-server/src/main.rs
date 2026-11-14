//! `rupta`: the error-tracking server.

mod config;
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

    let ingest = rupta_ingest::Ingest::start(
        store.clone(),
        rupta_ingest::IngestConfig {
            workers: config.ingest_workers,
            queue: config.ingest_queue,
            safe: config.durability == config::Durability::Safe,
            max_envelope_bytes: config.max_envelope_bytes,
            max_decompressed_bytes: config.max_decompressed_bytes,
            public_url: config.public_url.clone(),
        },
    );
    let app = Arc::new(App { config: config.clone(), store: store.clone(), ingest: ingest.clone() });

    let api = rupta_api::router(rupta_api::ApiState { store: store.clone(), public_url: config.public_url.clone() });
    let router = Router::new()
        .merge(ops::router(app.clone()))
        .merge(ingest.router())
        .merge(api)
        .fallback(web::serve)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));

    let listener = TcpListener::bind(config.listen).await.with_context(|| format!("binding {}", config.listen))?;
    tracing::info!(listen = %config.listen, public_url = %config.public_url, "listening");
    axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutting down: writing queued events");
    if tokio::time::timeout(config.shutdown_timeout, ingest.shutdown()).await.is_err() {
        tracing::warn!("ingest did not drain within RUPTA_SHUTDOWN_TIMEOUT_SECS");
    }
    drop(app);
    // Closing the database uploads anything not yet in the object store.
    tokio::task::spawn_blocking(move || drop(store)).await?;
    tracing::info!("stopped");
    Ok(())
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
