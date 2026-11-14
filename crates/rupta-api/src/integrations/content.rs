//! What rupta writes into tickets: a small document model rendered as
//! Markdown (GitHub, GitLab, Linear), Atlassian Document Format (Jira
//! Cloud, REST v3) or Jira wiki markup (Jira Data Center, REST v2), and
//! the ticket generated from an issue.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// Inline content.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Bold(String),
    Code(String),
    Link { text: String, url: String },
}

/// A block.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading(String),
    Paragraph(Vec<Inline>),
    Bullets(Vec<Vec<Inline>>),
    Code { language: Option<String>, text: String },
}

/// A document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Doc(pub Vec<Block>);

impl Doc {
    /// Plain text, paragraphs separated by blank lines.
    pub fn from_text(text: &str) -> Doc {
        Doc(text
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| Block::Paragraph(vec![Inline::Text(p.to_string())]))
            .collect())
    }

    pub fn markdown(&self) -> String {
        let mut out = Vec::new();
        for b in &self.0 {
            out.push(match b {
                Block::Heading(h) => format!("### {}", md_escape(h)),
                Block::Paragraph(inl) => md_inlines(inl),
                Block::Bullets(items) => {
                    items.iter().map(|i| format!("- {}", md_inlines(i))).collect::<Vec<_>>().join("\n")
                }
                Block::Code { language, text } => {
                    let fence = if text.contains("```") { "~~~~" } else { "```" };
                    format!("{fence}{}\n{}\n{fence}", language.as_deref().unwrap_or(""), text.trim_end())
                }
            });
        }
        out.join("\n\n")
    }

    /// Atlassian Document Format (version 1).
    pub fn adf(&self) -> Value {
        let mut content = Vec::new();
        for b in &self.0 {
            match b {
                Block::Heading(h) if !h.is_empty() => content
                    .push(json!({"type": "heading", "attrs": {"level": 3}, "content": [{"type": "text", "text": h}]})),
                Block::Heading(_) => {}
                Block::Paragraph(inl) => {
                    let c = adf_inlines(inl);
                    if !c.is_empty() {
                        content.push(json!({"type": "paragraph", "content": c}));
                    }
                }
                Block::Bullets(items) => {
                    let items: Vec<Value> = items
                        .iter()
                        .map(|i| adf_inlines(i))
                        .filter(|c| !c.is_empty())
                        .map(|c| json!({"type": "listItem", "content": [{"type": "paragraph", "content": c}]}))
                        .collect();
                    if !items.is_empty() {
                        content.push(json!({"type": "bulletList", "content": items}));
                    }
                }
                Block::Code { language, text } if !text.is_empty() => {
                    let mut node = json!({"type": "codeBlock", "content": [{"type": "text", "text": text.trim_end()}]});
                    if let Some(l) = language {
                        node["attrs"] = json!({"language": l});
                    }
                    content.push(node);
                }
                Block::Code { .. } => {}
            }
        }
        json!({"version": 1, "type": "doc", "content": content})
    }

    /// Jira wiki markup.
    pub fn wiki(&self) -> String {
        let mut out = Vec::new();
        for b in &self.0 {
            out.push(match b {
                Block::Heading(h) => format!("h3. {}", wiki_escape(h)),
                Block::Paragraph(inl) => wiki_inlines(inl),
                Block::Bullets(items) => {
                    items.iter().map(|i| format!("* {}", wiki_inlines(i))).collect::<Vec<_>>().join("\n")
                }
                Block::Code { text, .. } => {
                    format!("{{noformat}}\n{}\n{{noformat}}", text.trim_end().replace("{noformat}", ""))
                }
            });
        }
        out.join("\n\n")
    }
}

fn md_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

fn md_code(s: &str) -> String {
    let s = s.replace('\n', " ");
    if s.contains('`') { format!("`` {s} ``") } else { format!("`{s}`") }
}

fn md_inlines(inl: &[Inline]) -> String {
    inl.iter()
        .map(|i| match i {
            Inline::Text(t) => md_escape(t),
            Inline::Bold(t) => format!("**{}**", md_escape(t)),
            Inline::Code(t) => md_code(t),
            Inline::Link { text, url } => {
                format!("[{}]({})", md_escape(text), url.replace(')', "%29").replace(' ', "%20"))
            }
        })
        .collect()
}

fn adf_inlines(inl: &[Inline]) -> Vec<Value> {
    inl.iter()
        .filter_map(|i| {
            let (text, marks) = match i {
                Inline::Text(t) => (t, json!([])),
                Inline::Bold(t) => (t, json!([{"type": "strong"}])),
                Inline::Code(t) => (t, json!([{"type": "code"}])),
                Inline::Link { text, url } => (text, json!([{"type": "link", "attrs": {"href": url}}])),
            };
            if text.is_empty() {
                return None;
            }
            let mut node = json!({"type": "text", "text": text});
            if marks.as_array().is_some_and(|m| !m.is_empty()) {
                node["marks"] = marks;
            }
            Some(node)
        })
        .collect()
}

