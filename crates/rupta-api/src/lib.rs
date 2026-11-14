//! The REST API: `/api/v1` for the web app and automation, and the subset
//! of `/api/0` that the source map upload CLI uses.

use axum::Router;
use rupta_store::Store;

/// Everything the API handlers need.
#[derive(Clone)]
pub struct ApiState {
    pub store: Store,
    pub public_url: String,
}

/// Routes under `/api/v1`, `/api/0` and `/auth`.
pub fn router(_state: ApiState) -> Router {
    Router::new()
}
