//! Routers per resource.

pub mod admin;
pub mod alerts;
pub mod auth;
pub mod events;
pub mod issues;
pub mod logs;
pub mod metrics;
pub mod monitors;
pub mod orgs;
pub mod performance;
pub mod profiles;
pub mod projects;
pub mod releases;
pub mod replays;
pub mod tokens;

use utoipa_axum::router::OpenApiRouter;

use crate::state::ApiState;

/// Every `/api/v1` and `/auth` route.
pub fn all() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .merge(auth::routes())
        .merge(orgs::routes())
        .merge(tokens::routes())
        .merge(admin::routes())
        .merge(projects::routes())
        .merge(issues::routes())
        .merge(events::routes())
        .merge(releases::routes())
        .merge(alerts::routes())
        .merge(monitors::routes())
        .merge(performance::routes())
        .merge(logs::routes())
        .merge(metrics::routes())
        .merge(profiles::routes())
        .merge(replays::routes())
}
