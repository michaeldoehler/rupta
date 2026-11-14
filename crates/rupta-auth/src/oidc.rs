//! Single sign-on with OpenID Connect: authorization code flow with PKCE,
//! discovery, nonce and ID token validation.
//!
//! The flow state (CSRF state, nonce, PKCE verifier, where to go next) is
//! kept in a short-lived cookie signed with a key derived from
//! `RUPTA_SECRET_KEY`, so the server stores nothing between the redirect
//! to the provider and the callback.
//!
//! Roles come from a configurable claim. Zitadel's project roles claim
//! (`urn:zitadel:iam:org:project:roles`) is an object whose keys are the
//! role names; arrays of strings and single strings work as well. Role
//! names are mapped to rupta roles; the highest mapped role wins, and
//! users without a mapped role get the default role.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use rupta_core::role::Role;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::crypto::Keys;

/// Id of the provider configured by environment variables.
pub const INSTANCE_PROVIDER_ID: &str = "oidc";
/// Cookie holding the signed flow state.
pub const STATE_COOKIE: &str = "rupta_sso_state";
/// How long a sign-in may take at the provider.
pub const STATE_TTL_SECS: i64 = 600;

/// A configured provider.
#[derive(Clone)]
pub struct OidcSettings {
    /// `oidc` for the instance provider, `org-{slug}` for an organization's.
    pub id: String,
    /// Shown on the login button.
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    /// `None` for public clients (PKCE only).
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
    /// Claim carrying role names; `None` = no role mapping.
    pub role_claim: Option<String>,
    /// Provider role name → rupta role.
    pub role_mapping: Vec<(String, Role)>,
    pub default_role: Role,
    /// Create users on first sign-in.
    pub jit_provisioning: bool,
}

impl std::fmt::Debug for OidcSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcSettings")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "<redacted>"))
            .field("scopes", &self.scopes)
            .field("role_claim", &self.role_claim)
            .field("role_mapping", &self.role_mapping)
            .field("default_role", &self.default_role)
            .field("jit_provisioning", &self.jit_provisioning)
            .finish()
    }
}

/// Errors of the sign-in flow. Messages are safe to log; users see a
/// generic message.
#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("invalid provider configuration: {0}")]
    Config(String),
    #[error("discovery failed: {0}")]
    Discovery(String),
    #[error("the sign-in state is missing, expired or does not match")]
    State,
    #[error("the provider returned an error: {0}")]
    Provider(String),
    #[error("token exchange failed: {0}")]
    Token(String),
    #[error("the ID token is invalid: {0}")]
    IdToken(String),
    #[error("user info request failed: {0}")]
    UserInfo(String),
}

/// HTTP errors of the client used for the provider.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Http(#[from] http::Error),
    #[error("response larger than 1 MiB")]
    TooLarge,
}

/// Talks to OpenID providers; caches discovery documents for an hour.
#[derive(Clone)]
pub struct OidcClient {
    http: reqwest::Client,
    cache: Arc<Mutex<HashMap<String, (Instant, CoreProviderMetadata)>>>,
}

impl Default for OidcClient {
    fn default() -> OidcClient {
        OidcClient::new()
    }
}

const DISCOVERY_TTL: Duration = Duration::from_secs(3600);

async fn send(
    client: reqwest::Client,
    req: openidconnect::HttpRequest,
) -> Result<openidconnect::HttpResponse, HttpError> {
    let (parts, body) = req.into_parts();
    let mut resp = client.request(parts.method, parts.uri.to_string()).headers(parts.headers).body(body).send().await?;
    let mut out = http::Response::builder().status(resp.status());
    for (k, v) in resp.headers() {
        out = out.header(k, v);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > 1 << 20 {
            return Err(HttpError::TooLarge);
        }
    }
    Ok(out.body(body)?)
}

/// The signed flow state.
#[derive(Debug, Serialize, Deserialize)]
struct FlowState {
    /// Provider id.
    p: String,
    /// CSRF state.
    s: String,
    /// Nonce.
    n: String,
    /// PKCE verifier.
    v: String,
    /// Path to go to afterwards.
    next: String,
    /// Expiry (Unix seconds).
    exp: i64,
}

/// What [`OidcClient::start`] returns.
#[derive(Debug, Clone)]
pub struct Started {
    /// Where to redirect the browser.
    pub authorize_url: String,
    /// Value for the state cookie.
    pub state_cookie: String,
}

/// The verified identity from the provider.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
    /// Role names from the role claim (ID token, else user info).
    pub roles: Vec<String>,
    /// Where to go after signing in.
    pub next: String,
}

/// Accepts only local paths for the post-login redirect.
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') && n.len() < 2048 => n.to_string(),
        _ => "/".to_string(),
    }
}

