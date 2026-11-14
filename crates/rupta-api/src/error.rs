//! JSON errors: `{"error": {"code": "...", "message": "...", "details": {}}}`.

use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rupta_store::StoreError;
use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

/// An error answer.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Value,
    pub headers: Vec<(HeaderName, HeaderValue)>,
}

/// The error body, for the OpenAPI document.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Machine-readable code: `not_found`, `unauthorized`, `forbidden`,
    /// `csrf_failed`, `validation_failed`, `bad_request`, `conflict`,
    /// `rate_limited`, `payload_too_large`, `sso_required`,
    /// `unsupported`, `internal`.
    pub code: String,
    pub message: String,
    #[schema(value_type = Object)]
    pub details: Value,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, code, message: message.into(), details: json!({}), headers: Vec::new() }
    }

    pub fn with_details(mut self, details: Value) -> ApiError {
        self.details = details;
        self
    }

    pub fn not_found() -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", "not found")
    }

    pub fn unauthorized() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
    }

    pub fn forbidden() -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, "forbidden", "you do not have permission to do this")
    }

    pub fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    /// A field failed validation.
    pub fn invalid(field: &str, message: impl Into<String>) -> ApiError {
        let message = message.into();
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed", format!("{field}: {message}"))
            .with_details(json!({ "field": field, "reason": message }))
    }

    pub fn conflict(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn rate_limited(retry_after_secs: u64) -> ApiError {
        let mut e = ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "too many attempts; try again later")
            .with_details(json!({ "retry_after": retry_after_secs }));
        e.headers.push((header::RETRY_AFTER, HeaderValue::from(retry_after_secs)));
        e
    }

    pub fn unsupported(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_IMPLEMENTED, "unsupported", message)
    }

    pub fn internal(message: impl std::fmt::Display) -> ApiError {
        tracing::error!(error = %message, "internal error");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error")
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> ApiError {
        if e.is_unique_violation() {
            return ApiError::conflict("this already exists");
        }
        // Serialization failures and deadlocks: the client may retry.
        if matches!(e.sqlstate(), Some("40001" | "40P01")) {
            return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "conflict", "concurrent change; please retry");
        }
        ApiError::internal(e)
    }
}

impl From<rupta_store::squad::Error> for ApiError {
    fn from(e: rupta_store::squad::Error) -> ApiError {
        ApiError::from(StoreError::Db(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": { "code": self.code, "message": self.message, "details": self.details } });
        let mut resp = (self.status, axum::Json(body)).into_response();
        for (k, v) in self.headers {
            resp.headers_mut().insert(k, v);
        }
        resp
    }
}
