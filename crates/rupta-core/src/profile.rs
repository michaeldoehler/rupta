//! Sampled profiles and flamegraphs.
//!
//! The SDKs send two shapes of the same sampled format:
//!
//! - `profile` items (version 1, transaction profiling): `profile.samples`
//!   with `elapsed_since_start_ns` relative to the profile's `timestamp`,
//!   and a `transaction` object naming the transaction and its thread;
//! - `profile_chunk` items (version 2, continuous profiling): samples with
//!   an absolute `timestamp` in seconds, grouped by `profiler_id`.
//!
//! Both have `stacks` (lists of frame indices, innermost frame first),
//! `frames` and `thread_metadata`. A flamegraph merges the stacks of one
//! thread into a call tree whose weights are sample counts; the
//! aggregated flamegraph merges several profiles, identifying frames by
//! function, file and package.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

/// Deepest stack kept; frames beyond it (on the root side) are dropped.
pub const MAX_STACK_DEPTH: usize = 512;

/// One frame as the flamegraph presents it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Frame {
    /// Function name (`<anonymous>` when the SDK sent none).
    pub name: String,
    /// File path, module or URL.
    pub file: Option<String>,
    pub line: Option<u32>,
    /// Package, library or module the frame belongs to.
    pub package: Option<String>,
    /// True for the application's own code (`in_app`).
    pub is_application: bool,
}

