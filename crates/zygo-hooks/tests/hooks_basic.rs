use std::path::{Path, PathBuf};
use std::time::Duration;
use serde_json::{json, Value};
use zygo_core::hook::{guarded, HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_work::log::{self, Kind, LogLine};
use zygo_hooks::{role_guard, token, trim};
use zygo_usage::{Filter, Store as Usage};

struct Env { _t: tempfile::TempDir, dir: SessionDir }
fn env_with(results: bool, threshold: u64, head: u32, tail: u32) -> Env {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView { session: "sess-1".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: Some("lead".into()),
        role_prompt: None, user_prompt: None, profile: None, profile_prompt: None, work_item: None, legend: vec![],
        disabled_roles: vec![], work_units: true, results, compact_tool: true, navigation: false,
        trim_threshold_bytes: threshold, head_lines: head, tail_lines: tail, ..Default::default() }.save(&dir).unwrap();
    Env { _t: t, dir }
}
fn env(results: bool) -> Env { env_with(results, 16384, 60, 20) }

#[test] fn token_prints_file_content() {
    let e = env(true); e.dir.write_token("session-token-for-tests").unwrap();
    assert_eq!(guarded(&|_| token::token_in(&e.dir), b""), ("session-token-for-tests".to_string(), 0));
}
#[test] fn token_strips_trailing_newline() {
    let e = env(true); e.dir.write_token("session-token-for-tests\n").unwrap();
    assert_eq!(guarded(&|_| token::token_in(&e.dir), b""), ("session-token-for-tests".to_string(), 0));
}
#[test] fn token_missing_file_prints_nothing() {   // (reg)
    let e = env(true);
    assert_eq!(guarded(&|_| token::token_in(&e.dir), b""), (String::new(), 0));
}

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(format!("{name}.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn bytes(v: &Value) -> Vec<u8> { serde_json::to_vec(v).unwrap() }
fn parse(b: &[u8]) -> anyhow::Result<HookInput> { HookInput::parse(Agent::ClaudeCode, b) }
const DENY: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"zygo: the lead does not edit files. Delegate this change to the zygo-code subagent with the files, the change and how to check it."}}"#;
fn guard(b: &[u8]) -> (String, i32) { guarded(&|b| role_guard::guard_in(&parse(b)?, None), b) }
fn deny_json() -> Value { serde_json::from_str(DENY).unwrap() }

#[test] fn lead_main_edit_write_notebook_denied() {
    for n in ["PreToolUse-Edit-main", "PreToolUse-Write-main", "PreToolUse-NotebookEdit-main"] {
        let (o, c) = guard(&bytes(&fixture(n)));
        assert_eq!(c, 0, "{n}");
        assert_eq!(serde_json::from_str::<Value>(&o).unwrap(), deny_json(), "{n}");
    }
}
#[test] fn lead_tool_name_match_is_ascii_case_insensitive() {
    let mut v = fixture("PreToolUse-Edit-main"); v["tool_name"] = json!("edit");
    assert_eq!(serde_json::from_str::<Value>(&guard(&bytes(&v)).0).unwrap(), deny_json());
}
#[test] fn lead_newer_version_extra_fields_still_denied() {
    let mut v = fixture("PreToolUse-Edit-main"); v["future_field"] = json!({"a": [1, 2]}); v["effort"] = json!("max");
    assert_eq!(serde_json::from_str::<Value>(&guard(&bytes(&v)).0).unwrap(), deny_json());
}
#[test] fn lead_subagent_gets_no_output() {   // (reg)
    for n in ["PreToolUse-Edit-sub", "PreToolUse-Write-sub", "PreToolUse-NotebookEdit-sub"] { assert_eq!(guard(&bytes(&fixture(n))), (String::new(), 0), "{n}"); }
}
#[test] fn lead_agent_id_null_is_subagent() {   // (reg)
    let mut v = fixture("PreToolUse-Edit-main"); v["agent_id"] = Value::Null;
    assert_eq!(guard(&bytes(&v)), (String::new(), 0));
}
#[test] fn lead_other_tools_no_output() {   // (reg)
    for n in ["PreToolUse-Bash-main", "PreToolUse-Read-main"] { assert_eq!(guard(&bytes(&fixture(n))), (String::new(), 0), "{n}"); }
}
#[test] fn lead_not_json() { assert_eq!(guard(b"not json"), (String::new(), 0)); }   // (reg)
#[test] fn lead_empty() { assert_eq!(guard(b""), (String::new(), 0)); }   // (reg)
#[test] fn lead_wrong_types_no_decision() {   // (reg)
    assert_eq!(guard(br#"{"hook_event_name":"PreToolUse","tool_name":7}"#), (String::new(), 0));
    assert_eq!(guard(b"[]"), (String::new(), 0));
}

#[test] fn trim_view_defaults_when_fields_absent() {
    let e = env(true);
    let mut v: Value = serde_json::from_slice(&std::fs::read(e.dir.view_json()).unwrap()).unwrap();
    for k in ["trim_threshold_bytes", "head_lines", "tail_lines"] { v.as_object_mut().unwrap().remove(k); }
    std::fs::write(e.dir.view_json(), serde_json::to_vec(&v).unwrap()).unwrap();
    let w = SessionView::load(&e.dir).unwrap();
    assert_eq!((w.trim_threshold_bytes, w.head_lines, w.tail_lines), (16384, 60, 20));     // §10.1 defaults
}
#[test] fn trim_view_fields_round_trip() {   // (reg)
    let e = env_with(true, 1000, 3, 4);
    let w = SessionView::load(&e.dir).unwrap();
    assert_eq!((w.trim_threshold_bytes, w.head_lines, w.tail_lines), (1000, 3, 4));
}

