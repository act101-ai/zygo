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

fn run_child(case: &str) {
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