/// A parsed sampled profile.
#[derive(Debug, Clone, Default)]
pub struct SampledProfile {
    pub frames: Vec<Frame>,
    pub stacks: Vec<Vec<usize>>,
    pub samples: Vec<Sample>,
    /// Thread id → name.
    pub threads: Vec<(String, Option<String>)>,
    /// The thread the transaction ran on (version 1), if given.
    pub active_thread: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub stack: usize,
    pub thread: String,
    /// Absolute time in seconds since the Unix epoch.
    pub at: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid profile: {0}")]
pub struct ProfileError(pub String);

fn id_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn non_empty(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The package of a frame: the `package` field, or what the path says
/// (`node_modules/<pkg>`, `site-packages/<pkg>`, `vendor/<vendor>/<pkg>`).
fn package_of(f: &Value, file: Option<&str>) -> Option<String> {
    if let Some(p) = non_empty(f.get("package")) {
        return Some(p);
    }
    let path = non_empty(f.get("abs_path")).or_else(|| file.map(str::to_string))?;
    for marker in ["node_modules/", "site-packages/", "dist-packages/"] {
        if let Some(i) = path.rfind(marker) {
            let rest = &path[i + marker.len()..];
            let mut parts = rest.split('/');
            let first = parts.next()?;
            return Some(if first.starts_with('@') {
                format!("{first}/{}", parts.next().unwrap_or(""))
            } else {
                first.trim_end_matches(".py").to_string()
            });
        }
    }
    if let Some(i) = path.find("vendor/") {
        let mut parts = path[i + 7..].split('/');
        if let (Some(a), Some(b)) = (parts.next(), parts.next()) {
            return Some(format!("{a}/{b}"));
        }
    }
    if path.starts_with("node:") {
        return Some("node".into());
    }
    None
}

fn frame_from(f: &Value, platform: &str) -> Frame {
    let file =
        non_empty(f.get("filename")).or_else(|| non_empty(f.get("abs_path"))).or_else(|| non_empty(f.get("module")));
    let package = package_of(f, file.as_deref());
    let is_application = match f.get("in_app").and_then(Value::as_bool) {
        Some(b) => b,
        None => {
            package.is_none()
                && !file.as_deref().is_some_and(|p| {
                    p.starts_with('<') || p.contains("/lib/python") || (platform == "php" && p.contains("/vendor/"))
                })
        }
    };
    let name = non_empty(f.get("function")).unwrap_or_else(|| "<anonymous>".into());
    let line = f.get("lineno").and_then(Value::as_u64).map(|l| l.min(u32::MAX as u64) as u32);
    Frame { name, file, line, package, is_application }
}

impl SampledProfile {
    /// Parses a `profile` (version 1) or `profile_chunk` (version 2) document.
    pub fn parse(doc: &Value) -> Result<SampledProfile, ProfileError> {
        let platform = doc.get("platform").and_then(Value::as_str).unwrap_or("");
        let p = doc.get("profile").filter(|p| p.is_object()).ok_or_else(|| ProfileError("no `profile`".into()))?;
        let frames: Vec<Frame> = p
            .get("frames")
            .and_then(Value::as_array)
            .ok_or_else(|| ProfileError("no `profile.frames`".into()))?
            .iter()
            .map(|f| frame_from(f, platform))
            .collect();
        let stacks: Vec<Vec<usize>> = p
            .get("stacks")
            .and_then(Value::as_array)
            .ok_or_else(|| ProfileError("no `profile.stacks`".into()))?
            .iter()
            .map(|s| {
                s.as_array()
                    .map(|a| {
                        a.iter().filter_map(Value::as_u64).map(|i| i as usize).filter(|i| *i < frames.len()).collect()
                    })
                    .unwrap_or_default()
            })
            .collect();
        // Version 1: sample times relative to the profile's start.
        let base = doc
            .get("timestamp")
            .and_then(crate::normalize::parse_timestamp)
            .map(|t| t.timestamp_micros() as f64 / 1e6)
            .unwrap_or(0.0);
        let mut samples = Vec::new();
        for s in p.get("samples").and_then(Value::as_array).into_iter().flatten() {
            let Some(stack) = s.get("stack_id").and_then(Value::as_u64).map(|v| v as usize) else { continue };
            if stack >= stacks.len() {
                continue;
            }
            let thread = s.get("thread_id").and_then(id_string).unwrap_or_else(|| "0".into());
            let at = match (s.get("timestamp"), s.get("elapsed_since_start_ns")) {
                (Some(t), _) if t.is_number() => t.as_f64().unwrap_or(0.0),
                (Some(t), _) if t.is_string() => {
                    crate::normalize::parse_timestamp(t).map(|t| t.timestamp_micros() as f64 / 1e6).unwrap_or(base)
                }
                (_, Some(ns)) => {
                    let ns = ns.as_f64().or_else(|| ns.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                    base + ns / 1e9
                }
                _ => base,
            };
            samples.push(Sample { stack, thread, at });
        }
        let mut threads: Vec<(String, Option<String>)> = p
            .get("thread_metadata")
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), non_empty(v.get("name")))).collect())
            .unwrap_or_default();
        for s in &samples {
            if !threads.iter().any(|(t, _)| *t == s.thread) {
                threads.push((s.thread.clone(), None));
            }
        }
        let active_thread = doc
            .get("transaction")
            .and_then(|t| t.get("active_thread_id"))
            .and_then(id_string)
            .or_else(|| doc.get("transactions")?.as_array()?.first()?.get("active_thread_id").and_then(id_string));
        Ok(SampledProfile { frames, stacks, samples, threads, active_thread })
    }

    /// Samples per thread, most first.
    pub fn thread_sample_counts(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for s in &self.samples {
            *counts.entry(&s.thread).or_default() += 1;
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// The thread a flamegraph shows by default: the transaction's
    /// thread, else a thread named like a main thread, else the thread
    /// with the most samples.
    pub fn default_thread(&self) -> Option<String> {
        if let Some(t) = &self.active_thread
            && self.samples.iter().any(|s| s.thread == *t)
        {
            return Some(t.clone());
        }
        let counts = self.thread_sample_counts();
        for (id, name) in &self.threads {
            if name.as_deref().is_some_and(|n| matches!(n, "main" | "MainThread" | "Main Thread"))
                && counts.iter().any(|(t, _)| t == id)
            {
                return Some(id.clone());
            }
        }
        counts.first().map(|(t, _)| t.clone())
    }

    /// First and last sample time, in seconds.
    pub fn time_range(&self) -> Option<(f64, f64)> {
        let min = self.samples.iter().map(|s| s.at).fold(f64::INFINITY, f64::min);
        let max = self.samples.iter().map(|s| s.at).fold(f64::NEG_INFINITY, f64::max);
        (min.is_finite() && max.is_finite()).then_some((min, max))
    }
}

/// A node of the call tree. The root has no frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    /// Index into `frames`; `null` for the root.
    pub frame_index: Option<usize>,
    /// Samples in which this frame was the innermost one.
    pub self_weight: u64,
    /// Samples in which this frame was on the stack (below this path).
    pub total_weight: u64,
    /// Callees, in the order they were first seen (time order).
    pub children: Vec<Node>,
}

