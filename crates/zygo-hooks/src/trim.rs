use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};
use zygo_config::ResultsConfig;
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_results::{view, Kind, Store, Stored};
use zygo_usage::TrimRow;
use zygo_work::hooks::units_on_session;
use zygo_work::log::{self, Kind as LogKind, LogLine};

/// §10.1: the tools whose result text is trimmed.
const HOOKED: [&str; 5] = ["Bash", "Read", "Grep", "Glob", "WebFetch"];

/// Reads `ZYGO_SESSION_DIR` and the usage database path from the environment; loads no config.
pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    if target(input).is_none() {
        return Ok(HookOutput::None);
    }
    let Some(dir) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    trim_in(input, &dir, &Paths::from_env().usage_db)
}

/// `filenames` as one text, one name per line; `None` unless it is an array of strings.
fn joined_names(resp: &Value) -> Option<String> {
    let names = resp.get("filenames")?.as_array()?;
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        out.push(n.as_str()?);
    }
    Some(out.join("\n"))
}

/// The one text field of a tool response that is measured and trimmed; `None` when the tool or shape is not hooked.
fn text_of(tool: &str, resp: &Value) -> Option<String> {
    let field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    match tool.to_ascii_lowercase().as_str() {
        "bash" => {
            if resp.get("isImage").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            field(resp, "stdout")
        }
        "read" => {
            if resp.get("type").and_then(Value::as_str) != Some("text") {
                return None;
            }
            field(resp.get("file")?, "content")
        }
        "grep" => match resp.get("mode").and_then(Value::as_str)? {
            "content" => field(resp, "content"),
            "files_with_matches" => joined_names(resp),
            _ => None,
        },
        "glob" => joined_names(resp),
        "webfetch" => field(resp, "result"),
        _ => None,
    }
}

/// `resp` with the measured text replaced by `view`; every other field is copied unchanged.
fn with_view(tool: &str, resp: &Value, view: &str) -> Value {
    let mut out = resp.clone();
    let files = |out: &mut Value| {
        let names: Vec<Value> = view.lines().map(|l| json!(l)).collect();
        out["numFiles"] = json!(names.len());
        out["filenames"] = Value::Array(names);
    };
    match tool.to_ascii_lowercase().as_str() {
        "bash" => out["stdout"] = json!(view),
        "read" => out["file"]["content"] = json!(view),
        "grep" if resp.get("mode").and_then(Value::as_str) == Some("content") => out["content"] = json!(view),
        "grep" | "glob" => files(&mut out),
        "webfetch" => out["result"] = json!(view),
        _ => {}
    }
    out
}

/// The tool name and measured text of a hooked `PostToolUse` input.
fn target(input: &HookInput) -> Option<(String, String)> {
    if input.event.as_deref() != Some("PostToolUse") {
        return None;
    }
    let tool = input.tool_name.as_deref()?;
    if !HOOKED.iter().any(|h| h.eq_ignore_ascii_case(tool)) {
        return None;
    }
    let text = text_of(tool, input.raw.get("tool_response")?)?;
    Some((tool.to_owned(), text))
}

/// §10.1: store a result over the threshold in the session's results directory and return the head/tail view in its place.
pub fn trim_in(input: &HookInput, dir: &SessionDir, usage_db: &Path) -> anyhow::Result<HookOutput> {
    let Some((tool, text)) = target(input) else { return Ok(HookOutput::None) };
    let sv = SessionView::load(dir)?;
    // §17.1: `overrides.json` is read on every call.
    let ov = Overrides::load(dir);
    let threshold = ov.results_threshold.unwrap_or(sv.trim_threshold_bytes);
    if !ov.results.unwrap_or(sv.results) || text.len() as u64 <= threshold {
        return Ok(HookOutput::None);
    }
    let cfg = ResultsConfig { trim_threshold_bytes: threshold, head_lines: sv.head_lines, tail_lines: sv.tail_lines };
    // The marker length does not depend on the id's value, so a placeholder id sizes the view.
    let preview = Stored { id: "0".repeat(26), tool: tool.clone(), bytes: text.len() as u64, lines: text.lines().count() as u64,
        kind: Kind::Text, created_at: 0, session: None, data: text.as_bytes().to_vec() };
    if view(&preview, &cfg).len() >= text.len() {
        return Ok(HookOutput::None);
    }
    // C3: the store's own error text holds paths, so the error is a fixed text.
    let stored = Store::put(&dir.results(), &tool, Some(&sv.session), text.as_bytes()).map_err(|_| anyhow::anyhow!("cannot store the tool result"))?;
    let v = view(&stored, &cfg);
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    let row = TrimRow {
        ts,
        session: sv.session,
        agent_id: input.agent_id.clone(),
        tool: tool.clone(),
        kind: stored.kind.as_str().into(),
        stored_bytes: stored.bytes,
        view_bytes: v.len() as u64,
    };
    // C6: a bookkeeping failure never costs the agent its view.
    if zygo_usage::Store::open(usage_db).and_then(|u| u.insert_trim(&row)).is_err() {
        note("usage store unavailable");
    }
    let cwd = input.raw.get("cwd").and_then(Value::as_str).unwrap_or(".");
    // An `agent_id` that is present but not a string names no agent: no line, as in N24's hooks.
    let agent = match input.raw.get("agent_id") {
        None => Some("main"),
        Some(_) => input.agent_id.as_deref(),
    };
    if let Some(agent) = agent
        && units_on_session(dir, Path::new(cwd))
        && log::append(&dir.work(), agent, &LogLine { n: 0, ts, kind: LogKind::Trimmed }).is_err() {
            note("work log unavailable");
        }
    let resp = input.raw.get("tool_response").cloned().unwrap_or(Value::Null);
    Ok(HookOutput::Json(json!({"hookSpecificOutput": {
        "hookEventName": "PostToolUse",
        "updatedToolOutput": with_view(&tool, &resp, &v),
    }})))
}

/// One fixed stderr line; never tool output (C3).
fn note(msg: &str) {
    let _ = writeln!(std::io::stderr(), "zygo hook: {msg}");
}
