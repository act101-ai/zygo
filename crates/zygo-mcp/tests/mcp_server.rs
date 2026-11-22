use serde_json::{json, Value};
use zygo_core::Agent;
use zygo_mcp::server::serve;
use zygo_mcp::{Tool, ToolDef, ToolError};

fn def(name: &str) -> ToolDef {
    ToolDef { name: name.into(), description: "d".into(), input_schema: json!({"type":"object"}) }
}
struct Echo;
impl Tool for Echo {
    fn def(&self) -> ToolDef { def("echo") }
    fn call(&self, args: Value) -> Result<Value, ToolError> { Ok(args) }
}
struct Boom;
impl Tool for Boom {
    fn def(&self) -> ToolDef { def("boom") }
    fn call(&self, _: Value) -> Result<Value, ToolError> { Err(ToolError::Failed("it broke".into())) }
}
struct Bad;
impl Tool for Bad {
    fn def(&self) -> ToolDef { def("bad") }
    fn call(&self, _: Value) -> Result<Value, ToolError> { Err(ToolError::InvalidParams("need x".into())) }
}
struct Text;
impl Tool for Text {
    fn def(&self) -> ToolDef { def("text") }
    fn call(&self, _: Value) -> Result<Value, ToolError> { Ok(json!("plain")) }
}
struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl Tool for Counting {
    fn def(&self) -> ToolDef { self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst); def("count") }
    fn call(&self, _: Value) -> Result<Value, ToolError> { Ok(json!("c")) }
}

struct Panics;
impl Tool for Panics {
    fn def(&self) -> ToolDef { def("panics") }
    fn call(&self, _: Value) -> Result<Value, ToolError> { panic!("tool bug") }
}

fn tools() -> Vec<Box<dyn Tool>> { vec![Box::new(Echo), Box::new(Boom), Box::new(Bad), Box::new(Text), Box::new(Panics)] }

fn run(input: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    serve(Agent::ClaudeCode, &tools(), input, &mut out).unwrap();
    out.split(|b| *b == b'\n').filter(|l| !l.is_empty()).map(|l| serde_json::from_slice(l).unwrap()).collect()
}
fn one(line: Value) -> Value { run(format!("{line}\n").as_bytes()).remove(0) }
const PING: &str = "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}";

#[test]
fn initialize_echoes_a_supported_version() {
    for v in ["2025-06-18", "2024-11-05"] {
        let r = one(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":v,"capabilities":{},"clientInfo":{"name":"c","version":"1"}}}));
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], v);
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "zygo");
    }
}

#[test]
fn initialize_unknown_version_gets_2025_11_25_and_initialized_gets_no_reply() {
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"1999-01-01\"}}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    let out = run(input);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn initialize_without_a_version_is_invalid_params() {
    for params in [json!({}), json!({"protocolVersion": 5}), Value::Null] {
        let r = one(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":params}));
        assert_eq!(r["error"]["code"], -32602);
    }
}

#[test]
fn newer_revision_is_not_served() {
    let r = one(json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}));
    assert_eq!(r["error"]["code"], -32601);
}

#[test]
fn tools_list_and_call_shapes() {
    let l = one(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    let names: Vec<_> = l["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["echo", "boom", "bad", "text", "panics"]);
    assert_eq!(l["result"]["tools"][0]["inputSchema"]["type"], "object");
    assert_eq!(l["result"]["tools"][0]["description"], "d");
    let c = one(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"text","arguments":{}}}));
    assert_eq!(c["result"]["content"], json!([{"type":"text","text":"plain"}]));
    assert_eq!(c["result"]["isError"], false);
    let c = one(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"a":1}}}));
    assert_eq!(c["result"]["content"][0]["text"], "{\"a\":1}");
    let c = one(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"echo"}}));
    assert_eq!(c["result"]["content"][0]["text"], "{}");   // missing arguments → {}
}

#[test]
fn tool_errors_map_to_protocol_error_or_is_error() {
    let c = one(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"boom"}}));
    assert_eq!(c["result"]["isError"], true);
    assert_eq!(c["result"]["content"][0]["text"], "it broke");
    assert!(c.get("error").is_none());
    let c = one(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"bad"}}));
    assert_eq!(c["error"]["code"], -32602);
    assert_eq!(c["error"]["message"], "need x");
    let c = one(json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope"}}));
    assert_eq!(c["error"]["code"], -32602);
    assert!(c["error"]["message"].as_str().unwrap().contains("nope"));
    let c = one(json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{}}));
    assert_eq!(c["error"]["code"], -32602);
}