/// A flamegraph: deduplicated frames and the call tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Flamegraph {
    pub frames: Vec<Frame>,
    pub root: Node,
    /// Samples merged into the tree.
    pub samples: u64,
    /// Profiles that contributed samples.
    pub profiles: u64,
}

struct ArenaNode {
    frame: Option<usize>,
    self_weight: u64,
    total_weight: u64,
    children: Vec<usize>,
    by_frame: HashMap<usize, usize>,
}

/// Builds a flamegraph from one or more profiles.
pub struct FlamegraphBuilder {
    frames: Vec<Frame>,
    index: HashMap<(String, Option<String>, Option<String>), usize>,
    nodes: Vec<ArenaNode>,
    samples: u64,
    profiles: u64,
}

impl Default for FlamegraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FlamegraphBuilder {
    pub fn new() -> FlamegraphBuilder {
        FlamegraphBuilder {
            frames: Vec::new(),
            index: HashMap::new(),
            nodes: vec![ArenaNode {
                frame: None,
                self_weight: 0,
                total_weight: 0,
                children: Vec::new(),
                by_frame: HashMap::new(),
            }],
            samples: 0,
            profiles: 0,
        }
    }

    fn frame_id(&mut self, f: &Frame) -> usize {
        let key = (f.name.clone(), f.file.clone(), f.package.clone());
        if let Some(i) = self.index.get(&key) {
            return *i;
        }
        let i = self.frames.len();
        self.frames.push(f.clone());
        self.index.insert(key, i);
        i
    }

    /// Adds the samples of `thread` (the profile's default thread when
    /// `None`) whose time is within `window` (seconds, inclusive).
    /// Returns the number of samples added.
    pub fn add(&mut self, p: &SampledProfile, thread: Option<&str>, window: Option<(f64, f64)>) -> u64 {
        let thread = match thread {
            Some(t) => Some(t.to_string()),
            None => p.default_thread(),
        };
        let Some(thread) = thread else { return 0 };
        let local: Vec<usize> = p.frames.iter().map(|f| self.frame_id(f)).collect();
        let mut added = 0;
        for s in &p.samples {
            if s.thread != thread {
                continue;
            }
            if let Some((from, to)) = window
                && (s.at < from || s.at > to)
            {
                continue;
            }
            let stack = &p.stacks[s.stack];
            // Innermost first on the wire; walk from the root.
            let keep = stack.len().min(MAX_STACK_DEPTH);
            let mut node = 0;
            self.nodes[0].total_weight += 1;
            for &fi in stack[..keep].iter().rev() {
                let frame = local[fi];
                let next = match self.nodes[node].by_frame.get(&frame) {
                    Some(n) => *n,
                    None => {
                        let n = self.nodes.len();
                        self.nodes.push(ArenaNode {
                            frame: Some(frame),
                            self_weight: 0,
                            total_weight: 0,
                            children: Vec::new(),
                            by_frame: HashMap::new(),
                        });
                        self.nodes[node].children.push(n);
                        self.nodes[node].by_frame.insert(frame, n);
                        n
                    }
                };
                node = next;
                self.nodes[node].total_weight += 1;
            }
            self.nodes[node].self_weight += 1;
            added += 1;
        }
        self.samples += added;
        if added > 0 {
            self.profiles += 1;
        }
        added
    }

    fn tree(&self, i: usize) -> Node {
        let n = &self.nodes[i];
        Node {
            frame_index: n.frame,
            self_weight: n.self_weight,
            total_weight: n.total_weight,
            children: n.children.iter().map(|c| self.tree(*c)).collect(),
        }
    }

