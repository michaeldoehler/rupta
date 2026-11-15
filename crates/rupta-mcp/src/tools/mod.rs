//! The tools: their definitions (name, description, input schema,
//! annotations) and dispatch. Each tool maps to one or a few REST API
//! requests made as the caller (see [`crate::rest`]).

mod issues;
mod ops;
mod org;
mod releases;
mod telemetry;
mod tickets;

use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::rest::{Rest, ToolError, ToolResult};

/// Results longer than this many characters are shortened.
pub(crate) const MAX_RESULT_CHARS: usize = 50_000;

/// What a tool returns.
pub(crate) enum Output {
    /// A JSON object, returned as structured content and as its text.
    Json(Value),
    /// Readable text, with an optional JSON object as structured content.
    Text(String, Option<Value>),
}

impl Output {
    pub fn into_result(self) -> CallToolResult {
        match self {
            Output::Json(v) => {
                let v = fit(v);
                let mut r = CallToolResult::success(vec![ContentBlock::text(v.to_string())]);
                r.structured_content = Some(v);
                r
            }
            Output::Text(t, structured) => {
                let mut r =
                    CallToolResult::success(vec![ContentBlock::text(crate::format::truncate(&t, MAX_RESULT_CHARS))]);
                r.structured_content = structured.map(fit);
                r
            }
        }
    }
}

/// Shortens the `data` list of a result until it fits [`MAX_RESULT_CHARS`].
fn fit(mut v: Value) -> Value {
    let size = |v: &Value| v.to_string().len();
    if size(&v) <= MAX_RESULT_CHARS {
        return v;
    }
    let mut dropped = 0;
    if let Some(list) = v.get_mut("data").and_then(Value::as_array_mut) {
        while list.len() > 1 {
            list.pop();
            dropped += 1;
            if serde_json::to_string(list).map(|s| s.len()).unwrap_or(0) + 500 <= MAX_RESULT_CHARS {
                break;
            }
        }
    }
    if dropped > 0 {
        v["truncated"] = json!(format!(
            "{dropped} more items were left out to keep the result small; ask for fewer with limit or narrow the query. The next_cursor still continues after the last item of the full page."
        ));
    }
    if size(&v) > MAX_RESULT_CHARS {
        let text = crate::format::truncate(&v.to_string(), MAX_RESULT_CHARS);
        return json!({"truncated": "the result was too large and was cut", "text": text});
    }
    v
}

/// How a tool affects rupta.
#[derive(Clone, Copy)]
enum Effect {
    /// Reads only.
    Read,
    /// Changes something in rupta (reversible).
    Change { idempotent: bool },
    /// Creates something outside rupta (a ticket in an issue tracker).
    External,
}

fn tool<T: JsonSchema + 'static>(name: &'static str, title: &str, description: &'static str, effect: Effect) -> Tool {
    let a = ToolAnnotations::with_title(title);
    let a = match effect {
        Effect::Read => a.read_only(true).destructive(false).idempotent(true).open_world(false),
        Effect::Change { idempotent } => a.read_only(false).destructive(false).idempotent(idempotent).open_world(false),
        Effect::External => a.read_only(false).destructive(false).idempotent(false).open_world(true),
    };
    Tool::new(name, description, input_schema::<T>()).with_title(title).with_annotations(a)
}

/// The JSON schema of a tool's arguments. Subschemas (enums, nested
/// objects) are inlined: several clients do not resolve `$ref`.
fn input_schema<T: JsonSchema>() -> std::sync::Arc<JsonObject> {
    let generator =
        schemars::generate::SchemaSettings::draft2020_12().with(|s| s.inline_subschemas = true).into_generator();
    let schema = generator.into_root_schema_for::<T>();
    let Value::Object(mut o) = serde_json::to_value(schema).expect("schemas serialize") else {
        panic!("a tool's input schema must be an object");
    };
    for k in ["$schema", "title", "description"] {
        o.remove(k);
    }
    o.entry("properties").or_insert_with(|| json!({}));
    if let Some(Value::Object(props)) = o.get_mut("properties") {
        for p in props.values_mut() {
            not_null(p);
        }
    }
    std::sync::Arc::new(o)
}