#[test]
fn defs_are_read_once_per_server() {
    use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};
    let n = Arc::new(AtomicUsize::new(0));
    let t: Vec<Box<dyn Tool>> = vec![Box::new(Counting(n.clone()))];
    let mut out = Vec::new();
    let input = format!("{}\n{}\n{}\n",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"count"}}));
    serve(Agent::ClaudeCode, &t, input.as_bytes(), &mut out).unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[test]
fn protocol_errors_and_ping() {
    let e = one(json!({"jsonrpc":"2.0","id":"x","method":"resources/list"}));
    assert_eq!(e["id"], "x");
    assert_eq!(e["error"]["code"], -32601);
    assert_eq!(one(json!([1]))["error"]["code"], -32600);
    assert_eq!(one(json!({"jsonrpc":"2.0","id":9,"result":{}}))["error"]["code"], -32600);
    assert_eq!(one(json!({"jsonrpc":"2.0","id":7,"method":"ping"}))["result"], json!({}));
}

#[test]
fn null_id_is_a_request_not_a_notification() {
    let r = one(json!({"jsonrpc":"2.0","id":null,"method":"ping"}));
    assert!(r["id"].is_null());
    assert_eq!(r["result"], json!({}));
}

#[test]
fn malformed_line_gives_parse_error_and_server_keeps_running() {
    let mut input = b"{not json\n".to_vec();
    input.extend_from_slice(b"\xff\xfe\n");
    input.extend_from_slice(b"\n");
    input.extend_from_slice(PING.as_bytes());          // no trailing newline
    let out = run(&input);
    assert_eq!(out.len(), 3);
    assert_eq!(out[0]["error"]["code"], -32700);
    assert!(out[0]["id"].is_null());
    assert_eq!(out[1]["error"]["code"], -32700);
    assert_eq!(out[2]["id"], 7);
    assert_eq!(out[2]["result"], json!({}));
}

#[test]
fn line_over_4_mib_is_discarded_and_server_keeps_running() {
    const MAX: usize = 4 * 1024 * 1024;
    let mut input = vec![b'a'; MAX + 10];
    input.push(b'\n');
    input.extend_from_slice(PING.as_bytes());
    input.push(b'\n');
    let out = run(&input);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["error"]["code"], -32700);
    assert_eq!(out[1]["result"], json!({}));
    // too long and never terminated: one error, then a clean end
    let out = run(&vec![b'a'; MAX + 5]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["error"]["code"], -32700);
}

#[test]
fn tool_panic_is_an_is_error_result_and_server_keeps_serving() {
    let input = format!("{}\n{PING}\n", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"panics"}}));
    let out = run(input.as_bytes());
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["id"], 1);
    assert_eq!(out[0]["result"]["isError"], true);
    assert_eq!(out[0]["result"]["content"], json!([{"type":"text","text":"zygo: tool panics failed"}]));
    assert_eq!(out[1]["id"], 7);
    assert_eq!(out[1]["result"], json!({}));
}

#[test]
fn id_must_be_a_string_an_integer_or_null() {
    for id in [json!({"a":1}), json!([1]), json!(true), json!(1.5)] {
        let r = one(json!({"jsonrpc":"2.0","id":id,"method":"ping"}));
        assert_eq!(r["error"]["code"], -32600, "id {id}");
        assert!(r.get("id").is_some_and(Value::is_null), "id {id}");
    }
    for id in [json!("s"), json!(-3), json!(0), json!(u64::MAX)] {
        let r = one(json!({"jsonrpc":"2.0","id":id.clone(),"method":"ping"}));
        assert_eq!(r["id"], id);
        assert_eq!(r["result"], json!({}));
    }
}

#[test]
fn jsonrpc_must_be_2_0() {
    let r = one(json!({"id":1,"method":"ping"}));
    assert_eq!(r["error"]["code"], -32600);
    assert_eq!(r["id"], 1);
    let r = one(json!({"jsonrpc":"1.0","id":2,"method":"ping"}));
    assert_eq!(r["error"]["code"], -32600);
    assert_eq!(r["id"], 2);
    let r = one(json!({"jsonrpc":2.0,"id":3,"method":"ping"}));
    assert_eq!(r["error"]["code"], -32600);
}

#[test]
fn arguments_that_are_not_an_object_are_invalid_params() {
    for a in [json!([1]), json!("x"), json!(3), json!(true), Value::Null] {
        let r = one(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo","arguments":a.clone()}}));
        assert_eq!(r["error"]["code"], -32602, "arguments {a}");
    }
}

#[test]
fn line_of_exactly_4_mib_is_accepted_and_one_byte_more_is_refused() {
    const MAX: usize = 4 * 1024 * 1024;
    let mut ok = PING.as_bytes().to_vec();
    ok.resize(MAX, b' ');                               // valid JSON padded to exactly 4 MiB
    ok.push(b'\n');
    let out = run(&ok);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["result"], json!({}));
    let mut over = PING.as_bytes().to_vec();
    over.resize(MAX + 1, b' ');
    over.push(b'\n');
    let out = run(&over);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["error"]["code"], -32700);
}

