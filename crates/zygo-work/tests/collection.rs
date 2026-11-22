use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use serde_json::{json, Value};
use zygo_config::{LoadContext, Merged};
use zygo_core::hook::{guarded, HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_usage::{Filter, RequestRow, Store, WorkUnitRow};
use zygo_work::fold::fold;
use zygo_work::hooks::{self, HookCtx};
use zygo_work::log::{self, Begin, CheckResult, Kind, LogLine};
use zygo_work::{join, report, WorkItem};

const SUB: &str = "a1a80dd100f197242";   // agent_id of the *-sub.json fixtures (N2)
const ZSUB: &str = "acfd66ba917fc3494";  // agent_id of SubagentStop-zygo-code-sub.json
const ZC: Option<(&str, &str)> = Some((SUB, "zygo-code"));
const U1: &str = "beads:bd-a1b2";
const U2: &str = "beads:bd-c3d4";
const N12: &str = "work-graph:zygo-v1-N12";

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(name);
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn view(work_units: bool) -> SessionView {
    SessionView { session: "S1".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: None, role_prompt: None,
        user_prompt: None, profile: None, profile_prompt: None, work_item: None, legend: vec![], disabled_roles: vec![],
        work_units, results: true, compact_tool: true, navigation: false,
        trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }
}
struct Env { t: tempfile::TempDir, sd: SessionDir, db: PathBuf, repo: PathBuf, warns: Rc<RefCell<Vec<String>>>, sink: Box<dyn Fn(&str)> }
/// A session directory with a view (work units on), a usage-store path and a repository whose path has a space.
fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let sd = SessionDir::new(t.path().join("sessions").join("S1"));
    sd.create().unwrap();
    view(true).save(&sd).unwrap();
    let repo = t.path().join("repo dir");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let warns = Rc::new(RefCell::new(Vec::new()));
    let w = warns.clone();
    Env { db: t.path().join("usage.db"), sd, repo, t, warns, sink: Box::new(move |s: &str| w.borrow_mut().push(s.to_string())) }
}
/// A merged config from `user` TOML, outside any repository.
fn merged(user: &str) -> Merged {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    std::fs::write(p.join("config.toml"), user).unwrap();
    let paths = Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"), plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") };
    zygo_config::load(&LoadContext { paths, repo: None, team: None }).unwrap()
}
fn no_config(_: &Path) -> Option<Merged> { None }
fn plain_config(_: &Path) -> Option<Merged> { Some(merged("")) }
fn no_item(_: &Path, _: &Merged) -> Option<WorkItem> { None }
fn ctx<'a>(e: &'a Env, now: i64) -> HookCtx<'a> {
    HookCtx { session_dir: &e.sd, session: "S1".into(), usage_db: e.db.clone(), now, config: &no_config, item: &no_item, warn: &*e.sink,
              temp_roots: vec![] }
}
fn hook(v: &Value, cli_event: Option<&str>) -> HookInput {
    let mut i = HookInput::parse(Agent::ClaudeCode, v.to_string().as_bytes()).unwrap();
    i.cli_event = cli_event.map(String::from);
    i
}
/// None = the lead: no agent_id, no agent_type.
fn as_agent(mut v: Value, agent: Option<(&str, &str)>) -> Value {
    let o = v.as_object_mut().unwrap();
    o.remove("agent_id");
    o.remove("agent_type");
    if let Some((id, ty)) = agent {
        o.insert("agent_id".into(), json!(id));
        o.insert("agent_type".into(), json!(ty));
    }
    v
}
fn begin_v(agent: Option<(&str, &str)>, args: Value) -> Value {
    let mut v = as_agent(fixture("PreToolUse-Bash-sub.json"), agent);
    v["tool_name"] = json!("mcp__zygo__work_begin");
    v["tool_input"] = args;
    v
}
fn post_v(agent: Option<(&str, &str)>, tool: &str, input: Value, cwd: &Path, failed: bool) -> Value {
    let mut v = as_agent(fixture("PostToolUse-Bash-sub.json"), agent);
    v["tool_name"] = json!(tool);
    v["tool_input"] = input;
    v["cwd"] = json!(cwd.to_str().unwrap());
    if failed {
        v["hook_event_name"] = json!("PostToolUseFailure");
        v.as_object_mut().unwrap().remove("tool_response");
        v["error"] = json!("Exit code 1");
    }
    v
}
fn run(c: &HookCtx, v: &Value, ev: Option<&str>) {
    assert!(matches!(hooks::run_with(&hook(v, ev), c).unwrap(), HookOutput::None));
}
fn text(e: &Env, agent: &str) -> String {
    std::fs::read_to_string(log::log_path(&e.sd.work(), agent)).unwrap_or_default()
}
fn lines(e: &Env, agent: &str) -> Vec<LogLine> {
    text(e, agent).lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}
fn kinds(e: &Env, agent: &str) -> Vec<String> {
    text(e, agent).lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["t"].as_str().unwrap().to_string()).collect()
}
fn rows(db: &Path) -> Vec<WorkUnitRow> { Store::open(db).unwrap().work_units(&Filter::default()).unwrap() }
fn line(ts: i64, kind: Kind) -> LogLine { LogLine { n: 0, ts, kind } }
fn under(base: &Path, parts: &[&str]) -> String {
    parts.iter().fold(base.to_path_buf(), |p, s| p.join(s)).to_str().unwrap().to_string()
}
/// Appends a begin line and writes begin.json as the begin hook does.
fn start(d: &Path, agent: &str, unit: &str, round: u32, ts: i64) -> Begin {
    std::fs::create_dir_all(d).unwrap();
    let offset = std::fs::metadata(log::log_path(d, agent)).map(|m| m.len()).unwrap_or(0);
    log::append(d, agent, &line(ts, Kind::Begin { unit: unit.into(), round })).unwrap();
    let b = Begin { unit: unit.into(), round, check: "cargo test -p demo".into(), files: vec![], began_at: ts, offset,
                    session: "S1".into(), agent_type: Some("zygo-code".into()), stopped_at: None, edit_root: None };
    std::fs::write(log::begin_path(d, agent), serde_json::to_vec(&b).unwrap()).unwrap();
    b
}
/// `guarded` swaps the process-wide panic hook; tests that call it run one at a time.
static GUARDED: Mutex<()> = Mutex::new(());
/// `(stdout, exit code, stderr lines)`: the lines `ctx.warn` received during the call, then the line `guarded` prints for an error.
fn guarded_run(e: &Env, c: &HookCtx, bytes: &[u8], ev: Option<&str>) -> (String, i32, Vec<String>) {
    let _one = GUARDED.lock().unwrap_or_else(|e| e.into_inner());
    e.warns.borrow_mut().clear();
    let err = RefCell::new(None);
    let (out, code) = guarded(&|b| {
        let r = (|| { let mut i = HookInput::parse(Agent::ClaudeCode, b)?; i.cli_event = ev.map(String::from); hooks::run_with(&i, c) })();
        if let Err(x) = &r { *err.borrow_mut() = Some(format!("zygo hook: {}", format!("{x:#}").replace(['\n', '\r'], " "))); }
        r
    }, bytes);
    let mut lines = e.warns.borrow_mut().drain(..).collect::<Vec<_>>();
    lines.extend(err.into_inner());
    (out, code, lines)
}

#[test]
fn log_append_writes_one_json_line_with_n_and_ts() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    log::append(&d, "main", &line(100, Kind::Call { tool: "Bash".into(), h: "00ff00ff00ff00ff".into() })).unwrap();
    log::append(&d, "main", &line(101, Kind::Check { result: CheckResult::Passed })).unwrap();
    log::append(&d, "main", &line(102, Kind::PrOpened)).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("main.jsonl")).unwrap(), concat!(
        "{\"n\":1,\"ts\":100,\"t\":\"call\",\"tool\":\"Bash\",\"h\":\"00ff00ff00ff00ff\"}\n",
        "{\"n\":2,\"ts\":101,\"t\":\"check\",\"result\":\"passed\"}\n",
        "{\"n\":3,\"ts\":102,\"t\":\"pr_opened\"}\n"));
    assert_eq!(log::log_path(&d, "main"), d.join("main.jsonl"));
    assert_eq!(log::begin_path(&d, "main"), d.join("main.begin.json"));
}

#[test]
fn log_line_over_1_kib_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    assert_eq!(log::MAX_LINE_BYTES, 1024);
    let long = line(1, Kind::Navigated { paths: vec!["p".repeat(log::MAX_LINE_BYTES)] });
    assert!(log::append(&d, "main", &long).is_err());
    assert_eq!(std::fs::read_to_string(d.join("main.jsonl")).unwrap_or_default(), "");
}

