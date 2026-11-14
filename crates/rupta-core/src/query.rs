//! The issue search query language.
//!
//! A query is a list of terms separated by whitespace. Every term must
//! match (AND). A term is either free text, matched against an issue's
//! title and culprit, or a filter `key:value`:
//!
//! ```text
//! is:unresolved level:error browser:Chrome "connection reset" !environment:staging
//! times_seen:>10 first_seen:-24h tags[region]:eu-west user.email:ada@example.com
//! ```
//!
//! Values may be quoted (`"a b"`), lists (`level:[error,fatal]`) and may
//! contain `*` wildcards. A leading `!` negates a term. The reference is
//! `docs/search.md`.
//!
//! [`parse`] turns the text into an AST ([`Query`]); [`compile`] turns the
//! AST into a parameterized SQL condition over the `issues` table (alias
//! `i`). Filters on event attributes become a subquery on `events`
//! returning issue ids. Values never appear in the SQL text: they are
//! returned as [`Param`]s bound to `$n` placeholders.

use chrono::{DateTime, Duration, NaiveDate, Utc};

/// A parsed query.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Query {
    pub terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub negated: bool,
    pub kind: TermKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TermKind {
    /// Free text.
    Text(String),
    /// `key:value`, `key:>value`, `key:[a,b]`.
    Filter { key: String, op: Op, values: Vec<String> },
}

/// Comparison of a filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// A query that could not be parsed or compiled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct QueryError {
    pub message: String,
}

fn err<T>(message: impl Into<String>) -> Result<T, QueryError> {
    Err(QueryError { message: message.into() })
}

/// Longest query accepted, in bytes.
pub const MAX_QUERY_LEN: usize = 2048;
/// Most terms accepted in one query.
pub const MAX_TERMS: usize = 32;

/// Parses query text.
pub fn parse(input: &str) -> Result<Query, QueryError> {
    if input.len() > MAX_QUERY_LEN {
        return err(format!("the query is longer than {MAX_QUERY_LEN} bytes"));
    }
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0;
    let mut terms = Vec::new();
    while pos < chars.len() {
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }
        let negated = chars[pos] == '!' || (chars[pos] == '-' && pos + 1 < chars.len() && is_key_start(chars[pos + 1]));
        let start = pos;
        if negated {
            pos += 1;
        }
        // Quoted free text.
        if pos < chars.len() && chars[pos] == '"' {
            let (text, next) = quoted(&chars, pos)?;
            pos = next;
            if !text.is_empty() {
                terms.push(Term { negated, kind: TermKind::Text(text) });
            }
            continue;
        }
        // A key: letters, digits, `_`, `.`, or `tags[...]`.
        let key_start = pos;
        while pos < chars.len() && (chars[pos].is_alphanumeric() || matches!(chars[pos], '_' | '.' | '-')) {
            pos += 1;
        }
        if pos < chars.len() && chars[pos] == '[' && chars[key_start..pos].iter().collect::<String>() == "tags" {
            let close = chars[pos..].iter().position(|&c| c == ']').map(|i| pos + i);
            let Some(close) = close else { return err("unclosed `[` in tags[...]") };
            let tag: String = chars[pos + 1..close].iter().collect();
            if tag.is_empty() {
                return err("empty tag name in tags[]");
            }
            pos = close + 1;
            if pos < chars.len() && chars[pos] == ':' {
                let (op, values, next) = filter_value(&chars, pos + 1)?;
                pos = next;
                terms.push(Term { negated, kind: TermKind::Filter { key: format!("tags[{tag}]"), op, values } });
                continue;
            }
            return err(format!("expected `:` after tags[{tag}]"));
        }
        // `scheme://...` is text, not a filter.
        let url_like = chars.get(pos + 1) == Some(&'/');
        if pos > key_start && pos < chars.len() && chars[pos] == ':' && !url_like {
            let key: String = chars[key_start..pos].iter().collect::<String>().to_ascii_lowercase();
            let (op, values, next) = filter_value(&chars, pos + 1)?;
            pos = next;
            terms.push(Term { negated, kind: TermKind::Filter { key, op, values } });
            continue;
        }
        // Plain word: free text up to the next whitespace.
        pos = start;
        if negated {
            pos += 1;
        }
        let word_start = pos;
        while pos < chars.len() && !chars[pos].is_whitespace() {
            pos += 1;
        }
        let word: String = chars[word_start..pos].iter().collect();
        if !word.is_empty() {
            terms.push(Term { negated, kind: TermKind::Text(word) });
        }
    }
    if terms.len() > MAX_TERMS {
        return err(format!("more than {MAX_TERMS} terms"));
    }
    Ok(Query { terms })
}