#[test]
fn eof_is_a_clean_exit() {
    let mut out = Vec::new();
    serve(Agent::OpenCode, &tools(), &b""[..], &mut out).unwrap();
    assert!(out.is_empty());
}

use std::cell::Cell;
use zygo_config::Features;
use zygo_mcp::tools::{self, compact};
use zygo_mcp::McpCtx;

fn ctx(agent: Agent, f: impl FnOnce(&mut Features)) -> McpCtx {
    let mut features = Features::default();
    f(&mut features);
    McpCtx { agent, features, session_dir: None }
}
fn names(t: &[Box<dyn Tool>]) -> Vec<String> { t.iter().map(|t| t.def().name).collect() }
fn fake(name: &'static str) -> impl FnOnce(&McpCtx) -> Vec<Box<dyn Tool>> {
    move |_| {
        struct F(&'static str);
        impl Tool for F {
            fn def(&self) -> ToolDef { def(self.0) }
            fn call(&self, _: Value) -> Result<Value, ToolError> { Ok(json!("ok")) }
        }
        vec![Box::new(F(name))]
    }
}

#[test]
fn compact_description_is_exact_and_names_no_window_size_or_price() {
    assert_eq!(compact::DESCRIPTION, "Compact this conversation now. Call it at a boundary of your work, when what came before is no longer needed, for example before starting the next unit of a plan.");
    let d = compact::DESCRIPTION.to_lowercase();
    for w in ["window", "size", "price", "cost", "token", "context", "limit", "$"] {
        assert!(!d.contains(w), "description names `{w}`");
    }
    let d = compact::Compact.def();
    assert_eq!(d.name, "compact");
    assert_eq!(d.description, compact::DESCRIPTION);
    assert_eq!(d.input_schema, json!({"type":"object","additionalProperties":false}));
}

#[test]
fn compact_with_any_property_is_invalid_params() {
    assert!(matches!(compact::Compact.call(json!({"x":1})), Err(ToolError::InvalidParams(_))));
    assert_eq!(compact::Compact.call(json!({})), Ok(json!("compaction requested")));
    let t: Vec<Box<dyn Tool>> = vec![Box::new(compact::Compact)];
    let mut out = Vec::new();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"compact\",\"arguments\":{\"now\":true}}}\n";
    serve(Agent::ClaudeCode, &t, req.as_bytes(), &mut out).unwrap();
    let r: Value = serde_json::from_slice(out.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn compact_listed_for_claude_code_and_returns_compaction_requested() {
    let t = tools::build_with(&ctx(Agent::ClaudeCode, |_| {}), |_| vec![], |_| vec![]).unwrap();
    assert_eq!(names(&t), ["compact"]);
    let mut out = Vec::new();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"compact\",\"arguments\":{}}}\n";
    serve(Agent::ClaudeCode, &t, req.as_bytes(), &mut out).unwrap();
    let r: Value = serde_json::from_slice(out.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(r["result"]["content"], json!([{"type":"text","text":"compaction requested"}]));
    assert_eq!(r["result"]["isError"], false);
}

#[test]
fn compact_absent_for_opencode() {
    let t = tools::build_with(&ctx(Agent::OpenCode, |_| {}), |_| vec![], |_| vec![]).unwrap();
    assert!(names(&t).is_empty());
}

#[test]
fn feature_switches_remove_their_tools_and_skip_their_seams() {
    let called = Cell::new((false, false));
    let on = tools::build_with(&ctx(Agent::ClaudeCode, |_| {}),
        |c| { called.set((true, called.get().1)); fake("result_view")(c) },
        |c| { called.set((called.get().0, true)); fake("work_begin")(c) }).unwrap();
    assert_eq!(names(&on), ["compact", "result_view", "work_begin"]);
    assert_eq!(called.get(), (true, true));

    called.set((false, false));
    let off = tools::build_with(&ctx(Agent::ClaudeCode, |f| { f.compact_tool = false; f.results = false; f.work_units = false; }),
        |c| { called.set((true, called.get().1)); fake("result_view")(c) },
        |c| { called.set((called.get().0, true)); fake("work_begin")(c) }).unwrap();
    assert!(names(&off).is_empty());
    assert_eq!(called.get(), (false, false));

    let t = tools::build_with(&ctx(Agent::ClaudeCode, |f| f.results = false), fake("result_view"), fake("work_begin")).unwrap();
    assert_eq!(names(&t), ["compact", "work_begin"]);
    let t = tools::build_with(&ctx(Agent::ClaudeCode, |f| f.compact_tool = false), fake("result_view"), fake("work_begin")).unwrap();
    assert_eq!(names(&t), ["result_view", "work_begin"]);
}

#[test]
fn duplicate_tool_names_are_refused_naming_the_name() {
    let e = tools::build_with(&ctx(Agent::ClaudeCode, |_| {}), fake("compact"), |_| vec![]).err().unwrap();
    assert!(e.to_string().contains("compact"), "{e}");
    let e = tools::build_with(&ctx(Agent::OpenCode, |_| {}), fake("result_view"), fake("result_view")).err().unwrap();
    assert!(e.to_string().contains("result_view"), "{e}");
}

use std::path::Path;
use zygo_core::{overrides::Overrides, paths::Paths, session_dir::SessionDir, session_view::SessionView};

fn paths(root: &Path, config: &str) -> Paths {
    let config_file = root.join("config.toml");
    std::fs::write(&config_file, config).unwrap();
    Paths {
        config_file, cache_dir: root.join("cache"), data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"), marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"), approvals_file: root.join("data/approvals.toml"), usage_db: root.join("data/usage.db"),
    }
}
fn view(results: bool, compact_tool: bool, work_units: bool) -> SessionView {
    SessionView { session: "s".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: None,
        role_prompt: None, user_prompt: None, profile: None, profile_prompt: None, work_item: None,
        legend: vec![], disabled_roles: vec![], work_units, results, compact_tool, navigation: false,
        trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }
}
fn resolve(p: &Paths, sd: Option<SessionDir>) -> (anyhow::Result<McpCtx>, String) {
    let mut warn = Vec::new();
    let r = McpCtx::resolve_with(Agent::ClaudeCode, p, None, sd, &mut warn);
    (r, String::from_utf8(warn).unwrap())
}
fn session(root: &Path) -> SessionDir {
    let sd = SessionDir::new(root.join("s"));
    sd.create().unwrap();
    sd
}

#[test]
fn outside_a_session_config_decides() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "[features]\ncompact_tool = false\nresults = false\n");
    let (c, warn) = resolve(&p, None);
    let c = c.unwrap();
    assert!(!c.features.compact_tool && !c.features.results && c.features.work_units);
    assert!(c.session_dir.is_none());
    assert_eq!(warn, "");
    assert!(tools::build_with(&c, fake("result_view"), |_| vec![]).unwrap().is_empty());   // both switches off
}

#[test]
fn session_view_outranks_config_and_removes_the_tools() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");                        // config: everything on
    let sd = session(d.path());
    view(true, false, true).save(&sd).unwrap();         // view: compact_tool off
    let c = resolve(&p, Some(sd.clone())).0.unwrap();
    assert!(!c.features.compact_tool && c.features.results);
    let t = tools::build_with(&c, fake("result_view"), |_| vec![]).unwrap();
    assert_eq!(names(&t), ["result_view"]);             // `compact` removed
    view(false, true, true).save(&sd).unwrap();         // view: results off
    let c = resolve(&p, Some(sd.clone())).0.unwrap();
    let t = tools::build_with(&c, fake("result_view"), |_| vec![]).unwrap();
    assert_eq!(names(&t), ["compact"]);                 // result tools removed
    // the view says off while config says on, and the reverse
    let p2 = paths(d.path(), "[features]\nresults = false\ncompact_tool = false\n");
    view(true, true, true).save(&sd).unwrap();
    let c = resolve(&p2, Some(sd.clone())).0.unwrap();
    assert!(c.features.results && c.features.compact_tool);
}