fn wiki_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '{' | '}' | '[' | ']' | '*' | '_' | '|' | '^' | '~' | '+' | '-' | '?' | '!') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

fn wiki_inlines(inl: &[Inline]) -> String {
    inl.iter()
        .map(|i| match i {
            Inline::Text(t) => wiki_escape(t),
            Inline::Bold(t) => format!("*{}*", wiki_escape(t)),
            Inline::Code(t) => format!("{{{{{}}}}}", t.replace("}}", "} }").replace('\n', " ")),
            Inline::Link { text, url } => {
                format!("[{}|{}]", wiki_escape(text).replace('|', ""), url.replace('|', "%7C"))
            }
        })
        .collect()
}

/// A stack frame for the ticket.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub function: Option<String>,
    pub module: Option<String>,
    pub filename: Option<String>,
    pub lineno: Option<i64>,
    pub colno: Option<i64>,
    pub context_line: Option<String>,
}

/// What a generated ticket is made of.
#[derive(Debug, Clone, Default)]
pub struct IssueSummary {
    pub short_id: String,
    pub title: String,
    pub error_type: Option<String>,
    pub error_value: Option<String>,
    pub culprit: Option<String>,
    pub level: String,
    pub platform: Option<String>,
    pub project: String,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub count: i64,
    pub user_count: i64,
    pub first_release: Option<String>,
    pub last_release: Option<String>,
    /// `(key, [(value, share in percent)])`, most frequent first.
    pub top_tags: Vec<(String, Vec<(String, i64)>)>,
    /// In-app frames of the latest event, the crashing frame first.
    pub frames: Vec<Frame>,
    /// Link to the issue in rupta.
    pub url: String,
}

/// Frames shown in a ticket at most.
pub const MAX_FRAMES: usize = 10;

fn ts(t: &Option<DateTime<Utc>>) -> String {
    t.map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()).unwrap_or_else(|| "—".into())
}

fn frame_line(f: &Frame) -> String {
    let func = f.function.as_deref().unwrap_or("?");
    let loc = f.filename.as_deref().or(f.module.as_deref()).unwrap_or("?");
    let mut s = format!("at {func} ({loc}");
    if let Some(l) = f.lineno {
        s.push_str(&format!(":{l}"));
        if let Some(c) = f.colno {
            s.push_str(&format!(":{c}"));
        }
    }
    s.push(')');
    if let Some(ctx) = f.context_line.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        s.push_str(&format!("\n    {}", super::truncate(ctx, 200)));
    }
    s
}

/// The ticket title: `[SHORT-ID] title`, at most 255 characters.
pub fn ticket_title(s: &IssueSummary) -> String {
    super::truncate(&format!("[{}] {}", s.short_id, s.title), 255)
}