#[test]
fn log_overflow_after_4000_lines() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::fs::create_dir_all(&d).unwrap();
    let pre: String = (1..=3998).map(|n| format!("{{\"n\":{n},\"ts\":1,\"t\":\"call\",\"tool\":\"T\",\"h\":\"0\"}}\n")).collect();
    std::fs::write(d.join("main.jsonl"), pre).unwrap();
    let call = line(2, Kind::Call { tool: "T".into(), h: "1".into() });
    for _ in 0..3 {
        log::append(&d, "main", &call).unwrap();
    }
    let text = std::fs::read_to_string(d.join("main.jsonl")).unwrap();
    let all: Vec<&str> = text.lines().collect();
    assert_eq!(log::MAX_LINES, 4000);
    assert_eq!(all.len(), 4000);
    assert_eq!(all[3998], "{\"n\":3999,\"ts\":2,\"t\":\"call\",\"tool\":\"T\",\"h\":\"1\"}");
    assert_eq!(all[3999], "{\"n\":4000,\"ts\":2,\"t\":\"overflow\"}");
}

#[test]
fn log_tail_reads_only_the_last_256_kib() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::fs::create_dir_all(&d).unwrap();
    let mut text = String::new();
    for n in 1..=400u32 {   // 400 lines of exactly 1,000 bytes, newline included
        let head = format!("{{\"n\":{n},\"ts\":1,\"t\":\"navigated\",\"paths\":[\"");
        let end = "\"]}\n";
        text.push_str(&head);
        text.push_str(&"a".repeat(1000 - head.len() - end.len()));
        text.push_str(end);
    }
    assert_eq!(text.len(), 400_000);
    std::fs::write(d.join("main.jsonl"), &text).unwrap();
    assert_eq!(log::TAIL_BYTES, 256 * 1024);
    let got = log::tail(&d, "main");
    // window starts at 400000 - 262144 = 137856; the first whole line starts at 138000 (line 139)
    assert_eq!(got.len(), 262);
    assert_eq!((got[0].0, got[0].1.n), (138_000, 139));
    assert_eq!(got.last().unwrap().1.n, 400);
    assert!(log::tail(&d, "nobody").is_empty());
}

#[test]
fn log_concurrent_appends_keep_whole_lines_and_unique_n() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::thread::scope(|s| {
        for k in 0..8i64 {
            let d = &d;
            s.spawn(move || {
                for _ in 0..50 {
                    log::append(d, "main", &line(k, Kind::Call { tool: format!("T{k}"), h: "0".into() })).unwrap();
                }
            });
        }
    });
    let text = std::fs::read_to_string(d.join("main.jsonl")).unwrap();
    assert!(text.ends_with('\n'));
    let mut ns: Vec<u32> = text.lines().map(|l| serde_json::from_str::<LogLine>(l).unwrap().n).collect();
    ns.sort();
    assert_eq!(ns, (1..=400).collect::<Vec<u32>>());
}

#[test]
fn log_fragment_line_is_closed_before_append() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.jsonl"), "{\"n\":1,\"ts\":1,\"t\":\"pushed\"}\n{\"n\":2,\"ts\":1,\"t\":\"ca").unwrap();
    log::append(&d, "main", &line(5, Kind::PrOpened)).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("main.jsonl")).unwrap(), concat!(
        "{\"n\":1,\"ts\":1,\"t\":\"pushed\"}\n",
        "{\"n\":2,\"ts\":1,\"t\":\"ca\n",
        "{\"n\":2,\"ts\":5,\"t\":\"pr_opened\"}\n"));
}

#[test]
fn log_open_unit_and_agent_names() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::fs::create_dir_all(&d).unwrap();
    assert_eq!(log::open_unit(&d, "main"), None);
    let b = Begin { unit: U1.into(), round: 1, check: "make check".into(), files: vec!["src/".into()],
                    began_at: 100, offset: 0, session: "S1".into(), agent_type: None, stopped_at: None, edit_root: None };
    std::fs::write(log::begin_path(&d, "main"), serde_json::to_vec(&b).unwrap()).unwrap();
    assert_eq!(log::open_unit(&d, "main"), Some(b));
    std::fs::write(log::begin_path(&d, "main"), b"{broken").unwrap();
    assert_eq!(log::open_unit(&d, "main"), None);
    let x65 = "x".repeat(65);
    for bad in ["", "../x", "a/b", "a b", x65.as_str()] {
        assert!(log::append(&d, bad, &line(1, Kind::Pushed)).is_err(), "{bad}");
    }
    for good in ["main", SUB] {
        log::append(&d, good, &line(1, Kind::Pushed)).unwrap();
    }
}

#[test]
fn fold_counts_from_the_begin_offset() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    let call = |ts| line(ts, Kind::Call { tool: "Bash".into(), h: "0".into() });
    log::append(&d, "a1", &call(90)).unwrap();                       // before the unit: ignored by the fold
    start(&d, "a1", U1, 1, 100);
    let levels = BTreeMap::from([("claims".to_string(), "enforce".to_string()), ("scope".to_string(), "notify".to_string())]);
    for l in [call(101), line(102, Kind::Failed { tool: "Read".into(), h: "1".into() }),
              line(103, Kind::Edit { root: Some("/r".into()) }), line(104, Kind::Check { result: CheckResult::Passed }),
              line(105, Kind::Guard { guard: "scope".into(), level: "notify".into() }),
              line(106, Kind::Guard { guard: "scope".into(), level: "notify".into() }),
              line(107, Kind::Guard { guard: "trailers".into(), level: "enforce".into() }),
              line(108, Kind::Levels { levels: levels.clone() })] {
        log::append(&d, "a1", &l).unwrap();
    }
    std::fs::OpenOptions::new().append(true).open(log::log_path(&d, "a1")).unwrap().write_all(b"not json\n").unwrap();
    log::append(&d, "a1", &line(109, Kind::Repeat { tool: "Bash".into() })).unwrap();
    assert_eq!(fold(&d, "a1").unwrap(), WorkUnitRow {
        unit: U1.into(), round: 1, session: "S1".into(), agent_id: "a1".into(), agent_type: Some("zygo-code".into()),
        began_at: 100, ended_at: Some(109), tool_calls: 2, failed_calls: 1, check_ran: true, check_passed: true,
        guard_events: BTreeMap::from([("scope".to_string(), BTreeMap::from([("notify".to_string(), 2)])),
                                      ("trailers".to_string(), BTreeMap::from([("enforce".to_string(), 1)]))]),
        levels, verdict: None });
}

#[test]
fn fold_edit_after_the_pass_makes_the_check_stale() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    start(&d, "main", U1, 1, 100);
    let f = |d: &Path| { let r = fold(d, "main").unwrap(); (r.check_ran, r.check_passed) };
    assert_eq!(f(&d), (false, false));
    log::append(&d, "main", &line(101, Kind::Check { result: CheckResult::Passed })).unwrap();
    assert_eq!(f(&d), (true, true));
    log::append(&d, "main", &line(102, Kind::Edit { root: Some("/r".into()) })).unwrap();
    assert_eq!(f(&d), (false, false));
    log::append(&d, "main", &line(103, Kind::Check { result: CheckResult::Failed })).unwrap();
    assert_eq!(f(&d), (true, false));
    log::append(&d, "main", &line(104, Kind::Check { result: CheckResult::Passed })).unwrap();
    assert_eq!(f(&d), (true, true));
}

#[test]
fn fold_span_ends_at_end_line_and_needs_an_open_unit() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::fs::create_dir_all(&d).unwrap();
    assert_eq!(fold(&d, "main"), None);
    start(&d, "main", U1, 1, 100);
    let call = |ts| line(ts, Kind::Call { tool: "Bash".into(), h: "0".into() });
    log::append(&d, "main", &call(101)).unwrap();
    log::append(&d, "main", &line(102, Kind::End { reason: "superseded".into() })).unwrap();
    log::append(&d, "main", &call(103)).unwrap();
    let r = fold(&d, "main").unwrap();
    assert_eq!((r.tool_calls, r.ended_at), (1, Some(102)));
    let mut b = log::open_unit(&d, "main").unwrap();
    b.offset += 1;                                                   // no longer the begin line
    std::fs::write(log::begin_path(&d, "main"), serde_json::to_vec(&b).unwrap()).unwrap();
    assert_eq!(fold(&d, "main"), None);
}

#[test]
fn fold_row_round_trips_through_the_store() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    start(&d, "main", U1, 1, 100);
    log::append(&d, "main", &line(101, Kind::Call { tool: "Bash".into(), h: "0".into() })).unwrap();
    let mut row = fold(&d, "main").unwrap();
    row.ended_at = Some(110);
    let s = Store::open(&t.path().join("usage.db")).unwrap();
    s.insert_work_unit(&row).unwrap();
    assert_eq!(s.work_units(&Filter::default()).unwrap(), vec![row.clone()]);
    assert_eq!(s.work_units(&Filter { session: Some("S2".into()), since: None }).unwrap(), vec![]);
    assert_eq!(s.work_units(&Filter { session: Some("S1".into()), since: Some(101) }).unwrap(), vec![]);
    assert_eq!(s.work_units(&Filter { session: None, since: Some(100) }).unwrap(), vec![row]);
}

#[test]
fn begin_writes_line_and_begin_json() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "cargo test", "files": ["src/"]})), Some("begin"));
    assert_eq!(lines(&e, SUB), vec![LogLine { n: 1, ts: 1000, kind: Kind::Begin { unit: U1.into(), round: 1 } }]);
    assert_eq!(log::open_unit(&e.sd.work(), SUB), Some(Begin { unit: U1.into(), round: 1, check: "cargo test".into(),
        files: vec!["src/".into()], began_at: 1000, offset: 0, session: "S1".into(), agent_type: Some("zygo-code".into()), stopped_at: None, edit_root: None }));
}

