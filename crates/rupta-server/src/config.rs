//! Configuration from environment variables.
//!
//! Every setting is a `RUPTA_*` variable. Secrets can also be given as
//! `<NAME>_FILE` pointing to a file (Docker and Kubernetes secrets); the
//! file wins over the plain variable, and trailing whitespace is removed.
//! The full reference is `docs/configuration.md`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};

/// How the environment is read; a map in tests.
pub trait Env {
    fn get(&self, key: &str) -> Option<String>;
}

/// The process environment.
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

impl Env for HashMap<String, String> {
    fn get(&self, key: &str) -> Option<String> {
        HashMap::get(self, key).cloned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Acknowledge an event once it is queued (default).
    Fast,
    /// Acknowledge an event once it is committed.
    Safe,
}

/// Single sign-on through an OpenID Connect provider configured by
/// environment (instance-wide). Organizations can add their own provider
/// in the settings as well.
#[derive(Clone)]
pub struct OidcConfig {
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Vec<String>,
    pub role_claim: String,
    /// IdP role name → rupta role name.
    pub role_mapping: Vec<(String, String)>,
    pub default_role: String,
    pub jit_provisioning: bool,
}

impl std::fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcConfig")
            .field("name", &self.name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("scopes", &self.scopes)
            .field("role_claim", &self.role_claim)
            .field("role_mapping", &self.role_mapping)
            .field("default_role", &self.default_role)
            .field("jit_provisioning", &self.jit_provisioning)
            .finish()
    }
}

#[derive(Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub storage_url: Option<String>,
    pub remote_sync: bool,
    pub cache_bytes: usize,
    pub secret_key: Vec<u8>,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub default_org: String,
    pub local_login: bool,
    pub oidc: Option<OidcConfig>,
    pub smtp_url: Option<String>,
    /// `RUPTA_MAIL_FROM`, or `RUPTA_SMTP_FROM` as before.
    pub smtp_from: Option<String>,
    pub camelmailer_api_key: Option<String>,
    pub camelmailer_url: Option<String>,
    pub ingest_workers: usize,
    pub ingest_queue: usize,
    pub durability: Durability,
    pub max_envelope_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub retention_days: u32,
    pub metrics_token: Option<String>,
    pub log: String,
    pub log_format: LogFormat,
    pub trusted_proxies: Vec<String>,
    pub cors_allowed_origins: Vec<String>,
    pub shutdown_timeout: Duration,
    /// `RUPTA_SSO_ALLOW_PRIVATE_ISSUERS`: the SSO connection test may fetch
    /// issuers on loopback, link-local and private addresses.
    pub sso_allow_private_issuers: bool,
    /// `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS`: issue trackers and paging
    /// services may be reached on loopback, link-local and private addresses.
    pub integrations_allow_private_hosts: bool,
    /// `RUPTA_METRICS_PROJECT_LABELS`: projects with their own label in the
    /// per-project metrics; the rest are counted as `project="other"`.
    pub metrics_project_labels: usize,
    /// `RUPTA_MCP`: serve the Model Context Protocol endpoint `/mcp`.
    pub mcp: bool,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("listen", &self.listen)
            .field("public_url", &self.public_url)
            .field("data_dir", &self.data_dir)
            .field("storage_url", &self.storage_url.as_ref().map(|u| redact_url(u)))
            .field("remote_sync", &self.remote_sync)
            .field("admin_email", &self.admin_email)
            .field("local_login", &self.local_login)
            .field("oidc", &self.oidc)
            .field("smtp_url", &self.smtp_url.as_ref().map(|u| redact_url(u)))
            .field("camelmailer_api_key", &self.camelmailer_api_key.as_ref().map(|_| "<set>"))
            .field("camelmailer_url", &self.camelmailer_url)
            .field("ingest_workers", &self.ingest_workers)
            .field("ingest_queue", &self.ingest_queue)
            .field("durability", &self.durability)
            .field("retention_days", &self.retention_days)
            .field("mcp", &self.mcp)
            .finish_non_exhaustive()
    }
}