fn out(r: anyhow::Result<HookOutput>) -> String { r.unwrap().to_stdout() }
/// 21 bytes per line: "line 000000 xxxxxxxx\n".
fn lines(n: usize) -> String { (0..n).map(|i| format!("line {i:06} xxxxxxxx\n")).collect() }
fn db(e: &Env) -> PathBuf { e._t.path().join("usage").join("usage.db") }
fn run_trim(v: &Value, e: &Env) -> anyhow::Result<HookOutput> { trim::trim_in(&parse(&bytes(v))?, &e.dir, &db(e)) }
fn post(name: &str, resp: Value) -> Value { let mut v = fixture(name); v["tool_response"] = resp; v }
fn bash(stdout: &str) -> Value { post("PostToolUse-Bash-main", json!({"stdout": stdout, "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false})) }
fn updated(stdout: &str) -> Value {      // checks the envelope, returns updatedToolOutput
    let v: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert_eq!(v.as_object().unwrap().len(), 1);
    v["hookSpecificOutput"]["updatedToolOutput"].clone()
}
fn result_ids(e: &Env) -> Vec<String> {
    std::fs::read_dir(e.dir.results()).map(|d| d.filter_map(|f| f.ok()).map(|f| f.file_name().to_string_lossy().into_owned())
        .filter(|n| zygo_results::is_id(n)).collect()).unwrap_or_default()
}
fn bytes_trimmed(e: &Env) -> u64 {
    Usage::open(&db(e)).unwrap().summary(&Filter { session: Some("sess-1".into()), since: None }).unwrap().bytes_trimmed
}
fn dir_names(p: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(p).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).collect();
    n.sort(); n
}
/// The usage database as the launcher leaves it (O§6.5 item 3: the hook's bounded open creates nothing).
fn usage_ready(e: &Env) { drop(Usage::open(&db(e)).unwrap()); }
/// The result id in a §10.1 trim marker.
fn marker_id(view: &str) -> String { view.split("saved as result ").nth(1).unwrap().split('.').next().unwrap().to_string() }
type RowCols = (String, String, i64, i64, Option<String>, Option<String>, Option<String>);
fn trim_rows(e: &Env) -> Vec<RowCols> {
    let c = rusqlite::Connection::open(db(e)).unwrap();
    let mut q = c.prepare("SELECT session, tool, stored_bytes, view_bytes, result_id, command, shaper FROM trims ORDER BY rowid").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).unwrap().map(Result::unwrap).collect()
}
fn trim_cols(e: &Env) -> Vec<String> {
    let c = rusqlite::Connection::open(db(e)).unwrap();
    let mut q = c.prepare("PRAGMA table_xinfo(trims)").unwrap();
    q.query_map([], |r| r.get(1)).unwrap().map(Result::unwrap).collect()
}
/// The `trims` table and its indexes as at `97d8767`: `git show 97d8767:crates/zygo-usage/src/schema.rs`, lines 35-43 and 63-64,
/// copied byte for byte. The hook touches no other table. The full schema copy is `SCHEMA_97D8767` in `crates/zygo-usage/tests/store.rs:37`.
const TRIMS_97D8767: &str = "CREATE TABLE IF NOT EXISTS trims (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    agent_id TEXT,
    tool TEXT NOT NULL,
    kind TEXT NOT NULL,
    stored_bytes INTEGER NOT NULL,
    view_bytes INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS trims_session ON trims(session);
CREATE INDEX IF NOT EXISTS trims_ts ON trims(ts);";
const TRIM_SEVEN: [&str; 7] = ["ts", "session", "agent_id", "tool", "kind", "stored_bytes", "view_bytes"];

#[test] fn trim_bash_100kib_returns_view_and_stores_bytes() {       // §20 #14
    let e = env(true);
    let raw = lines(5000);                                             // 105000 bytes
    let o = updated(&out(run_trim(&bash(&raw), &e)));
    assert_eq!((o["stderr"].as_str(), o["interrupted"].as_bool(), o["isImage"].as_bool(), o["noOutputExpected"].as_bool()), (Some(""), Some(false), Some(false), Some(false)));
    let view = o["stdout"].as_str().unwrap();
    assert!(view.len() <= 8 * 1024);
    let ids = result_ids(&e); assert_eq!(ids.len(), 1);
    let vl: Vec<&str> = view.lines().collect();
    assert_eq!(vl.len(), 81);                                          // 60 head + marker + 20 tail (view.rs:24-48)
    assert_eq!((vl[0], vl[59]), ("line 000000 xxxxxxxx", "line 000059 xxxxxxxx"));
    assert_eq!((vl[61], vl[80]), ("line 004980 xxxxxxxx", "line 004999 xxxxxxxx"));
    assert_eq!(vl[60], format!("[zygo trimmed this output: 5000 lines, 105000 bytes; only part of it is shown. The full output is saved as result {id}. To read the rest, call result_search, result_view or result_tree with id \"{id}\".]", id = ids[0]));
    assert_eq!(zygo_results::Store::get(&e.dir.results(), &ids[0]).unwrap().data, raw.as_bytes());
}
#[test] fn trim_records_bytes_trimmed() {                            // §10.1 Measure
    let e = env(true); usage_ready(&e);
    let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
    assert_eq!(bytes_trimmed(&e), 105000 - o["stdout"].as_str().unwrap().len() as u64);
}
#[test] fn trim_session_is_stored_in_the_result_meta() {             // O§6.5 item 1: the session whose hook stored it
    let e = env(true);
    updated(&out(run_trim(&bash(&lines(5000)), &e)));
    let ids = result_ids(&e); assert_eq!(ids.len(), 1);
    assert_eq!(zygo_results::Store::get(&e.dir.results(), &ids[0]).unwrap().session.as_deref(), Some("sess-1"));
    let meta: Value = serde_json::from_slice(&std::fs::read(e.dir.results().join(format!("{}.json", ids[0]))).unwrap()).unwrap();
    assert_eq!(meta["session"], "sess-1");
}
#[test] fn trim_threshold_is_over_16kib_not_at() {                  // §10.1 default 16 KiB, "over"
    let e = env(true);
    assert_eq!(out(run_trim(&bash(&"a".repeat(16384)), &e)), "");
    assert!(result_ids(&e).is_empty());
    assert!(!out(run_trim(&bash(&"a".repeat(16385)), &e)).is_empty());
    assert_eq!(result_ids(&e).len(), 1);
}
#[test] fn trim_head_and_tail_come_from_the_view() {
    let e = env_with(true, 16384, 2, 1);
    let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
    let vl: Vec<&str> = o["stdout"].as_str().unwrap().lines().collect();
    assert_eq!((vl.len(), vl[0], vl[1], vl[3]), (4, "line 000000 xxxxxxxx", "line 000001 xxxxxxxx", "line 004999 xxxxxxxx"));
}
#[test] fn trim_tool_name_match_is_ascii_case_insensitive() {
    let e = env(true);
    let mut v = bash(&lines(5000)); v["tool_name"] = json!("bash");
    assert!(!out(run_trim(&v, &e)).is_empty());
}
#[test] fn trim_skips_when_view_not_smaller() {                      // chosen reading 8
    let e = env_with(true, 10, 60, 20);
    assert_eq!(out(run_trim(&bash(&lines(2)), &e)), "");               // 42 bytes: view (2 lines + marker) is longer
    assert!(result_ids(&e).is_empty());
    assert!(!out(run_trim(&bash(&lines(200)), &e)).is_empty());        // 4200 bytes: view about 1.9 KB
}
#[test] fn trim_writes_only_results_dir_when_units_off() {           // C3
    let e = env(true);
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.dir).unwrap();
    run_trim(&bash(&lines(5000)), &e).unwrap();
    assert_eq!(dir_names(&e.dir.root), ["overrides.json", "results", "view.json"]);
    assert_eq!(dir_names(&e.dir.results()).len(), 2);                  // <id> and <id>.json
}
#[test] fn trim_bash_counts_stdout_only() {   // (reg) O§9: shape = false counts stdout only
    let e = env(true);
    let mut sv = SessionView::load(&e.dir).unwrap(); sv.shape = false; sv.save(&e.dir).unwrap();
    let mut v = bash("small"); v["tool_response"]["stderr"] = json!("e".repeat(100_000));
    assert_eq!(out(run_trim(&v, &e)), "");
}
#[test] fn trim_bash_image_flag_passes() {   // (reg)
    let e = env(true);
    let mut v = bash(&lines(5000)); v["tool_response"]["isImage"] = json!(true);
    assert_eq!(out(run_trim(&v, &e)), "");
    assert!(result_ids(&e).is_empty());
}

