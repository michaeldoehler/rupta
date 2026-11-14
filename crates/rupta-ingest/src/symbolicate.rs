//! JavaScript source map symbolication.
//!
//! Source maps come from artifact bundles uploaded with the upload CLI: a
//! zip with a `manifest.json` (`{"files": {"<path>": {"url": "~/app.js",
//! "type": "minified_source" | "source_map" | "source", "headers":
//! {"debug-id": "...", "sourcemap": "app.js.map"}}}}`), stored in the
//! object store and indexed in `artifact_index`.
//!
//! A frame's map is found by debug id first (the event's
//! `debug_meta.images` of type `sourcemap` map a `code_file` to a
//! `debug_id`), then by release, dist and URL (`~/path` of the frame's
//! `abs_path`; the minified file's `sourcemap` header, or `<url>.map`,
//! names the map). Parsed maps are cached in memory (LRU by size); lookups
//! that found nothing are remembered for a minute. See `docs/sourcemaps.md`.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rupta_core::event::{Event, Frame};
use rupta_store::ingest::{ArtifactFile, artifact_bundle_count, artifacts_by_debug_id, artifacts_by_url};
use rupta_store::squad::object::ObjectStore;
use serde_json::Value;
use sourcemap::{DecodedMap, SourceView};

use crate::auth::ProjectInfo;

/// Lines of source shown before and after the line of a frame.
const CONTEXT_LINES: usize = 5;
const NEGATIVE_TTL: Duration = Duration::from_secs(60);
const BUNDLE_COUNT_TTL: Duration = Duration::from_secs(30);

/// A small LRU cache bounded by the total size of its values.
struct Lru<K, V> {
    map: HashMap<K, (V, usize, u64)>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> Lru<K, V> {
    fn new(budget: usize) -> Lru<K, V> {
        Lru { map: HashMap::new(), tick: 0, bytes: 0, budget }
    }

    fn get(&mut self, k: &K) -> Option<V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(k).map(|e| {
            e.2 = tick;
            e.0.clone()
        })
    }

    fn put(&mut self, k: K, v: V, size: usize) {
        if size > self.budget {
            return;
        }
        self.tick += 1;
        if let Some((_, old, _)) = self.map.insert(k, (v, size, self.tick)) {
            self.bytes -= old;
        }
        self.bytes += size;
        while self.bytes > self.budget {
            let Some(oldest) = self.map.iter().min_by_key(|(_, e)| e.2).map(|(k, _)| k.clone()) else { break };
            if let Some((_, s, _)) = self.map.remove(&oldest) {
                self.bytes -= s;
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ManifestEntry {
    url: Option<String>,
    sourcemap: Option<String>,
}

/// An artifact bundle: the zip and its manifest.
struct Bundle {
    zip: Vec<u8>,
    files: HashMap<String, ManifestEntry>,
}

impl Bundle {
    fn parse(zip: Vec<u8>) -> Option<Bundle> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&zip[..])).ok()?;
        let mut manifest = String::new();
        archive.by_name("manifest.json").ok()?.take(16 << 20).read_to_string(&mut manifest).ok()?;
        let manifest: Value = serde_json::from_str(&manifest).ok()?;
        let files = manifest
            .get("files")?
            .as_object()?
            .iter()
            .map(|(path, f)| {
                let headers = f.get("headers").and_then(Value::as_object);
                let header = |name: &str| {
                    headers
                        .and_then(|h| h.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)))
                        .and_then(|(_, v)| v.as_str())
                        .map(str::to_string)
                };
                (
                    path.clone(),
                    ManifestEntry {
                        url: f.get("url").and_then(Value::as_str).map(str::to_string),
                        sourcemap: header("sourcemap").or_else(|| header("x-sourcemap")),
                    },
                )
            })
            .collect();
        drop(archive);
        Some(Bundle { zip, files })
    }

    fn read(&self, path: &str, limit: u64) -> Option<Vec<u8>> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&self.zip[..])).ok()?;
        let mut out = Vec::new();
        archive.by_name(path).ok()?.take(limit).read_to_end(&mut out).ok()?;
        Some(out)
    }

    fn path_of_url(&self, url: &str) -> Option<&str> {
        self.files.iter().find(|(_, f)| f.url.as_deref() == Some(url)).map(|(p, _)| p.as_str())
    }
}

/// A parsed source map with the minified file it belongs to, if known.
pub struct ParsedMap {
    map: DecodedMap,
    minified: Option<SourceView>,
}

#[derive(Default)]
struct Caches {
    bundles: Option<Lru<i64, Arc<Bundle>>>,
    maps: Option<Lru<(i64, String), Arc<ParsedMap>>>,
    /// Lookup key → map (None: nothing found), with the time of the lookup.
    lookups: HashMap<String, (Option<(i64, String)>, Instant)>,
    bundle_counts: HashMap<i64, (i64, Instant)>,
}