#[test]
fn session_override_outranks_the_view() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    let sd = session(d.path());
    view(false, true, false).save(&sd).unwrap();
    Overrides { results: Some(true), work_units: Some(true), ..Default::default() }.save(&sd).unwrap();
    let c = resolve(&p, Some(sd)).0.unwrap();
    assert!(c.features.results && c.features.work_units && c.features.compact_tool);
}

#[test]
fn corrupt_view_warns_once_and_config_stays() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "[features]\ncompact_tool = false\n");
    let sd = session(d.path());
    std::fs::write(sd.view_json(), b"{broken").unwrap();
    let (c, warn) = resolve(&p, Some(sd));
    let c = c.unwrap();
    assert!(!c.features.compact_tool && c.features.results);
    assert_eq!(warn.lines().count(), 1, "{warn:?}");
    assert!(warn.contains("session view"), "{warn:?}");
    assert!(!warn.contains("broken"));                  // no file content in the message
}

#[test]
fn session_dir_without_view_is_silent_and_uses_config() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "[features]\nwork_units = false\n");
    let (c, warn) = resolve(&p, Some(SessionDir::new(d.path().join("none"))));
    assert!(c.unwrap().features.results);
    assert_eq!(warn, "");
}

#[test]
fn broken_config_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "[features\n");
    assert!(resolve(&p, None).0.is_err());
}

