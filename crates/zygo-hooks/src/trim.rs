use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use zygo_config::ResultsConfig;
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_results::kind::sniff;
use zygo_results::shape::{self, command, ShapeInput, Shaped, Shaper, MIN_SAVING_BYTES};
use zygo_results::view::{shaped_view, size_view};
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

/// O§7.8 "When PO2 fails": whether the agent shows a cleared `stderr` as empty. OS-34 sets it to the PO2 result.
pub const PO2_STDERR_CLEARED: bool = true;

/// Options of the trim hook.
#[derive(Debug, Clone, Copy)]
pub struct TrimOpts {
    /// O§7.11: how long the usage write waits on a busy database before the row is dropped.
    pub db_bound: Duration,
    /// O§7.8: true shapes stdout and stderr together and clears `stderr`; false shapes stdout only.
    pub stderr_cleared: bool,
    /// Test seam: steps 1-4 panic (`ZYGO_TEST_SHAPE_PANIC=1`).
    #[cfg(feature = "test-hooks")]
    pub shape_panic: bool,
}

impl Default for TrimOpts {
    fn default() -> Self {
        TrimOpts {
            db_bound: zygo_usage::store::HOOK_DB_BOUND,
            stderr_cleared: PO2_STDERR_CLEARED,
            #[cfg(feature = "test-hooks")]
            shape_panic: std::env::var_os("ZYGO_TEST_SHAPE_PANIC").is_some_and(|v| v == "1"),
        }
    }
}

/// §10.1: store a result over the threshold in the session's results directory and return the head/tail view in its place.
pub fn trim_in(input: &HookInput, dir: &SessionDir, usage_db: &Path) -> anyhow::Result<HookOutput> {
    trim_in_with(input, dir, usage_db, &TrimOpts::default())
}

/// The O§7.2 raw text: `stdout`, then `[stderr]` and `stderr` on a line of their own when `stderr` is not empty.
fn compose(stdout: &str, stderr: &str) -> String {
    let mut raw = stdout.to_string();
    if !stderr.is_empty() {
        if !raw.is_empty() && !raw.ends_with('\n') {
            raw.push('\n');
        }
        raw.push_str("[stderr]\n");
        raw.push_str(stderr);
    }
    raw
}

/// O§7.10: steps 1-4 under `catch_unwind`; a panic gives the raw text with only the size view.
#[cfg_attr(not(feature = "test-hooks"), allow(unused_variables))]
fn shape_guarded(src: &str, si: &ShapeInput, opts: &TrimOpts) -> Shaped {
    let steps = || {
        #[cfg(feature = "test-hooks")]
        if opts.shape_panic {
            panic!("forced by ZYGO_TEST_SHAPE_PANIC");
        }
        shape::run(src, si)
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(steps)).unwrap_or_else(|_| {
        let over = src.len() as u64 > si.threshold;
        Shaped { text: src.to_string(), steps: vec![], shaper: over.then_some(Shaper::Size), size_view: over }
    })
}

/// A result that may get a view: the stored raw text, the counted bytes, and the shaping result (`None` on the shape-off path).
struct Pending {
    raw: String,
    counted: u64,
    shaped: Option<Shaped>,
    command: Option<String>,
    shaper: Shaper,
    clear_stderr: bool,
}

/// O§7.1, O§7.2, O§9: routes the call to the shape-on or shape-off path; `None` when the output stays as it is.
fn pending(input: &HookInput, text: String, sv: &SessionView, threshold: u64, opts: &TrimOpts) -> Option<Pending> {
    let bash = input.tool_name.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("bash"));
    let cmd = input.tool_input.as_ref().and_then(|t| t.get("command")).and_then(Value::as_str);
    let parsed = cmd.filter(|_| bash).map(command::parse);
    let command = bash.then(|| parsed.as_ref().map_or_else(|| "other".to_string(), |p| p.name.clone()));
    if !bash || !sv.shape || parsed.as_ref().is_some_and(|p| p.file_printer) {
        if text.len() as u64 <= threshold {
            return None;
        }
        return Some(Pending { counted: text.len() as u64, raw: text, shaped: None, command, shaper: Shaper::Size, clear_stderr: false });
    }
    let resp = input.raw.get("tool_response");
    let raw = compose(&text, resp.and_then(|r| r.get("stderr")).and_then(Value::as_str).unwrap_or(""));
    let src = if opts.stderr_cleared { raw.as_str() } else { text.as_str() };
    let si = ShapeInput { parsed, kind: sniff(raw.as_bytes()), threshold };
    let shaped = shape_guarded(src, &si, opts);
    let shaper = shaped.shaper?;
    let counted = src.len() as u64;
    let clear_stderr = opts.stderr_cleared && resp.is_some_and(|r| r.get("stderr").is_some());
    Some(Pending { raw, counted, shaped: Some(shaped), command, shaper, clear_stderr })
}