/// Optional arguments are left out rather than sent as `null`: drops the
/// `null` alternative schemars adds for `Option`, which several clients
/// that expect a single `type` refuse.
fn not_null(v: &mut Value) {
    let Some(o) = v.as_object_mut() else { return };
    if let Some(Value::Array(types)) = o.get("type") {
        let rest: Vec<Value> = types.iter().filter(|t| *t != "null").cloned().collect();
        if rest.len() == 1 {
            o.insert("type".into(), rest[0].clone());
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(alts)) = o.get(key) {
            let rest: Vec<Value> = alts.iter().filter(|a| a.get("type") != Some(&json!("null"))).cloned().collect();
            if rest.len() == 1 && rest.len() < alts.len() {
                o.remove(key);
                if let Value::Object(inner) = &rest[0] {
                    for (k, x) in inner {
                        o.entry(k.clone()).or_insert_with(|| x.clone());
                    }
                }
            } else if rest.len() < alts.len() {
                o.insert(key.into(), Value::Array(rest));
            }
        }
    }
    if let Some(Value::Array(values)) = o.get_mut("enum") {
        values.retain(|x| !x.is_null());
    }
    if o.get("default") == Some(&Value::Null) {
        o.remove("default");
    }
    if o.get("type") == Some(&json!("integer")) {
        o.remove("format");
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(alts)) = o.get_mut(key) {
            alts.iter_mut().for_each(not_null);
        }
    }
    if let Some(items) = o.get_mut("items") {
        not_null(items);
    }
}

/// Every tool, in the order clients show them.
pub(crate) fn definitions() -> Vec<Tool> {
    let mut out = Vec::new();
    out.extend(org::definitions());
    out.extend(issues::definitions());
    out.extend(releases::definitions());
    out.extend(telemetry::definitions());
    out.extend(ops::definitions());
    out.extend(tickets::definitions());
    out
}

/// Runs a tool.
pub(crate) async fn call(rest: &Rest, name: &str, args: JsonObject) -> ToolResult<Output> {
    let a = Value::Object(args);
    if let Some(r) = org::call(rest, name, &a).await {
        return r;
    }
    if let Some(r) = issues::call(rest, name, &a).await {
        return r;
    }
    if let Some(r) = releases::call(rest, name, &a).await {
        return r;
    }
    if let Some(r) = telemetry::call(rest, name, &a).await {
        return r;
    }
    if let Some(r) = ops::call(rest, name, &a).await {
        return r;
    }
    if let Some(r) = tickets::call(rest, name, &a).await {
        return r;
    }
    Err(ToolError::new(format!("unknown tool {name}")))
}

/// Parses the arguments of a tool.
pub(crate) fn args<T: DeserializeOwned>(a: &Value) -> ToolResult<T> {
    serde_json::from_value(a.clone()).map_err(|e| ToolError::new(format!("Invalid arguments: {e}")))
}

/// Accepts a string or a number (agents send ids either way).
pub(crate) fn string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum S {
        S(String),
        N(i64),
    }
    Ok(match S::deserialize(d)? {
        S::S(s) => s.trim().to_string(),
        S::N(n) => n.to_string(),
    })
}

pub(crate) fn opt_string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum S {
        S(String),
        N(i64),
    }
    Ok(Option::<S>::deserialize(d)?.map(|s| match s {
        S::S(s) => s.trim().to_string(),
        S::N(n) => n.to_string(),
    }))
}