#[test]
fn begin_lead_uses_main() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(None, json!({"unit": U1, "round": 2})), Some("begin"));
    assert_eq!(log::open_unit(&e.sd.work(), "main"), Some(Begin { unit: U1.into(), round: 2, check: String::new(),
        files: vec![], began_at: 1000, offset: 0, session: "S1".into(), agent_type: None, stopped_at: None, edit_root: None }));
    assert!(!log::begin_path(&e.sd.work(), SUB).exists());
}

#[test]
fn begin_repeated_unit_and_round_is_a_noop() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "cargo test"})), Some("begin"));
    run(&ctx(&e, 1010), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "other"})), Some("begin"));
    assert_eq!(kinds(&e, SUB), ["begin"]);
    let b = log::open_unit(&e.sd.work(), SUB).unwrap();
    assert_eq!((b.began_at, b.check.as_str()), (1000, "cargo test"));
    assert!(!e.db.exists());
}

#[test]
fn begin_other_unit_or_round_supersedes_and_writes_the_row() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1})), Some("begin"));
    log::append(&e.sd.work(), SUB, &line(1005, Kind::Call { tool: "Read".into(), h: "0".into() })).unwrap();
    run(&ctx(&e, 1020), &begin_v(ZC, json!({"unit": U2, "round": 1})), Some("begin"));
    assert_eq!(kinds(&e, SUB), ["begin", "call", "end", "begin"]);
    assert_eq!(lines(&e, SUB)[2], LogLine { n: 3, ts: 1020, kind: Kind::End { reason: "superseded".into() } });
    let off: u64 = text(&e, SUB).lines().take(3).map(|l| l.len() as u64 + 1).sum();
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().offset, off);
    let r = rows(&e.db);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].unit.as_str(), r[0].round, r[0].began_at, r[0].ended_at, r[0].tool_calls), (U1, 1, 1000, Some(1020), 1));
    run(&ctx(&e, 1030), &begin_v(ZC, json!({"unit": U2, "round": 2})), Some("begin"));
    let r = rows(&e.db);
    assert_eq!(r.len(), 2);
    assert!(r.iter().any(|x| x.unit == U2 && x.round == 1 && x.began_at == 1020 && x.ended_at == Some(1030)));
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().round, 2);
}

#[test]
fn begin_non_zygo_subagent_records_nothing() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(Some((SUB, "claude")), json!({"unit": U1, "round": 1})), Some("begin"));
    assert!(!log::begin_path(&e.sd.work(), SUB).exists());
    assert!(!log::log_path(&e.sd.work(), SUB).exists());
}

#[test]
fn begin_empty_check_and_files_take_the_source_hints() {
    let calls = Cell::new(0);
    let item = |_: &Path, _: &Merged| {
        calls.set(calls.get() + 1);
        Some(WorkItem { source: "work-graph".into(), id: "zygo-v1/N12".into(), title: None, url: None, status: None,
            check_hint: Some("cargo test -p zygo-work --test collection".into()),
            files_hint: vec!["crates/zygo-work/src/".into(), "crates/zygo-work/tests/collection.rs".into()], lookup_failed: None })
    };
    let e = env();
    let c = HookCtx { config: &plain_config, item: &item, ..ctx(&e, 1000) };
    let hint_files = vec!["crates/zygo-work/src/".to_string(), "crates/zygo-work/tests/collection.rs".to_string()];
    let got = |agent: &str, args: Value| {
        run(&c, &begin_v(Some((agent, "zygo-code")), args), Some("begin"));
        log::open_unit(&e.sd.work(), agent).unwrap()
    };
    let b = got("a1", json!({"unit": N12, "round": 1, "check": "", "files": []}));
    assert_eq!((b.check.as_str(), &b.files), ("cargo test -p zygo-work --test collection", &hint_files));
    let b = got("a2", json!({"unit": format!("{N12}:review"), "round": 1, "check": "make check"}));
    assert_eq!((b.check.as_str(), &b.files), ("make check", &hint_files));
    let b = got("a3", json!({"unit": U1, "round": 1}));
    assert_eq!((b.check.as_str(), b.files.len()), ("", 0));
    let before = calls.get();
    got("a4", json!({"unit": N12, "round": 1, "check": "x", "files": ["a/"]}));
    assert_eq!(calls.get(), before);
}

#[test]
fn begin_writes_the_levels_in_force() {
    let e = env();
    let cfg = |_: &Path| Some(merged("[guards]\nscope = \"notify\"\n"));
    Overrides { guards: BTreeMap::from([("claims".to_string(), "enforce".to_string())]), ..Default::default() }.save(&e.sd).unwrap();
    let c = HookCtx { config: &cfg, ..ctx(&e, 1000) };
    run(&c, &begin_v(ZC, json!({"unit": U1, "round": 1})), Some("begin"));
    let want: BTreeMap<String, String> = [("claims", "enforce"), ("scope", "notify"), ("navigation", "monitor"), ("output", "monitor"),
        ("trailers", "notify"), ("repeat", "monitor")].into_iter().map(|(g, l)| (g.to_string(), l.to_string())).collect();
    assert_eq!(lines(&e, SUB)[1], LogLine { n: 2, ts: 1000, kind: Kind::Levels { levels: want.clone() } });
    assert_eq!(fold(&e.sd.work(), SUB).unwrap().levels, want);
}

#[test]
fn begin_invalid_arguments_record_nothing() {
    let e = env();
    let c = ctx(&e, 1000);
    let seg48 = "a".repeat(48);
    let long = format!("{seg48}:{seg48}:{}", "b".repeat(31));            // 129 bytes
    for args in [json!({"round": 1}), json!({"unit": "beads", "round": 1}), json!({"unit": "beads:-x", "round": 1}),
                 json!({"unit": format!("beads:{}", "a".repeat(49)), "round": 1}), json!({"unit": long, "round": 1}),
                 json!({"unit": "a:b:c:d", "round": 1}), json!({"unit": "beads:bd a", "round": 1}),
                 json!({"unit": U1, "round": 0}), json!({"unit": U1, "round": "1"}),
                 json!({"unit": U1, "round": 1, "check": "x".repeat(1025)}),
                 json!({"unit": U1, "round": 1, "files": vec!["f"; 65]}),
                 json!({"unit": U1, "round": 1, "files": ["/abs"]}),
                 json!({"unit": U1, "round": 1, "files": ["a/../b"]})] {
        assert!(hooks::run_with(&hook(&begin_v(ZC, args.clone()), Some("begin")), &c).is_err(), "{args}");
    }
    assert!(!log::begin_path(&e.sd.work(), SUB).exists());
    assert!(!log::log_path(&e.sd.work(), SUB).exists());
    let max = json!({"unit": format!("{seg48}:{seg48}:{}", "b".repeat(30)), "round": 1, "check": "x".repeat(1024), "files": vec!["f"; 64]});
    assert!(hooks::BeginArgs::parse(&max).is_ok());                       // 128 bytes, 1,024 bytes, 64 files
    assert!(hooks::BeginArgs::parse(&json!({"unit": U1, "round": 1, "future_arg": true})).is_ok());   // R16
}

#[test]
fn post_check_dir_matches_the_spec_forms() {
    let ck = "cargo test -p demo";
    let none: Option<Option<String>> = None;
    assert_eq!(hooks::check_dir("cargo test -p demo", ck), Some(None));
    assert_eq!(hooks::check_dir("  cargo  test\t-p demo ", ck), Some(None));
    assert_eq!(hooks::check_dir("RUST_LOG=info CI=1 cargo test -p demo", ck), Some(None));
    assert_eq!(hooks::check_dir("cargo test -p demo 2>&1", ck), Some(None));
    assert_eq!(hooks::check_dir("cd sub && cargo test -p demo 2>&1", ck), Some(Some("sub".to_string())));
    assert_eq!(hooks::check_dir("cd sub && RUST_LOG=1 cargo test -p demo", ck), Some(Some("sub".to_string())));
    assert_eq!(hooks::check_dir("cd \"my  dir\" && cargo test -p demo", ck), Some(Some("my  dir".to_string())));
    assert_eq!(hooks::check_dir("cd 'C:\\work\\my dir' && cargo test -p demo", ck), Some(Some("C:\\work\\my dir".to_string())));
    assert_eq!(hooks::check_dir("A=1 cargo test", "A=1 cargo test"), Some(None));
    assert_eq!(hooks::check_dir("cargo test -p demo 2>&1", "cargo test -p demo 2>&1"), Some(None));
    for miss in ["cargo test -p demo --release", "cargo test -p demo; rm -r x", "echo cargo test -p demo",
                 "cargo test -p demo 2>&1 | tail", "cd a b && cargo test -p demo", "cargo test -p demo && cd x",
                 "cd \"unclosed && cargo test -p demo", "cd ~/repo && cargo test -p demo", "cd ~ && cargo test -p demo",
                 "cd $REPO && cargo test -p demo", "cd \"$HOME/x\" && cargo test -p demo", "cd '~x' && cargo test -p demo"] {
        assert_eq!(hooks::check_dir(miss, ck), none, "{miss}");
    }
    assert_eq!(hooks::check_dir("", ""), none);                          // an empty check never matches
}

