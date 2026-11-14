//! Helpers shared by the telemetry endpoints (performance, traces, logs,
//! metrics, profiles, replays): the projects and time range a request
//! covers, and filters compiled from the search syntax.

use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_core::query::{self, Op, TermKind};
use rupta_store::squad::Connection;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::access::{OrgCtx, org_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::Auth;
use crate::sql::{Params, id_list};

/// Projects, environment and time range of a telemetry query.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Scope {
    /// Project ids or slugs, comma-separated (default: every project of
    /// the organization).
    pub project: Option<String>,
    pub environment: Option<String>,
    /// Time range back from now, e.g. `1h`, `24h`, `14d` (default 24h, at
    /// most 366d). Ignored when `start` is given.
    pub period: Option<String>,
    /// Start of the range (ISO 8601).
    pub start: Option<DateTime<Utc>>,
    /// End of the range (ISO 8601, default now).
    pub end: Option<DateTime<Utc>>,
}

/// A resolved scope.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub org: OrgCtx,
    pub projects: Vec<i64>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub environment: Option<String>,
}

impl Resolved {
    /// `project_id IN (...) AND timestamp >= $a AND timestamp < $b [AND environment = $c]`
    /// for a table aliased as `alias` (empty for none).
    pub fn filter(&self, params: &mut Params, alias: &str, time_column: &str) -> String {
        let a = if alias.is_empty() { String::new() } else { format!("{alias}.") };
        let (s, e) = (params.bind(self.start), params.bind(self.end));
        let mut sql = format!(
            "{a}project_id IN ({}) AND {a}{time_column} >= {s} AND {a}{time_column} < {e}",
            id_list(&self.projects)
        );
        if let Some(env) = &self.environment {
            sql.push_str(&format!(" AND {a}environment = {}", params.bind(env.clone())));
        }
        sql
    }

    pub fn minutes(&self) -> f64 {
        ((self.end - self.start).num_seconds().max(60)) as f64 / 60.0
    }
}

impl Scope {
    /// The time range (start, end).
    pub fn range(&self, default: Duration) -> ApiResult<(DateTime<Utc>, DateTime<Utc>)> {
        let end = self.end.unwrap_or_else(Utc::now);
        let start = match self.start {
            Some(s) => s,
            None => end - crate::routes::issues::parse_period("period", self.period.as_deref(), default)?,
        };
        if start >= end {
            return Err(ApiError::invalid("start", "must be before end"));
        }
        if end - start > Duration::days(366) {
            return Err(ApiError::invalid("start", "the range may span at most 366 days"));
        }
        Ok((start, end))
    }

    /// Checks access and resolves projects and time range (blocking).
    pub fn resolve(&self, c: &Connection, auth: &Auth, org: &str, default: Duration) -> ApiResult<Resolved> {
        let (start, end) = self.range(default)?;
        let org = org_sync(c, auth, org, Permission::ReadProject)?;
        let all: Vec<(i64, String)> = c
            .query("SELECT id, slug FROM projects WHERE org_id = $1 ORDER BY id", &[&org.id])?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();
        let wanted: Vec<String> = self
            .project
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let projects: Vec<i64> = if wanted.is_empty() {
            all.iter().map(|(id, _)| *id).collect()
        } else {
            all.iter()
                .filter(|(id, slug)| wanted.iter().any(|w| *w == id.to_string() || w == slug))
                .map(|(id, _)| *id)
                .collect()
        };
        Ok(Resolved { org, projects, start, end, environment: self.environment.clone().filter(|e| !e.is_empty()) })
    }
}