impl OidcClient {
    pub fn new() -> OidcClient {
        let http = reqwest::Client::builder()
            // Following redirects would open the client to SSRF.
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP client");
        OidcClient { http, cache: Arc::default() }
    }

    fn http_fn(
        &self,
    ) -> impl Fn(
        openidconnect::HttpRequest,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<openidconnect::HttpResponse, HttpError>> + Send>>
    + use<> {
        let client = self.http.clone();
        move |req| Box::pin(send(client.clone(), req))
    }

    async fn metadata(&self, issuer: &str) -> Result<CoreProviderMetadata, OidcError> {
        if let Some((at, m)) = self.cache.lock().unwrap().get(issuer)
            && at.elapsed() < DISCOVERY_TTL
        {
            return Ok(m.clone());
        }
        let url = IssuerUrl::new(issuer.to_string()).map_err(|e| OidcError::Config(e.to_string()))?;
        let http = self.http_fn();
        let m = CoreProviderMetadata::discover_async(url, &http)
            .await
            .map_err(|e| OidcError::Discovery(error_chain(&e)))?;
        self.cache.lock().unwrap().insert(issuer.to_string(), (Instant::now(), m.clone()));
        Ok(m)
    }

    /// Forgets cached discovery documents (after a configuration change).
    pub fn forget(&self, issuer: &str) {
        self.cache.lock().unwrap().remove(issuer);
    }

    async fn client(
        &self,
        settings: &OidcSettings,
        redirect_uri: &str,
    ) -> Result<
        openidconnect::Client<
            openidconnect::EmptyAdditionalClaims,
            openidconnect::core::CoreAuthDisplay,
            openidconnect::core::CoreGenderClaim,
            openidconnect::core::CoreJweContentEncryptionAlgorithm,
            openidconnect::core::CoreJsonWebKey,
            openidconnect::core::CoreAuthPrompt,
            openidconnect::StandardErrorResponse<openidconnect::core::CoreErrorResponseType>,
            openidconnect::core::CoreTokenResponse,
            openidconnect::core::CoreTokenIntrospectionResponse,
            openidconnect::core::CoreRevocableToken,
            openidconnect::core::CoreRevocationErrorResponse,
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointMaybeSet,
            openidconnect::EndpointMaybeSet,
        >,
        OidcError,
    > {
        let metadata = self.metadata(&settings.issuer).await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(settings.client_id.clone()),
            settings.client_secret.clone().filter(|s| !s.is_empty()).map(ClientSecret::new),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(|e| OidcError::Config(e.to_string()))?);
        Ok(client)
    }