#[test]
fn post_every_call_appends_call_or_failed() {
    let e = env();
    let c = ctx(&e, 1000);
    run(&c, &fixture("PostToolUse-Read-main.json"), None);
    run(&c, &fixture("PostToolUseFailure-Read-main.json"), None);
    let mut v = fixture("PostToolUse-Bash-main.json");
    v["tool_response"]["interrupted"] = json!(true);
    run(&c, &v, None);
    assert_eq!(kinds(&e, "main"), ["call", "failed", "failed"]);
    match &lines(&e, "main")[0].kind {
        Kind::Call { tool, h } => { assert_eq!(tool, "Read"); assert_eq!(h.len(), 16); assert!(h.bytes().all(|b| b.is_ascii_hexdigit())); }
        k => panic!("{k:?}"),
    }
}

#[test]
fn post_check_in_the_repository_of_the_last_edit() {
    // Checks the mapping event -> check result only; whether a non-zero Bash exit fires PostToolUseFailure is open (P-N24-1).
    let e = env();
    let other = e.t.path().join("other");
    std::fs::create_dir_all(other.join(".git")).unwrap();
    let elsewhere = e.t.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let c = ctx(&e, 1000);
    run(&c, &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "cargo test -p demo"})), Some("begin"));
    let bash = |cmd: &str, cwd: &Path, failed: bool| post_v(ZC, "Bash", json!({"command": cmd}), cwd, failed);
    run(&c, &bash("cargo test -p demo", &other, false), None);           // no edit yet: any repository counts
    run(&c, &post_v(ZC, "Edit", json!({"file_path": e.repo.join("src").join("a.rs"), "old_string": "a", "new_string": "b"}), &elsewhere, false), None);
    run(&c, &bash(&format!("cd \"{}\" && RUST_LOG=info cargo   test -p demo 2>&1", e.repo.display()), &elsewhere, false), None);
    run(&c, &bash("cargo test -p demo", &other, false), None);           // another repository: a plain call
    run(&c, &bash("cargo test -p demo 2>&1", &e.repo.join("src"), true), None);
    let mut v = bash("RUST_LOG=1 cargo test -p demo", &e.repo, false);
    v["tool_response"]["interrupted"] = json!(true);
    run(&c, &v, None);
    assert_eq!(kinds(&e, SUB), ["begin", "call", "check", "call", "edit", "call", "check", "call", "failed", "check", "failed", "check"]);
    let ls = lines(&e, SUB);
    assert_eq!(ls[4].kind, Kind::Edit { root: Some(e.repo.canonicalize().unwrap().to_str().unwrap().into()) });
    assert_eq!(ls[2].kind, Kind::Check { result: CheckResult::Passed });
    assert_eq!(ls[6].kind, Kind::Check { result: CheckResult::Passed });
    assert_eq!(ls[9].kind, Kind::Check { result: CheckResult::Failed });
    assert_eq!(ls[11].kind, Kind::Check { result: CheckResult::Failed });
}

#[test]
fn post_repeat_needs_same_input_and_no_edit_since() {
    let e = env();
    let c = ctx(&e, 1000);
    let read = |p: &str| post_v(None, "Read", json!({"file_path": p}), &e.repo, false);
    run(&c, &read("/r/x.rs"), None);
    run(&c, &read("/r/x.rs"), None);
    run(&c, &read("/r/y.rs"), None);
    run(&c, &post_v(None, "Write", json!({"file_path": e.repo.join("z.txt"), "content": "z"}), &e.repo, false), None);
    run(&c, &read("/r/x.rs"), None);
    run(&c, &read("/r/x.rs"), None);
    assert_eq!(kinds(&e, "main"), ["call", "call", "repeat", "guard", "call", "call", "edit", "call", "call", "repeat", "guard"]);
    assert_eq!(log::REPEAT_WINDOW, 256);
}

#[test]
fn post_navigation_tool_appends_absolute_paths() {
    let e = env();
    let c = ctx(&e, 1000);
    let nav = |tool: &str, input: Value| run(&c, &post_v(None, tool, input, &e.repo, false), None);
    nav("mcp__plugin_act101_act-lite__skeleton", json!({"path": "src/lib.rs"}));
    nav("mcp__act-lite__symbols_batch", json!({"paths": ["a.rs", "./b/../b.rs"]}));
    nav("mcp__act-lite__symbols", json!({"file_path": e.repo.join("x.rs")}));
    nav("mcp__act-lite__repo_outline", json!({}));                       // no path key: no navigated line
    nav("mcp__other__skeleton", json!({"path": "src/lib.rs"}));
    nav("mcp__act-lite__definition", json!({"path": "src/lib.rs"}));
    let many: Vec<String> = (0..60).map(|i| format!("src/module_{i:02}/file_with_a_long_name.rs")).collect();
    nav("mcp__act-lite__symbols_batch", json!({"paths": many}));
    let navs: Vec<Vec<String>> = lines(&e, "main").into_iter()
        .filter_map(|l| match l.kind { Kind::Navigated { paths } => Some(paths), _ => None }).collect();
    assert_eq!(navs.len(), 4);
    assert_eq!(navs[0], [under(&e.repo, &["src", "lib.rs"])]);
    assert_eq!(navs[1], [under(&e.repo, &["a.rs"]), under(&e.repo, &["b.rs"])]);
    assert_eq!(navs[2], [under(&e.repo, &["x.rs"])]);
    let want: Vec<String> = (0..60).map(|i| under(&e.repo, &["src", &format!("module_{i:02}"), "file_with_a_long_name.rs"])).collect();
    assert!(!navs[3].is_empty() && navs[3].len() < 60 && navs[3][..] == want[..navs[3].len()]);
    assert!(text(&e, "main").lines().all(|l| l.len() < log::MAX_LINE_BYTES));
}

#[test]
fn post_overlong_line_is_dropped_alone() {
    let e = env();
    let tool = format!("mcp__{}act-lite__skeleton", "x".repeat(1100));   // its call line is over 1 KiB
    run(&ctx(&e, 1000), &post_v(None, &tool, json!({"path": "src/lib.rs"}), &e.repo, false), None);
    assert_eq!(kinds(&e, "main"), ["navigated"]);
}

#[test]
fn post_push_and_pr_lines() {
    let e = env();
    let c = ctx(&e, 1000);
    for (cmd, failed) in [("git push origin HEAD", false), ("git -C sub push", false), ("gh pr create --fill", false),
                          ("echo git push", false), ("git push", true), ("cargo build && git push -u origin x", false)] {
        run(&c, &post_v(None, "Bash", json!({"command": cmd}), &e.repo, failed), None);
    }
    assert_eq!(kinds(&e, "main"), ["call", "pushed", "call", "pushed", "call", "pr_opened", "call", "failed", "call", "pushed"]);
}

#[test]
fn post_units_off_records_nothing() {
    let e = env();
    let v = fixture("PostToolUse-Read-main.json");
    let log_exists = || log::log_path(&e.sd.work(), "main").exists();
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    run(&ctx(&e, 1000), &v, None);
    run(&ctx(&e, 1000), &begin_v(None, json!({"unit": U1, "round": 1})), Some("begin"));
    assert!(!log_exists() && !log::begin_path(&e.sd.work(), "main").exists());
    std::fs::remove_file(e.sd.overrides_json()).unwrap();
    view(false).save(&e.sd).unwrap();                                    // the view, without an override
    run(&ctx(&e, 1000), &v, None);
    assert!(!log_exists());
    std::fs::remove_file(e.sd.view_json()).unwrap();                     // no view: the config decides (R14)
    let off = |_: &Path| Some(merged("[features]\nwork_units = false\n"));
    run(&HookCtx { config: &off, ..ctx(&e, 1000) }, &v, None);
    assert!(!log_exists());
    run(&HookCtx { config: &plain_config, ..ctx(&e, 1000) }, &v, None);
    assert_eq!(kinds(&e, "main"), ["call"]);
    view(false).save(&e.sd).unwrap();
    Overrides { work_units: Some(true), ..Default::default() }.save(&e.sd).unwrap();   // the session override wins
    run(&ctx(&e, 1000), &v, None);
    assert_eq!(kinds(&e, "main"), ["call", "call", "repeat", "guard"]);   // same Read input, no edit since
}

#[test]
fn post_lines_of_other_subagents_are_kept() {
    let e = env();
    run(&ctx(&e, 1000), &fixture("PostToolUse-Bash-sub.json"), None);   // agent_type "claude"
    assert_eq!(kinds(&e, SUB), ["call"]);
}