    pub fn build(self) -> Flamegraph {
        let root = self.tree(0);
        Flamegraph { frames: self.frames, root, samples: self.samples, profiles: self.profiles }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// main → a → b (2 samples), main → a (1), main → c (1); another
    /// thread with one sample.
    fn js_profile() -> Value {
        json!({
            "version": "1",
            "platform": "node",
            "timestamp": "2026-11-14T10:00:00Z",
            "transaction": {"name": "GET /", "id": "0123456789abcdef0123456789abcdef", "trace_id": "t", "active_thread_id": "0"},
            "profile": {
                "frames": [
                    {"function": "main", "abs_path": "/srv/app/index.js", "lineno": 1, "in_app": true},
                    {"function": "a", "abs_path": "/srv/app/index.js", "lineno": 5, "in_app": true},
                    {"function": "b", "abs_path": "/srv/app/node_modules/lodash/index.js", "lineno": 9},
                    {"function": "c", "abs_path": "node:internal/timers", "lineno": 3},
                    {"abs_path": "/srv/app/index.js"}
                ],
                "stacks": [[2, 1, 0], [1, 0], [3, 0], [4]],
                "samples": [
                    {"stack_id": 0, "thread_id": "0", "elapsed_since_start_ns": "0"},
                    {"stack_id": 0, "thread_id": "0", "elapsed_since_start_ns": "10000000"},
                    {"stack_id": 1, "thread_id": "0", "elapsed_since_start_ns": "20000000"},
                    {"stack_id": 2, "thread_id": "0", "elapsed_since_start_ns": "30000000"},
                    {"stack_id": 3, "thread_id": "7", "elapsed_since_start_ns": "30000000"}
                ],
                "thread_metadata": {"0": {"name": "main"}, "7": {"name": "worker"}}
            }
        })
    }

    #[test]
    fn builds_the_call_tree_of_the_active_thread() {
        let p = SampledProfile::parse(&js_profile()).unwrap();
        assert_eq!(p.default_thread().as_deref(), Some("0"));
        assert_eq!(p.frames[2].package.as_deref(), Some("lodash"));
        assert!(!p.frames[2].is_application && p.frames[1].is_application);
        assert_eq!(p.frames[3].package.as_deref(), Some("node"));
        assert_eq!(p.frames[4].name, "<anonymous>");
        let mut b = FlamegraphBuilder::new();
        assert_eq!(b.add(&p, None, None), 4);
        let g = b.build();
        assert_eq!((g.samples, g.profiles), (4, 1));
        assert_eq!(g.root.total_weight, 4);
        assert_eq!(g.root.children.len(), 1);
        let main = &g.root.children[0];
        assert_eq!(g.frames[main.frame_index.unwrap()].name, "main");
        assert_eq!((main.total_weight, main.self_weight), (4, 0));
        let names: Vec<&str> = main.children.iter().map(|c| g.frames[c.frame_index.unwrap()].name.as_str()).collect();
        assert_eq!(names, vec!["a", "c"], "children in time order");
        let a = &main.children[0];
        assert_eq!((a.total_weight, a.self_weight), (3, 1));
        assert_eq!((a.children[0].total_weight, a.children[0].self_weight), (2, 2));
        assert_eq!(main.children[1].self_weight, 1);
    }

    #[test]
    fn other_threads_and_time_windows() {
        let p = SampledProfile::parse(&js_profile()).unwrap();
        let mut b = FlamegraphBuilder::new();
        assert_eq!(b.add(&p, Some("7"), None), 1);
        let g = b.build();
        assert_eq!(g.frames[g.root.children[0].frame_index.unwrap()].name, "<anonymous>");
        // Only the samples in the first 15 ms.
        let start = chrono::DateTime::parse_from_rfc3339("2026-11-14T10:00:00Z").unwrap().timestamp() as f64;
        let mut b = FlamegraphBuilder::new();
        assert_eq!(b.add(&p, None, Some((start, start + 0.015))), 2);
    }

    #[test]
    fn python_continuous_chunk() {
        // Version 2: absolute timestamps; no transaction thread, so the
        // thread named MainThread is chosen even though another has more samples.
        let chunk = json!({
            "version": "2", "platform": "python", "profiler_id": "p", "chunk_id": "c",
            "profile": {
                "frames": [
                    {"function": "<module>", "module": "__main__", "abs_path": "/srv/app/main.py", "lineno": 3, "in_app": true},
                    {"function": "handle", "module": "app.views", "abs_path": "/srv/app/app/views.py", "lineno": 10, "in_app": true},
                    {"function": "execute", "module": "sqlite3", "abs_path": "/usr/lib/python3.13/sqlite3/__init__.py", "lineno": 1},
                    {"function": "run", "module": "threading", "abs_path": "/usr/lib/python3.13/threading.py", "lineno": 1}
                ],
                "stacks": [[2, 1, 0], [1, 0], [3]],
                "samples": [
                    {"stack_id": 0, "thread_id": "140", "timestamp": 1794650000.01},
                    {"stack_id": 1, "thread_id": "140", "timestamp": 1794650000.02},
                    {"stack_id": 2, "thread_id": "141", "timestamp": 1794650000.01},
                    {"stack_id": 2, "thread_id": "141", "timestamp": 1794650000.02},
                    {"stack_id": 2, "thread_id": "141", "timestamp": 1794650000.03}
                ],
                "thread_metadata": {"140": {"name": "MainThread"}, "141": {"name": "Thread-1"}}
            }
        });
        let p = SampledProfile::parse(&chunk).unwrap();
        assert_eq!(p.default_thread().as_deref(), Some("140"));
        assert!(!p.frames[2].is_application, "standard library frames are not application code");
        let (from, to) = p.time_range().unwrap();
        assert!((to - from - 0.02).abs() < 1e-6);
        let mut b = FlamegraphBuilder::new();
        b.add(&p, None, None);
        let g = b.build();
        let root = &g.root.children[0];
        assert_eq!(g.frames[root.frame_index.unwrap()].name, "<module>");
        assert_eq!(root.children[0].total_weight, 2);
        assert_eq!(root.children[0].children[0].self_weight, 1);
    }

    #[test]
    fn php_profile_and_aggregation_across_profiles() {
        // The PHP SDK sends version 1 profiles with numeric thread ids.
        let php = |stacks: Value, samples: Value| {
            json!({
                "version": "1", "platform": "php", "timestamp": 1794650000.0,
                "transaction": {"name": "GET /orders", "active_thread_id": 0},
                "profile": {
                    "frames": [
                        {"function": "{main}", "filename": "index.php", "abs_path": "/srv/app/public/index.php", "lineno": 1},
                        {"function": "App\\Controller::list", "filename": "src/Controller.php", "abs_path": "/srv/app/src/Controller.php", "lineno": 20},
                        {"function": "PDO::query", "filename": "vendor/doctrine/dbal/src/Connection.php", "abs_path": "/srv/app/vendor/doctrine/dbal/src/Connection.php", "lineno": 5}
                    ],
                    "stacks": stacks, "samples": samples, "thread_metadata": {"0": {"name": "main"}}
                }
            })
        };
        let one = SampledProfile::parse(&php(
            json!([[2, 1, 0]]),
            json!([{"stack_id": 0, "thread_id": 0, "elapsed_since_start_ns": 0}]),
        ))
        .unwrap();
        assert_eq!(one.frames[2].package.as_deref(), Some("doctrine/dbal"));
        assert!(!one.frames[2].is_application && one.frames[1].is_application);
        let two = SampledProfile::parse(&php(
            json!([[1, 0], [2, 1, 0]]),
            json!([{"stack_id": 0, "thread_id": 0, "elapsed_since_start_ns": 0}, {"stack_id": 1, "thread_id": 0, "elapsed_since_start_ns": 10}]),
        ))
        .unwrap();
        let mut b = FlamegraphBuilder::new();
        b.add(&one, None, None);
        b.add(&two, None, None);
        let g = b.build();
        assert_eq!((g.samples, g.profiles), (3, 2));
        assert_eq!(g.frames.len(), 3, "identical frames of different profiles are merged");
        let ctrl = &g.root.children[0].children[0];
        assert_eq!((ctrl.total_weight, ctrl.self_weight), (3, 1));
        assert_eq!(ctrl.children[0].self_weight, 2);
    }

    #[test]
    fn malformed_profiles() {
        assert!(SampledProfile::parse(&json!({})).is_err());
        assert!(SampledProfile::parse(&json!({"profile": {"frames": []}})).is_err());
        // Out-of-range indices are ignored.
        let p = SampledProfile::parse(&json!({"profile": {"frames": [{"function": "f"}], "stacks": [[0, 9]],
            "samples": [{"stack_id": 0, "thread_id": 1, "elapsed_since_start_ns": 0}, {"stack_id": 5, "thread_id": 1}]}}))
        .unwrap();
        assert_eq!(p.samples.len(), 1);
        assert_eq!(p.stacks[0], vec![0]);
    }
}