fn names(n: usize) -> Vec<String> { (0..n).map(|i| format!("src/module_{i:04}.rs")).collect() }

#[test] fn trim_read_text_view_has_no_tail() {
    let e = env(true);
    let v = post("PostToolUse-Read-main", json!({"type": "text", "file": {"filePath": "/repo/big.txt", "content": lines(1000), "numLines": 1000, "startLine": 1, "totalLines": 1000}}));
    let o = updated(&out(run_trim(&v, &e)));
    assert_eq!((o["type"].as_str(), o["file"]["filePath"].as_str(), o["file"]["totalLines"].as_u64()), (Some("text"), Some("/repo/big.txt"), Some(1000)));
    let view = o["file"]["content"].as_str().unwrap();
    assert_eq!(view.lines().count(), 61);                              // 60 head + marker, no tail
    assert!(view.lines().last().unwrap().starts_with("[zygo trimmed this output: 1000 lines, 21000 bytes;"));
}
#[test] fn trim_grep_content_mode() {
    let e = env(true);
    let v = post("PostToolUse-Grep-content-main", json!({"mode": "content", "numFiles": 0, "filenames": [], "content": lines(1000), "numLines": 1000, "totalLines": 1000}));
    let o = updated(&out(run_trim(&v, &e)));
    assert_eq!((o["mode"].as_str(), o["numFiles"].as_u64(), o["totalLines"].as_u64()), (Some("content"), Some(0), Some(1000)));
    assert_eq!(o["content"].as_str().unwrap().lines().count(), 81);
    assert!(o["content"].as_str().unwrap().contains("[zygo trimmed this output: 1000 lines, 21000 bytes;"));
}
#[test] fn trim_grep_files_mode_sets_num_files() {
    let e = env(true);
    let v = post("PostToolUse-Grep-main", json!({"mode": "files_with_matches", "filenames": names(3000), "numFiles": 3000, "totalFiles": 3000}));
    let o = updated(&out(run_trim(&v, &e)));
    let f = o["filenames"].as_array().unwrap();
    assert_eq!(f.len(), 81);                                           // 60 + marker + 20
    assert_eq!(o["numFiles"].as_u64(), Some(81));                      // Q4: numFiles = names returned
    assert_eq!((o["totalFiles"].as_u64(), o["mode"].as_str()), (Some(3000), Some("files_with_matches")));
    assert_eq!(f[0], "src/module_0000.rs");
    assert!(f[60].as_str().unwrap().starts_with("[zygo trimmed this output: 3000 lines,"));
}
#[test] fn trim_glob_filenames() {
    let e = env(true);
    let v = post("PostToolUse-Glob-main", json!({"filenames": names(3000), "durationMs": 17, "numFiles": 3000, "truncated": false, "totalMatches": 3000, "countIsComplete": true}));
    let o = updated(&out(run_trim(&v, &e)));
    assert_eq!((o["filenames"].as_array().unwrap().len(), o["numFiles"].as_u64()), (81, Some(81)));
    assert_eq!((o["truncated"].as_bool(), o["totalMatches"].as_u64(), o["countIsComplete"].as_bool()), (Some(false), Some(3000), Some(true)));
}
#[test] fn trim_webfetch_result() {
    let e = env(true);
    let v = post("PostToolUse-WebFetch-main", json!({"bytes": 21000, "code": 200, "codeText": "OK", "result": lines(1000), "durationMs": 127, "url": "https://example.test/page.html"}));
    let o = updated(&out(run_trim(&v, &e)));
    assert_eq!((o["code"].as_u64(), o["url"].as_str(), o["bytes"].as_u64()), (Some(200), Some("https://example.test/page.html"), Some(21000)));
    assert!(o["result"].as_str().unwrap().contains("[zygo trimmed this output: 1000 lines, 21000 bytes;"));
}
#[test] fn trim_read_image_pdf_notebook_pass() {   // (reg)
    let e = env(true);
    let big = "A".repeat(100_000);
    let mut img = fixture("PostToolUse-Read-image-main"); img["tool_response"]["file"]["base64"] = json!(big);
    let mut pdf = fixture("PostToolUse-Read-pdf-main"); pdf["tool_response"]["file"]["base64"] = json!(big);
    let mut nb = fixture("PostToolUse-Read-notebook-main"); nb["tool_response"]["file"]["cells"][0]["source"] = json!(big);
    for v in [img, pdf, nb] { assert_eq!(out(run_trim(&v, &e)), ""); }
    assert!(result_ids(&e).is_empty());
}
#[test] fn trim_read_non_text_type_with_content_passes() {   // (reg) §10.1: only type "text"
    let e = env(true);
    let big = lines(5000);
    let image = post("PostToolUse-Read-main", json!({"type": "image", "file": {"filePath": "/repo/a.png", "content": big}}));
    let untyped = post("PostToolUse-Read-main", json!({"file": {"filePath": "/repo/a.txt", "content": big}}));
    for v in [image, untyped] { assert_eq!(out(run_trim(&v, &e)), ""); }
    assert!(result_ids(&e).is_empty());
}
#[test] fn trim_other_tools_and_modes_pass() {   // (reg) Edit, Write, Agent not hooked; Grep count mode unconfirmed
    let e = env(true);
    let big = "x".repeat(100_000);
    let mut edit = fixture("PostToolUse-Edit-main"); edit["tool_response"]["originalFile"] = json!(big);
    let mut write = fixture("PostToolUse-Write-main"); write["tool_response"]["content"] = json!(big);
    let mut agent = fixture("PostToolUse-Agent-main"); agent["tool_response"]["content"] = json!([{"type": "text", "text": big}]);
    let count = post("PostToolUse-Grep-main", json!({"mode": "count", "content": big, "numFiles": 1, "totalFiles": 1}));
    for v in [edit, write, agent, count] { assert_eq!(out(run_trim(&v, &e)), ""); }
    assert!(result_ids(&e).is_empty());
}
#[test] fn trim_subagent_output_is_trimmed() {   // (reg) the sub payload takes the same path as main
    let e = env(true); usage_ready(&e);
    let mut v = fixture("PostToolUse-Bash-sub"); v["tool_response"]["stdout"] = json!(lines(5000));
    assert!(updated(&out(run_trim(&v, &e)))["stdout"].as_str().unwrap().contains("[zygo trimmed this output: 5000 lines,"));
    assert!(bytes_trimmed(&e) > 0);
}