/// Removes credentials from a URL for logging.
pub fn redact_url(url: &str) -> String {
    let mut out = url.to_string();
    if let Some(at) = out.find('@')
        && let Some(scheme) = out.find("://")
        && scheme + 3 < at
    {
        out.replace_range(scheme + 3..at, "***");
    }
    for key in ["secret_key=", "access_key=", "password="] {
        if let Some(i) = out.find(key) {
            let start = i + key.len();
            let end = out[start..].find('&').map_or(out.len(), |e| start + e);
            out.replace_range(start..end, "***");
        }
    }
    out
}

struct Reader<'a, E: Env> {
    env: &'a E,
}

impl<E: Env> Reader<'_, E> {
    fn raw(&self, key: &str) -> Option<String> {
        self.env.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    }

    /// A secret: `<KEY>_FILE` wins over `<KEY>`.
    fn secret(&self, key: &str) -> Result<Option<String>> {
        if let Some(path) = self.raw(&format!("{key}_FILE")) {
            let value = std::fs::read_to_string(&path).with_context(|| format!("{key}_FILE: cannot read {path}"))?;
            return Ok(Some(value.trim_end().to_string()).filter(|v| !v.is_empty()));
        }
        Ok(self.raw(key))
    }

    fn string(&self, key: &str, default: &str) -> String {
        self.raw(key).unwrap_or_else(|| default.to_string())
    }

    fn bool(&self, key: &str, default: bool) -> Result<bool> {
        match self.raw(key) {
            None => Ok(default),
            Some(v) => match v.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "0" | "false" | "no" | "off" => Ok(false),
                _ => bail!("{key}: expected true or false, got {v:?}"),
            },
        }
    }

    fn number<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T> {
        match self.raw(key) {
            None => Ok(default),
            Some(v) => v.replace('_', "").parse().map_err(|_| anyhow::anyhow!("{key}: expected a number, got {v:?}")),
        }
    }

    fn list(&self, key: &str) -> Vec<String> {
        self.raw(key)
            .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    }
}