/// One or several values: `"web"`, `"web,api"` or `["web", "api"]`.
pub(crate) fn opt_list<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum L {
        One(String),
        Num(i64),
        Many(Vec<Value>),
    }
    let split = |s: &str| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
    Ok(match Option::<L>::deserialize(d)? {
        None => None,
        Some(L::One(s)) => Some(split(&s)),
        Some(L::Num(n)) => Some(vec![n.to_string()]),
        Some(L::Many(v)) => Some(
            v.iter()
                .filter_map(|x| match x {
                    Value::String(s) => Some(s.trim().to_string()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .filter(|s| !s.is_empty())
                .collect(),
        ),
    }
    .filter(|v| !v.is_empty()))
}

/// A path segment, percent-encoded.
pub(crate) fn seg(s: &str) -> String {
    if s.is_empty() || s == "." || s == ".." {
        return s.replace('.', "%2E");
    }
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `limit` within the API's bounds.
pub(crate) fn limit(l: Option<u32>, default: u32, max: u32) -> Option<String> {
    Some(l.unwrap_or(default).clamp(1, max).to_string())
}

/// Keeps only the listed keys of an object.
pub(crate) fn pick(v: &Value, keys: &[&str]) -> Value {
    let mut out = serde_json::Map::new();
    for k in keys {
        if let Some(x) = v.get(*k) {
            out.insert((*k).to_string(), x.clone());
        }
    }
    Value::Object(out)
}

/// A page `{data, next_cursor}` with each item mapped.
pub(crate) fn page(v: &Value, f: impl Fn(&Value) -> Value) -> Value {
    let data: Vec<Value> = v["data"].as_array().map(|a| a.iter().map(&f).collect()).unwrap_or_default();
    json!({"data": data, "next_cursor": v["next_cursor"]})
}

/// A plain list wrapped as `{data}` (structured content must be an object).
pub(crate) fn list(v: &Value, f: impl Fn(&Value) -> Value) -> Value {
    let data: Vec<Value> = v.as_array().map(|a| a.iter().map(&f).collect()).unwrap_or_default();
    json!({ "data": data })
}

/// The ids of projects given by slug or id (the issue and release lists
/// take ids only).
pub(crate) async fn project_ids(rest: &Rest, org: &str, wanted: &[String]) -> ToolResult<Vec<String>> {
    let all = rest.get(&format!("/api/v1/orgs/{}/projects", seg(org)), &[]).await?;
    let mut out = Vec::new();
    for w in wanted {
        let found = all.as_array().into_iter().flatten().find(|p| p["slug"] == w.as_str() || p["id"] == w.as_str());
        match found {
            Some(p) => out.push(p["id"].as_str().unwrap_or_default().to_string()),
            None => {
                return Err(ToolError::new(format!("No project {w:?} in organization {org:?}; see list_projects.")));
            }
        }
    }
    Ok(out)
}

/// Shared input schema of tools that take no arguments.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct NoArgs {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(seg("web@1.0.0+build/2"), "web%401.0.0%2Bbuild%2F2");
        assert_eq!(seg(".."), "%2E%2E");
        assert_eq!(seg("acme"), "acme");
    }

    #[test]
    fn every_tool_has_an_object_schema_and_annotations() {
        let tools = definitions();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "tool names are unique");
        for t in &tools {
            assert_eq!(t.input_schema.get("type"), Some(&json!("object")), "{}", t.name);
            let schema = Value::Object(t.input_schema.as_ref().clone()).to_string();
            assert!(!schema.contains("$ref") && !schema.contains("$defs"), "{}: {schema}", t.name);
            assert!(!schema.contains("\"null\""), "{}: {schema}", t.name);
            let a = t.annotations.as_ref().expect("annotations");
            assert!(a.read_only_hint.is_some() && a.destructive_hint.is_some(), "{}", t.name);
            assert!(t.description.as_ref().is_some_and(|d| d.len() > 40), "{}", t.name);
            assert!(t.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", t.name);
        }
    }

    #[test]
    fn large_lists_are_shortened() {
        let big: Vec<Value> = (0..2000).map(|i| json!({"id": i.to_string(), "title": "x".repeat(100)})).collect();
        let v = fit(json!({"data": big, "next_cursor": "c"}));
        assert!(v.to_string().len() <= MAX_RESULT_CHARS);
        assert!(v["truncated"].as_str().unwrap().contains("more items"));
        assert_eq!(v["next_cursor"], "c");
    }

    #[test]
    fn lists_accept_strings_arrays_and_numbers() {
        #[derive(Deserialize)]
        struct A {
            #[serde(default, deserialize_with = "opt_list")]
            p: Option<Vec<String>>,
        }
        let p = |v: Value| serde_json::from_value::<A>(v).unwrap().p;
        assert_eq!(p(json!({"p": "web, api"})), Some(vec!["web".into(), "api".into()]));
        assert_eq!(p(json!({"p": ["web", 3]})), Some(vec!["web".into(), "3".into()]));
        assert_eq!(p(json!({"p": 7})), Some(vec!["7".into()]));
        assert_eq!(p(json!({})), None);
        assert_eq!(p(json!({"p": ""})), None);
    }
}