#[test] fn trim_override_results_off_passes() {
    let e = env(true);
    Overrides { results: Some(false), ..Default::default() }.save(&e.dir).unwrap();
    assert_eq!(out(run_trim(&bash(&lines(5000)), &e)), "");
    assert!(result_ids(&e).is_empty());
}
#[test] fn trim_view_results_false_passes_unless_override_on() {
    let e = env(false);
    assert_eq!(out(run_trim(&bash(&lines(5000)), &e)), "");
    Overrides { results: Some(true), ..Default::default() }.save(&e.dir).unwrap();
    assert!(!out(run_trim(&bash(&lines(5000)), &e)).is_empty());
}
#[test] fn trim_threshold_override_applies() {                      // §17.1 `results threshold <bytes>`
    let e = env(true);
    let v = bash(&lines(200));                                         // 4200 bytes, under the 16 KiB default
    assert_eq!(out(run_trim(&v, &e)), "");
    Overrides { results_threshold: Some(1000), ..Default::default() }.save(&e.dir).unwrap();
    assert!(!out(run_trim(&v, &e)).is_empty());
    Overrides { results_threshold: Some(1_000_000), ..Default::default() }.save(&e.dir).unwrap();
    assert_eq!(out(run_trim(&bash(&lines(5000)), &e)), "");
}
#[test] fn trim_other_events_pass() {   // (reg)
    let e = env(true);
    let mut fail = bash(&lines(5000)); fail["hook_event_name"] = json!("PostToolUseFailure");
    let mut pre = bash(&lines(5000)); pre["hook_event_name"] = json!("PreToolUse");
    for v in [fail, pre] { assert_eq!(out(run_trim(&v, &e)), ""); }
}