fn is_key_start(c: char) -> bool {
    c.is_alphabetic()
}

/// Reads a quoted string starting at the opening quote. `\"` and `\\`
/// are escapes.
fn quoted(chars: &[char], open: usize) -> Result<(String, usize), QueryError> {
    let mut out = String::new();
    let mut pos = open + 1;
    while pos < chars.len() {
        match chars[pos] {
            '\\' if pos + 1 < chars.len() => {
                out.push(chars[pos + 1]);
                pos += 2;
            }
            '"' => return Ok((out, pos + 1)),
            c => {
                out.push(c);
                pos += 1;
            }
        }
    }
    err("unclosed quote")
}

fn filter_value(chars: &[char], mut pos: usize) -> Result<(Op, Vec<String>, usize), QueryError> {
    let mut op = Op::Eq;
    for (text, o) in [(">=", Op::Gte), ("<=", Op::Lte), (">", Op::Gt), ("<", Op::Lt)] {
        let t: Vec<char> = text.chars().collect();
        if chars.len() >= pos + t.len() && chars[pos..pos + t.len()] == t[..] {
            op = o;
            pos += t.len();
            break;
        }
    }
    if pos < chars.len() && chars[pos] == '[' {
        let mut values = Vec::new();
        pos += 1;
        loop {
            while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
                pos += 1;
            }
            if pos >= chars.len() {
                return err("unclosed `[` in a value list");
            }
            if chars[pos] == ']' {
                pos += 1;
                break;
            }
            if chars[pos] == '"' {
                let (v, next) = quoted(chars, pos)?;
                values.push(v);
                pos = next;
            } else {
                let s = pos;
                while pos < chars.len() && !matches!(chars[pos], ',' | ']') && !chars[pos].is_whitespace() {
                    pos += 1;
                }
                values.push(chars[s..pos].iter().collect());
            }
        }
        if values.is_empty() {
            return err("empty value list");
        }
        return Ok((op, values, pos));
    }
    if pos < chars.len() && chars[pos] == '"' {
        let (v, next) = quoted(chars, pos)?;
        return Ok((op, vec![v], next));
    }
    let s = pos;
    while pos < chars.len() && !chars[pos].is_whitespace() {
        pos += 1;
    }
    let v: String = chars[s..pos].iter().collect();
    if v.is_empty() {
        return err("a filter needs a value after `:`");
    }
    Ok((op, vec![v], pos))
}

/// A bound value of a compiled query.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Text(String),
    Int(i64),
    Time(DateTime<Utc>),
}

/// What compilation needs to know about the request.
#[derive(Debug, Clone)]
pub struct CompileContext {
    /// The searching user, for `assigned:me` and `is:bookmarked`.
    pub user_id: Option<i64>,
    /// The organization, for `#team` lookups.
    pub org_id: i64,
    /// Projects searched; limits event subqueries.
    pub project_ids: Vec<i64>,
    /// "Now", for relative times.
    pub now: DateTime<Utc>,
    /// Number of the first placeholder (`$n`); earlier ones belong to the caller.
    pub first_param: usize,
}

/// A compiled condition: SQL over `issues i` with `$n` placeholders.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub sql: String,
    pub params: Vec<Param>,
}

struct Builder<'a> {
    ctx: &'a CompileContext,
    params: Vec<Param>,
}

