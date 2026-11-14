//! `GET /api/v1/orgs/{org}/annotations`: releases, deploys and hours with
//! unusually many new issues, as a flat JSON list of time annotations
//! that Grafana's Infinity data source (or any JSON annotation source)
//! reads directly. See docs/monitoring.md.

use std::collections::BTreeMap;

use axum::extract::State;
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::org_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Path, Query};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(annotations))
}

/// Annotations returned at most.
const MAX_ANNOTATIONS: usize = 500;
/// An hour is a spike with at least this many new issues …
const SPIKE_MIN: i64 = 5;
/// … and at least this many times the hourly average of the range.
const SPIKE_FACTOR: i64 = 3;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AnnotationParams {
    /// Start: Unix milliseconds (Grafana's `${__from}`), Unix seconds or
    /// RFC 3339. Default: 24 hours before `to`.
    pub from: Option<String>,
    /// End, same forms. Default: now. At most 90 days after `from`.
    pub to: Option<String>,
    /// Project slugs or ids, comma-separated. Default: all projects.
    pub project: Option<String>,
    /// `release`, `deploy`, `new_issues`, comma-separated. Default: all.
    pub kind: Option<String>,
}

/// A time annotation.
#[derive(Serialize, ToSchema, Debug)]
pub struct Annotation {
    /// Unix milliseconds.
    pub time: i64,
    /// End of a region (deploys with a start time), Unix milliseconds.
    pub time_end: Option<i64>,
    /// `release`, `deploy` or `new_issues`.
    pub kind: String,
    pub title: String,
    pub text: String,
    pub tags: Vec<String>,
    /// Project slug for project-specific annotations (spikes).
    pub project: Option<String>,
    /// Link into rupta.
    pub url: Option<String>,
}

fn parse_when(field: &str, s: &str) -> ApiResult<DateTime<Utc>> {
    if let Ok(n) = s.trim().parse::<i64>() {
        // Grafana sends milliseconds; anything this large cannot be seconds.
        let t = if n.abs() >= 100_000_000_000 {
            DateTime::from_timestamp_millis(n)
        } else {
            DateTime::from_timestamp(n, 0)
        };
        return t.ok_or_else(|| ApiError::invalid(field, "out of range"));
    }
    DateTime::parse_from_rfc3339(s.trim())
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ApiError::invalid(field, "Unix milliseconds, Unix seconds or RFC 3339"))
}

/// Hours whose count reaches the spike threshold.
pub(crate) fn spikes(hourly: &[(DateTime<Utc>, i64)], hours_in_range: i64) -> Vec<(DateTime<Utc>, i64)> {
    let total: i64 = hourly.iter().map(|(_, n)| n).sum();
    let mean_ceil = (total + hours_in_range.max(1) - 1) / hours_in_range.max(1);
    let threshold = SPIKE_MIN.max(SPIKE_FACTOR * mean_ceil);
    hourly.iter().filter(|(_, n)| *n >= threshold).cloned().collect()
}

/// Releases, deploys and new-issue spikes of an organization in a time
/// range, newest first.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/annotations", tag = "annotations", params(("org" = String, Path), AnnotationParams),
    responses((status = 200, body = Vec<Annotation>), (status = 422, body = crate::error::ErrorBody)))]