fn log_lines(e: &Env, agent: &str) -> Vec<Value> {
    std::fs::read_to_string(e.dir.work().join(format!("{agent}.jsonl"))).map(|s| s.lines().map(|l| serde_json::from_str(l).unwrap()).collect()).unwrap_or_default()
}
#[test] fn trim_appends_trimmed_line_to_main_log() {                // §13.3: `trimmed`
    let e = env(true);
    run_trim(&bash(&lines(5000)), &e).unwrap();
    let l = log_lines(&e, "main");
    assert_eq!(l.len(), 1);
    assert_eq!(l[0]["t"], "trimmed");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    assert!((l[0]["ts"].as_i64().unwrap() - now).abs() < 600, "{}", l[0]["ts"]);
    assert_eq!(l[0].as_object().unwrap().keys().cloned().collect::<std::collections::BTreeSet<_>>(),
               ["n", "t", "ts"].iter().map(|s| s.to_string()).collect());       // no tool name, no text (C3)
}
#[test] fn trim_trimmed_line_goes_to_the_subagent_log() {
    let e = env(true);
    let mut v = fixture("PostToolUse-Bash-sub"); v["tool_response"]["stdout"] = json!(lines(5000));
    run_trim(&v, &e).unwrap();
    assert_eq!(log_lines(&e, "a1a80dd100f197242").len(), 1);
    assert!(log_lines(&e, "main").is_empty());
}
#[test] fn trim_no_trimmed_line_when_nothing_trimmed_or_units_off() {   // (reg)
    let e = env(true);
    run_trim(&bash("small"), &e).unwrap();
    assert!(log_lines(&e, "main").is_empty());
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.dir).unwrap();
    run_trim(&bash(&lines(5000)), &e).unwrap();
    assert!(log_lines(&e, "main").is_empty());
}
#[test] fn trim_null_agent_id_writes_no_work_log_line() {           // agrees with N24: agent_id present but not a string is no usable agent
    let e = env(true);
    let mut v = bash(&lines(5000)); v["agent_id"] = Value::Null;
    assert!(!out(run_trim(&v, &e)).is_empty());
    assert!(log_lines(&e, "main").is_empty());
}
#[test] fn trim_work_log_failure_keeps_view() {   // (reg; partners: the two append tests)
    let e = env(true);
    std::fs::write(e.dir.work(), b"in the way").unwrap();              // work path is a file: append fails
    assert!(updated(&out(run_trim(&bash(&lines(5000)), &e)))["stdout"].as_str().unwrap().contains("[zygo trimmed this output:"));
}

