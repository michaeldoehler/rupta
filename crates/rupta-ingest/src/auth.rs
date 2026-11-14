//! DSN authentication and the in-memory cache of keys and projects.
//!
//! SDKs identify themselves with the public key of a DSN, sent as
//! `X-Sentry-Auth: Sentry sentry_key=<key>, ...` (or the same in
//! `Authorization`), as the `sentry_key` query parameter, or as the `dsn`
//! in the envelope header. The key must exist, be active and belong to the
//! project in the URL.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use rupta_core::settings::ProjectSettings;
use rupta_store::Store;

/// Fields of an `X-Sentry-Auth` header.
pub fn parse_auth_header(value: &str) -> HashMap<String, String> {
    let v = value.trim();
    let v = match v.split_once(char::is_whitespace) {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("sentry") => rest,
        _ => v,
    };
    v.split(',')
        .filter_map(|part| {
            let (k, val) = part.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), val.trim().trim_matches('"').to_string()))
        })
        .collect()
}

/// `(public_key, project_id)` of a DSN such as `https://key@host/path/42`.
pub fn parse_dsn(dsn: &str) -> Option<(String, String)> {
    let rest = dsn.split_once("://")?.1;
    let (userinfo, hostpath) = rest.split_once('@')?;
    let key = userinfo.split(':').next()?.to_string();
    let project = hostpath.trim_end_matches('/').rsplit('/').next()?.to_string();
    if key.is_empty() || project.is_empty() || !hostpath.contains('/') {
        return None;
    }
    Some((key, project))
}

/// A query parameter (no percent-decoding needed for keys and versions,
/// but `+` and `%XX` are decoded anyway).
pub fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == name).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyInfo {
    pub id: i64,
    pub project_id: i64,
    pub active: bool,
    pub rate_limit_per_min: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub id: i64,
    pub org_id: i64,
    pub settings: ProjectSettings,
}

#[derive(Debug, Default)]
pub struct Snapshot {
    pub keys: HashMap<String, KeyInfo>,
    pub projects: HashMap<i64, Arc<ProjectInfo>>,
}

/// Keys and projects, reloaded when `Store::config_generation()` changes
/// (and at most once a second when an unknown key shows up, so keys
/// inserted without bumping the generation are found too).
pub struct KeyCache {
    store: Store,
    snapshot: RwLock<Arc<Snapshot>>,
    loaded_generation: AtomicU64,
    last_load: RwLock<Option<Instant>>,
    reload: tokio::sync::Mutex<()>,
}

const MISS_RELOAD_INTERVAL: Duration = Duration::from_secs(1);

impl KeyCache {
    pub fn new(store: Store) -> KeyCache {
        KeyCache {
            store,
            snapshot: RwLock::new(Arc::new(Snapshot::default())),
            loaded_generation: AtomicU64::new(u64::MAX),
            last_load: RwLock::new(None),
            reload: tokio::sync::Mutex::new(()),
        }
    }

