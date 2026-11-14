//! The normalized event: what ingestion stores (compressed JSON in
//! `event_payloads`) and the API returns unchanged.
//!
//! Every top-level key is always present (`null` when unknown) so clients
//! can rely on the shape. Inside nested objects, absent values are left
//! out to keep payloads small.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ids::Id;

/// A JSON object.
pub type Object = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 32 lowercase hex digits, as the SDKs send it.
    pub event_id: String,
    pub project_id: Id,
    pub issue_id: Option<Id>,
    /// `error` (has an exception), `default` (a message), `csp`
    /// (a security report).
    #[serde(rename = "type")]
    pub ty: String,
    pub timestamp: DateTime<Utc>,
    pub received: DateTime<Utc>,
    pub level: String,
    pub platform: String,
    pub message: Option<String>,
    pub title: String,
    pub culprit: Option<String>,
    pub logger: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub dist: Option<String>,
    pub server_name: Option<String>,
    pub transaction: Option<String>,
    pub fingerprint: Vec<String>,
    /// Ordered key/value pairs, including tags derived from the event
    /// (level, environment, release, browser, os, ...).
    pub tags: Vec<(String, String)>,
    pub user: Option<User>,
    pub request: Option<Request>,
    pub contexts: Object,
    pub sdk: Option<Sdk>,
    pub exception: Option<Values<Exception>>,
    pub threads: Option<Values<Thread>>,
    pub breadcrumbs: Option<Values<Breadcrumb>>,
    pub extra: Object,
    pub modules: Object,
    pub debug_meta: Option<Value>,
    /// The violation of a `csp` event.
    pub csp: Option<Value>,
    /// Grouping information: the hash and what it was computed from.
    pub grouping: Option<GroupingInfo>,
    /// Problems found while processing the event.
    pub errors: Vec<ProcessingError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Values<T> {
    pub values: Vec<T>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct User {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl User {
    pub fn is_empty(&self) -> bool {
        self == &User::default()
    }

    /// A stable identity for counting distinct users: id, then email,
    /// username, IP address.
    pub fn identity(&self) -> Option<String> {
        if let Some(v) = &self.id {
            return Some(format!("id:{v}"));
        }
        if let Some(v) = &self.email {
            return Some(format!("email:{v}"));
        }
        if let Some(v) = &self.username {
            return Some(format!("username:{v}"));
        }
        self.ip_address.as_ref().map(|v| format!("ip:{v}"))
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cookies: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Value>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sdk {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Exception {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanism: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<Stacktrace>,
}

impl Exception {
    /// `false` if the mechanism says the exception was not handled.
    pub fn handled(&self) -> Option<bool> {
        self.mechanism.as_ref()?.get("handled")?.as_bool()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Stacktrace {
    /// Oldest call first; the frame that raised is last.
    pub frames: Vec<Frame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registers: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Frame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abs_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineno: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colno: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_app: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_line: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_addr: Option<String>,
    /// Set when the location was mapped back to original source.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub symbolicated: bool,
    /// The frame as the SDK sent it, before symbolication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Box<Frame>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Thread {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crashed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<Stacktrace>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Breadcrumb {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessingError {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

impl ProcessingError {
    pub fn new(ty: &str, name: impl Into<Option<String>>, value: impl Into<Option<Value>>) -> ProcessingError {
        ProcessingError { ty: ty.to_string(), name: name.into(), value: value.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupingInfo {
    /// Hex xxh3-128 hash identifying the issue within the project.
    pub hash: String,
    /// The grouping configuration that produced the hash.
    pub version: String,
    /// What the hash was computed from: `fingerprint`, `exception`,
    /// `stacktrace`, `message`, `csp` or `fallback`.
    pub method: String,
    /// The strings hashed, in order (useful to explain grouping).
    pub components: Vec<String>,
}

impl Event {
    /// An empty event, filled in by normalization.
    pub fn empty(event_id: String, project_id: i64, received: DateTime<Utc>) -> Event {
        Event {
            event_id,
            project_id: Id(project_id),
            issue_id: None,
            ty: "default".into(),
            timestamp: received,
            received,
            level: "error".into(),
            platform: "other".into(),
            message: None,
            title: String::new(),
            culprit: None,
            logger: None,
            environment: None,
            release: None,
            dist: None,
            server_name: None,
            transaction: None,
            fingerprint: Vec::new(),
            tags: Vec::new(),
            user: None,
            request: None,
            contexts: Object::new(),
            sdk: None,
            exception: None,
            threads: None,
            breadcrumbs: None,
            extra: Object::new(),
            modules: Object::new(),
            debug_meta: None,
            csp: None,
            grouping: None,
            errors: Vec::new(),
        }
    }

    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The exception values, innermost cause first as sent (the last value
    /// is the one that was raised).
    pub fn exceptions(&self) -> &[Exception] {
        self.exception.as_ref().map(|e| e.values.as_slice()).unwrap_or(&[])
    }

    /// The exception that was raised (the last one in the chain).
    pub fn main_exception(&self) -> Option<&Exception> {
        self.exceptions().last()
    }

    /// Every stack trace of the event, mutable (for symbolication).
    pub fn stacktraces_mut(&mut self) -> Vec<&mut Stacktrace> {
        let mut out = Vec::new();
        if let Some(e) = &mut self.exception {
            out.extend(e.values.iter_mut().filter_map(|v| v.stacktrace.as_mut()));
        }
        if let Some(t) = &mut self.threads {
            out.extend(t.values.iter_mut().filter_map(|v| v.stacktrace.as_mut()));
        }
        out
    }

    /// `false` if any exception's mechanism says it was not handled.
    pub fn handled(&self) -> Option<bool> {
        let mut seen = None;
        for e in self.exceptions() {
            match e.handled() {
                Some(false) => return Some(false),
                Some(true) => seen = Some(true),
                None => {}
            }
        }
        seen
    }

    /// Name and version of a context (`os`, `browser`, `runtime`, `device`)
    /// as one string, e.g. `Chrome 120.0`.
    pub fn context_summary(&self, key: &str) -> Option<String> {
        let c = self.contexts.get(key)?.as_object()?;
        let name = c.get("name").and_then(Value::as_str).or_else(|| {
            if key == "device" { c.get("model").or(c.get("family")).and_then(Value::as_str) } else { None }
        })?;
        match c.get("version").and_then(Value::as_str) {
            Some(v) if !v.is_empty() => Some(format!("{name} {v}")),
            _ => Some(name.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_top_level_key_is_present() {
        let e = Event::empty("a".repeat(32), 1, Utc::now());
        let v = serde_json::to_value(&e).unwrap();
        for key in [
            "event_id",
            "project_id",
            "issue_id",
            "type",
            "timestamp",
            "received",
            "level",
            "platform",
            "message",
            "title",
            "culprit",
            "logger",
            "environment",
            "release",
            "dist",
            "server_name",
            "transaction",
            "fingerprint",
            "tags",
            "user",
            "request",
            "contexts",
            "sdk",
            "exception",
            "threads",
            "breadcrumbs",
            "extra",
            "modules",
            "debug_meta",
            "errors",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v["project_id"], "1");
        let back: Event = serde_json::from_value(v).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn user_identity_prefers_id() {
        let u = User { id: Some("7".into()), email: Some("a@b.c".into()), ..User::default() };
        assert_eq!(u.identity().as_deref(), Some("id:7"));
        let u = User { ip_address: Some("1.2.3.4".into()), ..User::default() };
        assert_eq!(u.identity().as_deref(), Some("ip:1.2.3.4"));
        assert!(User::default().identity().is_none());
    }
}