fn trim_guard(e: &Env, b: &[u8]) -> (String, i32) { guarded(&|b| trim::trim_in(&parse(b)?, &e.dir, &db(e)), b) }

#[test] fn trim_usage_store_failure_keeps_view() {
    let e = env(true);
    std::fs::create_dir_all(db(&e)).unwrap();                          // the "database" path is a directory: open fails
    let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
    assert!(o["stdout"].as_str().unwrap().contains("[zygo trimmed this output:"));
    assert_eq!(result_ids(&e).len(), 1);
}
#[test] fn trim_row_bash_records_result_id_and_size_shaper() {      // O§7.9: result_id = stored id, shaper "size", command = O§7.2 name
    let e = env(true); usage_ready(&e);
    let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
    let view = o["stdout"].as_str().unwrap();
    let id = marker_id(view);
    assert_eq!(result_ids(&e), std::slice::from_ref(&id));
    assert_eq!(trim_rows(&e), [("sess-1".to_string(), "Bash".to_string(), 105000, view.len() as i64, Some(id), Some("other".to_string()), Some("size".to_string()))]);
}
#[test] fn trim_row_read_records_result_id_and_size_shaper() {      // O§7.9: shaper "size" for a trim of another tool too
    let e = env(true); usage_ready(&e);
    let v = post("PostToolUse-Read-main", json!({"type": "text", "file": {"filePath": "/repo/big.txt", "content": lines(1000), "numLines": 1000, "startLine": 1, "totalLines": 1000}}));
    let o = updated(&out(run_trim(&v, &e)));
    let view = o["file"]["content"].as_str().unwrap();
    let id = marker_id(view);
    assert_eq!(result_ids(&e), std::slice::from_ref(&id));
    assert_eq!(trim_rows(&e), [("sess-1".to_string(), "Read".to_string(), 21000, view.len() as i64, Some(id), None, Some("size".to_string()))]);
}
#[test] fn trim_row_on_97d8767_database_writes_seven_columns() {    // O§6.5 items 3-4, O§16 item 15
    let e = env(true);
    std::fs::create_dir_all(db(&e).parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(db(&e)).unwrap();
    c.execute_batch(TRIMS_97D8767).unwrap();
    c.execute_batch("PRAGMA user_version = 1").unwrap();
    drop(c);
    let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
    assert!(o["stdout"].as_str().unwrap().contains("[zygo trimmed this output: 5000 lines, 105000 bytes;"));
    assert_eq!(trim_cols(&e), TRIM_SEVEN);                             // no column added
    let c = rusqlite::Connection::open(db(&e)).unwrap();
    let row: (String, String, i64, i64) = c.query_row("SELECT session, tool, stored_bytes, view_bytes FROM trims", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert_eq!(row, ("sess-1".into(), "Bash".into(), 105000, o["stdout"].as_str().unwrap().len() as i64));
}
#[test] fn trim_row_missing_database_keeps_view_and_creates_nothing() {   // O§7.11, O§10 "broken (hook)"
    let e = env(true);
    let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
    assert!(o["stdout"].as_str().unwrap().contains("[zygo trimmed this output:"));
    assert_eq!(result_ids(&e).len(), 1);
    assert!(!db(&e).parent().unwrap().exists());
}
#[test] fn trim_row_not_written_under_write_lock_with_bound_zero() {   // O§7.11, O§10, O§16 item 15; O§8.4: no clock
    let e = env(true); usage_ready(&e);
    let input = parse(&bytes(&bash(&lines(5000)))).unwrap();
    let lock = rusqlite::Connection::open(db(&e)).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();                     // a second connection holds the write lock
    let locked = out(trim::trim_in_with(&input, &e.dir, &db(&e), &trim::TrimOpts { db_bound: Duration::ZERO, ..Default::default() }));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    assert!(trim_rows(&e).is_empty());                                 // no row
    let free = out(trim::trim_in(&input, &e.dir, &db(&e)));
    let id_of = |s: &str| marker_id(updated(s)["stdout"].as_str().unwrap());
    let (a, b) = (id_of(&locked), id_of(&free));
    assert_ne!(a, b);
    assert_eq!(locked.replace(&a, &b), free);                          // same hook output as with no lock, up to the result id
    assert_eq!(trim_rows(&e).len(), 1);
    assert_eq!(trim_rows(&e)[0].4.as_deref(), Some(b.as_str()));
    assert_eq!(result_ids(&e).len(), 2);                               // both results stored
}
#[test] fn trim_result_store_failure_error_is_fixed_text() {
    let e = env(true);
    std::fs::write(e.dir.results(), b"in the way").unwrap();           // results path is a file: put fails
    let body = bash(&format!("SENTINEL-BODY-TEXT\n{}", lines(5000)));
    let err = run_trim(&body, &e).err().unwrap();
    assert_eq!(format!("{err:#}"), "cannot store the tool result");
    assert_eq!(trim_guard(&e, &bytes(&body)), (String::new(), 0));
}
#[test] fn trim_not_json() { assert_eq!(trim_guard(&env(true), b"not json"), (String::new(), 0)); }   // (reg)
#[test] fn trim_empty() { assert_eq!(trim_guard(&env(true), b""), (String::new(), 0)); }                // (reg)
#[test] fn trim_non_object_json() { assert_eq!(trim_guard(&env(true), b"[1,2]"), (String::new(), 0)); } // (reg)
#[test] fn trim_newer_version_extra_fields_still_trimmed() {   // (reg)
    let e = env(true);
    let mut v = bash(&lines(5000)); v["future_field"] = json!({"a": 1}); v["tool_response"]["newField"] = json!(true);
    let (o, c) = trim_guard(&e, &bytes(&v));
    assert_eq!(c, 0);
    assert_eq!(updated(&o)["newField"], json!(true));                  // unknown fields are carried through
}
#[test] fn trim_newer_version_changed_shape_passes() {   // (reg)
    let e = env(true);
    let mut a = bash("x"); a["tool_response"]["stdout"] = json!(12345);                // stdout no longer a string
    let mut b = bash("x"); b.as_object_mut().unwrap().remove("tool_response");         // no tool_response
    let mut c = bash("x"); c["tool_response"] = json!({"output": lines(5000)});        // renamed field
    for v in [a, b, c] { assert_eq!(trim_guard(&e, &bytes(&v)), (String::new(), 0)); }
    assert!(result_ids(&e).is_empty());
}