fn broken() -> std::process::Stdio {
    let (r, w) = std::io::pipe().unwrap();
    drop(r);
    w.into()
}

/// Child half: runs only when the parent sets `ZYGO_MCP_CASE`; its stderr is a closed pipe.
#[test]
fn child_closed_stderr() {
    let Ok(case) = std::env::var("ZYGO_MCP_CASE") else { return };
    let input: Vec<u8> = match case.as_str() {
        "long" => { let mut v = vec![b'x'; 4 * 1024 * 1024 + 1]; v.push(b'\n'); v.extend_from_slice(PING.as_bytes()); v.push(b'\n'); v }
        "notjson" => format!("not json\n{PING}\n").into_bytes(),
        "panic" => format!("{}\n{PING}\n", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"panics"}})).into_bytes(),
        other => panic!("unknown case {other}"),
    };
    let mut out = Vec::new();
    serve(Agent::ClaudeCode, &tools(), &input[..], &mut out).unwrap();
    let n = String::from_utf8(out).unwrap().lines().count();
    assert_eq!(n, 2, "an error or is-error reply, then the ping reply");
}

/// Held by every test that writes a stub or forks a process, from its first stub write or fork until its last
/// program has ended. Thus no other test forks while a stub is open for writing (ETXTBSY).
static STUBS: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn forks() -> std::sync::MutexGuard<'static, ()> { STUBS.lock().unwrap_or_else(|e| e.into_inner()) }

fn run_child(case: &str) {
    let _forks = forks();
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_closed_stderr", "--nocapture", "--test-threads=1"])
        .env("ZYGO_MCP_CASE", case)
        .stderr(broken())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "case {case}: {}", String::from_utf8_lossy(&o.stdout));
}

#[test] fn closed_stderr_line_over_4_mib_keeps_serving() { run_child("long"); }
#[test] fn closed_stderr_not_json_keeps_serving() { run_child("notjson"); }
#[test] fn closed_stderr_tool_panic_keeps_serving() { run_child("panic"); }

// ---- workspace_down (W11; AU65, W§18 #78) ----

use std::path::PathBuf;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;
use zygo_core::session_view::{Member, ViewWorkspace};
use zygo_mcp::tools::workspace_down::{self, WorkspaceDown};
use zygo_workspace::state::Store;

const WS_ID: &str = "0123456789abcdef";

