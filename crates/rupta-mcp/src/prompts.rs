//! Prompts: step-by-step workflows that guide an agent through the tools.

use rmcp::ErrorData;
use rmcp::model::{GetPromptResult, JsonObject, Prompt, PromptArgument, PromptMessage, Role};
use serde_json::Value;

struct Def {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// (name, description, required)
    arguments: &'static [(&'static str, &'static str, bool)],
}

const PROMPTS: &[Def] = &[
    Def {
        name: "triage_new_issues",
        title: "Triage new issues",
        description: "Go through the new unresolved issues of a project (or organization), find out what they are, and propose (or, if allowed, apply) a priority, an assignee or a status for each.",
        arguments: &[
            ("org", "Organization slug", true),
            ("project", "Project slug (default: all projects)", false),
            ("period", "How far back to look, e.g. 24h or 7d (default 24h)", false),
        ],
    },
    Def {
        name: "investigate_issue",
        title: "Investigate an issue",
        description: "Find the likely cause of one issue from its events, stack trace, breadcrumbs, tags, releases, traces and logs, and suggest a fix.",
        arguments: &[
            ("issue", "Issue id, short id (e.g. WEB-31) or URL", true),
            ("org", "Organization slug (needed with a short id)", false),
        ],
    },
    Def {
        name: "check_release",
        title: "Check a release",
        description: "Summarize the health of a release: crash-free sessions and users, new issues and regressions since it was deployed.",
        arguments: &[("org", "Organization slug", true), ("version", "Release version, e.g. web@1.4.0", true)],
    },
];

pub(crate) fn list() -> Vec<Prompt> {
    PROMPTS
        .iter()
        .map(|d| {
            let args = d
                .arguments
                .iter()
                .map(|(n, desc, req)| PromptArgument::new(*n).with_description(*desc).with_required(*req))
                .collect();
            Prompt::new(d.name, Some(d.description), Some(args)).with_title(d.title)
        })
        .collect()
}

fn arg(args: &JsonObject, name: &str) -> Option<String> {
    match args.get(name)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn get(name: &str, args: JsonObject) -> Result<GetPromptResult, ErrorData> {
    let def = PROMPTS
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| ErrorData::invalid_params(format!("unknown prompt {name}"), None))?;
    for (n, _, required) in def.arguments {
        if *required && arg(&args, n).is_none() {
            return Err(ErrorData::invalid_params(format!("argument {n} is required"), None));
        }
    }
    let a = |n: &str| arg(&args, n);
    let text = match name {
        "triage_new_issues" => {
            let org = a("org").unwrap_or_default();
            let scope = match a("project") {
                Some(p) => format!("project {p:?} of organization {org:?}"),
                None => format!("organization {org:?}"),
            };
            let period = a("period").unwrap_or_else(|| "24h".into());
            let projects = a("project").map(|p| format!(", projects [{p:?}]")).unwrap_or_default();
            format!(
                "Triage the new issues of {scope} from the last {period}.\n\n\
                 1. Call search_issues with org {org:?}{projects}, query \"is:unresolved is:unassigned first_seen:-{period}\" \
                 and sort \"freq\". Also look at \"is:regressed\" and \"is:escalating\" issues of the same period.\n\
                 2. For each issue (start with the most frequent and those affecting most users): call get_issue for counts, \
                 tags (release, environment, browser, user) and linked tickets, and get_event with event \"recommended\" for \
                 the stack trace. Note the in-app frame where it fails and what the breadcrumbs show before it.\n\
                 3. Group issues that look like the same root cause. Use list_releases if a release seems to have introduced them.\n\
                 4. Propose for each issue: a priority (high: many users, crashes, payments or login; low: noise, bots, \
                 browser extensions), an owner (list_members, list_teams), and whether to resolve, ignore (with a condition) \
                 or keep it open. Present the proposal as a table first.\n\
                 5. Only after the user agrees, apply it with update_issue, and add_comment with a one-line explanation \
                 where helpful. Do not change issues the user did not approve."
            )
        }
        "investigate_issue" => {
            let issue = a("issue").unwrap_or_default();
            let org = a("org").map(|o| format!(" (organization {o:?})")).unwrap_or_default();
            format!(
                "Investigate issue {issue:?}{org} and find its likely cause.\n\n\
                 1. get_issue: title, culprit, level, status, counts, first and last seen, first and last release, the tag \
                 distribution and linked tickets. Note whether it is new, regressed or escalating.\n\
                 2. get_event with event \"recommended\" and then \"latest\": read the exception chain, the in-app frames with \
                 their source context, the breadcrumbs leading to it, the request and the contexts. Compare with event \
                 \"oldest\" to see what changed.\n\
                 3. list_issue_events (optionally with a query such as release:<version> or environment:<env>) to see whether \
                 it depends on a release, browser, user or environment; get_issue_tag_values for a tag that stands out.\n\
                 4. If the event has a trace_id, get_trace shows the request across services; search_logs with query \
                 \"trace:<trace_id>\" shows the logs of that request.\n\
                 5. If it started with a release, list_releases and get_release show when it was deployed.\n\
                 6. Explain the most likely cause with the evidence (frames, breadcrumbs, tags), suggest a fix in the code \
                 if the source context allows, and say what is still uncertain. Do not change the issue unless asked; if \
                 asked, update_issue can resolve it in the next release and add_comment can record the findings."
            )
        }
        "check_release" => {
            let org = a("org").unwrap_or_default();
            let version = a("version").unwrap_or_default();
            format!(
                "Check the health of release {version:?} in organization {org:?}.\n\n\
                 1. get_release with org {org:?} and version {version:?}: deploys, crash-free sessions and users per project, \
                 and daily sessions.\n\
                 2. search_issues with query \"first_release:{version}\" sorted by \"freq\" for the issues it introduced, and \
                 \"is:regressed release:{version}\" for regressions.\n\
                 3. get_project_health for the affected projects to compare the release with the others (adoption, crash-free \
                 rate).\n\
                 4. For the most important new issues, get_event with event \"recommended\" to understand them.\n\
                 5. Summarize: is the release healthy, which issues are new or regressed and how many users they affect, \
                 and whether a rollback or a fix looks needed."
            )
        }
        _ => unreachable!("checked above"),
    };
    Ok(GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]).with_description(def.description))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompts_fill_in_arguments_and_check_required_ones() {
        let args = |v: Value| v.as_object().cloned().unwrap();
        let r = get("investigate_issue", args(json!({"issue": "WEB-31", "org": "acme"}))).unwrap();
        let text = serde_json::to_string(&r.messages).unwrap();
        assert!(text.contains("WEB-31") && text.contains("acme") && text.contains("get_event"), "{text}");
        assert!(get("investigate_issue", args(json!({}))).is_err());
        assert!(get("nope", args(json!({}))).is_err());
        let r = get("triage_new_issues", args(json!({"org": "acme", "period": "7d"}))).unwrap();
        assert!(serde_json::to_string(&r.messages).unwrap().contains("first_seen:-7d"));
        assert_eq!(list().len(), PROMPTS.len());
    }
}