#[test]
fn stop_writes_and_refolds_the_row_and_the_unit_resumes() {
    let e = env();
    let zc = Some((ZSUB, "zygo-code"));
    run(&ctx(&e, 1000), &begin_v(zc, json!({"unit": U1, "round": 1})), Some("begin"));
    run(&ctx(&e, 1001), &post_v(zc, "Bash", json!({"command": "ls"}), &e.repo, false), None);
    let stop = fixture("SubagentStop-zygo-code-sub.json");
    run(&ctx(&e, 1010), &stop, None);
    let r = rows(&e.db);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].agent_id.as_str(), r[0].agent_type.as_deref(), r[0].session.as_str(), r[0].began_at, r[0].ended_at, r[0].tool_calls),
               (ZSUB, Some("zygo-code"), "S1", 1000, Some(1010), 1));
    assert_eq!(log::open_unit(&e.sd.work(), ZSUB).unwrap().stopped_at, Some(1010));   // R13: stopped, still resumable
    run(&ctx(&e, 1020), &post_v(zc, "Bash", json!({"command": "pwd"}), &e.repo, false), None);
    assert_eq!(log::open_unit(&e.sd.work(), ZSUB).unwrap().stopped_at, None);         // the continuation clears it
    run(&ctx(&e, 1030), &stop, None);
    let r = rows(&e.db);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].ended_at, r[0].tool_calls), (Some(1030), 2));
    assert_eq!(log::open_unit(&e.sd.work(), ZSUB).unwrap().stopped_at, Some(1030));
}

#[test]
fn stop_without_open_unit_or_from_internal_agent_writes_nothing() {
    let e = env();
    let c = ctx(&e, 1000);
    run(&c, &fixture("SubagentStop-zygo-code-sub.json"), None);
    let internal = fixture("SubagentStop-internal-sub.json");             // agent_type "" (P3 note)
    let id = internal["agent_id"].as_str().unwrap().to_string();
    start(&e.sd.work(), &id, U1, 1, 900);
    run(&c, &internal, None);
    assert!(!e.db.exists());
    assert_eq!(log::open_unit(&e.sd.work(), &id).unwrap().stopped_at, None);
}

#[test]
fn accept_34_work_unit_flow() {
    // §20 #34: work_begin through the hook writes the begin line; a matching check run in the unit's repository records
    // check: passed; an edit after it makes the claim stale; SubagentStop writes the work_units row with the folded facts.
    let e = env();
    let zc = Some((ZSUB, "zygo-code"));
    run(&ctx(&e, 1000), &begin_v(zc, json!({"unit": N12, "round": 1, "check": "cargo test -p demo", "files": ["src/"]})), Some("begin"));
    assert_eq!(kinds(&e, ZSUB), ["begin"]);
    let edit = post_v(zc, "Edit", json!({"file_path": e.repo.join("src").join("a.rs"), "old_string": "a", "new_string": "b"}), &e.repo, false);
    run(&ctx(&e, 1010), &edit, None);
    run(&ctx(&e, 1020), &post_v(zc, "Bash", json!({"command": "cargo test -p demo"}), &e.repo, false), None);
    assert_eq!(lines(&e, ZSUB).last().unwrap().kind, Kind::Check { result: CheckResult::Passed });
    let r = fold(&e.sd.work(), ZSUB).unwrap();
    assert!(r.check_ran && r.check_passed);
    run(&ctx(&e, 1030), &edit, None);
    let r = fold(&e.sd.work(), ZSUB).unwrap();
    assert!(!r.check_ran && !r.check_passed);
    run(&ctx(&e, 1040), &fixture("SubagentStop-zygo-code-sub.json"), None);
    assert_eq!(rows(&e.db), vec![WorkUnitRow { unit: N12.into(), round: 1, session: "S1".into(),
        agent_id: ZSUB.into(), agent_type: Some("zygo-code".into()), began_at: 1000, ended_at: Some(1040), tool_calls: 3,
        failed_calls: 0, check_ran: false, check_passed: false, guard_events: BTreeMap::new(), levels: BTreeMap::new(), verdict: None }]);
}

/// Exit 0, no stdout, exactly one `zygo hook: ` stderr line.
fn one_error_line(r: (String, i32, Vec<String>)) -> String {
    assert_eq!((r.0.as_str(), r.1, r.2.len()), ("", 0, 1), "{:?}", r.2);
    assert!(r.2[0].starts_with("zygo hook: "), "{}", r.2[0]);
    r.2[0].clone()
}

#[test]
fn c6_bad_or_newer_input_exits_0_without_output() {
    let e = env();
    let c = ctx(&e, 1000);
    let quiet = (String::new(), 0, Vec::<String>::new());
    for (bytes, ev) in [(b"not json".to_vec(), None), (Vec::new(), None), (b"[1,2]".to_vec(), None),
                        (b"not json".to_vec(), Some("begin")), (Vec::new(), Some("begin"))] {
        one_error_line(guarded_run(&e, &c, &bytes, ev));
    }
    let mut v = fixture("PostToolUse-Bash-sub.json");                     // a newer agent: unknown event, unknown field
    v["hook_event_name"] = json!("PostToolBatch");
    v["future_field"] = json!({"x": 1});
    assert_eq!(guarded_run(&e, &c, v.to_string().as_bytes(), None), quiet);
    let mut v = fixture("PostToolUse-Bash-sub.json");                     // agent_id present but null: a subagent without a name
    v["agent_id"] = Value::Null;
    assert_eq!(one_error_line(guarded_run(&e, &c, v.to_string().as_bytes(), None)), "zygo hook: agent_id is not a string");
    assert!(std::fs::read_dir(e.sd.work()).map(|d| d.count() == 0).unwrap_or(true));   // nothing recorded so far
    let v = begin_v(ZC, json!({"unit": U1, "round": 1, "future_arg": true}));   // unknown argument: ignored (R16)
    assert_eq!(guarded_run(&e, &c, v.to_string().as_bytes(), Some("begin")), quiet);
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().unit, U1);
    let mut v = fixture("PostToolUse-Bash-main.json");                    // a tool_input of another shape is still a call
    v["tool_input"] = json!("echo");
    assert_eq!(guarded_run(&e, &c, v.to_string().as_bytes(), None), quiet);
    assert_eq!(kinds(&e, "main"), ["call"]);
}

#[test]
fn c6_internal_errors_exit_0_without_output() {
    let e = env();
    std::fs::write(e.sd.work(), b"a file where the work directory belongs").unwrap();
    let c = ctx(&e, 1000);
    let l = one_error_line(guarded_run(&e, &c, fixture("PostToolUse-Read-main.json").to_string().as_bytes(), None));
    assert!(l.starts_with("zygo hook: creating "), "{l}");
    let l = one_error_line(guarded_run(&e, &c, begin_v(ZC, json!({"unit": U1, "round": 1})).to_string().as_bytes(), Some("begin")));
    assert!(l.starts_with("zygo hook: creating "), "{l}");
    assert!(e.sd.work().is_file());                                       // nothing recorded
    let e2 = env();
    std::fs::create_dir_all(&e2.db).unwrap();                             // the usage store cannot open
    let zc = Some((ZSUB, "zygo-code"));
    let c2 = ctx(&e2, 1000);
    run(&c2, &begin_v(zc, json!({"unit": U1, "round": 1})), Some("begin"));
    one_error_line(guarded_run(&e2, &c2, fixture("SubagentStop-zygo-code-sub.json").to_string().as_bytes(), None));
    assert!(e2.db.is_dir());                                              // no row
    assert_eq!(kinds(&e2, ZSUB), ["begin"]);
    assert_eq!(log::open_unit(&e2.sd.work(), ZSUB).unwrap().stopped_at, Some(1000));   // the stop mark is written before the fold
}

#[test]
fn c6_unparseable_begin_json_is_no_open_unit() {
    // Item 6: one stderr line per hook call; the call is recorded as without a unit, and a new begin replaces the file.
    let e = env();
    run(&ctx(&e, 1000), &begin_v(None, json!({"unit": U1, "round": 1, "check": "make check"})), Some("begin"));
    std::fs::write(log::begin_path(&e.sd.work(), "main"), b"{garbage").unwrap();
    let want = vec!["zygo hook: work: main.begin.json does not parse; treated as no open unit".to_string()];
    let v = post_v(None, "Bash", json!({"command": "make check"}), &e.repo, false);
    assert_eq!(guarded_run(&e, &ctx(&e, 1010), v.to_string().as_bytes(), None), (String::new(), 0, want.clone()));
    assert_eq!(kinds(&e, "main"), ["begin", "call"]);                     // no check line: no open unit
    let v = begin_v(None, json!({"unit": U2, "round": 1}));
    assert_eq!(guarded_run(&e, &ctx(&e, 1020), v.to_string().as_bytes(), Some("begin")), (String::new(), 0, want));
    assert_eq!(kinds(&e, "main"), ["begin", "call", "begin"]);            // no `end`: there was no open unit to supersede
    assert_eq!(log::open_unit(&e.sd.work(), "main").unwrap().unit, U2);
    assert!(!e.db.exists());
}

// ---------------- round 2 ----------------

fn log_len(e: &Env, agent: &str) -> u64 { std::fs::metadata(log::log_path(&e.sd.work(), agent)).map(|m| m.len()).unwrap_or(0) }