/// The generated ticket description.
pub fn ticket_body(s: &IssueSummary) -> Doc {
    let mut blocks = Vec::new();
    let mut first = vec![Inline::Text("Reported by rupta: ".into())];
    first.push(Inline::Link { text: s.short_id.clone(), url: s.url.clone() });
    blocks.push(Block::Paragraph(first));
    match (&s.error_type, &s.error_value) {
        (Some(t), v) => {
            let mut p = vec![Inline::Code(t.clone())];
            if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                p.push(Inline::Text(format!(": {}", super::truncate(v, 1000))));
            }
            blocks.push(Block::Paragraph(p));
        }
        (None, Some(v)) => blocks.push(Block::Paragraph(vec![Inline::Text(super::truncate(v, 1000))])),
        (None, None) => blocks.push(Block::Paragraph(vec![Inline::Text(super::truncate(&s.title, 1000))])),
    }
    let mut facts = Vec::new();
    let fact = |k: &str, v: String| vec![Inline::Bold(format!("{k}:")), Inline::Text(format!(" {v}"))];
    if let Some(c) = s.culprit.as_deref().filter(|c| !c.is_empty()) {
        facts.push(vec![Inline::Bold("Culprit:".into()), Inline::Text(" ".into()), Inline::Code(c.to_string())]);
    }
    facts.push(fact("Project", s.project.clone()));
    facts.push(fact("Level", s.level.clone()));
    facts.push(fact("First seen", ts(&s.first_seen)));
    facts.push(fact("Last seen", ts(&s.last_seen)));
    facts.push(fact("Events", s.count.to_string()));
    facts.push(fact("Users affected", s.user_count.to_string()));
    if let Some(r) = &s.first_release {
        facts.push(fact("First release", r.clone()));
    }
    if let Some(r) = s.last_release.as_ref().filter(|r| Some(*r) != s.first_release.as_ref()) {
        facts.push(fact("Last release", r.clone()));
    }
    blocks.push(Block::Bullets(facts));
    if !s.top_tags.is_empty() {
        blocks.push(Block::Heading("Top tags".into()));
        blocks.push(Block::Bullets(
            s.top_tags
                .iter()
                .map(|(k, vals)| {
                    let v = vals.iter().map(|(v, pct)| format!("{v} ({pct}%)")).collect::<Vec<_>>().join(", ");
                    vec![Inline::Bold(format!("{k}:")), Inline::Text(format!(" {v}"))]
                })
                .collect(),
        ));
    }
    if !s.frames.is_empty() {
        blocks.push(Block::Heading("Stack trace (in-app frames, most recent call first)".into()));
        let text = s.frames.iter().take(MAX_FRAMES).map(frame_line).collect::<Vec<_>>().join("\n");
        blocks.push(Block::Code { language: s.platform.clone().filter(|p| !p.is_empty()), text });
    }
    blocks.push(Block::Paragraph(vec![Inline::Link { text: "Open the issue in rupta".into(), url: s.url.clone() }]));
    Doc(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> IssueSummary {
        IssueSummary {
            short_id: "WEB-12".into(),
            title: "TypeError: x is undefined".into(),
            error_type: Some("TypeError".into()),
            error_value: Some("x is undefined".into()),
            culprit: Some("app.js in render".into()),
            level: "error".into(),
            platform: Some("javascript".into()),
            project: "web".into(),
            count: 42,
            user_count: 7,
            top_tags: vec![("browser".into(), vec![("Chrome".into(), 60), ("Firefox".into(), 40)])],
            frames: vec![Frame {
                function: Some("render".into()),
                filename: Some("src/app.js".into()),
                lineno: Some(10),
                colno: Some(5),
                context_line: Some("  x.y()".into()),
                ..Frame::default()
            }],
            url: "https://rupta.example/organizations/acme/issues/5".into(),
            ..IssueSummary::default()
        }
    }

    #[test]
    fn markdown_has_all_parts() {
        let md = ticket_body(&summary()).markdown();
        assert!(
            md.starts_with("Reported by rupta: [WEB-12](https://rupta.example/organizations/acme/issues/5)"),
            "{md}"
        );
        assert!(md.contains("`TypeError`: x is undefined"), "{md}");
        assert!(md.contains("**Events:** 42"), "{md}");
        assert!(md.contains("**browser:** Chrome (60%), Firefox (40%)"), "{md}");
        assert!(md.contains("```javascript\nat render (src/app.js:10:5)\n    x.y()\n```"), "{md}");
        assert_eq!(ticket_title(&summary()), "[WEB-12] TypeError: x is undefined");
    }

    #[test]
    fn adf_is_valid_shape() {
        let adf = ticket_body(&summary()).adf();
        assert_eq!(adf["version"], 1);
        assert_eq!(adf["type"], "doc");
        let types: Vec<&str> = adf["content"].as_array().unwrap().iter().map(|n| n["type"].as_str().unwrap()).collect();
        assert!(
            types.contains(&"codeBlock") && types.contains(&"bulletList") && types.contains(&"heading"),
            "{types:?}"
        );
        // No empty text nodes (Jira refuses them).
        fn walk(v: &Value) {
            if v["type"] == "text" {
                assert!(!v["text"].as_str().unwrap().is_empty());
            }
            for c in v["content"].as_array().into_iter().flatten() {
                walk(c);
            }
        }
        walk(&adf);
        let link = &adf["content"][0]["content"][1];
        assert_eq!(link["marks"][0]["attrs"]["href"], "https://rupta.example/organizations/acme/issues/5");
    }

    #[test]
    fn wiki_markup_escapes_and_formats() {
        let w = ticket_body(&summary()).wiki();
        assert!(w.contains("[WEB\\-12|https://rupta.example/organizations/acme/issues/5]"), "{w}");
        assert!(w.contains("{{TypeError}}"), "{w}");
        assert!(w.contains("h3. Top tags"), "{w}");
        assert!(w.contains("{noformat}\nat render (src/app.js:10:5)"), "{w}");
        assert_eq!(Doc(vec![Block::Paragraph(vec![Inline::Text("a*b{c}".into())])]).wiki(), "a\\*b\\{c\\}");
    }

    #[test]
    fn markdown_escapes_user_text() {
        let d = Doc(vec![Block::Paragraph(vec![Inline::Text("<img src=x> *bold*".into())])]);
        assert_eq!(d.markdown(), "\\<img src=x\\> \\*bold\\*");
        assert_eq!(Doc::from_text("a\n\n\nb").0.len(), 2);
    }
}
