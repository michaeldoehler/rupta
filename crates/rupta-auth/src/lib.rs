//! Authentication and authorization: local accounts, sessions, OpenID
//! Connect single sign-on, API tokens, organization roles and the audit log.
//!
//! This crate holds the mechanisms; the HTTP endpoints that use them live
//! in `rupta-api`.

pub mod apitoken;
pub mod audit;
pub mod crypto;
pub mod oidc;
pub mod password;
pub mod provision;
pub mod ratelimit;
pub mod rbac;
pub mod session;
pub mod users;

pub use rbac::{Denied, Permission, Principal, TokenGrant, authorize};
