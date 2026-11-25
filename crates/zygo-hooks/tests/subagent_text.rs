//! SX-1: the Claude Code subagent branch of `zygo hook session-start` (session text design T§6.3 item 2, T§9.1,
//! T§14 item 1, T§17 checks 2-6 and 12; T-C1, T-C5). No test spawns `claude` or `opencode` or reads a clock; the only
//! child process is this test binary.

use std::cell::RefCell;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use zygo_core::{
    hook::{guarded, HookInput},
    overrides::Overrides,
    paths::Paths,
    session_dir::SessionDir,
    session_view::SessionView,
    Agent,
};
use zygo_hooks::{session_start, session_text::mechanics};

const REDACT: &str = "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file.";
const ATTRIBUTION: &str = "Never add Claude attribution to commits, pull request titles or bodies, tags, release notes or comments: no `Co-Authored-By: Claude` trailer, no `noreply@anthropic.com` address, no `Generated with Claude Code` line and no Claude session link.";
const STORE: &str = "Large tool outputs are stored by zygo; read them with result_view, result_search or result_tree using the id in the marker; do not shorten output with head or tail.";
const NAV: &str = "Use act-lite's skeleton, symbols or repo_outline before reading a whole source file.";
const TRAILER: &str = "Add the commit trailers `Zygo-Session: <session>` and, when your brief names a unit, `Zygo-Work: <unit>`, each on its own line at the end of the message.";
const CHILD: &str = "ZYGO_SX1_C6_CHILD";

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/claude-code/hooks/{name}.json"));
    serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn with(mut v: Value, key: &str, val: Value) -> Value {
    v.as_object_mut().unwrap().insert(key.into(), val);
    v
}

fn without(mut v: Value, key: &str) -> Value {
    v.as_object_mut().unwrap().remove(key);
    v
}

fn input(v: &Value) -> HookInput {
    HookInput::parse(Agent::ClaudeCode, &serde_json::to_vec(v).unwrap()).unwrap()
}

/// Results, navigation and units on; no workspace.
fn view() -> SessionView {
    SessionView {
        session: "S1".into(),
        agent: Agent::ClaudeCode,
        main_role: "lead".into(),
        role_prompt: Some("ROLE".into()),
        results: true,
        navigation: true,
        work_units: true,
        compact_tool: true,
        trim_threshold_bytes: 16384,
        head_lines: 60,
        tail_lines: 20,
        ..Default::default()
    }
}

fn sd() -> (tempfile::TempDir, SessionDir) {
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().join("s"));
    d.create().unwrap();
    (t, d)
}

/// `data_dir` = `<p>/data` is not under `$HOME`, so the state folder is `<p>/data/state`.
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

/// The stdout of `run_in` and of `run_member` (no `--event member`, `zygo` = a path that does not exist); asserts
/// that both are equal and that `run_member` wrote nothing to `out`.
fn both(t: &Path, d: &SessionDir, i: &HookInput) -> String {
    let a = session_start::run_in(i, d).unwrap().to_stdout();
    let mut out = Vec::new();
    let b = session_start::run_member(i, d, &paths(t), &t.join("no-zygo"), &mut out).unwrap().to_stdout();
    assert!(out.is_empty());
    assert_eq!(a, b);
    a
}

fn envelope(m: &str) -> String {
    json!({"hookSpecificOutput": {"hookEventName": "SubagentStart", "additionalContext": m}}).to_string()
}

#[test]
fn subagent_start_claude_type_gets_mechanics() {
    // T§17 check 2
    let (t, d) = sd();
    let v = view();
    v.save(&d).unwrap();
    let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
    assert_eq!(m.len(), 721);
    assert_eq!(mechanics(&v), m);
    assert_eq!(session_start::mechanics_with_overrides(&d, &v), m);
    assert_eq!(fixture("SubagentStart-sub")["agent_type"], json!("claude"));
    assert_eq!(both(t.path(), &d, &input(&fixture("SubagentStart-sub"))), envelope(&m));
}

#[test]
fn subagent_start_other_types_get_the_same() {
    // T§17 check 3
    let (t, d) = sd();
    view().save(&d).unwrap();
    let want = envelope(&[REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" "));
    let sub = fixture("SubagentStart-sub");
    for ty in [json!("Explore"), json!("Plan"), json!("general-purpose"), json!("p315:probe"), json!(1)] {
        assert_eq!(both(t.path(), &d, &input(&with(sub.clone(), "agent_type", ty.clone()))), want, "{ty}");
    }
    assert_eq!(both(t.path(), &d, &input(&without(sub, "agent_type"))), want, "no agent_type");
}

