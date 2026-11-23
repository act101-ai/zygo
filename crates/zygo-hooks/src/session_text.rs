//! §8.6 session text and the mechanics text shared with the subagent prompts.

use zygo_core::session_view::SessionView;
use zygo_core::Agent;

const REDACT: &str = "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file.";
/// §8.6 item 7: the attribution sentence, always in (operator, 2026-11-22).
const ATTRIBUTION: &str = "Never add Claude attribution to commits, pull request titles or bodies, tags, release notes or comments: no `Co-Authored-By: Claude` trailer, no `noreply@anthropic.com` address, no `Generated with Claude Code` line and no Claude session link.";
const STORE: &str = "Large tool outputs are stored by zygo; read them with result_view, result_search or result_tree using the id in the marker; do not shorten output with head or tail.";
const NAV: &str = "Use act-lite's skeleton, symbols or repo_outline before reading a whole source file.";
const TRAILER: &str = "Add the commit trailers `Zygo-Session: <session>` and, when your brief names a unit, `Zygo-Work: <unit>`, each on its own line at the end of the message.";
const ALIAS_HEAD: &str = "A skill names a role or a capability: pick the subagent by the legend.";
const ALIAS_MID: &str = "When a skill still names a model, use the subagent instead:";
const ALIAS_TAIL: &str = "Do not set a model on the subagent call.";
const TEXT_ROLES: [&str; 3] = ["plan", "triage", "review"];

/// §8.6 item 7: reads only the view; the attribution sentence is always in.
pub fn mechanics(v: &SessionView) -> String {
    let mut parts = vec![REDACT, ATTRIBUTION];
    if v.results {
        parts.push(STORE);
    }
    if v.navigation {
        parts.push(NAV);
    }
    if v.work_units {
        parts.push(TRAILER);
    }
    parts.join(" ")
}

/// §8.6 items 1 to 7, joined by a blank line.
pub fn build(v: &SessionView) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut put = |s: Option<String>| {
        if let Some(s) = s.filter(|s| !s.is_empty()) {
            parts.push(s);
        }
    };
    put(v.role_prompt.clone());
    put(v.user_prompt.clone());
    put(v.profile_prompt.clone());
    put(v.work_item.as_ref().map(|w| match w.title.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => format!("Work item: {} — {t}", w.unit),
        None => format!("Work item: {}", w.unit),
    }));
    // Bundle roles first, custom roles after; view order inside each group.
    let legend: Vec<String> = [false, true]
        .into_iter()
        .flat_map(|custom| v.legend.iter().filter(move |e| e.custom == custom))
        .filter(|e| enabled(v, &e.role))
        .map(|e| match e.description.trim() {
            "" => format!("zygo-{}", e.role),
            _ => format!("zygo-{}: {}", e.role, e.description),
        })
        .collect();
    put(Some(legend.join("\n")));
    put(alias(v));
    put(capabilities(v));
    put(Some(mechanics(v)));
    parts.join("\n\n")
}

fn enabled(v: &SessionView, role: &str) -> bool {
    role != v.main_role && !v.disabled_roles.iter().any(|d| d == role)
}

/// `a`, `a` or `b`, `a`, `b` or `c`, each name in backticks.
fn list(names: &[&str]) -> String {
    let q: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match q.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

fn text_roles(v: &SessionView) -> Vec<String> {
    TEXT_ROLES.iter().filter(|r| enabled(v, r)).map(|r| format!("zygo-{r}")).collect()
}

fn alias(v: &SessionView) -> Option<String> {
    if v.agent != Agent::ClaudeCode {
        return None;
    }
    let mut clauses: Vec<String> = Vec::new();
    if enabled(v, "read") {
        clauses.push("`haiku` is `zygo-read`".into());
    }
    if enabled(v, "code") {
        clauses.push("`sonnet` is `zygo-code`".into());
    }
    let t = text_roles(v);
    if !t.is_empty() {
        let names: Vec<&str> = t.iter().map(String::as_str).collect();
        clauses.push(format!("`opus` is the premium subagent that fits the task ({})", list(&names)));
    }
    if enabled(v, "design") {
        clauses.push("`fable` is `zygo-design`".into());
    }
    Some(if clauses.is_empty() {
        format!("{ALIAS_HEAD} {ALIAS_TAIL}")
    } else {
        format!("{ALIAS_HEAD} {ALIAS_MID} {}. {ALIAS_TAIL}", clauses.join(", "))
    })
}

fn capabilities(v: &SessionView) -> Option<String> {
    let mut clauses: Vec<String> = Vec::new();
    if enabled(v, "read") {
        clauses.push("`lookup` is `zygo-read`".into());
    }
    if enabled(v, "code") {
        clauses.push("`mechanical` and `implementation` are `zygo-code`".into());
    }
    let mut judgment: Vec<String> = Vec::new();
    if enabled(v, "design") {
        judgment.push("`zygo-design` for a design question".into());
    }
    let t = text_roles(v);
    if !t.is_empty() {
        let names: Vec<&str> = t.iter().map(String::as_str).collect();
        judgment.push(format!("{} for text", list(&names)));
    }
    if !judgment.is_empty() {
        clauses.push(format!("`judgment` is {}", judgment.join(" and ")));
    }
    if clauses.is_empty() {
        None
    } else {
        Some(format!("{}.", clauses.join("; ")))
    }
}