impl Builder<'_> {
    fn bind(&mut self, p: Param) -> String {
        self.params.push(p);
        format!("${}", self.ctx.first_param + self.params.len() - 1)
    }

    fn projects(&self) -> String {
        if self.ctx.project_ids.is_empty() {
            "false".into()
        } else {
            // Project ids come from the database, never from the query text.
            let ids: Vec<String> = self.ctx.project_ids.iter().map(|i| i.to_string()).collect();
            format!("e.project_id IN ({})", ids.join(", "))
        }
    }

    fn events(&self, cond: &str) -> String {
        format!("i.id IN (SELECT e.issue_id FROM events e WHERE {} AND ({cond}))", self.projects())
    }

    fn me(&self) -> Result<i64, QueryError> {
        self.ctx.user_id.ok_or_else(|| QueryError { message: "`me` needs a signed-in user".into() })
    }
}

/// Columns of `events` addressable by name.
const EVENT_COLUMNS: &[(&str, &str)] = &[
    ("environment", "environment"),
    ("release", "release"),
    ("dist", "dist"),
    ("browser", "browser"),
    ("os", "os"),
    ("device", "device"),
    ("runtime", "runtime"),
    ("user.email", "user_email"),
    ("user.id", "user_id"),
    ("user.ip", "user_ip"),
    ("server_name", "server_name"),
    ("transaction", "transaction"),
    ("url", "url"),
    ("sdk", "sdk"),
    ("platform", "platform"),
    ("event.type", "exception_type"),
    ("error.type", "exception_type"),
    ("error.value", "exception_value"),
    ("message", "message"),
];

/// Columns whose value is "Name Version" and also match on the name alone.
const NAME_VERSION_COLUMNS: &[&str] = &["browser", "os", "runtime", "device"];

/// Keys that the language knows (for errors and autocompletion).
pub const KNOWN_KEYS: &[&str] = &[
    "is",
    "assigned",
    "level",
    "priority",
    "issue.type",
    "first_seen",
    "last_seen",
    "times_seen",
    "users_seen",
    "first_release",
    "has",
    "environment",
    "release",
    "dist",
    "browser",
    "os",
    "device",
    "runtime",
    "user.email",
    "user.id",
    "user.ip",
    "server_name",
    "transaction",
    "url",
    "sdk",
    "platform",
    "error.type",
    "error.value",
    "message",
];

fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            '*' => out.push('%'),
            c => out.push(c),
        }
    }
    out
}

/// Compiles a parsed query into a condition. An empty query compiles to `true`.
pub fn compile(query: &Query, ctx: &CompileContext) -> Result<Compiled, QueryError> {
    let mut b = Builder { ctx, params: Vec::new() };
    let mut parts = Vec::new();
    for term in &query.terms {
        let sql = match &term.kind {
            TermKind::Text(text) => {
                let p = b.bind(Param::Text(format!("%{}%", like_escape(text))));
                format!("(i.title ILIKE {p} OR coalesce(i.culprit, '') ILIKE {p})")
            }
            TermKind::Filter { key, op, values } => filter(&mut b, key, *op, values)?,
        };
        parts.push(if term.negated { format!("NOT coalesce({sql}, false)") } else { sql });
    }
    let sql = if parts.is_empty() { "true".to_string() } else { parts.join(" AND ") };
    Ok(Compiled { sql, params: b.params })
}

fn any_of(parts: Vec<String>) -> String {
    if parts.len() == 1 { parts.into_iter().next().unwrap() } else { format!("({})", parts.join(" OR ")) }
}

fn eq_only(key: &str, op: Op) -> Result<(), QueryError> {
    if op != Op::Eq {
        return err(format!("`{key}` does not support comparisons"));
    }
    Ok(())
}