/// Distinct act-lite navigation calls with ~1,000-byte `navigated` lines until the log has grown `bytes` past `from`;
/// returns how many calls that took.
fn nav_fill(e: &Env, agent: Option<(&str, &str)>, id: &str, from: u64, bytes: u64) -> usize {
    let pad = 930 - e.repo.to_str().unwrap().len();
    let mut i = 0;
    while log_len(e, id) - from <= bytes {
        let p = format!("{}{i:05}", "x".repeat(pad));
        run(&ctx(e, 1100), &post_v(agent, "mcp__act-lite__skeleton", json!({"path": p}), &e.repo, false), None);
        i += 1;
    }
    i
}

#[test]
fn post_background_bash_writes_no_check() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "cargo test -p demo"})), Some("begin"));
    run(&ctx(&e, 1010), &post_v(ZC, "Bash", json!({"command": "cargo test -p demo", "run_in_background": true}), &e.repo, false), None);
    run(&ctx(&e, 1020), &post_v(ZC, "Bash", json!({"command": "cargo test -p demo", "run_in_background": false}), &e.repo, false), None);
    assert_eq!(kinds(&e, SUB), ["begin", "call", "call", "check"]);
}

#[test]
fn post_check_uses_the_edit_root_kept_in_begin_json() {
    let e = env();
    let elsewhere = e.t.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "make check"})), Some("begin"));
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().edit_root, None);
    run(&ctx(&e, 1001), &post_v(ZC, "Edit", json!({"file_path": e.repo.join("src").join("a.rs")}), &e.repo, false), None);
    let root = e.repo.canonicalize().unwrap().to_str().unwrap().to_string();
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().edit_root.as_deref(), Some(root.as_str()));
    nav_fill(&e, ZC, SUB, log_len(&e, SUB), log::TAIL_BYTES + 2048);      // the edit line is now outside the tail
    let check = |cwd: &Path| { run(&ctx(&e, 1200), &post_v(ZC, "Bash", json!({"command": "make check"}), cwd, false), None); kinds(&e, SUB).last().unwrap().clone() };
    assert_eq!(check(&elsewhere), "call");                                // not the repository of the last edit
    assert_eq!(check(&e.repo), "check");
    // A new unit starts without an edit: an edit of the earlier unit does not bind its check.
    run(&ctx(&e, 1300), &begin_v(ZC, json!({"unit": U2, "round": 1, "check": "make check"})), Some("begin"));
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().edit_root, None);
    run(&ctx(&e, 1301), &post_v(ZC, "Bash", json!({"command": "make check"}), &elsewhere, false), None);
    assert_eq!(kinds(&e, SUB).last().unwrap(), "check");
}

#[test]
fn post_check_ignores_an_edit_of_an_earlier_unit_in_the_tail() {
    let e = env();
    let elsewhere = e.t.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": "make check"})), Some("begin"));
    run(&ctx(&e, 1001), &post_v(ZC, "Edit", json!({"file_path": e.repo.join("src").join("a.rs")}), &e.repo, false), None);
    run(&ctx(&e, 1002), &begin_v(ZC, json!({"unit": U2, "round": 1, "check": "make check"})), Some("begin"));
    run(&ctx(&e, 1003), &post_v(ZC, "Bash", json!({"command": "make check"}), &elsewhere, false), None);
    assert_eq!(kinds(&e, SUB), ["begin", "call", "edit", "end", "begin", "call", "check"]);
}

#[test]
fn post_repeat_is_found_within_the_tail_and_never_beyond_256_kib() {
    // Spec note (N24 review): the look-back is the last 256 calls that lie within the last 256 KiB.
    let tool = "mcp__act-lite__skeleton";
    let first = json!({"path": "first.rs"});
    let e = env();
    run(&ctx(&e, 1000), &post_v(None, tool, first.clone(), &e.repo, false), None);
    let n = nav_fill(&e, None, "main", 0, log::TAIL_BYTES - 8 * 1024);
    assert!(n < log::REPEAT_WINDOW, "{n}");
    run(&ctx(&e, 2000), &post_v(None, tool, first.clone(), &e.repo, false), None);
    assert_eq!(kinds(&e, "main").iter().rev().take(4).rev().collect::<Vec<_>>(), ["call", "repeat", "guard", "navigated"]);
    let e = env();
    run(&ctx(&e, 1000), &post_v(None, tool, first.clone(), &e.repo, false), None);
    let n = nav_fill(&e, None, "main", 0, log::TAIL_BYTES + 2048);
    assert!(n < log::REPEAT_WINDOW, "{n}");                               // fewer than 256 calls: the byte bound decides
    run(&ctx(&e, 2000), &post_v(None, tool, first, &e.repo, false), None);
    assert_eq!(kinds(&e, "main").iter().rev().take(3).rev().collect::<Vec<_>>(), ["navigated", "call", "navigated"]);   // the first call lies before the last 256 KiB
}

#[test]
fn post_repeat_window_is_256_calls() {
    let read = |e: &Env, p: &str| run(&ctx(e, 1000), &post_v(None, "Read", json!({"file_path": p}), &e.repo, false), None);
    for (between, want) in [(log::REPEAT_WINDOW - 1, "guard"), (log::REPEAT_WINDOW, "call")] {
        let e = env();
        read(&e, "/r/a.rs");
        for i in 0..between {
            read(&e, &format!("/r/{i}.rs"));
        }
        read(&e, "/r/a.rs");
        assert_eq!(kinds(&e, "main").last().unwrap(), want, "{between}");
    }
}

#[test]
fn units_on_session_reads_the_override_then_the_view() {
    let e = env();
    assert!(hooks::units_on_session(&e.sd, &e.repo));                     // view: on
    view(false).save(&e.sd).unwrap();
    assert!(!hooks::units_on_session(&e.sd, &e.repo));
    Overrides { work_units: Some(true), ..Default::default() }.save(&e.sd).unwrap();
    assert!(hooks::units_on_session(&e.sd, &e.repo));
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    view(true).save(&e.sd).unwrap();
    assert!(!hooks::units_on_session(&e.sd, &e.repo));
}

#[test]
fn verdict_from_a_subagent_is_denied() {
    let e = env();
    let deny = json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
        "permissionDecisionReason": "zygo: work_verdict is for the lead only. Report your result to the lead; the lead records the verdict."}});
    let verdict = |agent| {
        let mut v = as_agent(fixture("PreToolUse-Bash-sub.json"), agent);
        v["tool_name"] = json!("mcp__zygo__work_verdict");
        v["tool_input"] = json!({"unit": U1, "round": 1, "verdict": "keep"});
        v
    };
    let out = |v: &Value| match hooks::run_with(&hook(v, Some("verdict")), &ctx(&e, 1000)).unwrap() {
        HookOutput::Json(j) => Some(j),
        HookOutput::None => None,
        HookOutput::Text(t) => panic!("{t}"),
    };
    assert_eq!(out(&verdict(ZC)), Some(deny.clone()));
    let mut null_id = verdict(None);
    null_id["agent_id"] = Value::Null;                                    // key present: inside a subagent
    assert_eq!(out(&null_id), Some(deny.clone()));
    assert_eq!(out(&verdict(None)), None);                                // the lead
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    assert_eq!(out(&verdict(ZC)), Some(deny));                            // lead only holds with units off too
    assert!(!e.sd.work().exists());
}

#[test]
fn begin_largest_record_is_recorded_and_the_begin_line_holds_unit_and_round() {
    let e = env();
    let files: Vec<String> = (0..64).map(|i| format!("src/f{i}.rs")).collect();
    let check = "c".repeat(1024);
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1, "check": check, "files": files})), Some("begin"));
    assert_eq!(text(&e, SUB), format!("{{\"n\":1,\"ts\":1000,\"t\":\"begin\",\"unit\":\"{U1}\",\"round\":1}}\n"));
    let b = log::open_unit(&e.sd.work(), SUB).unwrap();
    assert_eq!((b.check, b.files), (check, files));
}

#[test]
fn begin_limits_are_1024_byte_check_64_files_4096_byte_paths() {
    let ok = |check: usize, files: usize, path: usize| {
        let mut f: Vec<String> = (0..files).map(|i| format!("f{i}")).collect();
        if path > 0 { f[0] = "p".repeat(path); }
        hooks::BeginArgs::parse(&json!({"unit": U1, "round": 1, "check": "c".repeat(check), "files": f}))
    };
    assert!(ok(1024, 1, 0).is_ok());
    assert_eq!(ok(1025, 1, 0), Err("`check` is over 1,024 bytes".to_string()));
    assert!(ok(0, 64, 0).is_ok());
    assert_eq!(ok(0, 65, 0), Err("`files` has more than 64 paths".to_string()));
    assert!(ok(0, 1, 4096).is_ok());
    assert_eq!(ok(0, 1, 4097), Err(hooks::PATH_OVER_4096.to_string()));
    assert_eq!(hooks::PATH_OVER_4096, "`files` has a path over 4,096 bytes");
}

