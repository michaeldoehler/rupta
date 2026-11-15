//! The icon of an organization, taken from the favicon of its website.
//!
//! The start page of the domain is fetched over HTTPS with the outbound
//! client of the integrations (internal hosts refused, size and time
//! limits). Redirects are followed by hand, at most five, each target
//! checked again. The best `<link rel="…icon">` of the page wins (an
//! `apple-touch-icon` or the largest `sizes`, SVG before bitmaps), and
//! `/favicon.ico` is the fallback. The image is accepted only if its bytes
//! are a PNG, ICO, GIF, JPEG, WebP or SVG file of at most 256 KiB.

use reqwest::Method;

use crate::integrations::http::{Outbound, Req, Resp};

/// Largest icon kept.
pub const MAX_ICON_BYTES: usize = 256 << 10;
const MAX_REDIRECTS: usize = 5;

/// The host of a domain as typed: `revenexx.com`, `https://www.revenexx.com/`.
pub fn normalize_domain(input: &str) -> Result<String, String> {
    let t = input.trim();
    let with_scheme = if t.contains("://") { t.to_string() } else { format!("https://{t}") };
    let url = url::Url::parse(&with_scheme).map_err(|_| format!("{t:?} is not a domain"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{t:?} is not a domain"));
    }
    match url.host() {
        Some(url::Host::Domain(d)) if d.contains('.') && d.len() <= 253 => {
            Ok(d.trim_end_matches('.').to_ascii_lowercase())
        }
        _ => Err(format!("{t:?} is not a domain name (for example example.com)")),
    }
}

/// Fetches the icon of `domain`: the image and its media type.
pub async fn fetch(outbound: &Outbound, domain: &str) -> Result<(Vec<u8>, &'static str), String> {
    let home = url::Url::parse(&format!("https://{domain}/")).map_err(|e| e.to_string())?;
    fetch_from(outbound, home, domain).await
}

/// Fetches the icon of the site whose start page is `home`; `name` names
/// the site in errors.
pub async fn fetch_from(outbound: &Outbound, home: url::Url, name: &str) -> Result<(Vec<u8>, &'static str), String> {
    let domain = name;
    let mut candidates = Vec::new();
    let mut base = home.clone();
    match get(outbound, home.clone()).await {
        Ok((resp, at)) if resp.status == 200 => {
            base = at;
            candidates = icon_links(&String::from_utf8_lossy(&resp.body[..resp.body.len().min(512 << 10)]), &base);
        }
        Ok(_) | Err(_) => {}
    }
    if let Ok(ico) = base.join("/favicon.ico") {
        candidates.push(ico);
    }
    let mut last = format!("no icon found on {domain}");
    for url in candidates {
        match get(outbound, url.clone()).await {
            Ok((resp, _)) if resp.status == 200 => match sniff(&resp.body) {
                Some(_) if resp.body.len() > MAX_ICON_BYTES => {
                    last = format!("the icon at {url} is larger than 256 KiB")
                }
                Some(kind) => return Ok((resp.body, kind)),
                None => last = format!("{url} is not an image"),
            },
            Ok((resp, _)) => last = format!("{url} answered {}", resp.status),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// GET with redirects followed by hand; returns the answer and its URL.
async fn get(outbound: &Outbound, mut url: url::Url) -> Result<(Resp, url::Url), String> {
    for _ in 0..=MAX_REDIRECTS {
        let req = Req::new(Method::GET, url.clone()).header("accept", "text/html,image/*;q=0.9,*/*;q=0.5");
        let resp = outbound.send(req).await.map_err(|e| format!("{}: {e}", url.host_str().unwrap_or_default()))?;
        if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
            let next = resp.header("location").ok_or_else(|| format!("{url} redirects without a location"))?;
            let next = url.join(next).map_err(|_| format!("{url} redirects to an invalid location"))?;
            if !matches!(next.scheme(), "http" | "https") {
                return Err(format!("{url} redirects to {next}"));
            }
            url = next;
            continue;
        }
        return Ok((resp, url));
    }
    Err(format!("too many redirects from {url}"))
}

/// The icons a page links to, best first.
fn icon_links(html: &str, base: &url::Url) -> Vec<url::Url> {
    let mut found: Vec<(i64, url::Url)> = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("<link") {
        let start = at + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &html[start..end];
        at = end;
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        if !rel
            .split_whitespace()
            .any(|r| r == "icon" || r == "apple-touch-icon" || r == "apple-touch-icon-precomposed")
        {
            continue;
        }
        let Some(href) = attr(tag, "href") else { continue };
        let Ok(url) = base.join(href.trim()) else { continue };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        let ty = attr(tag, "type").unwrap_or_default().to_ascii_lowercase();
        let size = attr(tag, "sizes")
            .and_then(|s| s.split_whitespace().filter_map(|p| p.split(['x', 'X']).next()?.parse::<i64>().ok()).max())
            .unwrap_or(0);
        let score = if ty.contains("svg") || url.path().ends_with(".svg") {
            10_000
        } else if rel.contains("apple-touch-icon") {
            5_000 + size.max(180)
        } else {
            size.max(16)
        };
        found.push((score, url));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, u)| u).collect()
}

/// The value of attribute `name` in a tag, quoted or not.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let s = from + i;
        from = s + name.len();
        let before_ok = s > 0 && lower.as_bytes()[s - 1].is_ascii_whitespace();
        let rest = lower[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let vstart = tag.len() - tag[from..].trim_start().len() + 1;
        let v = tag[vstart..].trim_start();
        return Some(match v.chars().next()? {
            q @ ('"' | '\'') => v[1..].split(q).next()?,
            _ => v.split(|c: char| c.is_whitespace() || c == '/').next()?,
        });
    }
    None
}

/// The media type of an image, judged by its bytes.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(1024)]).to_ascii_lowercase();
        let t = head.trim_start_matches('\u{feff}').trim_start();
        ((t.starts_with("<svg") || t.starts_with("<?xml")) && head.contains("<svg")).then_some("image/svg+xml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains() {
        assert_eq!(normalize_domain("revenexx.com").unwrap(), "revenexx.com");
        assert_eq!(normalize_domain(" https://WWW.Revenexx.com/about ").unwrap(), "www.revenexx.com");
        assert!(normalize_domain("localhost").is_err());
        assert!(normalize_domain("10.0.0.1").is_err());
        assert!(normalize_domain("ftp://example.com").is_err());
        assert!(normalize_domain("not a domain").is_err());
    }

    #[test]
    fn links_best_first() {
        let base = url::Url::parse("https://www.example.com/de/").unwrap();
        let html = r#"<html><head>
            <link rel="stylesheet" href="/x.css">
            <LINK REL="shortcut icon" href="/favicon-16.png" sizes="16x16">
            <link href='icons/32.png' rel=icon sizes=32x32>
            <link rel="apple-touch-icon" href="https://cdn.example.com/apple.png">
            <link rel="icon" type="image/svg+xml" href="/logo.svg">
            <link rel="icon" href="javascript:alert(1)">
        </head></html>"#;
        let got: Vec<String> = icon_links(html, &base).into_iter().map(|u| u.to_string()).collect();
        assert_eq!(
            got,
            [
                "https://www.example.com/logo.svg",
                "https://cdn.example.com/apple.png",
                "https://www.example.com/de/icons/32.png",
                "https://www.example.com/favicon-16.png"
            ]
        );
    }

    #[test]
    fn sniffing() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"), Some("image/svg+xml"));
        assert_eq!(sniff(b"<!doctype html><html>"), None);
        assert_eq!(sniff(b""), None);
    }
}