fn member_view() -> SessionView {
    SessionView { workspace: Some(ViewWorkspace { member: Member { ws_id: WS_ID.into(), pane: "kabcdefgh".into() }, resume_note: None }),
        ..view(true, true, true) }
}
/// A valid `workspace.json` for `WS_ID` with the name `app`.
fn workspace_state(p: &Paths) {
    Store::open(p, WS_ID)
        .update(Duration::from_secs(1), &mut |ws| { ws.name = "app".into(); ws.tmux_session = "zygo-app".into(); Ok(()) })
        .unwrap();
}
fn in_session(mut c: McpCtx, sd: &SessionDir) -> McpCtx { c.session_dir = Some(sd.clone()); c }
/// An executable `/bin/sh` script `zygo-stub` in `dir`; the caller holds `forks()`.
#[cfg(unix)]
fn stub(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("zygo-stub");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
/// Whether `pid` is gone within 2 s. A killed process whose parent has ended stays a zombie until init reaps it,
/// and `kill -0` succeeds on a zombie. Thus this polls. The caller holds `forks()`.
#[cfg(unix)]
fn gone(pid: &str) -> bool {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        let alive = std::process::Command::new("kill").args(["-0", pid])
            .stderr(std::process::Stdio::null()).status().unwrap().success();
        if !alive {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn workspace_down_listed_only_with_the_toggle_and_a_member_marker() {
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    member_view().save(&sd).unwrap();
    let has = |t: &[Box<dyn Tool>]| names(t).iter().any(|n| n == "workspace_down");
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let c = in_session(ctx(agent, |f| f.workspace_tool = true), &sd);
        assert_eq!(names(&workspace_down::tools(&c)), ["workspace_down"]);
        assert!(has(&tools::build_with(&c, |_| vec![], |_| vec![]).unwrap()));
    }
    let off = in_session(ctx(Agent::ClaudeCode, |_| {}), &sd);       // the default: off
    assert!(!off.features.workspace_tool);
    assert!(workspace_down::tools(&off).is_empty());
    assert!(!has(&tools::build_with(&off, |_| vec![], |_| vec![]).unwrap()));
    assert!(workspace_down::tools(&ctx(Agent::ClaudeCode, |f| f.workspace_tool = true)).is_empty());   // no session
    view(true, true, true).save(&sd).unwrap();                       // not a member
    let plain = in_session(ctx(Agent::ClaudeCode, |f| f.workspace_tool = true), &sd);
    assert!(workspace_down::tools(&plain).is_empty());
    assert!(!has(&tools::build_with(&plain, |_| vec![], |_| vec![]).unwrap()));
}

#[test]
fn workspace_tool_comes_from_the_config_not_the_view() {
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    member_view().save(&sd).unwrap();
    let c = resolve(&paths(d.path(), "[features]\nworkspace_tool = true\n"), Some(sd.clone())).0.unwrap();
    assert!(c.features.workspace_tool);
    assert_eq!(names(&workspace_down::tools(&c)), ["workspace_down"]);
    let c = resolve(&paths(d.path(), ""), Some(sd)).0.unwrap();
    assert!(!c.features.workspace_tool);
    assert!(workspace_down::tools(&c).is_empty());
}

#[test]
fn workspace_down_definition() {
    let d = tempfile::tempdir().unwrap();
    let t = WorkspaceDown::new(PathBuf::from("/nonexistent/zygo"), paths(d.path(), ""), WS_ID.into(), "s".into());
    let def = t.def();
    assert_eq!(def.name, "workspace_down");
    assert_eq!(def.description, workspace_down::DESCRIPTION);
    assert_eq!(def.input_schema, json!({"type":"object","properties":{"now":{"type":"boolean"}},"additionalProperties":false}));
}

#[cfg(unix)]
#[test]
fn workspace_down_runs_down_with_the_name_and_zygo_session() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), r#"dir=$(dirname "$0")
printf '%s\n' "$@" > "$dir/argv"
printf '%s' "$ZYGO_SESSION" > "$dir/session"
cat > "$dir/stdin"
echo 'zygo: down started; end your turn'"#);
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    let read = |f: &str| std::fs::read_to_string(d.path().join(f)).unwrap();
    assert_eq!(t.call(json!({})), Ok(json!("zygo: down started; end your turn")));
    assert_eq!(read("argv"), "down\napp\n");
    assert_eq!(read("session"), "s");
    assert_eq!(read("stdin"), "");                                   // stdin is /dev/null
    assert_eq!(t.call(json!({"now": true})), Ok(json!("zygo: down started; end your turn")));
    assert_eq!(read("argv"), "down\napp\n--now\n");
    assert_eq!(t.call(json!({"now": false})), Ok(json!("zygo: down started; end your turn")));
    assert_eq!(read("argv"), "down\napp\n");
}

#[cfg(unix)]
#[test]
fn workspace_down_non_zero_exit_is_a_tool_error_with_the_text() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), "echo 'zygo: workspace app is not up' >&2\nexit 1");
    let t = WorkspaceDown::new(prog.clone(), p.clone(), WS_ID.into(), "s".into());
    assert_eq!(t.call(json!({})), Err(ToolError::Failed("zygo: workspace app is not up".into())));
    // stdout without a final newline, then stderr: the two parts are joined with a newline (M5).
    let prog = stub(d.path(), "printf 'out' \necho 'err' >&2\nexit 1");
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    assert_eq!(t.call(json!({})), Err(ToolError::Failed("out\nerr".into())));
}

/// The stub does not `exec`: its `sleep` is a child of the shell. If only the shell were killed, the `sleep` would
/// still run, so the test checks both pids.
#[cfg(unix)]
#[test]
fn workspace_down_that_does_not_return_in_5_s_is_killed() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), r#"dir=$(dirname "$0")
echo $$ > "$dir/shell"
sleep 10 &
echo $! > "$dir/child"
wait"#);
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    let start = Instant::now();
    assert_eq!(t.call(json!({})), Err(ToolError::Failed("zygo: down did not return within 5 s".into())));
    let took = start.elapsed();
    assert!(took >= Duration::from_secs(5) && took < Duration::from_secs(6), "{took:?}");
    for f in ["shell", "child"] {
        let pid = std::fs::read_to_string(d.path().join(f)).unwrap().trim().to_string();
        assert!(gone(&pid), "{f} {pid} still runs");
    }
}

