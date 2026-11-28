//! SX-1: the Claude Code subagent branch of `zygo hook session-start` (session text design T§6.3 item 2, T§9.1,
//! T§14 item 1, T§17 checks 2-6 and 12; T-C1, T-C5). No test spawns `claude` or `opencode` or reads a clock; the only
//! child process is this test binary.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use zygo_core::{
    hook::{guarded, HookInput, HookOutput},
    overrides::Overrides,
    paths::Paths,
    session_dir::SessionDir,
    session_view::{Member, ResumeNote, SessionView, SubagentRef, ViewWorkspace},
    Agent,
};
use zygo_hooks::{
    session_start,
    session_text::{build, mechanics},
    workspace::note,
};
use zygo_workspace::state::{MemberRecord, MemberState, Pending, Phase, Store};

const REDACT: &str = "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file.";
const ATTRIBUTION: &str = "Never add Claude attribution to commits, pull request titles or bodies, tags, release notes or comments: no `Co-Authored-By: Claude` trailer, no `noreply@anthropic.com` address, no `Generated with Claude Code` line and no Claude session link.";
const STORE: &str = "Large tool outputs are stored by zygo; read them with result_view, result_search or result_tree using the id in the marker; do not shorten output with head or tail.";
const NAV: &str = "Use act-lite's skeleton, symbols or repo_outline before reading a whole source file.";
const TRAILER: &str = "Add the commit trailers `Zygo-Session: <session>` and, when your brief names a unit, `Zygo-Work: <unit>`, each on its own line at the end of the message.";
const CHILD: &str = "ZYGO_SX1_C6_CHILD";
/// The workspace id, pane key and zygo session of the member view (AU84 forms).
const WS_ID: &str = "0123456789abcdef";
const PANE: &str = "kaaaaaaaa";
const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";

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
    // The one line is the error of the `view.json` load (a JSON parse error, which names no path), nothing else.
    let load = format!("{:#}", SessionView::load(&d).unwrap_err());
    assert!(load.contains("EOF while parsing"), "{load:?}");
    assert_eq!(err, format!("zygo hook: {load}\n"));
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

/// A member session (H1): `view()` with a workspace marker and a resume note, in `<t>/cache/sessions/<SID>`, and a
/// workspace state with a pending member record for the pane, so the member part of `run_member` would print the
/// session text, commit the record and clear the note. Returns the session folder and the paths.
fn member_fx(t: &Path) -> (SessionDir, Paths) {
    member_fx_as(t, Agent::ClaudeCode)
}

/// `member_fx` with `agent` as the view agent and the member record agent.
fn member_fx_as(t: &Path, agent: Agent) -> (SessionDir, Paths) {
    let p = paths(t);
    let d = SessionDir::new(p.sessions_dir.join(SID));
    d.create().unwrap();
    let note = ResumeNote { clean_stop: true, interrupted: vec![SubagentRef { agent_id: "a1".into(), agent_type: "Explore".into(), ..Default::default() }],
        closed_units: vec!["u-1".into()], ..Default::default() };
    let ws = ViewWorkspace { member: Member { ws_id: WS_ID.into(), pane: PANE.into(), ..Default::default() }, resume_note: Some(note), ..Default::default() };
    SessionView { session: SID.into(), workspace: Some(ws), agent, ..view() }.save(&d).unwrap();
    let mut m = MemberRecord::new(agent);
    m.state = MemberState::Resumable;
    m.pending = Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: 7, ..Default::default() });
    m.closed_units = vec!["u-1".into()];
    m.note_pending = true;
    let root = t.to_path_buf();
    Store::open(&p, WS_ID)
        .update(Duration::from_secs(5), &mut |w| {
            w.name = "app".into();
            w.tmux_session = "zygo-app".into();
            w.common_dir = root.join("app/.git");
            w.main_root = root.join("app");
            w.loadout = "solo".into();
            w.phase = Phase::Up;
            w.members = BTreeMap::from([(PANE.to_string(), m.clone())]);
            Ok(())
        })
        .unwrap();
    (d, p)
}

/// Every file under `p` and its bytes.
fn tree(p: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut all = BTreeMap::new();
    for e in std::fs::read_dir(p).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            all.extend(tree(&path));
        } else {
            all.insert(path.clone(), std::fs::read(&path).unwrap());
        }
    }
    all
}