impl Config {
    pub fn from_env(env: &impl Env) -> Result<Config> {
        let r = Reader { env };
        let listen: SocketAddr = r.string("RUPTA_LISTEN", "0.0.0.0:8080").parse().context("RUPTA_LISTEN")?;
        let public_url = r.string("RUPTA_PUBLIC_URL", &format!("http://localhost:{}", listen.port()));
        if !(public_url.starts_with("http://") || public_url.starts_with("https://")) {
            bail!("RUPTA_PUBLIC_URL must start with http:// or https://, got {public_url:?}");
        }
        let public_url = public_url.trim_end_matches('/').to_string();

        let secret_key = match r.secret("RUPTA_SECRET_KEY")? {
            Some(k) if k.len() >= 32 => k.into_bytes(),
            Some(_) => bail!("RUPTA_SECRET_KEY must be at least 32 characters (try: openssl rand -hex 32)"),
            None => bail!("RUPTA_SECRET_KEY is required (at least 32 characters; try: openssl rand -hex 32)"),
        };

        let mut storage_url = r.raw("RUPTA_STORAGE_URL");
        if let Some(url) = &mut storage_url {
            // Credentials from separate (secret) variables are added to the URL.
            let sep = if url.contains('?') { '&' } else { '?' };
            let mut extra = Vec::new();
            if let Some(k) = r.secret("RUPTA_S3_ACCESS_KEY_ID")? {
                extra.push(format!("access_key={}", urlencode(&k)));
            }
            if let Some(s) = r.secret("RUPTA_S3_SECRET_ACCESS_KEY")? {
                extra.push(format!("secret_key={}", urlencode(&s)));
            }
            if !extra.is_empty() {
                url.push(sep);
                url.push_str(&extra.join("&"));
            }
        }
        let remote_sync = match r.string("RUPTA_REMOTE_SYNC", "sync").as_str() {
            "sync" => true,
            "async" => false,
            other => bail!("RUPTA_REMOTE_SYNC must be sync or async, got {other:?}"),
        };

        let oidc = match r.raw("RUPTA_OIDC_ISSUER") {
            None => None,
            Some(issuer) => {
                let client_id =
                    r.raw("RUPTA_OIDC_CLIENT_ID").context("RUPTA_OIDC_CLIENT_ID is required with RUPTA_OIDC_ISSUER")?;
                let client_secret = r
                    .secret("RUPTA_OIDC_CLIENT_SECRET")?
                    .context("RUPTA_OIDC_CLIENT_SECRET is required with RUPTA_OIDC_ISSUER")?;
                let role_mapping = r
                    .list("RUPTA_OIDC_ROLE_MAPPING")
                    .into_iter()
                    .map(|pair| {
                        let (idp, role) = pair.split_once(':').with_context(|| {
                            format!("RUPTA_OIDC_ROLE_MAPPING: expected idp-role:rupta-role, got {pair:?}")
                        })?;
                        if rupta_core::role::Role::parse(role).is_none() {
                            bail!("RUPTA_OIDC_ROLE_MAPPING: unknown role {role:?} (owner, admin, member, viewer)");
                        }
                        Ok((idp.trim().to_string(), role.trim().to_ascii_lowercase()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let default_role = r.string("RUPTA_OIDC_DEFAULT_ROLE", "member").to_ascii_lowercase();
                if rupta_core::role::Role::parse(&default_role).is_none() {
                    bail!("RUPTA_OIDC_DEFAULT_ROLE: unknown role {default_role:?}");
                }
                Some(OidcConfig {
                    name: r.string("RUPTA_OIDC_NAME", "Single sign-on"),
                    issuer: issuer.trim_end_matches('/').to_string(),
                    client_id,
                    client_secret,
                    scopes: r
                        .string("RUPTA_OIDC_SCOPES", "openid email profile urn:zitadel:iam:org:project:roles")
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                    role_claim: r.string("RUPTA_OIDC_ROLE_CLAIM", "urn:zitadel:iam:org:project:roles"),
                    role_mapping,
                    default_role,
                    jit_provisioning: r.bool("RUPTA_OIDC_JIT", true)?,
                })
            }
        };

        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        let durability = match r.string("RUPTA_DURABILITY", "fast").as_str() {
            "fast" => Durability::Fast,
            "safe" => Durability::Safe,
            other => bail!("RUPTA_DURABILITY must be fast or safe, got {other:?}"),
        };
        let log_format = match r.string("RUPTA_LOG_FORMAT", "text").as_str() {
            "text" => LogFormat::Text,
            "json" => LogFormat::Json,
            other => bail!("RUPTA_LOG_FORMAT must be text or json, got {other:?}"),
        };

        let config = Config {
            listen,
            public_url,
            data_dir: PathBuf::from(r.string("RUPTA_DATA_DIR", "/data")),
            storage_url,
            remote_sync,
            cache_bytes: r.number::<usize>("RUPTA_CACHE_MB", 256)? << 20,
            secret_key,
            admin_email: r.raw("RUPTA_ADMIN_EMAIL"),
            admin_password: r.secret("RUPTA_ADMIN_PASSWORD")?,
            default_org: r.string("RUPTA_DEFAULT_ORG", "default"),
            local_login: r.bool("RUPTA_LOCAL_LOGIN", true)?,
            oidc,
            smtp_url: r.secret("RUPTA_SMTP_URL")?,
            smtp_from: r.raw("RUPTA_MAIL_FROM").or_else(|| r.raw("RUPTA_SMTP_FROM")),
            camelmailer_api_key: r.secret("RUPTA_CAMELMAILER_API_KEY")?,
            camelmailer_url: r.raw("RUPTA_CAMELMAILER_URL"),
            ingest_workers: r.number("RUPTA_INGEST_WORKERS", cores)?.max(1),
            ingest_queue: r.number("RUPTA_INGEST_QUEUE", 100_000)?.max(1),
            durability,
            max_envelope_bytes: r.number::<usize>("RUPTA_MAX_ENVELOPE_MB", 20)? << 20,
            max_decompressed_bytes: r.number::<usize>("RUPTA_MAX_DECOMPRESSED_MB", 100)? << 20,
            retention_days: r.number("RUPTA_RETENTION_DAYS", 90)?,
            metrics_token: r.secret("RUPTA_METRICS_TOKEN")?,
            log: r.string("RUPTA_LOG", "info"),
            log_format,
            trusted_proxies: r.list("RUPTA_TRUSTED_PROXIES"),
            cors_allowed_origins: r.list("RUPTA_CORS_ALLOWED_ORIGINS"),
            shutdown_timeout: Duration::from_secs(r.number("RUPTA_SHUTDOWN_TIMEOUT_SECS", 30)?),
            sso_allow_private_issuers: r.bool("RUPTA_SSO_ALLOW_PRIVATE_ISSUERS", false)?,
            integrations_allow_private_hosts: r.bool("RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS", false)?,
            metrics_project_labels: r.number::<usize>("RUPTA_METRICS_PROJECT_LABELS", 50)?,
            mcp: r.bool("RUPTA_MCP", true)?,
        };
        if !config.local_login && config.oidc.is_none() {
            bail!("RUPTA_LOCAL_LOGIN=false needs single sign-on (RUPTA_OIDC_ISSUER), or nobody could log in");
        }
        if config.smtp_url.is_some() && config.camelmailer_api_key.is_some() {
            bail!("set either RUPTA_SMTP_URL or RUPTA_CAMELMAILER_API_KEY, not both");
        }
        if (config.smtp_url.is_some() || config.camelmailer_api_key.is_some()) && config.smtp_from.is_none() {
            bail!("RUPTA_MAIL_FROM is required with RUPTA_SMTP_URL or RUPTA_CAMELMAILER_API_KEY");
        }
        if let Some(url) = &config.camelmailer_url
            && !(url.starts_with("https://") || url.starts_with("http://"))
        {
            bail!("RUPTA_CAMELMAILER_URL must start with https:// or http://, got {url:?}");
        }
        if config.admin_email.is_some() != config.admin_password.is_some() {
            bail!("RUPTA_ADMIN_EMAIL and RUPTA_ADMIN_PASSWORD must be set together");
        }
        Ok(config)
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    const KEY: (&str, &str) = ("RUPTA_SECRET_KEY", "0123456789abcdef0123456789abcdef");

    #[test]
    fn defaults() {
        let c = Config::from_env(&env(&[KEY])).unwrap();
        assert_eq!(c.listen.to_string(), "0.0.0.0:8080");
        assert_eq!(c.public_url, "http://localhost:8080");
        assert_eq!(c.data_dir, PathBuf::from("/data"));
        assert!(c.storage_url.is_none());
        assert!(c.remote_sync);
        assert_eq!(c.durability, Durability::Fast);
        assert_eq!(c.retention_days, 90);
        assert!(c.oidc.is_none());
        assert!(!c.integrations_allow_private_hosts);
        assert_eq!(c.metrics_project_labels, 50);
        assert!(c.mcp);
    }

    #[test]
    fn the_mcp_endpoint_can_be_turned_off() {
        assert!(!Config::from_env(&env(&[KEY, ("RUPTA_MCP", "false")])).unwrap().mcp);
        assert!(Config::from_env(&env(&[KEY, ("RUPTA_MCP", "maybe")])).unwrap_err().to_string().contains("RUPTA_MCP"));
    }

    #[test]
    fn integration_and_metrics_settings() {
        let c = Config::from_env(&env(&[
            KEY,
            ("RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS", "true"),
            ("RUPTA_METRICS_PROJECT_LABELS", "10"),
        ]))
        .unwrap();
        assert!(c.integrations_allow_private_hosts);
        assert_eq!(c.metrics_project_labels, 10);
        assert!(Config::from_env(&env(&[KEY, ("RUPTA_METRICS_PROJECT_LABELS", "many")])).is_err());
    }

    #[test]
    fn a_secret_key_is_required() {
        assert!(Config::from_env(&env(&[])).unwrap_err().to_string().contains("RUPTA_SECRET_KEY"));
        assert!(Config::from_env(&env(&[("RUPTA_SECRET_KEY", "short")])).is_err());
    }

    #[test]
    fn mail_transports() {
        let c = Config::from_env(&env(&[
            KEY,
            ("RUPTA_CAMELMAILER_API_KEY", "cm_secret"),
            ("RUPTA_CAMELMAILER_URL", "https://mail.example.com"),
            ("RUPTA_MAIL_FROM", "rupta <rupta@example.com>"),
        ]))
        .unwrap();
        assert_eq!(c.camelmailer_api_key.as_deref(), Some("cm_secret"));
        assert_eq!(c.smtp_from.as_deref(), Some("rupta <rupta@example.com>"));
        assert!(!format!("{c:?}").contains("cm_secret"));
        // RUPTA_SMTP_FROM still works as the sender.
        let c = Config::from_env(&env(&[KEY, ("RUPTA_SMTP_URL", "smtp://h:25"), ("RUPTA_SMTP_FROM", "a@example.com")]))
            .unwrap();
        assert_eq!(c.smtp_from.as_deref(), Some("a@example.com"));
        let both = Config::from_env(&env(&[
            KEY,
            ("RUPTA_SMTP_URL", "smtp://h:25"),
            ("RUPTA_CAMELMAILER_API_KEY", "cm"),
            ("RUPTA_MAIL_FROM", "a@example.com"),
        ]));
        assert!(both.unwrap_err().to_string().contains("not both"));
        let no_from = Config::from_env(&env(&[KEY, ("RUPTA_CAMELMAILER_API_KEY", "cm")]));
        assert!(no_from.unwrap_err().to_string().contains("RUPTA_MAIL_FROM"));
    }

    #[test]
    fn secrets_come_from_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        std::fs::write(&path, "fedcba9876543210fedcba9876543210\n").unwrap();
        let c = Config::from_env(&env(&[
            ("RUPTA_SECRET_KEY_FILE", path.to_str().unwrap()),
            ("RUPTA_SECRET_KEY", "ignored"),
        ]))
        .unwrap();
        assert_eq!(c.secret_key, b"fedcba9876543210fedcba9876543210");
    }

    #[test]
    fn s3_credentials_are_added_to_the_storage_url_and_redacted() {
        let c = Config::from_env(&env(&[
            KEY,
            ("RUPTA_STORAGE_URL", "s3://rupta/prod?endpoint=http://garage:3900&region=garage&path_style=true"),
            ("RUPTA_S3_ACCESS_KEY_ID", "GKabc"),
            ("RUPTA_S3_SECRET_ACCESS_KEY", "s3cr/t"),
        ]))
        .unwrap();
        let url = c.storage_url.clone().unwrap();
        assert!(url.ends_with("&access_key=GKabc&secret_key=s3cr%2Ft"), "{url}");
        let shown = format!("{c:?}");
        assert!(!shown.contains("s3cr") && !shown.contains("GKabc"), "{shown}");
    }

    #[test]
    fn oidc_settings_with_role_mapping() {
        let c = Config::from_env(&env(&[
            KEY,
            ("RUPTA_OIDC_ISSUER", "https://auth.example.com/"),
            ("RUPTA_OIDC_CLIENT_ID", "123@rupta"),
            ("RUPTA_OIDC_CLIENT_SECRET", "secret"),
            ("RUPTA_OIDC_ROLE_MAPPING", "rupta-admin:admin, rupta-viewer:Viewer"),
        ]))
        .unwrap();
        let oidc = c.oidc.unwrap();
        assert_eq!(oidc.issuer, "https://auth.example.com");
        assert_eq!(oidc.role_claim, "urn:zitadel:iam:org:project:roles");
        assert_eq!(
            oidc.role_mapping,
            vec![("rupta-admin".into(), "admin".into()), ("rupta-viewer".into(), "viewer".into())]
        );
        assert!(!format!("{oidc:?}").contains("secret\""));
    }

    #[test]
    fn invalid_values_are_rejected_with_the_variable_name() {
        for (k, v) in [
            ("RUPTA_LISTEN", "nope"),
            ("RUPTA_REMOTE_SYNC", "sometimes"),
            ("RUPTA_DURABILITY", "maybe"),
            ("RUPTA_RETENTION_DAYS", "ninety"),
            ("RUPTA_LOCAL_LOGIN", "perhaps"),
            ("RUPTA_PUBLIC_URL", "errors.example.com"),
        ] {
            let err = Config::from_env(&env(&[KEY, (k, v)])).unwrap_err().to_string();
            assert!(err.contains(k), "{k}: {err}");
        }
    }

    #[test]
    fn disabling_local_login_needs_sso() {
        assert!(Config::from_env(&env(&[KEY, ("RUPTA_LOCAL_LOGIN", "false")])).is_err());
    }
}
