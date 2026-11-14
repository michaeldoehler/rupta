//! `POST /api/v1/orgs/{org}/sso/test`: checks an identity provider before
//! its settings are saved. The server fetches the issuer's discovery
//! document and reports what it found and what is wrong with it.
//!
//! The fetch is a request the server makes on behalf of an owner, so it
//! is guarded against reaching internal services: the issuer's host must
//! resolve only to public addresses (no loopback, private, link-local,
//! carrier-grade NAT or unspecified addresses), the connection goes to
//! the address that was checked, redirects are not followed, the answer
//! is limited to 1 MiB and the request to 5 seconds. Issuers on internal
//! addresses are allowed when their host is the host of the instance's
//! own provider (`RUPTA_OIDC_ISSUER`) or when
//! `RUPTA_SSO_ALLOW_PRIVATE_ISSUERS=true`.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use axum::extract::State;
use rupta_auth::rbac::Permission;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Path};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(test_sso))
}

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BYTES: usize = 1 << 20;

/// The settings to test (the fields of the SSO form; others are ignored).
#[derive(Deserialize, ToSchema)]
pub struct SsoTestRequest {
    pub issuer: String,
    pub client_id: Option<String>,
    /// Omit or leave empty for a public client.
    pub client_secret: Option<String>,
}

#[derive(Serialize, ToSchema, Default, Debug)]
pub struct SsoTestResult {
    /// True when the provider can be used: no problems.
    pub ok: bool,
    /// The issuer as the provider states it.
    pub issuer: Option<String>,
    pub authorization_endpoint: Option<String>,
    pub token_endpoint: Option<String>,
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: Option<String>,
    pub scopes_supported: Vec<String>,
    /// What is wrong, in plain words; empty when `ok`.
    pub problems: Vec<String>,
}

/// True for addresses that must not be fetched by default.
pub fn is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
        }
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() {
                return is_internal(IpAddr::V4(v4));
            }
            let s = a.segments();
            a.is_loopback()
                || a.is_unspecified()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
        }
    }
}

/// The addresses to connect to, after the SSRF check.
async fn resolve(state: &ApiState, url: &url::Url) -> Result<Vec<SocketAddr>, String> {
    let instance_host = state
        .config
        .oidc
        .as_ref()
        .and_then(|o| url::Url::parse(&o.issuer).ok())
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
    resolve_guarded(
        url,
        state.config.sso_allow_private_issuers,
        instance_host.as_deref(),
        "RUPTA_SSO_ALLOW_PRIVATE_ISSUERS",
        TIMEOUT,
    )
    .await
}

/// Resolves the host of `url` and refuses it when any of its addresses is
/// internal (see [`is_internal`]), unless `allow_private` is set or the
/// host is `trusted_host`. `setting` names the variable that allows
/// internal hosts, for the error message. The caller connects to the
/// returned addresses, not to a second lookup, so that a DNS answer that
/// changes between the check and the connection cannot reach an internal
/// address.
pub async fn resolve_guarded(
    url: &url::Url,
    allow_private: bool,
    trusted_host: Option<&str>,
    setting: &str,
    timeout: Duration,
) -> Result<Vec<SocketAddr>, String> {
    let host = url.host_str().ok_or("the URL has no host")?.trim_start_matches('[').trim_end_matches(']');
    let port = url.port_or_known_default().ok_or("the URL has no port")?;
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::time::timeout(timeout, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| format!("resolving {host} timed out"))?
            .map_err(|e| format!("{host} could not be resolved: {e}"))?
            .collect(),
    };
    if addrs.is_empty() {
        return Err(format!("{host} could not be resolved"));
    }
    let trusted = allow_private || trusted_host == Some(&host.to_ascii_lowercase());
    if !trusted && addrs.iter().any(|a| is_internal(a.ip())) {
        return Err(format!(
            "{host} resolves to a loopback, private or link-local address; such hosts are only reached when {setting}=true"
        ));
    }
    Ok(addrs)
}

/// Refuses an issuer that resolves to an internal address, with the same
/// rules as the connection test. Used before an organization's SSO
/// settings are saved.
pub async fn check_issuer(state: &ApiState, issuer: &str) -> Result<(), String> {
    let url = url::Url::parse(issuer).map_err(|e| format!("not a URL: {e}"))?;
    resolve(state, &url).await.map(|_| ())
}

