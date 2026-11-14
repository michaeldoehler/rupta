//! Server-side data scrubbing, applied to every event before it is stored.
//!
//! With scrubbing enabled (the default), values are replaced by
//! `[Filtered]` when
//!
//! - their key contains one of the default sensitive names (password,
//!   secret, token, auth, credentials, cookie, session, api key, private
//!   key, ...) or a project's extra field name, case-insensitively, unless
//!   the key is one of the project's safe fields;
//! - the value looks like a credit card number (13–19 digits passing the
//!   Luhn check), a bearer token or a private key block.
//!
//! Scrubbed places: request headers, cookies, query string, body and
//! environment, `extra`, contexts, user data, breadcrumb messages and
//! data, frame variables, tags and exception values (patterns only).
//! `Authorization` and `Cookie` headers and all cookie values are always
//! removed. With `scrub_ip_addresses`, IP addresses are removed from the
//! user and the request. See `docs/data-scrubbing.md`.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::event::Event;
use crate::settings::Scrubbing;

pub const FILTERED: &str = "[Filtered]";

/// Key fragments that mark a value as sensitive.
pub const DEFAULT_FIELDS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "auth",
    "credential",
    "cookie",
    "session",
    "csrf",
    "xsrf",
    "api_key",
    "apikey",
    "api-key",
    "private_key",
    "privatekey",
    "private-key",
    "access_key",
    "accesskey",
    "signature",
    "ssn",
    "credit_card",
    "creditcard",
    "card_number",
    "cardnumber",
    "cvv",
    "cvc",
    "iban",
];

/// Keys that contain a sensitive fragment but are harmless.
const DEFAULT_SAFE: &[&str] = &["session_id_hash", "author", "authority", "oauth_provider", "tokenizer"];

static CARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d[ \-]?){12,18}\d\b").unwrap());
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(bearer|basic|token)\s+[A-Za-z0-9\-._~+/]{8,}=*").unwrap());
static PRIVATE_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)-----BEGIN[A-Z ]*PRIVATE KEY-----.*?-----END[A-Z ]*PRIVATE KEY-----").unwrap());
static URL_PASSWORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\b[a-z][a-z0-9+.\-]*://[^:/@\s]+:)[^@/\s]+@").unwrap());

/// The scrubber for one project's settings.
pub struct Scrubber<'a> {
    settings: &'a Scrubbing,
    extra: Vec<String>,
    safe: Vec<String>,
}

impl<'a> Scrubber<'a> {
    pub fn new(settings: &'a Scrubbing) -> Scrubber<'a> {
        Scrubber {
            settings,
            extra: settings.extra_fields.iter().map(|s| s.to_ascii_lowercase()).filter(|s| !s.is_empty()).collect(),
            safe: settings.safe_fields.iter().map(|s| s.to_ascii_lowercase()).collect(),
        }
    }

    /// True if values under this key must be removed.
    pub fn is_sensitive_key(&self, key: &str) -> bool {
        let k = key.to_ascii_lowercase();
        if self.safe.contains(&k) || DEFAULT_SAFE.contains(&k.as_str()) {
            return false;
        }
        DEFAULT_FIELDS.iter().any(|f| k.contains(f)) || self.extra.iter().any(|f| k.contains(f.as_str()))
    }

    /// Replaces sensitive patterns inside a string. Returns `None` if
    /// nothing changed.
    pub fn scrub_str(&self, s: &str) -> Option<String> {
        let mut out = std::borrow::Cow::Borrowed(s);
        if s.len() >= 13 && s.bytes().filter(u8::is_ascii_digit).count() >= 13 {
            let replaced = CARD.replace_all(
                &out,
                |c: &regex::Captures<'_>| {
                    if luhn(&c[0]) { FILTERED.to_string() } else { c[0].to_string() }
                },
            );
            out = std::borrow::Cow::Owned(replaced.into_owned());
        }
        if out.len() > 12 {
            let lower = out.to_ascii_lowercase();
            if lower.contains("bearer") || lower.contains("basic") || lower.contains("token") {
                out = std::borrow::Cow::Owned(BEARER.replace_all(&out, format!("$1 {FILTERED}")).into_owned());
            }
            if lower.contains("private key") {
                out = std::borrow::Cow::Owned(PRIVATE_KEY.replace_all(&out, FILTERED).into_owned());
            }
            if out.contains("://") && out.contains('@') {
                out =
                    std::borrow::Cow::Owned(URL_PASSWORD.replace_all(&out, format!("${{1}}{FILTERED}@")).into_owned());
            }
        }
        match out {
            std::borrow::Cow::Owned(o) if o != s => Some(o),
            _ => None,
        }
    }

