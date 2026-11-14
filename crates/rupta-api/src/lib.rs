//! The REST API: `/api/v1` for the web app and automation, the subset of
//! `/api/0` that the source map upload CLI uses, the single sign-on
//! redirects under `/auth`, and the alert engine.

pub mod access;
pub mod alerts;
pub mod bootstrap;
pub mod compat;
pub mod error;
pub mod extract;
pub mod integrations;
pub mod mail;
pub mod maintenance;
pub mod notify;
pub mod preferences;
pub mod routes;
pub mod sql;
pub mod state;
pub mod telemetry;
pub mod workflow;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

pub use state::{ApiConfig, ApiMetrics, ApiState};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "rupta API",
        description = "Management and query API of rupta. Authenticate with the `rupta_session` cookie plus the `X-CSRF-Token` header on mutating requests, or with `Authorization: Bearer rpt_...`. See docs/api.md.",
        license(name = "MIT")
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Login, sessions and single sign-on"),
        (name = "orgs", description = "Organizations, members, teams, tokens, audit log, SSO"),
        (name = "user", description = "The signed-in user"),
        (name = "projects", description = "Projects, keys, settings and statistics"),
        (name = "issues", description = "Issues, events, tags, activity"),
        (name = "releases", description = "Releases and release health"),
        (name = "alerts", description = "Alert rules and notification channels"),
        (name = "integrations", description = "Issue tracker integrations, project defaults and tracker webhooks"),
        (name = "annotations", description = "Releases, deploys and new-issue spikes as time annotations (Grafana)"),
        (name = "monitors", description = "Cron monitors"),
        (name = "performance", description = "Transactions, traces and spans"),
        (name = "logs", description = "Structured logs and the live tail"),
        (name = "metrics", description = "Metrics and dashboards"),
        (name = "profiling", description = "Profiles and flamegraphs"),
        (name = "replays", description = "Session replays"),
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "token",
            SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).bearer_format("rpt_...").build()),
        );
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(rupta_auth::session::SESSION_COOKIE))),
        );
        openapi.security = Some(vec![
            utoipa::openapi::security::SecurityRequirement::new("token", Vec::<String>::new()),
            utoipa::openapi::security::SecurityRequirement::new("session", Vec::<String>::new()),
        ]);
    }
}

fn api_routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).merge(routes::all())
}

/// The OpenAPI 3.1 document (served at `/api/v1/openapi.json`).
pub fn openapi() -> utoipa::openapi::OpenApi {
    let (_, api) = api_routes().split_for_parts();
    api
}

/// The OpenAPI document as committed in `docs/api/openapi.json`.
pub fn openapi_json() -> String {
    let mut s = openapi().to_pretty_json().expect("OpenAPI serializes");
    s.push('\n');
    s
}

/// Routes under `/api/v1`, `/api/0` and `/auth`.
pub fn router(state: ApiState) -> Router {
    let (v1, _) = api_routes().split_for_parts();
    let doc = openapi_json();
    let v1 = v1
        .route(
            "/api/v1/openapi.json",
            axum::routing::get(move || {
                let doc = doc.clone();
                async move { ([(header::CONTENT_TYPE, "application/json")], doc) }
            }),
        )
        .route("/api/v1/{*rest}", axum::routing::any(|| async { error::ApiError::not_found() }))
        .layer(DefaultBodyLimit::max(state.config.max_body_bytes));

    let mut api = Router::new().merge(v1).merge(compat::router()).with_state(state.clone());
    api = security_headers(api);
    if !state.config.cors_allowed_origins.is_empty() {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_allowed_origins
            .iter()
            .filter_map(|o| HeaderValue::from_str(o.trim_end_matches('/')).ok())
            .collect();
        api = api.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_credentials(true)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::AUTHORIZATION,
                    HeaderName::from_static(rupta_auth::session::CSRF_HEADER),
                ])
                .max_age(std::time::Duration::from_secs(600)),
        );
    }
    api
}

fn security_headers(r: Router) -> Router {
    let set = |name: &'static str, value: &'static str| {
        SetResponseHeaderLayer::if_not_present(HeaderName::from_static(name), HeaderValue::from_static(value))
    };
    r.layer(set("x-content-type-options", "nosniff"))
        .layer(set("x-frame-options", "DENY"))
        .layer(set("referrer-policy", "no-referrer"))
        .layer(set("cache-control", "no-store"))
        .layer(set("content-security-policy", "default-src 'none'; frame-ancestors 'none'"))
        .layer(set("cross-origin-resource-policy", "same-origin"))
}