#[test]
fn begin_over_the_limits_is_refused_and_records_nothing() {
    let e = env();
    run(&ctx(&e, 1000), &begin_v(ZC, json!({"unit": U1, "round": 1})), Some("begin"));
    let args = json!({"unit": U2, "round": 1, "files": ["p".repeat(4097)]});
    let got = guarded_run(&e, &ctx(&e, 1010), begin_v(ZC, args).to_string().as_bytes(), Some("begin"));
    assert_eq!(got, (String::new(), 0, vec![format!("zygo hook: work_begin: {}", hooks::PATH_OVER_4096)]));
    assert_eq!(kinds(&e, SUB), ["begin"]);                                // nothing recorded, the open unit stays
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().unit, U1);
    assert!(!e.db.exists());
    // A source hint is held to the same limits.
    let item = |_: &Path, _: &Merged| Some(WorkItem { source: "beads".into(), id: "bd-c3d4".into(), title: None, url: None, status: None,
        check_hint: Some("x".repeat(1025)), files_hint: vec![], lookup_failed: None });
    let c = HookCtx { config: &plain_config, item: &item, ..ctx(&e, 1020) };
    let got = guarded_run(&e, &c, begin_v(ZC, json!({"unit": U2, "round": 1, "files": ["src/"]})).to_string().as_bytes(), Some("begin"));
    assert_eq!(got.2, vec!["zygo hook: work_begin: `check` is over 1,024 bytes".to_string()]);
    assert_eq!(log::open_unit(&e.sd.work(), SUB).unwrap().unit, U1);
}

#[test]
fn begin_supersede_after_stop_ends_the_unit_at_its_stop() {
    let e = env();
    let zc = Some((ZSUB, "zygo-code"));
    run(&ctx(&e, 1000), &begin_v(zc, json!({"unit": U1, "round": 1})), Some("begin"));
    run(&ctx(&e, 1010), &post_v(zc, "Read", json!({"file_path": "/x"}), &e.repo, false), None);
    run(&ctx(&e, 1040), &fixture("SubagentStop-zygo-code-sub.json"), None);
    run(&ctx(&e, 9000), &begin_v(zc, json!({"unit": U2, "round": 1})), Some("begin"));
    let r = rows(&e.db);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].unit.as_str(), r[0].ended_at, r[0].tool_calls), (U1, Some(1040), 1));
}

#[test]
fn begin_same_unit_and_round_after_stop_resumes_it() {
    let e = env();
    let zc = Some((ZSUB, "zygo-code"));
    run(&ctx(&e, 1000), &begin_v(zc, json!({"unit": U1, "round": 1})), Some("begin"));
    run(&ctx(&e, 1040), &fixture("SubagentStop-zygo-code-sub.json"), None);
    assert_eq!(log::open_unit(&e.sd.work(), ZSUB).unwrap().stopped_at, Some(1040));
    run(&ctx(&e, 1050), &begin_v(zc, json!({"unit": U1, "round": 1})), Some("begin"));
    let b = log::open_unit(&e.sd.work(), ZSUB).unwrap();
    assert_eq!((b.stopped_at, b.began_at), (None, 1000));
    assert_eq!(kinds(&e, ZSUB), ["begin"]);
}

#[test]
fn log_unknown_kind_still_counts_for_n() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    log::append(&d, "main", &line(1, Kind::Pushed)).unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open(d.join("main.jsonl")).unwrap();
    f.write_all(b"{\"n\":2,\"ts\":2,\"t\":\"future_kind\"}\n").unwrap();     // a newer zygo in the same session
    log::append(&d, "main", &line(3, Kind::Pushed)).unwrap();
    assert!(std::fs::read_to_string(d.join("main.jsonl")).unwrap().ends_with("{\"n\":3,\"ts\":3,\"t\":\"pushed\"}\n"));
}

#[test]
fn log_n_falls_back_to_the_tail_past_a_long_fragment() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    for ts in 1..=3 {
        log::append(&d, "main", &line(ts, Kind::Pushed)).unwrap();
    }
    let mut f = std::fs::OpenOptions::new().append(true).open(d.join("main.jsonl")).unwrap();
    f.write_all(&vec![b'z'; 3 * log::MAX_LINE_BYTES]).unwrap();          // a torn write longer than the 2 KiB look-back
    log::append(&d, "main", &line(4, Kind::Pushed)).unwrap();
    assert!(std::fs::read_to_string(d.join("main.jsonl")).unwrap().ends_with("z\n{\"n\":4,\"ts\":4,\"t\":\"pushed\"}\n"));
}

#[test]
fn log_tail_window_starting_at_a_line_start_keeps_that_line() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    std::fs::create_dir_all(&d).unwrap();
    let mut text = String::from("{\"n\":1,\"ts\":1,\"t\":\"pushed\"}\n");
    let first = text.len() as u64;
    for n in 2..=257u32 {   // 256 lines of exactly 1,024 bytes: the window starts exactly at line 2
        let head = format!("{{\"n\":{n},\"ts\":1,\"t\":\"repeat\",\"tool\":\"");
        let end = "\"}\n";
        text.push_str(&head);
        text.push_str(&"a".repeat(1024 - head.len() - end.len()));
        text.push_str(end);
    }
    assert_eq!(text.len() as u64, first + log::TAIL_BYTES);
    std::fs::write(d.join("main.jsonl"), &text).unwrap();
    let got = log::tail(&d, "main");
    assert_eq!(got.len(), 256);
    assert_eq!((got[0].0, got[0].1.n), (first, 2));
}

#[test]
fn log_append_takes_exactly_1_kib() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    let sized = |n: u32, total: usize| {
        let base = serde_json::to_string(&LogLine { n, ts: 1, kind: Kind::Repeat { tool: String::new() } }).unwrap().len() + 1;
        line(1, Kind::Repeat { tool: "a".repeat(total - base) })
    };
    log::append(&d, "main", &sized(1, log::MAX_LINE_BYTES)).unwrap();
    assert!(log::append(&d, "main", &sized(2, log::MAX_LINE_BYTES + 1)).is_err());
    let text = std::fs::read_to_string(d.join("main.jsonl")).unwrap();
    assert_eq!(text.len(), log::MAX_LINE_BYTES);
}

#[test]
fn post_line_of_exactly_1_kib_at_line_4000_is_kept() {
    let e = env();
    let h = "0".repeat(16);
    let at4000 = |tool: &str| serde_json::to_string(&LogLine { n: log::MAX_LINES, ts: 1000, kind: Kind::Call { tool: tool.into(), h: h.clone() } }).unwrap().len() + 1;
    let k = log::MAX_LINE_BYTES - at4000("");
    let fit = "T".repeat(k);
    let over = "T".repeat(k + 1);
    assert_eq!(at4000(&fit), log::MAX_LINE_BYTES);
    let r = guarded_run(&e, &ctx(&e, 1000), post_v(None, &fit, json!({}), &e.repo, false).to_string().as_bytes(), None);
    assert_eq!(r, (String::new(), 0, vec![]));
    let r = guarded_run(&e, &ctx(&e, 1000), post_v(None, &over, json!({}), &e.repo, false).to_string().as_bytes(), None);
    assert_eq!(r, (String::new(), 0, vec!["zygo hook: work log line over 1 KiB dropped (call)".to_string()]));
    assert_eq!(kinds(&e, "main"), ["call"]);
}

#[test]
fn fold_stops_at_a_later_begin() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("work");
    let call = |ts| line(ts, Kind::Call { tool: "Bash".into(), h: "0".into() });
    let b = start(&d, "main", U1, 1, 100);
    log::append(&d, "main", &call(101)).unwrap();
    log::append(&d, "main", &line(102, Kind::Begin { unit: U2.into(), round: 1 })).unwrap();
    log::append(&d, "main", &call(103)).unwrap();
    let r = zygo_work::fold::fold_begin(&d, "main", &b).unwrap();
    assert_eq!((r.tool_calls, r.ended_at), (1, Some(101)));
}

fn wu(unit: &str, session: &str, agent: &str, began: i64) -> WorkUnitRow {
    WorkUnitRow { unit: unit.into(), round: 1, session: session.into(), agent_id: agent.into(), began_at: began, ..Default::default() }
}
fn rq(session: &str, agent: &str, agent_id: Option<&str>, ts: i64) -> RequestRow {
    RequestRow { ts, session: session.into(), agent: agent.into(), agent_id: agent_id.map(String::from), model: "m1".into(),
                 role_served: Some("lead".into()), ..Default::default() }
}
fn owned(j: &[join::Joined]) -> Vec<(String, Vec<i64>)> {
    j.iter().map(|x| (x.row.unit.clone(), x.requests.iter().map(|r| r.ts).collect())).collect()
}

#[test]
fn join_cuts_each_agent_by_began_at() {
    let units = [wu("u1", "S1", "main", 100), wu("u2", "S1", "main", 200), wu("s1", "S1", "a1", 150)];
    let cc = "claude-code";
    let reqs = [rq("S1", cc, None, 100), rq("S1", cc, None, 101), rq("S1", cc, None, 200), rq("S1", cc, None, 201),
                rq("S1", cc, None, 999), rq("S1", cc, Some("a1"), 150), rq("S1", cc, Some("a1"), 151), rq("S1", cc, Some("a2"), 300)];
    assert_eq!(owned(&join::join(&units, &reqs)),
               vec![("u1".to_string(), vec![101, 200]), ("u2".to_string(), vec![201, 999]), ("s1".to_string(), vec![151])]);
}