    /// Begins a sign-in: the URL to send the browser to and the signed state.
    pub async fn start(
        &self,
        keys: &Keys,
        settings: &OidcSettings,
        redirect_uri: &str,
        next: Option<&str>,
    ) -> Result<Started, OidcError> {
        let client = self.client(settings, redirect_uri).await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut req =
            client.authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random);
        for s in settings.scopes.iter().filter(|s| s.as_str() != "openid") {
            req = req.add_scope(Scope::new(s.clone()));
        }
        let (url, csrf, nonce) = req.set_pkce_challenge(challenge).url();
        let state = FlowState {
            p: settings.id.clone(),
            s: csrf.secret().clone(),
            n: nonce.secret().clone(),
            v: verifier.secret().clone(),
            next: safe_next(next),
            exp: Utc::now().timestamp() + STATE_TTL_SECS,
        };
        let state_cookie = keys.sign(&serde_json::to_vec(&state).expect("state serializes"));
        Ok(Started { authorize_url: url.to_string(), state_cookie })
    }

    /// Completes a sign-in from the callback's query parameters and the
    /// state cookie. Validates state, exchanges the code (with the PKCE
    /// verifier), validates the ID token (signature, issuer, audience,
    /// expiry, nonce) and reads the role claim.
    pub async fn finish(
        &self,
        keys: &Keys,
        settings: &OidcSettings,
        redirect_uri: &str,
        query: &HashMap<String, String>,
        state_cookie: Option<&str>,
    ) -> Result<Identity, OidcError> {
        let state: FlowState = state_cookie
            .and_then(|c| keys.verify(c))
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .ok_or(OidcError::State)?;
        if state.exp < Utc::now().timestamp() || state.p != settings.id {
            return Err(OidcError::State);
        }
        let given = query.get("state").ok_or(OidcError::State)?;
        if !crate::crypto::ct_eq(given.as_bytes(), state.s.as_bytes()) {
            return Err(OidcError::State);
        }
        if let Some(e) = query.get("error") {
            let desc = query.get("error_description").map(String::as_str).unwrap_or("");
            return Err(OidcError::Provider(format!("{e} {desc}").trim().to_string()));
        }
        let code = query.get("code").ok_or_else(|| OidcError::Provider("no code in the callback".into()))?;

        let client = self.client(settings, redirect_uri).await?;
        let http = self.http_fn();
        let token = client
            .exchange_code(AuthorizationCode::new(code.clone()))
            .map_err(|e| OidcError::Config(e.to_string()))?
            .set_pkce_verifier(PkceCodeVerifier::new(state.v.clone()))
            .request_async(&http)
            .await
            .map_err(|e| OidcError::Token(error_chain(&e)))?;
        let id_token =
            token.id_token().ok_or_else(|| OidcError::IdToken("the provider returned no ID token".into()))?;
        let verifier = client.id_token_verifier();
        let claims = id_token
            .claims(&verifier, &Nonce::new(state.n.clone()))
            .map_err(|e| OidcError::IdToken(error_chain(&e)))?;
        // The signature is verified; read every claim from the payload.
        let raw = serde_json::to_value(id_token).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let payload: Value = raw
            .split('.')
            .nth(1)
            .and_then(crate::crypto::unb64)
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);

        let subject = claims.subject().as_str().to_string();
        let mut email = claims.email().map(|e| e.as_str().to_string());
        let mut email_verified = claims.email_verified().unwrap_or(false);
        let mut name = claims.name().and_then(|n| n.get(None)).map(|n| n.as_str().to_string());
        let mut picture = claims.picture().and_then(|n| n.get(None)).map(|n| n.as_str().to_string());
        let mut roles = settings.role_claim.as_deref().map(|c| roles_from_claim(payload.get(c))).unwrap_or_default();

        // Providers often put profile and role claims only into user info.
        let need_userinfo = email.is_none()
            || (settings.role_claim.is_some() && payload.get(settings.role_claim.as_deref().unwrap()).is_none());
        if need_userinfo && let Some(url) = client.user_info_url() {
            let info = self.userinfo(url.as_str(), token.access_token().secret()).await?;
            if info.get("sub").and_then(Value::as_str) != Some(subject.as_str()) {
                return Err(OidcError::UserInfo("the user info subject does not match the ID token".into()));
            }
            if email.is_none() {
                email = info.get("email").and_then(Value::as_str).map(str::to_string);
                email_verified = info.get("email_verified").and_then(Value::as_bool).unwrap_or(false);
            }
            if name.is_none() {
                name = info.get("name").and_then(Value::as_str).map(str::to_string);
            }
            if picture.is_none() {
                picture = info.get("picture").and_then(Value::as_str).map(str::to_string);
            }
            if roles.is_empty()
                && let Some(c) = settings.role_claim.as_deref()
            {
                roles = roles_from_claim(info.get(c));
            }
        }
        Ok(Identity { subject, email, email_verified, name, picture, roles, next: state.next })
    }

    async fn userinfo(&self, url: &str, access_token: &str) -> Result<Value, OidcError> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(access_token)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| OidcError::UserInfo(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(OidcError::UserInfo(format!("status {}", resp.status())));
        }
        resp.json().await.map_err(|e| OidcError::UserInfo(e.to_string()))
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cur = c.source();
    }
    s
}

/// Role names in a claim value: the keys of an object (Zitadel), the
/// strings of an array, or a single string.
pub fn roles_from_claim(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Object(m)) => m.keys().cloned().collect(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(Value::String(s)) => s.split([' ', ',']).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// The highest mapped role, if any role name is mapped.
pub fn map_roles(roles: &[String], mapping: &[(String, Role)]) -> Option<Role> {
    roles.iter().filter_map(|r| mapping.iter().find(|(name, _)| name == r).map(|(_, role)| *role)).max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zitadel_role_claims_map_to_the_highest_role() {
        let claim =
            json!({"rupta-viewer": {"123": "acme.zitadel.cloud"}, "rupta-admin": {"123": "acme.zitadel.cloud"}});
        let roles = roles_from_claim(Some(&claim));
        let mapping = vec![("rupta-admin".to_string(), Role::Admin), ("rupta-viewer".to_string(), Role::Viewer)];
        assert_eq!(map_roles(&roles, &mapping), Some(Role::Admin));
        assert_eq!(map_roles(&["other".into()], &mapping), None);
        assert_eq!(roles_from_claim(Some(&json!(["a", "b"]))), vec!["a", "b"]);
        assert_eq!(roles_from_claim(Some(&json!("a b"))), vec!["a", "b"]);
        assert!(roles_from_claim(None).is_empty());
    }

    #[test]
    fn only_local_paths_are_followed() {
        assert_eq!(safe_next(Some("/organizations/acme/issues?x=1")), "/organizations/acme/issues?x=1");
        for bad in ["https://evil.example", "//evil.example", "/\\evil", "evil"] {
            assert_eq!(safe_next(Some(bad)), "/", "{bad}");
        }
        assert_eq!(safe_next(None), "/");
    }
}