    fn current(&self) -> Arc<Snapshot> {
        self.snapshot.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    async fn load(&self) -> Result<(), rupta_store::StoreError> {
        let _guard = self.reload.lock().await;
        let generation = self.store.config_generation();
        let (keys, projects) = self
            .store
            .run(|c| Ok((rupta_store::ingest::load_keys(c)?, rupta_store::ingest::load_projects(c)?)))
            .await?;
        let snap = Snapshot {
            keys: keys
                .into_iter()
                .map(|k| {
                    (
                        k.public_key,
                        KeyInfo {
                            id: k.id,
                            project_id: k.project_id,
                            active: k.is_active,
                            rate_limit_per_min: k.rate_limit_per_min.and_then(|v| u32::try_from(v).ok()),
                        },
                    )
                })
                .collect(),
            projects: projects
                .into_iter()
                .map(|p| {
                    (
                        p.id,
                        Arc::new(ProjectInfo {
                            id: p.id,
                            org_id: p.org_id,
                            settings: ProjectSettings::from_json(&p.settings),
                        }),
                    )
                })
                .collect(),
        };
        *self.snapshot.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(snap);
        self.loaded_generation.store(generation, Ordering::Release);
        *self.last_load.write().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        Ok(())
    }

    /// The current snapshot, reloaded first if the configuration changed.
    pub async fn snapshot(&self) -> Result<Arc<Snapshot>, rupta_store::StoreError> {
        if self.loaded_generation.load(Ordering::Acquire) != self.store.config_generation() {
            self.load().await?;
        }
        Ok(self.current())
    }

    /// Looks up a key, reloading once if it is unknown.
    pub async fn key(&self, public_key: &str) -> Result<(Option<KeyInfo>, Arc<Snapshot>), rupta_store::StoreError> {
        let snap = self.snapshot().await?;
        if let Some(k) = snap.keys.get(public_key) {
            return Ok((Some(k.clone()), snap));
        }
        let stale = self
            .last_load
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_none_or(|t| t.elapsed() >= MISS_RELOAD_INTERVAL);
        if stale {
            self.load().await?;
            let snap = self.current();
            return Ok((snap.keys.get(public_key).cloned(), snap));
        }
        Ok((None, snap))
    }

    /// A project's settings, reloading if the configuration changed.
    pub async fn project(&self, id: i64) -> Result<Option<Arc<ProjectInfo>>, rupta_store::StoreError> {
        Ok(self.snapshot().await?.projects.get(&id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_header_forms() {
        let h = parse_auth_header(
            "Sentry sentry_version=7, sentry_client=sentry.php/4.0, sentry_key=abc, sentry_secret=def",
        );
        assert_eq!(h.get("sentry_key").map(String::as_str), Some("abc"));
        assert_eq!(h.get("sentry_version").map(String::as_str), Some("7"));
        let h = parse_auth_header("sentry_key=abc,sentry_version=7");
        assert_eq!(h.get("sentry_key").map(String::as_str), Some("abc"));
    }

    #[test]
    fn dsns() {
        assert_eq!(parse_dsn("https://abc@example.com/42"), Some(("abc".into(), "42".into())));
        assert_eq!(parse_dsn("http://abc:secret@localhost:8080/sub/path/7/"), Some(("abc".into(), "7".into())));
        assert_eq!(parse_dsn("https://example.com/42"), None);
        assert_eq!(parse_dsn("garbage"), None);
    }

    #[test]
    fn query_params() {
        let q = Some("sentry_version=7&sentry_key=k%2B1&sentry_client=a+b&x");
        assert_eq!(query_param(q, "sentry_key").as_deref(), Some("k+1"));
        assert_eq!(query_param(q, "sentry_client").as_deref(), Some("a b"));
        assert_eq!(query_param(q, "x").as_deref(), Some(""));
        assert_eq!(query_param(q, "nope"), None);
        assert_eq!(query_param(Some("a=%"), "a").as_deref(), Some("%"));
        assert_eq!(query_param(Some("a=%4"), "a").as_deref(), Some("%4"));
    }

    #[tokio::test]
    async fn cache_reloads_on_generation_and_misses() {
        let store = Store::open_in_memory().unwrap();
        let cache = KeyCache::new(store.clone());
        assert!(cache.key("pk").await.unwrap().0.is_none());
        store
            .run(|c| {
                c.batch_execute(
                    "INSERT INTO organizations (slug, name) VALUES ('o', 'O');
                     INSERT INTO projects (org_id, slug, name, settings) VALUES (1, 'p', 'P', '{\"rate_limit_per_min\": 5}');
                     INSERT INTO project_keys (project_id, public_key, secret_key, rate_limit_per_min) VALUES (1, 'pk', 'sk', 10);",
                )?;
                Ok(())
            })
            .await
            .unwrap();
        store.bump_config();
        let (k, snap) = cache.key("pk").await.unwrap();
        let k = k.unwrap();
        assert_eq!((k.project_id, k.active, k.rate_limit_per_min), (1, true, Some(10)));
        assert_eq!(snap.projects[&1].settings.rate_limit_per_min, Some(5));
        store.run(|c| Ok(c.execute("UPDATE project_keys SET is_active = false", &[])?)).await.unwrap();
        store.bump_config();
        assert!(!cache.key("pk").await.unwrap().0.unwrap().active);
    }
}