/// A searchable column of a telemetry table.
#[derive(Debug, Clone, Copy)]
pub enum Col<'a> {
    /// Compared as text (`=`, `IN`, negated `<>`).
    Text(&'a str),
    /// Compared as a number (`=`, `>`, `>=`, `<`, `<=`).
    Number(&'a str),
    /// Text matched as a substring (`ILIKE`).
    Contains(&'a str),
}

/// What a filter may refer to.
pub struct FilterSpec<'a> {
    /// Search keys and their columns.
    pub keys: &'a [(&'a str, Col<'a>)],
    /// Column searched by free text (substring, case-insensitive).
    pub text: Option<&'a str>,
    /// `jsonb` column holding other keys (`key:value` matches
    /// `column ->> 'key'`).
    pub attributes: Option<&'a str>,
}

fn valid_attribute_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 200 && k.chars().all(|c| c.is_ascii_alphanumeric() || "._-:/@".contains(c))
}

/// A `jsonb ->> 'key'` expression for a validated key.
pub fn attribute_expr(column: &str, key: &str) -> ApiResult<String> {
    if !valid_attribute_key(key) {
        return Err(ApiError::invalid("query", format!("invalid attribute name {key:?}")));
    }
    Ok(format!("{column} ->> '{key}'"))
}

fn like_pattern(s: &str) -> String {
    format!("%{}%", s.replace(['%', '_', '\\'], ""))
}

/// Compiles a search query (docs/search.md syntax: free text, `key:value`,
/// `key:[a,b]`, `!key:value`, `key:>n`) into SQL over the columns of `spec`.
/// Returns `TRUE` for an empty query.
pub fn compile_filter(q: &str, spec: &FilterSpec<'_>, params: &mut Params) -> ApiResult<String> {
    let parsed = query::parse(q).map_err(|e| ApiError::invalid("query", e.message))?;
    let mut parts = Vec::new();
    for term in &parsed.terms {
        let sql = match &term.kind {
            TermKind::Text(t) => {
                let Some(col) = spec.text else {
                    return Err(ApiError::invalid("query", "free text is not supported here"));
                };
                format!("{col} ILIKE {}", params.bind(like_pattern(t)))
            }
            TermKind::Filter { key, op, values } => {
                let col = spec.keys.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, c)| *c);
                let attr_expr;
                let col = match (col, spec.attributes) {
                    (Some(c), _) => c,
                    (None, Some(attrs)) => {
                        attr_expr = attribute_expr(attrs, key)?;
                        Col::Text(attr_expr.as_str())
                    }
                    (None, None) => return Err(ApiError::invalid("query", format!("unknown key {key:?}"))),
                };
                match col {
                    Col::Text(c) => {
                        if *op != Op::Eq {
                            return Err(ApiError::invalid("query", format!("{key} does not support comparisons")));
                        }
                        let list: Vec<String> = values.iter().map(|v| params.bind(v.clone())).collect();
                        if term.negated {
                            format!("({c} IS NULL OR {c} NOT IN ({}))", list.join(", "))
                        } else {
                            format!("{c} IN ({})", list.join(", "))
                        }
                    }
                    Col::Contains(c) => {
                        let list: Vec<String> =
                            values.iter().map(|v| format!("{c} ILIKE {}", params.bind(like_pattern(v)))).collect();
                        let any = format!("({})", list.join(" OR "));
                        if term.negated { format!("NOT {any}") } else { any }
                    }
                    Col::Number(c) => {
                        let v = values.first().ok_or_else(|| ApiError::invalid("query", "missing value"))?;
                        let n: f64 = v
                            .trim_end_matches("ms")
                            .parse()
                            .map_err(|_| ApiError::invalid("query", format!("{key} needs a number")))?;
                        let sym = match op {
                            Op::Eq => "=",
                            Op::Gt => ">",
                            Op::Gte => ">=",
                            Op::Lt => "<",
                            Op::Lte => "<=",
                        };
                        let e = format!("{c} {sym} {}", params.bind(n));
                        if term.negated { format!("NOT ({e})") } else { e }
                    }
                }
            }
        };
        parts.push(sql);
    }
    Ok(if parts.is_empty() { "TRUE".into() } else { parts.join(" AND ") })
}