/// `run_member` on the member session of `t` with `--event member` and without; asserts that it printed nothing to
/// `out`, that no file under `t` changed (view.json, the workspace state, the session log, `errors.log`) and that the
/// output equals `run_in`'s. Returns the output.
fn member_run(t: &Path, d: &SessionDir, p: &Paths, v: &Value) -> String {
    let before = tree(t);
    let mut got = Vec::new();
    for ev in [Some("member".to_string()), None] {
        let i = HookInput { cli_event: ev.clone(), ..input(v) };
        let mut out = Vec::new();
        let a = session_start::run_member(&i, d, p, &t.join("no-zygo"), &mut out).unwrap().to_stdout();
        assert!(out.is_empty(), "{ev:?}: the member part printed");
        assert_eq!(session_start::run_in(&i, d).unwrap().to_stdout(), a, "{ev:?}");
        assert_eq!(tree(t), before, "{ev:?}: a file changed");
        got.push(a);
    }
    assert_eq!(got[0], got[1]);
    got.remove(0)
}

#[test]
fn subagent_start_member_view_sub_session_start_compact_gets_nothing() {
    // T§6.3 item 2, T§17 check 5: the member part never runs for an input with `agent_id`.
    let t = tempfile::tempdir().unwrap();
    let (d, p) = member_fx(t.path());
    let sub = fixture("SessionStart-sub");
    assert_eq!((sub["hook_event_name"].as_str(), sub["source"].as_str(), sub.get("agent_id").is_some()),
        (Some("SessionStart"), Some("compact"), true));
    let view = std::fs::read(d.view_json()).unwrap();
    assert_eq!(member_run(t.path(), &d, &p, &sub), "");
    assert_eq!(std::fs::read(d.view_json()).unwrap(), view);
    assert!(!d.log().exists());
    // The same input without `agent_id` runs the member part: the fixture reaches it.
    let mut out = Vec::new();
    let i = HookInput { cli_event: Some("member".into()), ..input(&without(sub, "agent_id")) };
    session_start::run_member(&i, &d, &p, &t.path().join("no-zygo"), &mut out).unwrap();
    assert!(!out.is_empty());
    assert_ne!(std::fs::read(d.view_json()).unwrap(), view);
}

#[test]
fn subagent_start_member_view_gets_only_mechanics() {
    // T§6.3 item 2, T§17 check 2: on a member view the output is the mechanics envelope and the member part did not run.
    let t = tempfile::tempdir().unwrap();
    let (d, p) = member_fx(t.path());
    let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
    assert_eq!(session_start::mechanics_with_overrides(&d, &SessionView::load(&d).unwrap()), m);
    assert_eq!(member_run(t.path(), &d, &p, &fixture("SubagentStart-sub")), envelope(&m));
    assert!(!d.log().exists());
}

#[test]
fn subagent_start_zygo_prefix_rule_matches_is_zygo() {
    // T§6.3 item 2: the rule of `zygo_work::hooks::is_zygo`: a string `agent_type` that starts with `zygo-`, exactly.
    let (t, d) = sd();
    view().save(&d).unwrap();
    let want = envelope(&[REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" "));
    let sub = fixture("SubagentStart-sub");
    for ty in [json!("zygote"), json!("my-plugin:zygo-x"), json!(" zygo-code"), json!("Zygo-code"), json!("ZYGO-code"),
        json!(""), json!("zygo"), Value::Null, json!(7)]
    {
        assert_eq!(both(t.path(), &d, &input(&with(sub.clone(), "agent_type", ty.clone()))), want, "{ty}");
    }
    for ty in ["zygo-", "zygo-plan"] {
        assert_eq!(both(t.path(), &d, &input(&with(sub.clone(), "agent_type", json!(ty)))), "", "{ty}");
    }
}

/// The recorded transform payload (`fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json`) as an
/// OpenCode input with `--event subagent`. Constructed: `input.modelID` changed from `lead` to `read`, a request
/// that the plugin sends with `--event subagent` (T§7.2 item 1). The hook does not read the model.
fn oc_subagent() -> HookInput {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["input"]["modelID"] = json!("read");
    HookInput { cli_event: Some("subagent".into()), ..HookInput::parse(Agent::OpenCode, &serde_json::to_vec(&v).unwrap()).unwrap() }
}