fn filter(b: &mut Builder<'_>, key: &str, op: Op, values: &[String]) -> Result<String, QueryError> {
    match key {
        "is" => {
            eq_only(key, op)?;
            let mut parts = Vec::new();
            for v in values {
                parts.push(match v.to_ascii_lowercase().as_str() {
                    "unresolved" => "i.status = 'unresolved'".to_string(),
                    "resolved" => "i.status = 'resolved'".to_string(),
                    "ignored" | "archived" => "i.status = 'ignored'".to_string(),
                    "assigned" => "(i.assignee_user_id IS NOT NULL OR i.assignee_team_id IS NOT NULL)".to_string(),
                    "unassigned" => "(i.assignee_user_id IS NULL AND i.assignee_team_id IS NULL)".to_string(),
                    "bookmarked" => {
                        let me = b.bind(Param::Int(b.me()?));
                        format!("i.id IN (SELECT issue_id FROM issue_bookmarks WHERE user_id = {me})")
                    }
                    "unseen" => {
                        let me = b.bind(Param::Int(b.me()?));
                        format!("i.id NOT IN (SELECT issue_id FROM issue_seen WHERE user_id = {me})")
                    }
                    "new" | "ongoing" | "regressed" | "escalating" => {
                        let p = b.bind(Param::Text(v.to_ascii_lowercase()));
                        format!("i.substatus = {p}")
                    }
                    other => return err(format!("unknown value is:{other}")),
                });
            }
            Ok(any_of(parts))
        }
        "assigned" | "assigned_to" => {
            eq_only(key, op)?;
            let mut parts = Vec::new();
            for v in values {
                if v.eq_ignore_ascii_case("me") {
                    let me = b.bind(Param::Int(b.me()?));
                    parts.push(format!(
                        "(i.assignee_user_id = {me} OR i.assignee_team_id IN (SELECT team_id FROM team_members WHERE user_id = {me}))"
                    ));
                } else if v.eq_ignore_ascii_case("none") {
                    parts.push("(i.assignee_user_id IS NULL AND i.assignee_team_id IS NULL)".into());
                } else if let Some(team) = v.strip_prefix('#') {
                    let t = b.bind(Param::Text(team.to_string()));
                    let o = b.bind(Param::Int(b.ctx.org_id));
                    parts.push(format!(
                        "i.assignee_team_id IN (SELECT id FROM teams WHERE slug = {t} AND org_id = {o})"
                    ));
                } else {
                    let e = b.bind(Param::Text(v.to_lowercase()));
                    parts.push(format!("i.assignee_user_id IN (SELECT id FROM users WHERE lower(email) = {e})"));
                }
            }
            Ok(any_of(parts))
        }
        "level" | "priority" | "issue.type" | "issue.priority" | "first_release" | "issue.platform" => {
            eq_only(key, op)?;
            let col = match key {
                "level" => "i.level",
                "priority" | "issue.priority" => "i.priority",
                "issue.type" => "i.type",
                "issue.platform" => "i.platform",
                _ => "i.first_release",
            };
            Ok(any_of(values.iter().map(|v| text_match(b, col, v, false)).collect()))
        }
        "first_seen" | "last_seen" | "age" => {
            let col = if key == "last_seen" { "i.last_seen" } else { "i.first_seen" };
            let v = single(key, values)?;
            time_filter(b, col, op, v)
        }
        "times_seen" | "users_seen" | "count" => {
            let col = if key == "users_seen" { "i.users_seen" } else { "i.times_seen" };
            let v = single(key, values)?;
            let n: i64 = v.parse().map_err(|_| QueryError { message: format!("`{key}` needs a number, got {v:?}") })?;
            let p = b.bind(Param::Int(n));
            Ok(format!("{col} {} {p}", op_sql(op)))
        }
        "has" => {
            eq_only(key, op)?;
            let mut parts = Vec::new();
            for v in values {
                if let Some((_, col)) = EVENT_COLUMNS.iter().find(|(k, _)| *k == v.as_str()) {
                    parts.push(b.events(&format!("e.{col} IS NOT NULL AND e.{col} <> ''")));
                } else if v == "user" {
                    parts.push(b.events("e.user_id IS NOT NULL OR e.user_email IS NOT NULL OR e.user_ip IS NOT NULL"));
                } else {
                    let k = b.bind(Param::Text(v.clone()));
                    parts.push(b.events(&format!("e.tags ? {k}")));
                }
            }
            Ok(any_of(parts))
        }
        _ => {
            eq_only(key, op)?;
            if let Some((_, col)) = EVENT_COLUMNS.iter().find(|(k, _)| *k == key) {
                let name_version = NAME_VERSION_COLUMNS.contains(col);
                let cond = any_of(values.iter().map(|v| text_match(b, &format!("e.{col}"), v, name_version)).collect());
                return Ok(b.events(&cond));
            }
            let tag = key.strip_prefix("tags[").and_then(|k| k.strip_suffix(']')).unwrap_or(key);
            if tag.is_empty() || tag.len() > 200 {
                return err(format!("invalid tag name {tag:?}"));
            }
            let k = b.bind(Param::Text(tag.to_string()));
            let mut parts = Vec::new();
            for v in values {
                if v.contains('*') {
                    let p = b.bind(Param::Text(like_escape(v)));
                    parts.push(format!("e.tags ->> {k} LIKE {p}"));
                } else {
                    let p = b.bind(Param::Text(v.clone()));
                    // Tags are a JSON object; an array of pairs also matches.
                    parts.push(format!(
                        "(e.tags ->> {k} = {p} OR e.tags @> jsonb_build_array(jsonb_build_array({k}, {p})))"
                    ));
                }
            }
            Ok(b.events(&any_of(parts)))
        }
    }
}

