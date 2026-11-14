//! Inbound filters: events a project chose not to store.

use rupta_core::event::Event;
use rupta_core::normalize::parse_user_agent;
use rupta_core::settings::InboundFilters;

/// Messages and paths typical of errors caused by browser extensions.
const EXTENSION_MESSAGES: &[&str] = &[
    "top.GLOBALS",
    "originalCreateNotification",
    "canvas.contentDocument",
    "MyApp_RemoveAllHighlights",
    "http://tt.epicplay.com",
    "Can't find variable: ZiteReader",
    "jigsaw is not defined",
    "ComboSearch is not defined",
    "http://loading.retry.widdit.com/",
    "atomicFindClose",
    "fb_xd_fragment",
    "bmi_SafeAddOnload",
    "EBCallBackMessageReceived",
    "conduitPage",
    "null is not an object (evaluating 'elt.parentNode')",
    "plugin.setSuspendState is not a function",
    "Extension context invalidated",
    "webkit-masked-url:",
];

const EXTENSION_PATHS: &[&str] = &[
    "chrome-extension://",
    "moz-extension://",
    "safari-extension://",
    "safari-web-extension://",
    "ms-browser-extension://",
    "chrome://",
    "resource://",
    "webkit-masked-url://",
    "/graphics/ad.gif",
    "googletagmanager.com/gtm.js",
];

/// The reason an event is filtered, or `None` to keep it.
pub fn filter_reason(ev: &Event, filters: &InboundFilters, request_user_agent: Option<&str>) -> Option<&'static str> {
    if let Some(release) = &ev.release
        && filters.releases.iter().any(|p| glob(p, release))
    {
        return Some("release-version");
    }
    if !filters.error_messages.is_empty() {
        let mut texts: Vec<String> = Vec::new();
        if let Some(e) = ev.main_exception() {
            let ty = e.ty.clone().unwrap_or_default();
            match &e.value {
                Some(v) => {
                    texts.push(format!("{ty}: {v}"));
                    texts.push(v.clone());
                }
                None => texts.push(ty),
            }
        }
        if let Some(m) = &ev.message {
            texts.push(m.clone());
        }
        if texts.iter().any(|t| filters.error_messages.iter().any(|p| glob(p, t))) {
            return Some("error-message");
        }
    }
    let ua = ev.request.as_ref().and_then(|r| r.header("user-agent")).or(request_user_agent);
    if filters.web_crawlers
        && let Some(ua) = ua
        && parse_user_agent(ua).is_bot
    {
        return Some("web-crawlers");
    }
    if filters.legacy_browsers
        && let Some(ua) = ua
        && is_legacy_browser(ua)
    {
        return Some("legacy-browsers");
    }
    if filters.localhost && is_localhost(ev) {
        return Some("localhost");
    }
    if filters.browser_extensions && is_browser_extension(ev) {
        return Some("browser-extensions");
    }
    None
}

fn major(version: &str) -> Option<u32> {
    version.split('.').next()?.parse().ok()
}

/// Browsers without current security support: Internet Explorer, and
/// Chrome < 80, Firefox < 78, Safari < 13, Edge (legacy) < 79, Opera < 67.
pub fn is_legacy_browser(ua: &str) -> bool {
    let parsed = parse_user_agent(ua);
    let Some((name, version)) = parsed.browser else { return false };
    let Some(v) = major(&version) else { return name == "IE" };
    match name.as_str() {
        "IE" => true,
        "Chrome" | "Chrome Mobile" => v < 80,
        "Firefox" => v < 78,
        "Safari" | "Mobile Safari" => v < 13,
        "Edge" => v < 79,
        "Opera" => v < 67,
        _ => false,
    }
}

fn is_localhost(ev: &Event) -> bool {
    let ip = ev.user.as_ref().and_then(|u| u.ip_address.as_deref());
    if matches!(ip, Some("127.0.0.1") | Some("::1")) {
        return true;
    }
    let Some(url) = ev.request.as_ref().and_then(|r| r.url.as_deref()) else { return false };
    let host = url.split_once("://").map_or(url, |(_, r)| r).split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or("")
    };
    host == "localhost" || host.ends_with(".localhost") || host == "127.0.0.1" || host == "::1"
}

fn is_browser_extension(ev: &Event) -> bool {
    if let Some(e) = ev.main_exception() {
        let text = format!("{} {}", e.ty.as_deref().unwrap_or(""), e.value.as_deref().unwrap_or(""));
        if EXTENSION_MESSAGES.iter().any(|m| text.contains(m)) {
            return true;
        }
        if let Some(st) = &e.stacktrace {
            // The frame that raised decides.
            if let Some(f) = st.frames.last() {
                let path = f.abs_path.as_deref().or(f.filename.as_deref()).unwrap_or("");
                if EXTENSION_PATHS.iter().any(|p| path.contains(p)) {
                    return true;
                }
            }
        }
    }
    false
}

/// `*` matches any run of characters; case-insensitive.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rupta_core::normalize::{Context, normalize};
    use rupta_core::settings::ProjectSettings;
    use serde_json::json;

    fn ev(v: serde_json::Value) -> Event {
        let s = ProjectSettings::default();
        normalize(
            &v,
            &Context {
                project_id: 1,
                event_id: "0".repeat(32),
                received: Utc::now(),
                client_ip: None,
                user_agent: None,
                settings: &s,
            },
        )
    }

    #[test]
    fn globs() {
        assert!(glob("*ResizeObserver*", "Error: resizeobserver loop limit exceeded"));
        assert!(glob("1.0.*", "1.0.3"));
        assert!(!glob("1.0.*", "1.1.0"));
        assert!(glob("a*b*c", "aXXbYYc"));
        assert!(!glob("a*b*c", "aXXbYY"));
    }

    #[test]
    fn each_filter() {
        let all = InboundFilters {
            browser_extensions: true,
            localhost: true,
            legacy_browsers: true,
            web_crawlers: true,
            error_messages: vec!["*ResizeObserver loop*".into()],
            releases: vec!["*-beta".into()],
        };
        let none = InboundFilters::default();
        let cases = [
            (json!({"release": "2.0-beta"}), "release-version"),
            (json!({"exception": [{"type": "Error", "value": "ResizeObserver loop limit exceeded"}]}), "error-message"),
            (
                json!({"request": {"headers": {"User-Agent": "Googlebot/2.1 (+http://www.google.com/bot.html)"}}}),
                "web-crawlers",
            ),
            (
                json!({"request": {"headers": {"User-Agent": "Mozilla/5.0 (Windows NT 6.1; Trident/7.0; rv:11.0) like Gecko"}}}),
                "legacy-browsers",
            ),
            (json!({"request": {"url": "http://localhost:3000/x"}}), "localhost"),
            (
                json!({"exception": [{"type": "TypeError", "value": "x", "stacktrace": {"frames": [{"abs_path": "chrome-extension://abc/content.js"}]}}]}),
                "browser-extensions",
            ),
        ];
        for (v, reason) in cases {
            let e = ev(v);
            assert_eq!(filter_reason(&e, &all, None), Some(reason));
            assert_eq!(filter_reason(&e, &none, None), None);
        }
        let modern = ev(
            json!({"request": {"url": "https://app.example.com", "headers": {"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"}}}),
        );
        assert_eq!(filter_reason(&modern, &all, None), None);
    }
}