pub struct Symbolicator {
    caches: Mutex<Caches>,
}

/// Candidate artifact URLs for a frame's `abs_path`: `~/path` (without
/// scheme and host, query or fragment), shorter suffixes of it, and the
/// path as sent.
pub fn url_candidates(abs_path: &str) -> Vec<String> {
    let clean = abs_path.split(['?', '#']).next().unwrap_or(abs_path);
    let path = match clean.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("/", |i| &rest[i..]),
        None => clean,
    };
    let path = path.trim_start_matches('/');
    let mut out = vec![abs_path.to_string()];
    if clean != abs_path {
        out.push(clean.to_string());
    }
    let segments: Vec<&str> = path.split('/').collect();
    for i in 0..segments.len() {
        out.push(format!("~/{}", segments[i..].join("/")));
    }
    out.dedup();
    out
}

fn resolve_relative(base_url: &str, rel: &str) -> String {
    if rel.contains("://") || rel.starts_with('~') {
        return rel.to_string();
    }
    if let Some(stripped) = rel.strip_prefix('/') {
        return format!("~/{stripped}");
    }
    let dir = base_url.rsplit_once('/').map_or("~", |(d, _)| d);
    let mut parts: Vec<&str> = dir.split('/').collect();
    for seg in rel.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// A readable file name for an original source (`webpack://app/./src/a.js`
/// becomes `src/a.js`).
fn clean_source(source: &str) -> String {
    let s = source.strip_prefix("webpack://").map_or(source, |r| r.split_once('/').map_or(r, |(_, p)| p));
    let s = s.strip_prefix("webpack:///").unwrap_or(s);
    let s = s.strip_prefix("app:///").unwrap_or(s);
    s.trim_start_matches("./").to_string()
}

fn is_js_frame(f: &Frame) -> bool {
    let Some(path) = f.abs_path.as_deref().or(f.filename.as_deref()) else { return false };
    let path = path.split(['?', '#']).next().unwrap_or(path);
    f.lineno.is_some()
        && f.colno.is_some()
        && !f.symbolicated
        && [".js", ".mjs", ".cjs", ".jsx"].iter().any(|e| path.ends_with(e))
}

impl Symbolicator {
    pub fn new(cache_bytes: usize) -> Symbolicator {
        Symbolicator {
            caches: Mutex::new(Caches {
                bundles: Some(Lru::new(cache_bytes / 2)),
                maps: Some(Lru::new(cache_bytes / 2)),
                ..Caches::default()
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Caches> {
        self.caches.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn has_bundles(&self, conn: &rupta_store::squad::Connection, org_id: i64) -> bool {
        if let Some((n, at)) = self.lock().bundle_counts.get(&org_id)
            && at.elapsed() < BUNDLE_COUNT_TTL
        {
            return *n > 0;
        }
        let n = artifact_bundle_count(conn, org_id).unwrap_or(0);
        self.lock().bundle_counts.insert(org_id, (n, Instant::now()));
        n > 0
    }

    fn bundle(&self, objects: &dyn ObjectStore, file: &ArtifactFile) -> Option<Arc<Bundle>> {
        if let Some(b) = self.lock().bundles.as_mut()?.get(&file.bundle_id) {
            return Some(b);
        }
        let zip = objects.get(&file.object_key).ok()??;
        let size = zip.len();
        let b = Arc::new(Bundle::parse(zip)?);
        self.lock().bundles.as_mut()?.put(file.bundle_id, b.clone(), size);
        Some(b)
    }

    fn parsed(
        &self,
        bundle_id: i64,
        bundle: &Bundle,
        map_path: &str,
        minified_path: Option<&str>,
    ) -> Option<Arc<ParsedMap>> {
        let key = (bundle_id, map_path.to_string());
        if let Some(m) = self.lock().maps.as_mut()?.get(&key) {
            return Some(m);
        }
        let bytes = bundle.read(map_path, 256 << 20)?;
        let map = match sourcemap::decode_slice(&bytes).ok()? {
            DecodedMap::Index(i) => DecodedMap::Regular(i.flatten().ok()?),
            other => other,
        };
        let minified_bytes = minified_path.and_then(|p| bundle.read(p, 64 << 20));
        let size = bytes.len() * 2 + minified_bytes.as_ref().map_or(0, Vec::len);
        let minified = minified_bytes.map(|b| SourceView::from_string(String::from_utf8_lossy(&b).into_owned()));
        let parsed = Arc::new(ParsedMap { map, minified });
        self.lock().maps.as_mut()?.put(key, parsed.clone(), size);
        Some(parsed)
    }

    /// Finds the map for a frame: `(bundle, map path, minified path)`.
    #[allow(clippy::too_many_arguments)]
    fn locate(
        &self,
        conn: &rupta_store::squad::Connection,
        objects: &dyn ObjectStore,
        org_id: i64,
        abs_path: &str,
        debug_id: Option<[u8; 16]>,
        release: Option<&str>,
        dist: Option<&str>,
    ) -> Option<(Arc<Bundle>, i64, String, Option<String>)> {
        if let Some(id) = debug_id {
            let files = artifacts_by_debug_id(conn, org_id, id).unwrap_or_default();
            if let Some(map) = files.iter().find(|f| f.file_type == "source_map") {
                let bundle = self.bundle(objects, map)?;
                let minified = files
                    .iter()
                    .find(|f| f.file_type == "minified_source" && f.bundle_id == map.bundle_id)
                    .map(|f| f.file_path.clone());
                return Some((bundle, map.bundle_id, map.file_path.clone(), minified));
            }
        }
        let release = release?;
        let candidates = url_candidates(abs_path);
        let files = artifacts_by_url(conn, org_id, release, dist, &candidates).unwrap_or_default();
        // Prefer the most specific URL.
        for url in &candidates {
            for f in files.iter().filter(|f| f.url.as_deref() == Some(url.as_str())) {
                let bundle = self.bundle(objects, f)?;
                if f.file_type == "source_map" {
                    return Some((bundle.clone(), f.bundle_id, f.file_path.clone(), None));
                }
                let entry = bundle.files.get(&f.file_path).cloned();
                let map_url = entry
                    .as_ref()
                    .and_then(|e| e.sourcemap.clone())
                    .map(|m| resolve_relative(url, &m))
                    .unwrap_or_else(|| format!("{url}.map"));
                if let Some(path) = bundle.path_of_url(&map_url).map(str::to_string) {
                    return Some((bundle.clone(), f.bundle_id, path, Some(f.file_path.clone())));
                }
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn map_for(
        &self,
        conn: &rupta_store::squad::Connection,
        objects: &dyn ObjectStore,
        org_id: i64,
        abs_path: &str,
        debug_id: Option<[u8; 16]>,
        release: Option<&str>,
        dist: Option<&str>,
    ) -> Option<Arc<ParsedMap>> {
        let key = format!("{org_id}\0{abs_path}\0{debug_id:?}\0{release:?}\0{dist:?}");
        let cached = self.lock().lookups.get(&key).cloned();
        if let Some((found, at)) = cached {
            match found {
                None if at.elapsed() < NEGATIVE_TTL => return None,
                None => {}
                Some((bundle_id, path)) => {
                    if let Some(m) = self.lock().maps.as_mut()?.get(&(bundle_id, path)) {
                        return Some(m);
                    }
                }
            }
        }
        let found = self.locate(conn, objects, org_id, abs_path, debug_id, release, dist);
        let mut caches = self.lock();
        if caches.lookups.len() > 100_000 {
            caches.lookups.clear();
        }
        match found {
            Some((bundle, bundle_id, map_path, minified)) => {
                drop(caches);
                let parsed = self.parsed(bundle_id, &bundle, &map_path, minified.as_deref());
                self.lock().lookups.insert(key, (parsed.as_ref().map(|_| (bundle_id, map_path)), Instant::now()));
                parsed
            }
            None => {
                caches.lookups.insert(key, (None, Instant::now()));
                None
            }
        }
    }

    /// Maps the JavaScript frames of an event back to original source.
    /// Frames without a map stay as they are.
    pub fn symbolicate(
        &self,
        conn: &rupta_store::squad::Connection,
        objects: &dyn ObjectStore,
        project: &ProjectInfo,
        ev: &mut Event,
    ) {
        if !matches!(ev.platform.as_str(), "javascript" | "node") {
            return;
        }
        let any =
            ev.exceptions().iter().filter_map(|e| e.stacktrace.as_ref()).any(|st| st.frames.iter().any(is_js_frame))
                || ev.threads.as_ref().is_some_and(|t| {
                    t.values.iter().filter_map(|t| t.stacktrace.as_ref()).any(|st| st.frames.iter().any(is_js_frame))
                });
        if !any || !self.has_bundles(conn, project.org_id) {
            return;
        }
        let debug_ids: HashMap<String, [u8; 16]> = ev
            .debug_meta
            .as_ref()
            .and_then(|d| d.get("images"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|i| i.get("type").and_then(Value::as_str) == Some("sourcemap"))
            .filter_map(|i| {
                let file = i.get("code_file")?.as_str()?.to_string();
                let id = uuid::Uuid::try_parse(i.get("debug_id")?.as_str()?).ok()?;
                Some((file, *id.as_bytes()))
            })
            .collect();
        let release = ev.release.clone();
        let dist = ev.dist.clone();
        let mut missing = Vec::new();
        for st in ev.stacktraces_mut() {
            // The identifier called at a frame's position names the function
            // of the next (newer) frame.
            let mut caller_name: Option<String> = None;
            for f in st.frames.iter_mut() {
                if !is_js_frame(f) {
                    caller_name = None;
                    continue;
                }
                let abs = f.abs_path.clone().or(f.filename.clone()).unwrap_or_default();
                let debug_id = debug_ids.get(&abs).copied();
                let Some(map) =
                    self.map_for(conn, objects, project.org_id, &abs, debug_id, release.as_deref(), dist.as_deref())
                else {
                    if debug_id.is_some() {
                        missing.push(abs);
                    }
                    caller_name = None;
                    continue;
                };
                caller_name = apply(f, &map, caller_name.take());
            }
        }
        missing.sort();
        missing.dedup();
        for m in missing {
            ev.errors.push(rupta_core::event::ProcessingError::new("js_missing_source_map", Some(m), None));
        }
    }
}

/// Rewrites one frame from its map. Returns the name of the identifier at
/// the frame's position (the function the next frame runs in).
fn apply(f: &mut Frame, map: &ParsedMap, caller_name: Option<String>) -> Option<String> {
    let (line, col) = (f.lineno?.checked_sub(1)?, f.colno?.checked_sub(1)?);
    let token = map.map.lookup_token(line, col)?;
    let source = token.get_source()?.to_string();
    let original = f.clone();
    let minified_name = f.function.as_deref().map(|n| n.rsplit('.').next().unwrap_or(n).to_string());
    let function = minified_name
        .as_deref()
        .and_then(|n| map.map.get_original_function_name(line, col, Some(n), map.minified.as_ref()))
        .map(str::to_string)
        .or(caller_name)
        .or(original.function.clone());
    let src_line = token.get_src_line();
    let contents = match &map.map {
        DecodedMap::Regular(sm) => sm.get_source_contents(token.get_src_id()),
        _ => None,
    };
    f.raw = Some(Box::new(Frame { raw: None, ..original }));
    f.lineno = Some(src_line + 1);
    f.colno = Some(token.get_src_col() + 1);
    f.function = function;
    f.filename = Some(clean_source(&source));
    f.abs_path = Some(source.clone());
    f.symbolicated = true;
    f.in_app = Some(!source.contains("node_modules/") && !source.contains("webpack/bootstrap"));
    if let Some(text) = contents {
        let lines: Vec<&str> = text.lines().collect();
        let i = src_line as usize;
        if i < lines.len() {
            let cut = |l: &str| rupta_core::normalize::truncate(l, 1024);
            f.context_line = Some(cut(lines[i]));
            f.pre_context = lines[i.saturating_sub(CONTEXT_LINES)..i].iter().map(|l| cut(l)).collect();
            f.post_context = lines[i + 1..(i + 1 + CONTEXT_LINES).min(lines.len())].iter().map(|l| cut(l)).collect();
        }
    } else {
        f.context_line = None;
        f.pre_context.clear();
        f.post_context.clear();
    }
    token.get_name().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_candidates_strip_host_and_query() {
        assert_eq!(
            url_candidates("https://cdn.example.com/static/js/app.js?v=1"),
            vec![
                "https://cdn.example.com/static/js/app.js?v=1",
                "https://cdn.example.com/static/js/app.js",
                "~/static/js/app.js",
                "~/js/app.js",
                "~/app.js"
            ]
        );
        assert_eq!(url_candidates("app:///dist/index.js")[1], "~/dist/index.js");
        assert_eq!(url_candidates("/srv/app/dist/index.js")[1], "~/srv/app/dist/index.js");
    }

    #[test]
    fn relative_map_urls() {
        assert_eq!(resolve_relative("~/static/app.js", "app.js.map"), "~/static/app.js.map");
        assert_eq!(resolve_relative("~/static/js/app.js", "../maps/app.js.map"), "~/static/maps/app.js.map");
        assert_eq!(resolve_relative("~/static/app.js", "/maps/app.js.map"), "~/maps/app.js.map");
        assert_eq!(clean_source("webpack://my-app/./src/index.js"), "src/index.js");
        assert_eq!(clean_source("../src/a.ts"), "../src/a.ts");
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut l: Lru<i32, i32> = Lru::new(10);
        l.put(1, 1, 4);
        l.put(2, 2, 4);
        assert_eq!(l.get(&1), Some(1));
        l.put(3, 3, 4);
        assert_eq!(l.get(&2), None);
        assert_eq!(l.get(&1), Some(1));
        l.put(4, 4, 11);
        assert_eq!(l.get(&4), None);
    }
}