/// The EOF path: the stub exits 0 at once, but its background child keeps stdout open. The call waits for EOF
/// at most 5 s, then kills the group. `down` exited 0, so the call succeeds with the output read so far and one
/// line about the stopped process (round 2, F4).
#[cfg(unix)]
#[test]
fn workspace_down_whose_child_keeps_stdout_open_is_killed() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), r#"dir=$(dirname "$0")
sleep 10 &
echo $! > "$dir/child"
echo 'zygo: down started; end your turn'
exit 0"#);
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    let start = Instant::now();
    assert_eq!(t.call(json!({})), Ok(json!(format!("zygo: down started; end your turn\n{HELD_STOPPED}"))));
    let took = start.elapsed();
    assert!(took >= Duration::from_secs(5) && took < Duration::from_secs(6), "{took:?}");
    let pid = std::fs::read_to_string(d.path().join("child")).unwrap().trim().to_string();
    assert!(gone(&pid), "child {pid} still runs");
}

#[test]
fn workspace_down_unknown_key_is_invalid_params() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let t = WorkspaceDown::new(PathBuf::from("/nonexistent/zygo"), p, WS_ID.into(), "s".into());
    assert_eq!(t.call(json!({"wait": true})), Err(ToolError::InvalidParams("unknown key `wait`; the only key is `now`".into())));
    assert_eq!(t.call(json!({"now": "yes"})), Err(ToolError::InvalidParams("now must be true or false".into())));
    for bad in [json!({"now": true, "all": true}), json!({"now": 1}), json!({"now": null})] {
        assert!(matches!(t.call(bad.clone()), Err(ToolError::InvalidParams(_))), "{bad}");
    }
    let listed: Vec<Box<dyn Tool>> = vec![Box::new(t)];
    let mut out = Vec::new();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"workspace_down\",\"arguments\":{\"all\":true}}}\n";
    serve(Agent::ClaudeCode, &listed, req.as_bytes(), &mut out).unwrap();
    let r: Value = serde_json::from_slice(out.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32602);
}

#[cfg(unix)]
#[test]
fn workspace_down_without_workspace_json_names_the_id() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let prog = stub(d.path(), "touch \"$(dirname \"$0\")/ran\"");
    let t = WorkspaceDown::new(prog.clone(), paths(d.path(), ""), WS_ID.into(), "s".into());
    assert_eq!(t.call(json!({})), Err(ToolError::Failed(format!("zygo: workspace {WS_ID} has no state"))));
    assert!(!d.path().join("ran").exists());                         // nothing ran
    let bad = WorkspaceDown::new(prog, paths(d.path(), ""), "bad".into(), "s".into());
    match bad.call(json!({})) {
        Err(ToolError::Failed(m)) => assert!(m.starts_with("zygo: workspace bad: "), "{m}"),
        other => panic!("{other:?}"),
    }
}

// ---- workspace_down round 2 (W11 diff review) ----

/// The note line when a process of `down`'s group still held stdout or stderr at the limit and the group was killed.
#[cfg(unix)]
const HELD_STOPPED: &str = "zygo: a process of down still held its output after 5 s and was stopped";

/// A valid `workspace.json` for `WS_ID` with the name `name`.
#[cfg(unix)]
fn workspace_named(p: &Paths, name: &str) {
    Store::open(p, WS_ID)
        .update(Duration::from_secs(1), &mut |ws| { ws.name = name.into(); ws.tmux_session = format!("zygo-{name}"); Ok(()) })
        .unwrap();
}

/// F4, non-zero exit: `down` exits 3 and its child holds stdout. The call fails with the output read so far and the
/// note line, not with the timeout text, because the exit status is known.
#[cfg(unix)]
#[test]
fn workspace_down_non_zero_exit_with_a_held_pipe_reports_the_output() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), r#"dir=$(dirname "$0")
sleep 10 &
echo $! > "$dir/child"
echo 'zygo: workspace app is not up'
exit 3"#);
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    let start = Instant::now();
    assert_eq!(t.call(json!({})), Err(ToolError::Failed(format!("zygo: workspace app is not up\n{HELD_STOPPED}"))));
    let took = start.elapsed();
    assert!(took >= Duration::from_secs(5) && took < Duration::from_secs(6), "{took:?}");
    let pid = std::fs::read_to_string(d.path().join("child")).unwrap().trim().to_string();
    assert!(gone(&pid), "child {pid} still runs");
}