async fn annotations(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(q): Query<AnnotationParams>,
) -> ApiResult<axum::Json<Vec<Annotation>>> {
    let to =
        q.to.as_deref().filter(|s| !s.is_empty()).map(|s| parse_when("to", s)).transpose()?.unwrap_or_else(Utc::now);
    let from = q
        .from
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| parse_when("from", s))
        .transpose()?
        .unwrap_or(to - Duration::hours(24));
    if from >= to {
        return Err(ApiError::invalid("from", "must be before to"));
    }
    if to - from > Duration::days(90) {
        return Err(ApiError::invalid("to", "the range is limited to 90 days"));
    }
    let kinds: Vec<String> = q
        .kind
        .as_deref()
        .map(|k| k.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_else(|| vec!["release".into(), "deploy".into(), "new_issues".into()]);
    for k in &kinds {
        if !matches!(k.as_str(), "release" | "deploy" | "new_issues") {
            return Err(ApiError::invalid("kind", "release, deploy or new_issues"));
        }
    }
    let wanted: Vec<String> = q
        .project
        .as_deref()
        .map(|p| p.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let public_url = state.config.public_url.clone();
    let list = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let mut projects: Vec<(i64, String)> = Vec::new();
            for r in c.query("SELECT id, slug FROM projects WHERE org_id = $1 ORDER BY id", &[&o.id])? {
                let (id, slug): (i64, String) = (r.get(0), r.get(1));
                if wanted.is_empty() || wanted.iter().any(|w| *w == slug || w.parse::<i64>().ok() == Some(id)) {
                    projects.push((id, slug));
                }
            }
            if !wanted.is_empty() && projects.is_empty() {
                return Err(ApiError::invalid("project", "no such project"));
            }
            let ids = crate::sql::id_list(&projects.iter().map(|(id, _)| *id).collect::<Vec<_>>());
            let slug_of: BTreeMap<i64, String> = projects.iter().cloned().collect();
            let mut out = Vec::new();
            let release_url = |v: &str| {
                let mut u = url::Url::parse(&format!("{public_url}/organizations/{}/releases/", o.slug)).ok();
                if let Some(u) = u.as_mut()
                    && let Ok(mut s) = u.path_segments_mut()
                {
                    s.pop_if_empty().push(v);
                }
                u.map(|u| u.to_string())
            };
            if kinds.iter().any(|k| k == "release") && !projects.is_empty() {
                let rows = c.query(
                    &format!(
                        "SELECT r.id, r.version, coalesce(r.date_released, r.created_at) AS t, r.ref FROM releases r
                         WHERE r.org_id = $1 AND coalesce(r.date_released, r.created_at) >= $2 AND coalesce(r.date_released, r.created_at) <= $3
                           AND EXISTS (SELECT 1 FROM release_projects rp WHERE rp.release_id = r.id AND rp.project_id IN ({ids}))
                         ORDER BY t DESC LIMIT {MAX_ANNOTATIONS}"
                    ),
                    &[&o.id, &from, &to],
                )?;
                for r in rows {
                    let rid: i64 = r.get(0);
                    let version: String = r.get(1);
                    let t: DateTime<Utc> = r.get(2);
                    let mut tags = vec!["release".to_string()];
                    for pr in c.query(&format!("SELECT project_id FROM release_projects WHERE release_id = $1 AND project_id IN ({ids})"), &[&rid])? {
                        if let Some(s) = slug_of.get(&pr.get::<i64>(0)) {
                            tags.push(s.clone());
                        }
                    }
                    let git_ref: Option<String> = r.get(3);
                    out.push(Annotation {
                        time: t.timestamp_millis(),
                        time_end: None,
                        kind: "release".into(),
                        title: format!("Release {version}"),
                        text: git_ref.map(|g| format!("Release {version} ({g})")).unwrap_or_else(|| format!("Release {version}")),
                        tags,
                        project: None,
                        url: release_url(&version),
                    });
                }
            }
            if kinds.iter().any(|k| k == "deploy") && !projects.is_empty() {
                let rows = c.query(
                    &format!(
                        "SELECT r.version, d.environment, d.name, d.date_started, d.date_finished, d.url FROM deploys d JOIN releases r ON r.id = d.release_id
                         WHERE r.org_id = $1 AND d.date_finished >= $2 AND d.date_finished <= $3
                           AND EXISTS (SELECT 1 FROM release_projects rp WHERE rp.release_id = r.id AND rp.project_id IN ({ids}))
                         ORDER BY d.date_finished DESC LIMIT {MAX_ANNOTATIONS}"
                    ),
                    &[&o.id, &from, &to],
                )?;
                for r in rows {
                    let (version, env): (String, String) = (r.get(0), r.get(1));
                    let name: Option<String> = r.get(2);
                    let started: Option<DateTime<Utc>> = r.get(3);
                    let finished: DateTime<Utc> = r.get(4);
                    let url: Option<String> = r.get(5);
                    let (time, time_end) = match started {
                        Some(s) if s < finished => (s.timestamp_millis(), Some(finished.timestamp_millis())),
                        _ => (finished.timestamp_millis(), None),
                    };
                    out.push(Annotation {
                        time,
                        time_end,
                        kind: "deploy".into(),
                        title: format!("Deploy {version} to {env}"),
                        text: name.map(|n| format!("{n}: {version} to {env}")).unwrap_or_else(|| format!("{version} deployed to {env}")),
                        tags: vec!["deploy".into(), env],
                        project: None,
                        url: url.or_else(|| release_url(&version)),
                    });
                }
            }
            if kinds.iter().any(|k| k == "new_issues") && !projects.is_empty() {
                let hours = ((to - from).num_minutes() + 59) / 60;
                let rows = c.query(
                    &format!(
                        "SELECT project_id, date_trunc('hour', first_seen) AS h, count(*) FROM issues
                         WHERE project_id IN ({ids}) AND first_seen >= $1 AND first_seen <= $2 GROUP BY project_id, h"
                    ),
                    &[&from, &to],
                )?;
                let mut per_project: BTreeMap<i64, Vec<(DateTime<Utc>, i64)>> = BTreeMap::new();
                for r in rows {
                    per_project.entry(r.get(0)).or_default().push((r.get(1), r.get(2)));
                }
                for (pid, hourly) in per_project {
                    let slug = slug_of.get(&pid).cloned().unwrap_or_default();
                    for (h, n) in spikes(&hourly, hours) {
                        let end = h + Duration::hours(1);
                        out.push(Annotation {
                            time: h.timestamp_millis(),
                            time_end: Some(end.timestamp_millis()),
                            kind: "new_issues".into(),
                            title: format!("{n} new issues in {slug}"),
                            text: format!("{n} new issues in {slug} between {} and {} UTC", h.format("%Y-%m-%d %H:%M"), end.format("%H:%M")),
                            tags: vec!["new_issues".into(), slug.clone()],
                            project: Some(slug.clone()),
                            url: Some(format!(
                                "{public_url}/organizations/{}/issues?project={pid}&query=firstSeen%3A%3E{}",
                                o.slug,
                                h.format("%Y-%m-%dT%H:%M:%SZ")
                            )),
                        });
                    }
                }
            }
            out.sort_by_key(|a| std::cmp::Reverse(a.time));
            out.truncate(MAX_ANNOTATIONS);
            Ok(out)
        })
        .await?;
    Ok(axum::Json(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spike_threshold() {
        let t = |h: i64| DateTime::from_timestamp(h * 3600, 0).unwrap();
        // 24 hours, 30 new issues: mean 2 (rounded up), threshold max(5, 6) = 6.
        let hourly = vec![(t(1), 2), (t(2), 6), (t(3), 22)];
        let s = spikes(&hourly, 24);
        assert_eq!(s, vec![(t(2), 6), (t(3), 22)]);
        // Quiet ranges still need five.
        assert!(spikes(&[(t(1), 4)], 24).is_empty());
    }

    #[test]
    fn times() {
        assert_eq!(parse_when("from", "1700000000000").unwrap().timestamp(), 1_700_000_000);
        assert_eq!(parse_when("from", "1700000000").unwrap().timestamp(), 1_700_000_000);
        assert!(parse_when("from", "2026-11-14T10:00:00Z").is_ok());
        assert!(parse_when("from", "yesterday").is_err());
    }
}