fn single<'v>(key: &str, values: &'v [String]) -> Result<&'v str, QueryError> {
    match values {
        [v] => Ok(v),
        _ => err(format!("`{key}` takes a single value")),
    }
}

fn op_sql(op: Op) -> &'static str {
    match op {
        Op::Eq => "=",
        Op::Gt => ">",
        Op::Gte => ">=",
        Op::Lt => "<",
        Op::Lte => "<=",
    }
}

fn text_match(b: &mut Builder<'_>, col: &str, v: &str, name_version: bool) -> String {
    if v.contains('*') {
        let p = b.bind(Param::Text(like_escape(v)));
        return format!("{col} LIKE {p}");
    }
    let p = b.bind(Param::Text(v.to_string()));
    if name_version {
        let q = b.bind(Param::Text(format!("{} %", like_escape(v))));
        format!("({col} = {p} OR {col} LIKE {q})")
    } else {
        format!("{col} = {p}")
    }
}

/// Parses a relative duration such as `24h`, `7d`, `30m`, `2w`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = num.parse().ok()?;
    if n > 100_000 {
        return None;
    }
    Some(match unit {
        "s" => Duration::seconds(n),
        "m" | "min" => Duration::minutes(n),
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        "w" => Duration::weeks(n),
        _ => return None,
    })
}

