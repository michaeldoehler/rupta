//! Storage for rupta on the embedded squad database.
//!
//! [`Store`] owns the open [`squad::Database`]. squad's API is synchronous,
//! so every database call runs on tokio's blocking thread pool through
//! [`Store::run`]. Schema changes are numbered SQL migrations in
//! `migrations/`, applied in order at start-up.

pub mod ingest;
mod migrations;
pub mod telemetry;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use squad::{Config as SquadConfig, Connection, Database, RemoteConfig, RemoteSync};

pub use squad;

/// Errors of the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] squad::Error),
    #[error("object store: {0}")]
    Object(String),
    #[error("{0}")]
    Invalid(String),
    #[error("background task failed: {0}")]
    Join(String),
}

impl StoreError {
    /// The SQLSTATE of a database error, if there is one.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            StoreError::Db(e) => Some(e.code()),
            _ => None,
        }
    }

    /// True for a violated unique constraint (SQLSTATE 23505).
    pub fn is_unique_violation(&self) -> bool {
        self.sqlstate() == Some("23505")
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Where and how the database is stored.
#[derive(Debug, Clone)]
pub struct StoreOptions {
    /// Local directory: the working copy (and the only copy without a remote).
    pub data_dir: PathBuf,
    /// Durable copy in an object store (`s3://bucket/prefix?...`), if any.
    pub remote_url: Option<String>,
    /// Wait for the upload to the object store before a commit returns.
    pub remote_sync: bool,
    /// Bytes of memory for the row store's page cache.
    pub cache_bytes: usize,
}

impl StoreOptions {
    pub fn local(dir: impl Into<PathBuf>) -> StoreOptions {
        StoreOptions { data_dir: dir.into(), remote_url: None, remote_sync: true, cache_bytes: 256 << 20 }
    }
}

/// The open database.
#[derive(Clone)]
pub struct Store {
    db: Database,
    objects: Arc<dyn squad::object::ObjectStore>,
    generation: Arc<AtomicU64>,
}

impl Store {
    /// Opens (or creates) the database and applies pending migrations.
    pub fn open(opts: &StoreOptions) -> Result<Store> {
        std::fs::create_dir_all(&opts.data_dir)
            .map_err(|e| StoreError::Invalid(format!("{}: {e}", opts.data_dir.display())))?;
        let db_dir = opts.data_dir.join("db");
        let (db, objects) = match &opts.remote_url {
            Some(url) => {
                let store = squad::object::open_url(url).map_err(|e| StoreError::Object(e.to_string()))?;
                let mode = if opts.remote_sync {
                    RemoteSync::Sync
                } else {
                    RemoteSync::Async { interval: std::time::Duration::from_secs(1) }
                };
                let remote = RemoteConfig { restore: true, ..RemoteConfig::new(store.clone(), mode) };
                let config =
                    SquadConfig { cache_size: opts.cache_bytes, remote: Some(remote), ..SquadConfig::default() };
                (Database::open(&db_dir, config)?, store)
            }
            None => {
                let config = SquadConfig { cache_size: opts.cache_bytes, ..SquadConfig::default() };
                let objects: Arc<dyn squad::object::ObjectStore> = Arc::new(
                    squad::object::LocalStore::new(opts.data_dir.join("objects"))
                        .map_err(|e| StoreError::Object(e.to_string()))?,
                );
                (Database::open(&db_dir, config)?, objects)
            }
        };
        let store = Store { db, objects, generation: Arc::default() };
        migrations::apply(&store.db.connect())?;
        Ok(store)
    }

    /// An in-memory database, for tests.
    pub fn open_in_memory() -> Result<Store> {
        let db = Database::open_in_memory()?;
        let store = Store { db, objects: Arc::new(squad::object::MemoryStore::new()), generation: Arc::default() };
        migrations::apply(&store.db.connect())?;
        Ok(store)
    }

    /// The squad database.
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// The object store for attachments and uploaded artifacts: the remote
    /// bucket if there is one, otherwise a local directory.
    pub fn objects(&self) -> &Arc<dyn squad::object::ObjectStore> {
        &self.objects
    }

    /// A new connection. Connections are cheap; use one per unit of work.
    pub fn connect(&self) -> Connection {
        self.db.connect()
    }

    /// Runs `f` with a fresh connection on the blocking thread pool.
    pub async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || f(&db.connect())).await.map_err(|e| StoreError::Join(e.to_string()))?
    }

    /// Counter of configuration changes (projects, keys, project settings).
    /// Caches such as the ingest key cache compare it to reload cheaply.
    pub fn config_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Records a configuration change; call after committing it.
    pub fn bump_config(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Health of the durable copy: `None` without a remote.
    pub fn remote_status(&self) -> Option<squad::RemoteStats> {
        self.db.remote_status()
    }

    /// The directory the database lives in.
    pub fn db_dir(opts: &StoreOptions) -> PathBuf {
        Path::new(&opts.data_dir).join("db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let opts = StoreOptions::local(dir.path());
        {
            let store = Store::open(&opts).unwrap();
            let conn = store.connect();
            conn.execute("INSERT INTO organizations (slug, name) VALUES ('acme', 'Acme')", &[]).unwrap();
        }
        // Reopening applies nothing twice and keeps the data.
        let store = Store::open(&opts).unwrap();
        let n: i64 = store.connect().query_one("SELECT count(*) FROM organizations", &[]).unwrap().get(0);
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn run_executes_on_the_blocking_pool() {
        let store = Store::open_in_memory().unwrap();
        let n =
            store.run(|c| Ok(c.query_one("SELECT count(*) FROM schema_migrations", &[])?.get::<i64>(0))).await.unwrap();
        assert!(n >= 2);
    }
}
