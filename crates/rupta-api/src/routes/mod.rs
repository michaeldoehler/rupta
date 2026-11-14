//! Routers per resource.

pub mod admin;
pub mod auth;
pub mod issues;
pub mod orgs;
pub mod projects;
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
}