#[test]
fn subagent_start_zygo_type_gets_nothing() {
    // T§17 check 3
    let (t, d) = sd();
    view().save(&d).unwrap();
    assert_eq!(both(t.path(), &d, &input(&with(fixture("SubagentStart-sub"), "agent_type", json!("zygo-code")))), "");
}

#[test]
fn subagent_start_overrides_off_gives_redaction_and_attribution() {
    // T§17 check 4
    let (t, d) = sd();
    let v = SessionView { navigation: false, ..view() };
    v.save(&d).unwrap();
    Overrides { results: Some(false), work_units: Some(false), ..Default::default() }.save(&d).unwrap();
    let m = format!("{REDACT} {ATTRIBUTION}");
    assert_eq!(m.len(), 317);
    assert_eq!(session_start::mechanics_with_overrides(&d, &v), m);
    assert_eq!(both(t.path(), &d, &input(&fixture("SubagentStart-sub"))), envelope(&m));
}

#[test]
fn subagent_start_sub_session_start_compact_gets_nothing() {
    // T§17 check 5
    let (t, d) = sd();
    view().save(&d).unwrap();
    let sub = fixture("SessionStart-sub");
    assert_eq!((sub["source"].as_str(), sub.get("agent_id").is_some()), (Some("compact"), true));
    let before = std::fs::read(d.view_json()).unwrap();
    assert_eq!(both(t.path(), &d, &input(&sub)), "");
    // The member part did not run.
    assert_eq!(std::fs::read(d.view_json()).unwrap(), before);
    assert!(!d.log().exists());
}

#[test]
fn subagent_start_unparseable_view_one_stderr_line() {
    // T§17 check 12, T§14 item 1
    let (t, d) = sd();
    std::fs::write(d.view_json(), "{").unwrap();
    let i = input(&fixture("SubagentStart-sub"));
    let mut out = Vec::new();
    assert!(session_start::run_member(&i, &d, &paths(t.path()), &t.path().join("no-zygo"), &mut out).is_err());
    assert!(session_start::run_in(&i, &d).is_err());
    assert!(out.is_empty());
    // `guarded` writes to the process stderr: run the child test in a child process and read its stderr.
    let o = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "c6_child_unparseable_view", "--nocapture", "--test-threads=1"])
        .env(CHILD, t.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(String::from_utf8(o.stdout).unwrap().contains("c6_child_unparseable_view ... ok"));
    let err = String::from_utf8(o.stderr).unwrap();
    assert_eq!(err.lines().count(), 1, "{err:?}");
    assert!(err.starts_with("zygo hook: ") && err.ends_with('\n'), "{err:?}");
}

/// Child half of `subagent_start_unparseable_view_one_stderr_line`; returns at once without `CHILD`.
#[test]
#[ignore]
fn c6_child_unparseable_view() {
    let Some(root) = std::env::var_os(CHILD) else { return };
    let t = Path::new(&root);
    let d = SessionDir::new(t.join("s"));
    let stdin = serde_json::to_vec(&fixture("SubagentStart-sub")).unwrap();
    // `guarded` takes a `Fn`, so the output buffer is a `RefCell`.
    let out = RefCell::new(Vec::new());
    let got = guarded(
        &|b| {
            session_start::run_member(&HookInput::parse(Agent::ClaudeCode, b)?, &d, &paths(t), &t.join("no-zygo"),
                &mut *out.borrow_mut())
        },
        &stdin,
    );
    assert_eq!(got, (String::new(), 0));
    assert!(out.borrow().is_empty());
}

#[test]
fn main_compact_output_equals_startup() {
    // T§17 check 6, T§9.1
    let (t, d) = sd();
    view().save(&d).unwrap();
    let startup = fixture("SessionStart-main");
    assert_eq!(startup["source"], json!("startup"));
    // Constructed input: SessionStart-main.json with `source` changed to "compact".
    let compact = with(startup.clone(), "source", json!("compact"));
    let a = both(t.path(), &d, &input(&startup));
    assert!(!a.is_empty());
    assert_eq!(both(t.path(), &d, &input(&compact)), a);
    let got: Value = serde_json::from_str(&a).unwrap();
    assert_eq!(got["hookSpecificOutput"]["hookEventName"], json!("SessionStart"));
}