/// Child half of F3: runs only when the parent sets `ZYGO_MCP_SIGCHLD_DIR`. SIGCHLD is ignored here, so the kernel
/// reaps the stub itself and `waitid`/`try_wait` give ECHILD.
#[cfg(unix)]
#[test]
fn child_sigchld_ignored() {
    let Ok(dir) = std::env::var("ZYGO_MCP_SIGCHLD_DIR") else { return };
    // SAFETY: `signal` with `SIG_IGN` has no memory effect; this process runs this one test only.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
    let dir = PathBuf::from(dir);
    let t = WorkspaceDown::new(dir.join("zygo-stub"), paths(&dir, ""), WS_ID.into(), "s".into());
    assert_eq!(t.call(json!({})), Ok(json!("zygo: down started; end your turn")));
}

/// F3: with SIGCHLD ignored, the exit is seen as ECHILD. The call returns the output (an unknown status counts as
/// success) and sends no kill to the freed pid.
#[cfg(unix)]
#[test]
fn workspace_down_with_sigchld_ignored_returns_the_output() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    stub(d.path(), "echo 'zygo: down started; end your turn'");
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_sigchld_ignored", "--nocapture", "--test-threads=1"])
        .env("ZYGO_MCP_SIGCHLD_DIR", d.path())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// F5: `zygo down` has no `--` handling (crates/zygo/src/cmd has no `down` subcommand yet), so a name that starts
/// with `-` is refused before anything runs.
#[cfg(unix)]
#[test]
fn workspace_down_refuses_a_name_that_starts_with_a_dash() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_named(&p, "--all");
    let prog = stub(d.path(), "touch \"$(dirname \"$0\")/ran\"\necho ran");
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    assert_eq!(t.call(json!({})),
        Err(ToolError::Failed("zygo: workspace name `--all` starts with `-`; zygo down would read it as a flag".into())));
    assert!(!d.path().join("ran").exists());                         // nothing ran
}

/// F7: stdout and stderr are each cut at 64 KiB; the rest is drained, so the stub is not blocked.
#[cfg(unix)]
#[test]
fn workspace_down_caps_stdout_and_stderr_at_64_kib() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), r"head -c 100000 /dev/zero | tr '\0' a
head -c 100000 /dev/zero | tr '\0' b >&2");
    let t = WorkspaceDown::new(prog, p, WS_ID.into(), "s".into());
    let start = Instant::now();
    assert_eq!(t.call(json!({})), Ok(json!(format!("{}\n{}", "a".repeat(64 * 1024), "b".repeat(64 * 1024)))));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn workspace_down_with_a_corrupt_workspace_json_names_the_id() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    let dir = p.workspaces_dir().join(WS_ID);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("workspace.json"), b"{broken").unwrap();
    let t = WorkspaceDown::new(PathBuf::from("/nonexistent/zygo"), p, WS_ID.into(), "s".into());
    match t.call(json!({})) {
        Err(ToolError::Failed(m)) => {
            assert!(m.starts_with(&format!("zygo: workspace {WS_ID}: ")), "{m}");
            assert!(m.contains("workspace.json: "), "{m}");
            assert!(!m.contains("broken"), "{m}");                   // no file content in the message
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn workspace_down_with_a_workspace_json_of_another_id_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    let other = "fedcba9876543210";
    Store::open(&p, other)
        .update(Duration::from_secs(1), &mut |ws| { ws.name = "app".into(); ws.tmux_session = "zygo-app".into(); Ok(()) })
        .unwrap();
    let dir = p.workspaces_dir().join(WS_ID);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(p.workspaces_dir().join(other).join("workspace.json"), dir.join("workspace.json")).unwrap();
    let t = WorkspaceDown::new(PathBuf::from("/nonexistent/zygo"), p, WS_ID.into(), "s".into());
    match t.call(json!({})) {
        Err(ToolError::Failed(m)) => {
            assert!(m.starts_with(&format!("zygo: workspace {WS_ID}: ")), "{m}");
            assert!(m.ends_with(": invalid id"), "{m}");
        }
        other => panic!("{other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn workspace_down_tools_call_success_is_not_an_error() {
    let _forks = forks();
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path(), "");
    workspace_state(&p);
    let prog = stub(d.path(), r#"printf '%s ' "$@" > "$(dirname "$0")/argv"
echo 'zygo: down started; end your turn'"#);
    let listed: Vec<Box<dyn Tool>> = vec![Box::new(WorkspaceDown::new(prog, p, WS_ID.into(), "s".into()))];
    let mut out = Vec::new();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"workspace_down\",\"arguments\":{\"now\":true}}}\n";
    serve(Agent::ClaudeCode, &listed, req.as_bytes(), &mut out).unwrap();
    let r: Value = serde_json::from_slice(out.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["isError"], false);
    assert_eq!(r["result"]["content"], json!([{"type":"text","text":"zygo: down started; end your turn"}]));
    assert_eq!(std::fs::read_to_string(d.path().join("argv")).unwrap(), "down app --now ");
}