fn lead_in(e: &Env, v: &Value) -> anyhow::Result<HookOutput> { role_guard::guard_in(&parse(&bytes(v))?, Some(&e.dir)) }
fn lead_out(e: &Env, v: &Value) -> (String, i32) { guarded(&|b| role_guard::guard_in(&parse(b)?, Some(&e.dir)), &bytes(v)) }

#[test] fn lead_log_refusal_appends_one_guard_line() {
    // kills: no line; line only for one tool; wrong guard or level text; line written to another agent's log
    let e = env(true);
    for (i, n) in ["PreToolUse-Edit-main", "PreToolUse-Write-main", "PreToolUse-NotebookEdit-main"].iter().enumerate() {
        let (o, c) = lead_out(&e, &fixture(n));
        assert_eq!(c, 0, "{n}");
        assert_eq!(serde_json::from_str::<Value>(&o).unwrap(), deny_json(), "{n}");
        let l = log_lines(&e, "main");
        assert_eq!(l.len(), i + 1, "{n}");
        let x = &l[i];
        assert_eq!(x["n"], i as u64 + 1);
        assert_eq!((x["t"].as_str(), x["guard"].as_str(), x["level"].as_str()), (Some("guard"), Some("lead"), Some("enforce")));
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!((x["ts"].as_i64().unwrap() - now).abs() < 600);
        assert_eq!(x.as_object().unwrap().keys().cloned().collect::<std::collections::BTreeSet<_>>(),
                   ["n", "t", "ts", "guard", "level"].iter().map(|s| s.to_string()).collect());
    }
}
#[test] fn lead_log_allowed_actions_append_none() {   // (reg) kills: line written before the refusal test; line for a subagent edit
    let e = env(true);
    let mut null_id = fixture("PreToolUse-Edit-main"); null_id["agent_id"] = Value::Null;
    for v in [fixture("PreToolUse-Bash-main"), fixture("PreToolUse-Read-main"), fixture("PreToolUse-Edit-sub"), null_id] {
        assert_eq!(lead_out(&e, &v), (String::new(), 0));
    }
    assert!(log_lines(&e, "main").is_empty());
    assert!(log_lines(&e, "a1a80dd100f197242").is_empty());
}
#[test] fn lead_log_full_log_still_denies() {   // (reg) kills: Ok(None) treated as an error
    let e = env(true);
    for _ in 0..log::MAX_LINES { log::append(&e.dir.work(), "main", &LogLine { n: 0, ts: 1, kind: Kind::Trimmed }).unwrap(); }
    let (o, c) = lead_out(&e, &fixture("PreToolUse-Edit-main"));
    assert_eq!(c, 0);
    assert_eq!(serde_json::from_str::<Value>(&o).unwrap(), deny_json());
    assert_eq!(log_lines(&e, "main").len(), log::MAX_LINES as usize);
}
#[test] fn lead_log_write_error_prints_no_deny() {   // kills: a failed write swallowed and the deny printed
    let e = env(true);
    std::fs::write(e.dir.work(), b"in the way").unwrap();
    assert!(lead_in(&e, &fixture("PreToolUse-Edit-main")).is_err());
    assert_eq!(lead_out(&e, &fixture("PreToolUse-Edit-main")), (String::new(), 0));
}
#[test] fn lead_log_written_with_work_units_off() {   // kills: the units gate applied to the lead guard
    let e = env(true);
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.dir).unwrap();
    let (o, _) = lead_out(&e, &fixture("PreToolUse-Edit-main"));
    assert_eq!(serde_json::from_str::<Value>(&o).unwrap(), deny_json());
    let l = log_lines(&e, "main");
    assert_eq!(l.len(), 1);
    assert_eq!((l[0]["t"].as_str(), l[0]["guard"].as_str(), l[0]["level"].as_str()), (Some("guard"), Some("lead"), Some("enforce")));
}
#[test] fn lead_log_no_session_dir_denies_and_writes_nothing() {   // (reg) kills: a missing session dir turned into an error or a lost refusal
    let t = tempfile::tempdir().unwrap();
    let o = role_guard::guard_in(&parse(&bytes(&fixture("PreToolUse-Edit-main"))).unwrap(), None).unwrap().to_stdout();
    assert_eq!(serde_json::from_str::<Value>(&o).unwrap(), deny_json());
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
}