/// O§7.8: the view of `p` with the marker of `s`, the stored raw text.
fn view_for(p: &Pending, s: &Stored, cfg: &ResultsConfig) -> String {
    match &p.shaped {
        None => view(s, cfg),
        Some(sh) if sh.size_view => size_view(&sh.text, s, cfg),
        Some(sh) => shaped_view(&sh.steps, &sh.text, s),
    }
}

/// Shape off: the view is smaller than the text. Shape on: the O§7.8 use rule, on the final view.
fn used(p: &Pending, tool: &str, cfg: &ResultsConfig) -> bool {
    // The marker length does not depend on the id's value, so a placeholder id sizes the view.
    let preview = Stored { id: "0".repeat(26), tool: tool.to_string(), bytes: p.raw.len() as u64, lines: p.raw.lines().count() as u64,
        kind: Kind::Text, created_at: 0, session: None, data: p.raw.as_bytes().to_vec() };
    let pv = view_for(p, &preview, cfg).len() as u64;
    if p.shaped.is_none() { pv < p.counted } else { p.counted.saturating_sub(pv) >= MIN_SAVING_BYTES as u64 }
}

/// §10.1, O§7.8: store a result in the session's results directory and return its shaped or head/tail view in its place.
pub fn trim_in_with(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput> {
    let Some((tool, text)) = target(input) else { return Ok(HookOutput::None) };
    let sv = SessionView::load(dir)?;
    // §17.1: `overrides.json` is read on every call.
    let ov = Overrides::load(dir);
    let threshold = ov.results_threshold.unwrap_or(sv.trim_threshold_bytes);
    if !ov.results.unwrap_or(sv.results) {
        return Ok(HookOutput::None);
    }
    let Some(p) = pending(input, text, &sv, threshold, opts) else { return Ok(HookOutput::None) };
    let cfg = ResultsConfig { trim_threshold_bytes: threshold, head_lines: sv.head_lines, tail_lines: sv.tail_lines, shape: sv.shape };
    if !used(&p, &tool, &cfg) {
        return Ok(HookOutput::None);
    }
    // C3: the store's own error text holds paths, so the error is a fixed text.
    let stored = Store::put(&dir.results(), &tool, Some(&sv.session), p.raw.as_bytes()).map_err(|_| anyhow::anyhow!("cannot store the tool result"))?;
    let v = view_for(&p, &stored, &cfg);
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    let row = TrimRow {
        ts,
        session: sv.session,
        agent_id: input.agent_id.clone(),
        tool: tool.clone(),
        kind: stored.kind.as_str().into(),
        stored_bytes: p.counted,
        view_bytes: v.len() as u64,
        result_id: Some(stored.id.clone()),
        command: p.command.clone(),
        shaper: Some(p.shaper.as_str().into()),
    };
    // C6, O§7.11: busy past the bound or broken: no row, the view stands.
    if zygo_usage::Store::open_bounded(usage_db, opts.db_bound).and_then(|u| u.insert_trim(&row)).is_err() {
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
    let mut updated = with_view(&tool, &resp, &v);
    if p.clear_stderr {
        updated["stderr"] = json!("");
    }
    Ok(HookOutput::Json(json!({"hookSpecificOutput": {
        "hookEventName": "PostToolUse",
        "updatedToolOutput": updated,
    }})))
}

/// One fixed stderr line; never tool output (C3).
fn note(msg: &str) {
    let _ = writeln!(std::io::stderr(), "zygo hook: {msg}");
}