/// `run_member` (`zygo` = a path that does not exist) and `run_in` on `i`. Asserts that both return
/// `HookOutput::Text` with equal text and that `run_member` wrote nothing to `out`. Returns the text.
fn oc_text(t: &Path, d: &SessionDir, p: &Paths, i: &HookInput) -> String {
    let mut out = Vec::new();
    let a = session_start::run_member(i, d, p, &t.join("no-zygo"), &mut out).unwrap();
    assert!(out.is_empty(), "the member part printed");
    let b = session_start::run_in(i, d).unwrap();
    let (HookOutput::Text(a), HookOutput::Text(b)) = (a, b) else { panic!("not plain text") };
    assert_eq!(a, b);
    a
}

#[test]
fn opencode_subagent_prints_mechanics_as_plain_text() {
    // T§7.2 item 2, T-C5: a view that is not a member; plain text for either view agent.
    for agent in [Agent::OpenCode, Agent::ClaudeCode] {
        let (t, d) = sd();
        SessionView { agent, ..view() }.save(&d).unwrap();
        let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
        assert_eq!(m.len(), 721);
        assert_eq!(oc_text(t.path(), &d, &paths(t.path()), &oc_subagent()), m, "{agent:?}");
        assert!(!d.log().exists());
    }
}

#[test]
fn opencode_subagent_overrides_apply() {
    // T-C5, T§17 check 4 form: overrides results and work_units off, navigation off.
    let (t, d) = sd();
    SessionView { agent: Agent::OpenCode, navigation: false, ..view() }.save(&d).unwrap();
    Overrides { results: Some(false), work_units: Some(false), ..Default::default() }.save(&d).unwrap();
    let m = format!("{REDACT} {ATTRIBUTION}");
    assert_eq!(m.len(), 317);
    assert_eq!(oc_text(t.path(), &d, &paths(t.path()), &oc_subagent()), m);
}

#[test]
fn opencode_subagent_member_view_prints_only_mechanics() {
    // T§17 check 11, review focus 70: a member view that holds resume-note inputs gives M as plain text, and no
    // file under the root changes (view.json, the workspace state, the session log, errors.log).
    let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
    for agent in [Agent::OpenCode, Agent::ClaudeCode] {
        let t = tempfile::tempdir().unwrap();
        let (d, p) = member_fx_as(t.path(), agent);
        let view = std::fs::read(d.view_json()).unwrap();
        let before = tree(t.path());
        assert_eq!(oc_text(t.path(), &d, &p, &oc_subagent()), m, "{agent:?}");
        assert_eq!(std::fs::read(d.view_json()).unwrap(), view, "{agent:?}: view.json changed");
        assert_eq!(tree(t.path()), before, "{agent:?}: a file changed");
        assert!(!d.log().exists());
    }
    // The same OpenCode input without `--event subagent` reaches the member part: session text, blank line, note.
    let t = tempfile::tempdir().unwrap();
    let (d, p) = member_fx_as(t.path(), Agent::OpenCode);
    let v = SessionView::load(&d).unwrap();
    let n = note(v.workspace.as_ref().unwrap().resume_note.as_ref().unwrap()).unwrap();
    let i = HookInput { cli_event: None, ..oc_subagent() };
    let got = session_start::run_member(&i, &d, &p, &t.path().join("no-zygo"), &mut Vec::new()).unwrap().to_stdout();
    assert_eq!(got, format!("{}\n\n{n}", build(&v)));
}

#[test]
fn opencode_subagent_unparseable_view_is_an_error() {
    // T§14 item 1: the load error goes to the C6 wrapper; nothing printed to `out`, no session-log line.
    let (t, d) = sd();
    std::fs::write(d.view_json(), "{").unwrap();
    let i = oc_subagent();
    let mut out = Vec::new();
    assert!(session_start::run_member(&i, &d, &paths(t.path()), &t.path().join("no-zygo"), &mut out).is_err());
    assert!(session_start::run_in(&i, &d).is_err());
    assert!(out.is_empty());
    assert!(!d.log().exists());
}