/// Bucket unit for a time range: the coarsest of minute, hour and day
/// that still gives enough points, or the unit asked for.
pub fn interval_for(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    asked: Option<&str>,
) -> ApiResult<(&'static str, Duration)> {
    let units = [("minute", Duration::minutes(1)), ("hour", Duration::hours(1)), ("day", Duration::days(1))];
    if let Some(a) = asked.filter(|a| !a.is_empty()) {
        let found = match a {
            "1m" | "minute" => units[0],
            "1h" | "hour" => units[1],
            "1d" | "day" => units[2],
            _ => return Err(ApiError::invalid("interval", "one of 1m, 1h, 1d")),
        };
        if (end - start).num_seconds() / found.1.num_seconds() > 10_000 {
            return Err(ApiError::invalid("interval", "too many buckets for this range"));
        }
        return Ok(found);
    }
    let span = end - start;
    Ok(if span <= Duration::hours(3) {
        units[0]
    } else if span <= Duration::hours(72) {
        units[1]
    } else {
        units[2]
    })
}

/// Bucket starts covering `[start, end)`.
pub fn buckets(start: DateTime<Utc>, end: DateTime<Utc>, step: Duration) -> Vec<i64> {
    let s = step.num_seconds().max(1);
    let first = start.timestamp() - start.timestamp().rem_euclid(s);
    let mut out = Vec::new();
    let mut t = first;
    while t < end.timestamp() && out.len() < 10_000 {
        out.push(t);
        t += s;
    }
    out
}

/// 32 lowercase hex digits for a 16-byte id.
pub fn hex_id(b: [u8; 16]) -> String {
    uuid::Uuid::from_bytes(b).simple().to_string()
}

/// Parses a 32-hex-digit (or hyphenated) id; malformed ids answer 404.
pub fn parse_hex_id(s: &str) -> ApiResult<[u8; 16]> {
    uuid::Uuid::try_parse(s).map(|u| *u.as_bytes()).map_err(|_| ApiError::not_found())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: FilterSpec<'static> = FilterSpec {
        keys: &[
            ("level", Col::Text("level")),
            ("duration", Col::Number("duration_ms")),
            ("url", Col::Contains("urls::text")),
        ],
        text: Some("body"),
        attributes: Some("attributes"),
    };

    #[test]
    fn filters_compile() {
        let mut p = Params::new();
        let sql =
            compile_filter("level:[warn,error] !region:eu duration:>=100 timeout url:checkout", &SPEC, &mut p).unwrap();
        assert_eq!(
            sql,
            "level IN ($1, $2) AND (attributes ->> 'region' IS NULL OR attributes ->> 'region' NOT IN ($3)) AND duration_ms >= $4 AND body ILIKE $5 AND (urls::text ILIKE $6)"
        );
        assert_eq!(p.0.len(), 6);
        assert_eq!(compile_filter("", &SPEC, &mut Params::new()).unwrap(), "TRUE");
        assert!(compile_filter("level:>3", &SPEC, &mut Params::new()).is_err());
        // Attribute names are embedded in SQL, so only safe characters pass.
        assert!(attribute_expr("attributes", "bad'key").is_err());
        assert!(attribute_expr("attributes", "").is_err());
        assert_eq!(attribute_expr("attributes", "http.route").unwrap(), "attributes ->> 'http.route'");
        assert!(compile_filter("duration:abc", &SPEC, &mut Params::new()).is_err());
    }

    #[test]
    fn intervals() {
        let end = Utc::now();
        assert_eq!(interval_for(end - Duration::hours(1), end, None).unwrap().0, "minute");
        assert_eq!(interval_for(end - Duration::hours(24), end, None).unwrap().0, "hour");
        assert_eq!(interval_for(end - Duration::days(14), end, None).unwrap().0, "day");
        assert!(interval_for(end - Duration::days(30), end, Some("1m")).is_err());
        let t = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 14, 12, 30, 0).unwrap();
        assert_eq!(
            buckets(t - Duration::hours(2), t, Duration::hours(1)),
            vec![
                (t - Duration::minutes(150)).timestamp(),
                (t - Duration::minutes(90)).timestamp(),
                (t - Duration::minutes(30)).timestamp()
            ]
        );
    }
}
