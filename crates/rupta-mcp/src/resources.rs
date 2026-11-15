//! Resources: documentation pages an agent needs (query language, event
//! fields, grouping), and issues and events addressed by URI.

use rmcp::ErrorData;
use rmcp::model::{CacheScope, ReadResourceResult, Resource, ResourceContents, ResourceTemplate};
use serde_json::json;

use crate::rest::Rest;
use crate::tools::{self, Output};

/// (URI, name, title, description, content)
const DOCS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "rupta://docs/search",
        "search",
        "Query language",
        "The search syntax of search_issues and of the event, transaction, span and log filters: terms, keys, times.",
        include_str!("../../../docs/search.md"),
    ),
    (
        "rupta://docs/event-fields",
        "event-fields",
        "Event fields",
        "The fields of an event as get_event and the API return it: exception, stack frames, breadcrumbs, request, user, contexts, tags.",
        include_str!("../../../docs/event-fields.md"),
    ),
    (
        "rupta://docs/grouping",
        "grouping",
        "Grouping",
        "How events become issues: fingerprints, exception and in-app frames, regressions.",
        include_str!("../../../docs/grouping.md"),
    ),
];

pub(crate) fn list() -> Vec<Resource> {
    DOCS.iter()
        .map(|(uri, name, title, description, text)| {
            Resource::new(*uri, *name)
                .with_title(*title)
                .with_description(*description)
                .with_mime_type("text/markdown")
                .with_size(text.len() as u64)
        })
        .collect()
}

pub(crate) fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("rupta://issue/{id}", "issue")
            .with_title("Issue")
            .with_description(
                "An issue with its tag distribution, linked tickets and recent activity (as get_issue), as JSON.",
            )
            .with_mime_type("application/json"),
        ResourceTemplate::new("rupta://issue/{id}/event/{event}", "issue-event")
            .with_title("Event of an issue")
            .with_description(
                "An event of an issue as readable text with the stack trace (as get_event). `event` is latest, oldest, \
                 recommended or an event id.",
            )
            .with_mime_type("text/plain"),
    ]
}

pub(crate) async fn read(rest: &Rest, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    if let Some((_, _, _, _, text)) = DOCS.iter().find(|d| d.0 == uri) {
        return Ok(ReadResourceResult::new(vec![ResourceContents::text(*text, uri).with_mime_type("text/markdown")])
            .with_ttl_ms(3_600_000));
    }
    let not_found = || ErrorData::resource_not_found(format!("no resource {uri}"), Some(json!({"uri": uri})));
    let rest_of = uri.strip_prefix("rupta://issue/").ok_or_else(not_found)?;
    let (id, event) = match rest_of.split_once("/event/") {
        Some((id, ev)) => (id, Some(ev)),
        None => (rest_of, None),
    };
    if id.is_empty()
        || !id.bytes().all(|b| b.is_ascii_digit())
        || event.is_some_and(|e| e.is_empty() || e.contains('/'))
    {
        return Err(not_found());
    }
    let (tool, args) = match event {
        None => ("get_issue", json!({"issue": id})),
        Some(e) => ("get_event", json!({"issue": id, "event": e})),
    };
    let args = args.as_object().cloned().unwrap_or_default();
    let out = tools::call(rest, tool, args)
        .await
        .map_err(|e| if e.0.starts_with("Not found") { not_found() } else { ErrorData::invalid_params(e.0, None) })?;
    let contents = match out {
        Output::Json(v) => ResourceContents::text(v.to_string(), uri).with_mime_type("application/json"),
        Output::Text(t, _) => ResourceContents::text(t, uri).with_mime_type("text/plain"),
    };
    // What an issue shows depends on the caller and changes all the time.
    Ok(ReadResourceResult::new(vec![contents]).with_ttl_ms(0).with_cache_scope(CacheScope::Private))
}