#[test]
fn join_opencode_cuts_over_the_whole_session() {
    let units = [wu("u1", "S1", "main", 100), wu("u2", "S1", "x", 200)];
    let reqs = [rq("S1", "opencode", None, 150), rq("S1", "opencode", None, 250)];
    assert_eq!(owned(&join::join(&units, &reqs)), vec![("u1".to_string(), vec![150]), ("u2".to_string(), vec![250])]);
}

#[test]
fn join_keeps_sessions_apart() {
    let units = [wu("u1", "S1", "main", 100), wu("u1", "S2", "main", 100)];
    let reqs = [rq("S1", "claude-code", None, 150), rq("S2", "claude-code", None, 160)];
    assert_eq!(owned(&join::join(&units, &reqs)), vec![("u1".to_string(), vec![150]), ("u1".to_string(), vec![160])]);
}

fn report_db(dir: &Path) -> PathBuf {
    let db = dir.join("usage.db");
    let s = Store::open(&db).unwrap();
    let row = |unit: &str, round: u32, began: i64, ran: bool, verdict: Option<&str>, guards: &[(&str, &str, u32)]| WorkUnitRow {
        unit: unit.into(), round, session: "S1".into(), agent_id: "main".into(), began_at: began, ended_at: Some(began + 50),
        check_ran: ran, check_passed: ran, verdict: verdict.map(String::from),
        guard_events: guards.iter().fold(BTreeMap::new(), |mut m, (g, l, n)| {
            m.entry(g.to_string()).or_insert_with(BTreeMap::new).insert(l.to_string(), *n); m }),
        ..Default::default() };
    s.insert_work_unit(&row(N12, 1, 100, false, Some("rework"), &[("scope", "notify", 2)])).unwrap();
    s.insert_work_unit(&row(N12, 2, 200, true, Some("keep"), &[])).unwrap();
    s.insert_work_unit(&row(U1, 1, 300, false, None, &[])).unwrap();
    let req = |ts: i64, role: &str, model: &str, input: u64, output: u64, usd: f64| RequestRow { ts, session: "S1".into(),
        agent: "claude-code".into(), role_served: Some(role.into()), model: model.into(), input_tokens: input,
        output_tokens: output, estimated_usd: usd, ..Default::default() };
    for r in [req(150, "lead", "m1", 1000, 10, 0.5), req(250, "lead", "m1", 2000, 10, 0.25),
              req(260, "code", "m2", 5, 0, 0.125), req(350, "lead", "m1", 7, 1, 0.0625)] {
        s.insert_request(&r).unwrap();
    }
    db
}

#[test]
fn report_prints_rounds_tokens_checks_guards_verdicts() {
    let t = tempfile::tempdir().unwrap();
    let db = report_db(t.path());
    assert_eq!(report::report(&db, None, None).unwrap(), concat!(
        "work-graph:zygo-v1-N12\n",
        "  rounds: 2\n",
        "  round 1 main: check ran no, passed no; verdict rework\n",
        "  round 2 main: check ran yes, passed yes; verdict keep\n",
        "  guard events: scope notify 2\n",
        "  code m2: 1 requests, 5 in, 0 out, 0 cache read, 0 cache write, $0.1250\n",
        "  lead m1: 2 requests, 3000 in, 20 out, 0 cache read, 0 cache write, $0.7500\n",
        "\n",
        "beads:bd-a1b2\n",
        "  rounds: 1\n",
        "  round 1 main: check ran no, passed no; verdict none\n",
        "  guard events: none\n",
        "  lead m1: 1 requests, 7 in, 1 out, 0 cache read, 0 cache write, $0.0625"));
    let absent = t.path().join("absent.db");
    assert_eq!(report::report(&absent, None, None).unwrap(), "no work units");
    assert!(!absent.exists());
}

#[test]
fn report_filters_by_unit_and_since() {
    let t = tempfile::tempdir().unwrap();
    let db = report_db(t.path());
    let s = Store::open(&db).unwrap();
    s.insert_work_unit(&WorkUnitRow { unit: format!("{N12}:review"), round: 1, session: "S1".into(), agent_id: "a1".into(),
        began_at: 120, ..Default::default() }).unwrap();
    s.insert_request(&RequestRow { ts: 130, session: "S1".into(), agent: "claude-code".into(), agent_id: Some("a1".into()),
        role_served: Some("review".into()), model: "m3".into(), input_tokens: 9, ..Default::default() }).unwrap();
    let only = report::report(&db, Some(N12), None).unwrap();
    assert!(only.starts_with("work-graph:zygo-v1-N12\n"));
    assert!(only.contains("\n\nwork-graph:zygo-v1-N12:review\n"));
    assert!(only.contains("  review m3: 1 requests, 9 in, 0 out, 0 cache read, 0 cache write, $0.0000"));
    assert!(!only.contains("beads:"));
    assert_eq!(report::report(&db, Some("work-graph:zygo-v1-N1"), None).unwrap(), "no work units");
    let since = report::report(&db, None, Some(150)).unwrap();
    assert!(since.starts_with(concat!(
        "work-graph:zygo-v1-N12\n",
        "  rounds: 1\n",
        "  round 2 main: check ran yes, passed yes; verdict keep\n",
        "  guard events: none\n",
        "  code m2: 1 requests, 5 in, 0 out, 0 cache read, 0 cache write, $0.1250\n",
        "  lead m1: 1 requests, 2000 in, 10 out, 0 cache read, 0 cache write, $0.2500\n")));
    assert!(!since.contains("round 1 main: check ran no, passed no; verdict rework") && !since.contains(":review"));
}

#[test]
fn report_since_takes_a_date() {
    assert_eq!(report::parse_since("2026-11-18").unwrap(), 1_794_960_000);
    for bad in ["1d", "2026-13-01", "18-11-2026", ""] {
        assert!(report::parse_since(bad).is_err(), "{bad}");
    }
}

#[test] // N66: the session's `team.toml` is the hooks' team layer (§6.1 layer 2, §14 note 2026-11-19); its `min_level` lifts the claims guard (§13.4)
fn team_toml_min_level_reaches_the_claims_guard() {
    const ENFORCE: &str = "zygo: run the acceptance check `cargo test -p demo` alone, in the repository you changed, after your last edit, and quote its output in your report; say so if it fails.";
    let notify = json!({"hookSpecificOutput": {"hookEventName": "SubagentStop",
        "additionalContext": format!("zygo: this subagent did not run its acceptance check after its last edit (`{U1}`).")}});
    // The SubagentStop output and the config line, with `team` as the session's team.toml and claims = notify in user config.
    let stop = |team: Option<&str>| -> (Option<Value>, Option<String>) {
        let e = env();
        let h = e.t.path().join("home");
        std::fs::create_dir_all(&h).unwrap();
        std::fs::write(h.join("config.toml"), "[guards]\nclaims = \"notify\"\n").unwrap();
        let paths = Paths { config_file: h.join("config.toml"), cache_dir: h.join("cache"), data_dir: h.join("data"),
            sessions_dir: h.join("cache/sessions"), marketplaces_dir: h.join("cache/marketplaces"), plugin_store: h.join("data/plugins"),
            approvals_file: h.join("data/approvals.toml"), usage_db: h.join("data/usage.db") };
        if let Some(t) = team {
            std::fs::write(e.sd.root.join("team.toml"), t).unwrap();
        }
        let line = RefCell::new(None);
        let cfg = |_: &Path| {
            let (m, l) = hooks::session_config(&paths, Some(&e.sd), None);
            *line.borrow_mut() = l;
            m
        };
        let c = HookCtx { config: &cfg, ..ctx(&e, 1000) };
        run(&c, &begin_v(Some((ZSUB, "zygo-code")), json!({"unit": U1, "round": 1, "check": "cargo test -p demo"})), Some("begin"));
        let out = match hooks::run_with(&hook(&fixture("SubagentStop-zygo-code-sub.json"), None), &c).unwrap() {
            HookOutput::Json(v) => Some(v),
            _ => None,
        };
        let l = line.borrow_mut().take();
        (out, l)
    };
    assert_eq!(stop(None), (Some(notify.clone()), None));                     // no team.toml: no team layer (N37 limit N3)
    assert_eq!(stop(Some("[guards]\nmin_level = \"enforce\"\n")), (Some(json!({"decision": "block", "reason": ENFORCE})), None));
    assert_eq!(stop(Some("[guards\n")), (Some(notify.clone()), Some("team.toml not read (not valid TOML); user levels apply".into())));
    let (out, l) = stop(Some("[guards]\nmin_level = \"bogus\"\n"));
    assert_eq!(out, Some(notify));
    assert!(l.as_deref().is_some_and(|l| l.starts_with("team.toml not used (") && l.ends_with("); user levels apply")), "{l:?}");
} // mutations: `session_config` loads with `team: None` (→ case 2 prints the notify, not the block: the acceptance check);
  // an unreadable or invalid team.toml gives no config (→ case 3/4 print nothing); no line for a dropped team.toml (→ None);
  // a read error fails the hook (→ `unwrap` panics); the team layer under the user layer's level without the floor (→ notify)