async fn fetch(state: &ApiState, issuer: &str) -> Result<Value, String> {
    let url = url::Url::parse(&format!("{issuer}/.well-known/openid-configuration")).map_err(|e| e.to_string())?;
    let addrs = resolve(state, &url).await?;
    let host = url.host_str().unwrap_or_default().to_string();
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .user_agent(concat!("rupta/", env!("CARGO_PKG_VERSION")));
    if host.parse::<IpAddr>().is_err() {
        // Connect to the addresses that were checked, not to a second lookup.
        builder = builder.resolve_to_addrs(&host, &addrs);
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let mut resp = client.get(url.clone()).header("accept", "application/json").send().await.map_err(|e| {
        if e.is_timeout() {
            "the request timed out after 5 seconds".to_string()
        } else {
            format!("the request failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(format!("{url} answered {}", resp.status()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("reading the answer failed: {e}"))? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            return Err("the discovery document is larger than 1 MiB".into());
        }
    }
    serde_json::from_slice(&body).map_err(|e| format!("the discovery document is not JSON: {e}"))
}

fn strings(v: &Value, key: &str) -> Option<Vec<String>> {
    v.get(key)?.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

/// Checks a discovery document against the requested settings.
pub fn validate(issuer: &str, client_secret: Option<&str>, doc: &Value) -> SsoTestResult {
    let s = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_string);
    let mut r = SsoTestResult {
        issuer: s("issuer"),
        authorization_endpoint: s("authorization_endpoint"),
        token_endpoint: s("token_endpoint"),
        userinfo_endpoint: s("userinfo_endpoint"),
        jwks_uri: s("jwks_uri"),
        scopes_supported: strings(doc, "scopes_supported").unwrap_or_default(),
        ..SsoTestResult::default()
    };
    let p = &mut r.problems;
    match &r.issuer {
        None => p.push("the document has no issuer".into()),
        Some(i) if i.trim_end_matches('/') != issuer => {
            p.push(format!("the document's issuer {i:?} differs from the configured issuer {issuer:?}"))
        }
        _ => {}
    }
    for (name, v) in [
        ("authorization_endpoint", &r.authorization_endpoint),
        ("token_endpoint", &r.token_endpoint),
        ("jwks_uri", &r.jwks_uri),
    ] {
        match v {
            None => p.push(format!("{name} is missing")),
            Some(u) if !(u.starts_with("https://") || u.starts_with("http://")) || url::Url::parse(u).is_err() => {
                p.push(format!("{name} is not an http(s) URL"))
            }
            _ => {}
        }
    }
    if !strings(doc, "response_types_supported").unwrap_or_default().iter().any(|t| t == "code") {
        p.push("the provider does not support the authorization code flow (response type code)".into());
    }
    match strings(doc, "code_challenge_methods_supported") {
        Some(m) if m.iter().any(|m| m == "S256") => {}
        Some(_) => p.push("the provider does not support PKCE with S256".into()),
        None => {
            p.push("the provider does not announce PKCE support (code_challenge_methods_supported with S256)".into())
        }
    }
    if !r.scopes_supported.is_empty() && !r.scopes_supported.iter().any(|s| s == "openid") {
        p.push("the provider does not list the openid scope".into());
    }
    // Without a list, client_secret_basic is the default (OpenID Connect Discovery).
    let auth_methods =
        strings(doc, "token_endpoint_auth_methods_supported").unwrap_or_else(|| vec!["client_secret_basic".into()]);
    let public = client_secret.is_none_or(str::is_empty);
    if public && !auth_methods.iter().any(|m| m == "none") {
        p.push(
            "without a client secret the provider must allow public clients (token endpoint auth method none)".into(),
        );
    }
    if !public && !auth_methods.iter().any(|m| m == "client_secret_basic" || m == "client_secret_post") {
        p.push("the provider accepts neither client_secret_basic nor client_secret_post".into());
    }
    r.ok = r.problems.is_empty();
    r
}

/// Tests the connection to an identity provider: fetches and checks its
/// discovery document. Nothing is saved. Owners only.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/sso/test", tag = "orgs", params(("org" = String, Path)),
    request_body = SsoTestRequest, responses((status = 200, body = SsoTestResult), (status = 422, body = crate::error::ErrorBody)))]
async fn test_sso(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<SsoTestRequest>,
) -> ApiResult<axum::Json<SsoTestResult>> {
    // Authorize before fetching anything from the given issuer.
    state.org(&auth, &org, Permission::ManageSso).await?;
    let issuer = req.issuer.trim().trim_end_matches('/').to_string();
    let parsed = url::Url::parse(&issuer).map_err(|_| ApiError::invalid("issuer", "must be an http(s) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ApiError::invalid("issuer", "must be an http(s) URL"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ApiError::invalid("issuer", "must not contain credentials"));
    }
    let mut result = match fetch(&state, &issuer).await {
        Ok(doc) => validate(&issuer, req.client_secret.as_deref(), &doc),
        Err(e) => SsoTestResult { problems: vec![e], ..SsoTestResult::default() },
    };
    if req.client_id.as_deref().is_some_and(|c| c.trim().is_empty()) {
        result.problems.push("the client id is empty".into());
        result.ok = false;
    }
    Ok(axum::Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn internal_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_internal(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "203.0.113.5", "2001:db8::1", "2606:4700::1"] {
            assert!(!is_internal(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn validation_reports_problems() {
        let good = json!({"issuer": "https://id.example.com", "authorization_endpoint": "https://id.example.com/auth",
            "token_endpoint": "https://id.example.com/token", "jwks_uri": "https://id.example.com/jwks",
            "response_types_supported": ["code", "id_token"], "code_challenge_methods_supported": ["plain", "S256"],
            "scopes_supported": ["openid", "email"]});
        let r = validate("https://id.example.com", Some("secret"), &good);
        assert!(r.ok, "{:?}", r.problems);
        let r = validate("https://other.example.com", None, &good);
        assert!(!r.ok);
        assert_eq!(r.problems.len(), 2, "{:?}", r.problems); // issuer, public client
        let r = validate("https://id.example.com", Some("s"), &json!({"issuer": "https://id.example.com"}));
        assert_eq!(r.problems.len(), 5, "{:?}", r.problems);
    }
}