    /// Scrubs a JSON value: sensitive keys at any depth, patterns in strings.
    pub fn scrub_value(&self, v: &mut Value) {
        match v {
            Value::Object(map) => {
                for (k, val) in map.iter_mut() {
                    if self.is_sensitive_key(k) && !val.is_null() {
                        *val = Value::String(FILTERED.into());
                    } else {
                        self.scrub_value(val);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|x| self.scrub_value(x)),
            Value::String(s) => {
                if let Some(new) = self.scrub_str(s) {
                    *s = new;
                }
            }
            _ => {}
        }
    }

    fn scrub_pairs(&self, pairs: &mut [(String, String)]) {
        for (k, v) in pairs.iter_mut() {
            if self.is_sensitive_key(k) {
                *v = FILTERED.into();
            } else if let Some(new) = self.scrub_str(v) {
                *v = new;
            }
        }
    }

    fn scrub_query(&self, q: &str) -> String {
        q.split('&')
            .map(|part| match part.split_once('=') {
                Some((k, _)) if self.is_sensitive_key(k) => format!("{k}={FILTERED}"),
                Some((k, v)) => match self.scrub_str(v) {
                    Some(new) => format!("{k}={new}"),
                    None => part.to_string(),
                },
                None => part.to_string(),
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Scrubs an event in place.
    pub fn scrub(&self, ev: &mut Event) {
        if self.settings.scrub_ip_addresses {
            if let Some(u) = &mut ev.user {
                u.ip_address = None;
                if u.is_empty() {
                    ev.user = None;
                }
            }
            if let Some(r) = &mut ev.request {
                r.headers.retain(|(k, _)| {
                    !matches!(
                        k.to_ascii_lowercase().as_str(),
                        "x-forwarded-for" | "x-real-ip" | "forwarded" | "cf-connecting-ip"
                    )
                });
                if let Some(env) = r.env.as_mut().and_then(Value::as_object_mut) {
                    env.remove("REMOTE_ADDR");
                }
            }
            ev.tags.retain(|(k, v)| !(k == "user" && v.starts_with("ip:")));
        }
        if !self.settings.enabled {
            return;
        }
        if let Some(r) = &mut ev.request {
            for (k, v) in r.headers.iter_mut() {
                let lk = k.to_ascii_lowercase();
                if lk == "authorization" || lk == "cookie" || lk == "set-cookie" || lk == "proxy-authorization" {
                    *v = FILTERED.into();
                }
            }
            self.scrub_pairs(&mut r.headers);
            for (_, v) in r.cookies.iter_mut() {
                *v = FILTERED.into();
            }
            if let Some(q) = &r.query_string {
                r.query_string = Some(self.scrub_query(q));
            }
            if let Some(url) = &r.url
                && let Some((base, q)) = url.split_once('?')
            {
                r.url = Some(format!("{base}?{}", self.scrub_query(q)));
            }
            if let Some(d) = &mut r.data {
                if let Value::String(s) = d
                    && s.contains('=')
                    && !s.trim_start().starts_with(['{', '['])
                {
                    *s = self.scrub_query(s);
                } else {
                    self.scrub_value(d);
                }
            }
            if let Some(e) = &mut r.env {
                self.scrub_value(e);
            }
        }
        let mut extra = Value::Object(std::mem::take(&mut ev.extra));
        self.scrub_value(&mut extra);
        if let Value::Object(m) = extra {
            ev.extra = m;
        }
        let mut contexts = Value::Object(std::mem::take(&mut ev.contexts));
        self.scrub_value(&mut contexts);
        if let Value::Object(m) = contexts {
            ev.contexts = m;
        }
        if let Some(u) = &mut ev.user
            && let Some(d) = &mut u.data
        {
            self.scrub_value(d);
        }
        if let Some(b) = &mut ev.breadcrumbs {
            for c in &mut b.values {
                if let Some(m) = &c.message
                    && let Some(new) = self.scrub_str(m)
                {
                    c.message = Some(new);
                }
                if let Some(d) = &mut c.data {
                    self.scrub_value(d);
                }
            }
        }
        if let Some(m) = &ev.message
            && let Some(new) = self.scrub_str(m)
        {
            ev.message = Some(new);
        }
        if let Some(ex) = &mut ev.exception {
            for e in &mut ex.values {
                if let Some(v) = &e.value
                    && let Some(new) = self.scrub_str(v)
                {
                    e.value = Some(new);
                }
            }
        }
        for st in ev.stacktraces_mut() {
            for f in &mut st.frames {
                if let Some(v) = &mut f.vars {
                    self.scrub_value(v);
                }
            }
        }
        self.scrub_pairs(&mut ev.tags);
    }
}

/// Luhn checksum of the digits in `s`.
fn luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{Context, normalize};
    use crate::settings::ProjectSettings;
    use chrono::Utc;
    use serde_json::json;

    fn ev(v: Value, s: &ProjectSettings) -> Event {
        let mut e = normalize(
            &v,
            &Context {
                project_id: 1,
                event_id: "0".repeat(32),
                received: Utc::now(),
                client_ip: Some("198.51.100.1".parse().unwrap()),
                user_agent: None,
                settings: s,
            },
        );
        Scrubber::new(&s.scrubbing).scrub(&mut e);
        e
    }

    #[test]
    fn default_fields_and_patterns() {
        let s = ProjectSettings::default();
        let e = ev(
            json!({
                "request": {"url": "https://x/login?user=a&password=hunter2", "headers": {"Authorization": "Bearer abc", "X-Api-Key": "k", "Accept": "json"},
                    "cookies": {"sid": "1"}, "data": {"user": {"password": "p", "name": "ann"}, "card": "4111 1111 1111 1111"},
                    "query_string": "token=t&q=1"},
                "extra": {"db_url": "postgres://app:s3cret@db/app", "note": "Authorization: Bearer abcdefghijkl", "count": 3},
                "breadcrumbs": [{"message": "paid with 4242424242424242", "data": {"secret_value": "x"}}],
                "exception": [{"type": "E", "value": "card 4111111111111111 declined", "stacktrace": {"frames": [{"function": "f", "vars": {"passwd": "x", "ok": "y"}}]}}],
                "tags": {"session_token": "abc", "plain": "1"}
            }),
            &s,
        );
        let r = e.request.as_ref().unwrap();
        assert_eq!(r.header("authorization"), Some(FILTERED));
        assert_eq!(r.header("x-api-key"), Some(FILTERED));
        assert_eq!(r.header("accept"), Some("json"));
        assert_eq!(r.cookies[0].1, FILTERED);
        assert_eq!(r.url.as_deref(), Some("https://x/login?user=a&password=[Filtered]"));
        assert_eq!(r.query_string.as_deref(), Some("token=[Filtered]&q=1"));
        let d = r.data.as_ref().unwrap();
        assert_eq!(d["user"]["password"], FILTERED);
        assert_eq!(d["user"]["name"], "ann");
        assert_eq!(d["card"], FILTERED);
        assert_eq!(e.extra["db_url"], "postgres://app:[Filtered]@db/app");
        assert_eq!(e.extra["note"], "Authorization: Bearer [Filtered]");
        assert_eq!(e.extra["count"], 3);
        let b = &e.breadcrumbs.as_ref().unwrap().values[0];
        assert_eq!(b.message.as_deref(), Some("paid with [Filtered]"));
        assert_eq!(b.data.as_ref().unwrap()["secret_value"], FILTERED);
        let ex = e.main_exception().unwrap();
        assert_eq!(ex.value.as_deref(), Some("card [Filtered] declined"));
        let vars = ex.stacktrace.as_ref().unwrap().frames[0].vars.as_ref().unwrap();
        assert_eq!((vars["passwd"].as_str(), vars["ok"].as_str()), (Some(FILTERED), Some("y")));
        assert_eq!(e.tag("session_token"), Some(FILTERED));
        assert_eq!(e.tag("plain"), Some("1"));
    }

    #[test]
    fn non_luhn_numbers_survive() {
        let s = Scrubbing::default();
        assert_eq!(Scrubber::new(&s).scrub_str("order 1234567890123 shipped"), None);
    }

    #[test]
    fn project_rules() {
        let mut s = ProjectSettings::default();
        s.scrubbing.extra_fields = vec!["iban_code".into(), "Phone".into()];
        s.scrubbing.safe_fields = vec!["auth_method".into()];
        s.scrubbing.scrub_ip_addresses = true;
        let e = ev(
            json!({"extra": {"phone_number": "1", "auth_method": "sso", "auth": "x"}, "user": {"id": "1", "ip_address": "{{auto}}"},
                   "request": {"headers": [["X-Forwarded-For", "1.2.3.4"]]}}),
            &s,
        );
        assert_eq!(e.extra["phone_number"], FILTERED);
        assert_eq!(e.extra["auth_method"], "sso");
        assert_eq!(e.extra["auth"], FILTERED);
        assert!(e.user.as_ref().unwrap().ip_address.is_none());
        assert!(e.request.as_ref().unwrap().header("x-forwarded-for").is_none());
    }

    #[test]
    fn disabled_scrubbing_keeps_values() {
        let mut s = ProjectSettings::default();
        s.scrubbing.enabled = false;
        let e = ev(json!({"extra": {"password": "p"}, "user": {"ip_address": "{{auto}}"}}), &s);
        assert_eq!(e.extra["password"], "p");
        assert_eq!(e.user.unwrap().ip_address.as_deref(), Some("198.51.100.1"));
    }
}