/// Time filters. Relative values are ages: `-24h` and `<24h` mean "within
/// the last 24 hours", `+24h` and `>24h` "longer ago than 24 hours".
/// Absolute dates compare the timestamp: `>2026-01-01` means "after".
fn time_filter(b: &mut Builder<'_>, col: &str, op: Op, v: &str) -> Result<String, QueryError> {
    let (sign, rest) = match v.as_bytes().first() {
        Some(b'-') => (Some('-'), &v[1..]),
        Some(b'+') => (Some('+'), &v[1..]),
        _ => (None, v),
    };
    if let Some(d) = parse_duration(rest) {
        let cutoff = b.ctx.now - d;
        let p = b.bind(Param::Time(cutoff));
        let newer = match (sign, op) {
            (Some('-'), Op::Eq) | (None, Op::Lt | Op::Lte | Op::Eq) => true,
            (Some('+'), Op::Eq) | (None, Op::Gt | Op::Gte) => false,
            _ => return err(format!("cannot combine a sign and a comparison in {v:?}")),
        };
        return Ok(if newer { format!("{col} >= {p}") } else { format!("{col} < {p}") });
    }
    if sign.is_some() {
        return err(format!("invalid duration {v:?} (use e.g. 24h, 7d, 2w)"));
    }
    let at = if let Ok(t) = DateTime::parse_from_rfc3339(v) {
        t.with_timezone(&Utc)
    } else if let Ok(d) = NaiveDate::parse_from_str(v, "%Y-%m-%d") {
        d.and_hms_opt(0, 0, 0).unwrap().and_utc()
    } else {
        return err(format!("invalid time {v:?} (use a duration such as 24h or a date such as 2026-01-31)"));
    };
    let p = b.bind(Param::Time(at));
    Ok(match op {
        // A bare date matches that whole day.
        Op::Eq => {
            let q = b.bind(Param::Time(at + Duration::days(1)));
            format!("({col} >= {p} AND {col} < {q})")
        }
        op => format!("{col} {} {p}", op_sql(op)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(key: &str, op: Op, values: &[&str]) -> TermKind {
        TermKind::Filter { key: key.into(), op, values: values.iter().map(|s| s.to_string()).collect() }
    }

    fn term(negated: bool, kind: TermKind) -> Term {
        Term { negated, kind }
    }

    #[test]
    fn parser_table() {
        let cases: Vec<(&str, Vec<Term>)> = vec![
            ("", vec![]),
            ("is:unresolved", vec![term(false, f("is", Op::Eq, &["unresolved"]))]),
            (
                "  level:error   oops ",
                vec![term(false, f("level", Op::Eq, &["error"])), term(false, TermKind::Text("oops".into()))],
            ),
            ("\"connection reset\"", vec![term(false, TermKind::Text("connection reset".into()))]),
            ("!environment:staging", vec![term(true, f("environment", Op::Eq, &["staging"]))]),
            ("-environment:staging", vec![term(true, f("environment", Op::Eq, &["staging"]))]),
            ("times_seen:>10", vec![term(false, f("times_seen", Op::Gt, &["10"]))]),
            ("times_seen:<=3", vec![term(false, f("times_seen", Op::Lte, &["3"]))]),
            ("first_seen:-24h", vec![term(false, f("first_seen", Op::Eq, &["-24h"]))]),
            ("tags[region]:eu-west", vec![term(false, f("tags[region]", Op::Eq, &["eu-west"]))]),
            ("level:[error, fatal]", vec![term(false, f("level", Op::Eq, &["error", "fatal"]))]),
            ("user.email:\"a b@x\"", vec![term(false, f("user.email", Op::Eq, &["a b@x"]))]),
            ("Level:error", vec![term(false, f("level", Op::Eq, &["error"]))]),
            ("assigned:#backend", vec![term(false, f("assigned", Op::Eq, &["#backend"]))]),
            ("http://x.com/a:b", vec![term(false, TermKind::Text("http://x.com/a:b".into()))]),
            ("\"say \\\"hi\\\"\"", vec![term(false, TermKind::Text("say \"hi\"".into()))]),
            ("!\"bad thing\"", vec![term(true, TermKind::Text("bad thing".into()))]),
            ("-5", vec![term(false, TermKind::Text("-5".into()))]),
        ];
        for (input, want) in cases {
            assert_eq!(parse(input).unwrap().terms, want, "{input}");
        }
    }

    #[test]
    fn parser_errors() {
        for input in ["\"open", "level:", "level:[a,", "tags[x", "tags[]:a", "level:[]"] {
            assert!(parse(input).is_err(), "{input}");
        }
        assert!(parse(&"a ".repeat(40)).is_err());
        assert!(parse(&"a".repeat(3000)).is_err());
    }

    fn ctx() -> CompileContext {
        CompileContext {
            user_id: Some(7),
            org_id: 3,
            project_ids: vec![1, 2],
            now: DateTime::parse_from_rfc3339("2026-11-14T12:00:00Z").unwrap().with_timezone(&Utc),
            first_param: 1,
        }
    }

    fn c(q: &str) -> Compiled {
        compile(&parse(q).unwrap(), &ctx()).unwrap()
    }

    fn t(s: &str) -> Param {
        Param::Text(s.into())
    }

    #[test]
    fn compiler_table() {
        let now = ctx().now;
        let cases: Vec<(&str, &str, Vec<Param>)> = vec![
            ("", "true", vec![]),
            ("is:unresolved", "i.status = 'unresolved'", vec![]),
            ("is:[resolved,ignored]", "(i.status = 'resolved' OR i.status = 'ignored')", vec![]),
            ("is:bookmarked", "i.id IN (SELECT issue_id FROM issue_bookmarks WHERE user_id = $1)", vec![Param::Int(7)]),
            ("level:error", "i.level = $1", vec![t("error")]),
            ("!level:error", "NOT coalesce(i.level = $1, false)", vec![t("error")]),
            ("oops", "(i.title ILIKE $1 OR coalesce(i.culprit, '') ILIKE $1)", vec![t("%oops%")]),
            ("100%_done", "(i.title ILIKE $1 OR coalesce(i.culprit, '') ILIKE $1)", vec![t("%100\\%\\_done%")]),
            ("times_seen:>10", "i.times_seen > $1", vec![Param::Int(10)]),
            ("first_seen:-24h", "i.first_seen >= $1", vec![Param::Time(now - Duration::hours(24))]),
            ("first_seen:>24h", "i.first_seen < $1", vec![Param::Time(now - Duration::hours(24))]),
            ("last_seen:-7d", "i.last_seen >= $1", vec![Param::Time(now - Duration::days(7))]),
            (
                "last_seen:>2026-01-01",
                "i.last_seen > $1",
                vec![Param::Time(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc())],
            ),
            (
                "environment:prod",
                "i.id IN (SELECT e.issue_id FROM events e WHERE e.project_id IN (1, 2) AND (e.environment = $1))",
                vec![t("prod")],
            ),
            (
                "browser:Chrome",
                "i.id IN (SELECT e.issue_id FROM events e WHERE e.project_id IN (1, 2) AND ((e.browser = $1 OR e.browser LIKE $2)))",
                vec![t("Chrome"), t("Chrome %")],
            ),
            (
                "user.email:ada@x.org",
                "i.id IN (SELECT e.issue_id FROM events e WHERE e.project_id IN (1, 2) AND (e.user_email = $1))",
                vec![t("ada@x.org")],
            ),
            (
                "tags[region]:eu",
                "i.id IN (SELECT e.issue_id FROM events e WHERE e.project_id IN (1, 2) AND ((e.tags ->> $1 = $2 OR e.tags @> jsonb_build_array(jsonb_build_array($1, $2)))))",
                vec![t("region"), t("eu")],
            ),
            (
                "region:eu*",
                "i.id IN (SELECT e.issue_id FROM events e WHERE e.project_id IN (1, 2) AND (e.tags ->> $1 LIKE $2))",
                vec![t("region"), t("eu%")],
            ),
            (
                "assigned:me",
                "(i.assignee_user_id = $1 OR i.assignee_team_id IN (SELECT team_id FROM team_members WHERE user_id = $1))",
                vec![Param::Int(7)],
            ),
            (
                "assigned:#backend",
                "i.assignee_team_id IN (SELECT id FROM teams WHERE slug = $1 AND org_id = $2)",
                vec![t("backend"), Param::Int(3)],
            ),
            (
                "assigned:Ada@X.org",
                "i.assignee_user_id IN (SELECT id FROM users WHERE lower(email) = $1)",
                vec![t("ada@x.org")],
            ),
            ("is:unresolved level:fatal", "i.status = 'unresolved' AND i.level = $1", vec![t("fatal")]),
        ];
        for (q, sql, params) in cases {
            let got = c(q);
            assert_eq!(got.sql, sql, "{q}");
            assert_eq!(got.params, params, "{q}");
        }
    }

    #[test]
    fn placeholders_start_after_the_callers() {
        let mut cx = ctx();
        cx.first_param = 4;
        let got = compile(&parse("level:error oops").unwrap(), &cx).unwrap();
        assert!(got.sql.contains("$4") && got.sql.contains("$5") && !got.sql.contains("$1"), "{}", got.sql);
    }

    #[test]
    fn compiler_errors() {
        for q in [
            "is:whatever",
            "times_seen:many",
            "first_seen:yesterday",
            "level:>error",
            "first_seen:[1h,2h]",
            "first_seen:-1y",
        ] {
            assert!(compile(&parse(q).unwrap(), &ctx()).is_err(), "{q}");
        }
        let anon = CompileContext { user_id: None, ..ctx() };
        assert!(compile(&parse("assigned:me").unwrap(), &anon).is_err());
    }

    #[test]
    fn values_never_reach_the_sql_text() {
        let got = c("level:\"x'; DROP TABLE issues; --\" \"'or 1=1\" tags[a'b]:c");
        assert!(!got.sql.contains("DROP") && !got.sql.contains("1=1") && !got.sql.contains("a'b"), "{}", got.sql);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("24h"), Some(Duration::hours(24)));
        assert_eq!(parse_duration("2w"), Some(Duration::weeks(2)));
        assert_eq!(parse_duration("h"), None);
        assert_eq!(parse_duration("5y"), None);
    }
}
