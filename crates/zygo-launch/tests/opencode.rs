//! N33: the OpenCode adapter (§8.3, §8.7, §15.3) and its hook payloads. No process-environment change.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use zygo_config::schema::CustomRole;
use zygo_config::{load_with_builtin, LoadContext};
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::hook_opencode::normalize;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_hooks::session_text::mechanics;
use zygo_launch::env::Env;
use zygo_launch::opencode::OpenCode;
use zygo_launch::{AgentAdapter, ChildSpec, LaunchCtx, Plugins};
use zygo_proxy::client::WindowArg;
use zygo_proxy::session::SessionConfig;
use zygo_work::hooks::HookCtx;

fn fixture(rel: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode").join(rel);
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn hook_input(v: &Value) -> HookInput {
    HookInput::parse(Agent::OpenCode, v.to_string().as_bytes()).unwrap()
}

fn session(dir: &Path) -> SessionDir {
    let sd = SessionDir::new(dir.join("s"));
    sd.create().unwrap();
    view().save(&sd).unwrap();
    sd
}

fn view() -> SessionView {
    SessionView {
        session: "S1".into(),
        agent: Agent::OpenCode,
        main_role: "lead".into(),
        effective_role: Some("lead".into()),
        role_prompt: None,
        user_prompt: None,
        profile: None,
        profile_prompt: None,
        work_item: None,
        legend: vec![],
        disabled_roles: vec![],
        work_units: false,
        results: true,
        compact_tool: true,
        navigation: false,
        trim_threshold_bytes: 16384,
        head_lines: 60,
        tail_lines: 20,
    }
}

// ---- Task 1: normalize and the hooks on the fixtures ----

fn before(tool: &str, args: Value) -> Value {
    json!({"input": {"tool": tool, "sessionID": "s", "callID": "c"}, "output": {"args": args}, "cwd": "/r"})
}

#[test]
fn normalize_before_bash_fixture() {
    let got = normalize(fixture("plugin-hooks/tool.execute.before-bash.json"));
    let want = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"echo tracer-oc-bash-original","description":"echo"}});
    assert_eq!(got, want);
}

#[test]
fn normalize_after_bash_fixture() {
    let got = normalize(fixture("plugin-hooks/tool.execute.after-bash.json"));
    let want = json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"echo tracer-oc-bash-original","description":"echo"},"tool_response":{"stdout":"tracer-oc-bash-original\n"}});
    assert_eq!(got, want);
}

#[test]
fn normalize_maps_tool_and_argument_names() {
    for (tool, want) in [("bash", "Bash"), ("read", "Read"), ("edit", "Edit"), ("write", "Write"), ("grep", "Grep"), ("glob", "Glob"), ("webfetch", "webfetch"), ("zygo_compact", "zygo_compact")] {
        assert_eq!(normalize(before(tool, json!({})))["tool_name"], want, "{tool}");
    }
    let n = normalize(before("edit", json!({"filePath":"/r/a.rs","oldString":"a","newString":"b"})));
    assert_eq!(n["tool_input"], json!({"file_path":"/r/a.rs","oldString":"a","new_string":"b"}));
    let n = normalize(before("read", json!({"filePath":"/r/a.rs","limit":5})));
    assert_eq!(n["tool_input"], json!({"file_path":"/r/a.rs","limit":5}));
    let n = normalize(before("write", json!({"filePath":"/r/a.rs","content":"c"})));
    assert_eq!(n["tool_input"], json!({"file_path":"/r/a.rs","content":"c"}));
    assert_eq!(n["cwd"], "/r");
    assert_eq!(n["hook_event_name"], "PreToolUse");
}

#[test]
fn normalize_after_other_tool_has_no_response() {
    let v = json!({"input": {"tool": "read", "args": {"filePath": "/r/a.rs"}}, "output": {"title": "t", "output": "text"}});
    let n = normalize(v);
    assert_eq!(n["tool_name"], "Read");
    assert_eq!(n["hook_event_name"], "PostToolUse");
    assert_eq!(n["tool_input"], json!({"file_path": "/r/a.rs"}));
    assert!(n.get("tool_response").is_none());
}

/// The `after` bash fixture whose output is large.
fn big_after() -> Value {
    let mut p = fixture("plugin-hooks/tool.execute.after-bash.json");
    p["output"]["output"] = json!("x\n".repeat(20_000));
    p
}

fn marker_text() -> String {
    format!("[REDACTED:{}]", "k1")
}

fn guard_ctx<'a>(sd: &'a SessionDir, db: PathBuf, config: &'a dyn Fn(&Path) -> Option<zygo_config::Merged>) -> HookCtx<'a> {
    fn no_item(_: &Path, _: &zygo_config::Merged) -> Option<zygo_work::WorkItem> {
        None
    }
    fn quiet(_: &str) {}
    HookCtx { session_dir: sd, session: "S1".into(), usage_db: db, now: 1000, config, item: &no_item, warn: &quiet, temp_roots: vec![] }
}

fn no_config(_: &Path) -> Option<zygo_config::Merged> {
    None
}

#[test]
fn hooks_on_fixtures_give_the_specified_output() {
    let t = tempfile::tempdir().unwrap();
    let sd = session(t.path());
    let db = t.path().join("usage.db");
    let out = zygo_hooks::trim::trim_in(&hook_input(&big_after()), &sd, &db).unwrap();
    let HookOutput::Json(v) = out else { panic!("trim gave no JSON") };
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let stdout = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"].as_str().unwrap();
    assert!(stdout.contains("[zygo trimmed this output: "), "{stdout}");
    assert_eq!(std::fs::read_dir(sd.results()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".json")).count(), 1);

    let ctx = guard_ctx(&sd, db, &no_config);
    let want = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file."}});
    for (tool, args) in [("write", json!({"filePath":"/r/a.rs","content":marker_text()})), ("edit", json!({"filePath":"/r/a.rs","newString":marker_text()}))] {
        let p = before(tool, args);
        let HookOutput::Json(got) = zygo_work::guards::run_with(&hook_input(&p), &ctx).unwrap() else { panic!("{tool}: no JSON") };
        assert_eq!(got, want, "{tool}");
    }
}

#[test]
fn hooks_never_break_opencode() {
    let t = tempfile::tempdir().unwrap();
    let sd = session(t.path());
    let db = t.path().join("usage.db");
    let ctx = guard_ctx(&sd, db.clone(), &no_config);
    type Hook<'a> = Box<dyn Fn(&[u8]) -> anyhow::Result<HookOutput> + 'a>;
    let trim: Hook = Box::new(|b| zygo_hooks::trim::trim_in(&HookInput::parse(Agent::OpenCode, b)?, &sd, &db));
    let guard: Hook = Box::new(|b| zygo_work::guards::run_with(&HookInput::parse(Agent::OpenCode, b)?, &ctx));
    for h in [&trim, &guard] {
        for bad in [&b"not json"[..], b""] {
            assert_eq!(zygo_core::hook::guarded(h, bad), (String::new(), 0));
        }
    }
    let plain = big_after();
    let mut extra = plain.clone();
    extra["zz"] = json!(1);
    extra["input"]["future"] = json!([1]);
    extra["output"]["future"] = json!([1]);
    let (a, _) = zygo_core::hook::guarded(&trim, plain.to_string().as_bytes());
    let (b, _) = zygo_core::hook::guarded(&trim, extra.to_string().as_bytes());
    assert!(!a.is_empty());
    assert_eq!(a.len(), b.len());
}

// ---- Task 2: the adapter and the inline config ----

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let h = root.join("home");
    let paths = Paths {
        config_file: h.join("config.toml"),
        cache_dir: h.join("cache"),
        data_dir: h.join("data"),
        sessions_dir: h.join("cache/sessions"),
        marketplaces_dir: h.join("cache/marketplaces"),
        plugin_store: h.join("data/plugins"),
        approvals_file: h.join("data/approvals.toml"),
        usage_db: h.join("data/usage.db"),
    };
    std::fs::create_dir_all(&h).unwrap();
    Fx { _t: t, root, paths }
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

fn ctx(fx: &Fx, f: impl FnOnce(&mut LaunchCtx)) -> LaunchCtx {
    let cfg = Arc::new(load_with_builtin(&LoadContext { paths: fx.paths.clone(), repo: None, team: None }, builtin()).unwrap());
    let sd = SessionDir::new(fx.paths.sessions_dir.join("S1"));
    sd.create().unwrap();
    let v = view();
    v.save(&sd).unwrap();
    let config = SessionConfig {
        main_role: cfg.config.main_role.clone(),
        roles: cfg.config.role.clone(),
        custom_roles: BTreeMap::new(),
        models: vec![],
        guards: cfg.config.guards.clone(),
    };
    let mut c = LaunchCtx {
        agent: Agent::OpenCode,
        program: PathBuf::from("opencode"),
        agent_version: "1.18.16".into(),
        cfg,
        profile: None,
        session: "S1".into(),
        token: format!("tok-{}", 42),
        session_dir: sd,
        proxy_url: "http://127.0.0.1:4/".parse().unwrap(),
        config,
        view: v,
        effective_role: Some("lead".into()),
        repo: None,
        work: None,
        plugins: Plugins::Delivered(None),
        args: vec![],
        passthrough: false,
        yolo: false,
        compact: None,
        cwd: fx.root.join("work"),
        repo_root: None,
        env: Env(vec![("HOME".into(), fx.root.join("home").into_os_string())]),
        paths: fx.paths.clone(),
    };
    f(&mut c);
    c
}

fn oc() -> OpenCode {
    OpenCode { zygo: PathBuf::from("/opt/zy go/zygo") }
}

fn config_of(spec: &ChildSpec) -> Value {
    let (_, text) = spec.env.iter().find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT").expect("OPENCODE_CONFIG_CONTENT");
    serde_json::from_str(text).unwrap()
}

fn built(ctx: &LaunchCtx) -> (ChildSpec, Value) {
    let spec = oc().build(ctx).unwrap();
    let cfg = config_of(&spec);
    (spec, cfg)
}

struct StubModel;

impl jsonschema::Retrieve for StubModel {
    fn retrieve(&self, _uri: &jsonschema::Uri<String>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Ok(json!({"$defs": {"Model": {"type": "string"}}}))
    }
}

fn validator() -> jsonschema::Validator {
    let s = fixture("config-schema.json");
    jsonschema::options().with_retriever(StubModel).build(&s["schema"]).unwrap()
}

fn assert_valid(cfg: &Value) {
    let v = validator();
    let errs: Vec<String> = v.iter_errors(cfg).map(|e| format!("{e} at {}", e.instance_path())).collect();
    assert!(errs.is_empty(), "{errs:?}");
}

fn lead_model(c: &mut LaunchCtx) -> &mut zygo_config::schema::ModelRow {
    let id = c.config.roles["lead"].model.clone();
    Arc::make_mut(&mut c.cfg).config.catalog.0.iter_mut().find(|m| m.id == id).unwrap()
}

fn picked(args: &[&str]) -> Option<String> {
    oc().picked_model(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn picked_model_table() {
    assert_eq!(picked(&[]), None);
    for a in [&["-m", "zygo/code"][..], &["--model", "zygo/code"], &["--model=zygo/code"]] {
        assert_eq!(picked(a).as_deref(), Some("code"), "{a:?}");
    }
    assert_eq!(picked(&["-m", "anthropic/claude-x"]).as_deref(), Some("anthropic/claude-x"));
    for (agent, role) in [("build", "lead"), ("plan", "plan"), ("general", "code"), ("explore", "read"), ("zygo-review", "review"), ("mine", "mine")] {
        assert_eq!(picked(&["--agent", agent]).as_deref(), Some(role), "{agent}");
    }
    assert_eq!(picked(&["--agent=zygo-review"]).as_deref(), Some("review"));
    assert_eq!(picked(&["-m", "zygo/code", "--agent", "plan"]).as_deref(), Some("code"));
    assert_eq!(picked(&["run", "--", "-m", "zygo/code"]), None);
}

#[test]
fn inline_config_values() {
    let fx = fx();
    let c = ctx(&fx, |_| {});
    let (_, cfg) = built(&c);
    assert_eq!(cfg["model"], "zygo/lead");
    assert_eq!(cfg["$schema"], "https://opencode.ai/config.json");
    let p = &cfg["provider"]["zygo"];
    assert_eq!(p["npm"], "@ai-sdk/anthropic");
    assert_eq!(p["options"]["baseURL"], "http://127.0.0.1:4/anthropic/v1");
    assert_eq!(p["options"]["apiKey"], c.token.as_str());
    assert_eq!(p["options"]["headers"], json!({"X-Zygo-Session": "S1", "X-Zygo-Agent-Version": "1.18.16"}));
    let roles: BTreeSet<&str> = c.config.roles.keys().map(String::as_str).collect();
    let keys: BTreeSet<&str> = p["models"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, roles);
    assert_eq!(cfg["mcp"]["zygo"], json!({"type": "local", "command": ["/opt/zy go/zygo", "mcp", "--agent", "opencode"]}));

    let c = ctx(&fx, |c| c.agent_version = String::new());
    assert!(built(&c).1["provider"]["zygo"]["options"]["headers"].get("X-Zygo-Agent-Version").is_none());

    let c = ctx(&fx, |c| {
        c.repo = Some("example.invalid/a/b".into());
        c.work = Some(zygo_work::WorkItem { source: "src".into(), id: "7".into(), title: None, url: None, status: None, check_hint: None, files_hint: vec![], lookup_failed: None });
    });
    let h = built(&c).1["provider"]["zygo"]["options"]["headers"].clone();
    assert_eq!(h["X-Zygo-Repo"], "example.invalid/a/b");
    assert_eq!(h["X-Zygo-Work"], "src:7");
}

#[test]
fn small_model_follows_read() {
    let fx = fx();
    assert_eq!(built(&ctx(&fx, |_| {})).1["small_model"], "zygo/read");
    let c = ctx(&fx, |c| {
        c.config.roles.remove("read");
    });
    assert_eq!(built(&c).1["small_model"], "zygo/lead");
}

#[test]
fn model_limits() {
    let fx = fx();
    let limit = |c: &LaunchCtx| built(c).1["provider"]["zygo"]["models"]["lead"]["limit"].clone();
    let c = ctx(&fx, |c| {
        let m = lead_model(c);
        m.soft_context_window = Some(600_000);
        let f = m.facts.as_mut().unwrap();
        f.context_window = 1_000_000;
        f.max_output_tokens = 64_000;
    });
    assert_eq!(limit(&c), json!({"context": 600000, "output": 64000}));
    let c = ctx(&fx, |c| {
        let m = lead_model(c);
        m.soft_context_window = None;
        let f = m.facts.as_mut().unwrap();
        f.context_window = 1_000_000;
        f.max_output_tokens = 64_000;
    });
    assert_eq!(limit(&c), json!({"context": 1000000, "output": 64000}));
    let c = ctx(&fx, |c| {
        lead_model(c).facts.as_mut().unwrap().max_output_tokens = 0;
    });
    assert_eq!(limit(&c)["output"], 0);
    assert_valid(&built(&c).1);
    let c = ctx(&fx, |c| c.config.roles.get_mut("lead").unwrap().model = "no-such-model".into());
    assert_eq!(limit(&c), json!({"context": 200000, "output": 0}));
}

#[test]
fn compact_flag_sets_the_window() {
    let fx = fx();
    let limit = |w| built(&ctx(&fx, |c| {
        let m = lead_model(c);
        m.soft_context_window = Some(600_000);
        m.facts.as_mut().unwrap().context_window = 1_000_000;
        c.compact = Some(w);
    })).1["provider"]["zygo"]["models"]["lead"]["limit"]["context"].clone();
    assert_eq!(limit(WindowArg::Tokens(500_000)), 500000);
    assert_eq!(limit(WindowArg::Tokens(5_000_000)), 1000000);
    assert_eq!(limit(WindowArg::Off), 1000000);
}

#[test]
fn agents_map_to_roles() {
    let fx = fx();
    let ro = CustomRole { model: "claude-sonnet-5-5".into(), description: "read only".into(), prompt: "P".into(), writes: false };
    let rw = CustomRole { model: "claude-sonnet-5-5".into(), description: "writer".into(), prompt: "Q".into(), writes: true };
    let c = ctx(&fx, |c| {
        c.config.custom_roles.insert("ro".into(), ro.clone());
        c.config.custom_roles.insert("rw".into(), rw.clone());
    });
    let (_, cfg) = built(&c);
    let a = &cfg["agent"];
    for (name, role) in [("build", "lead"), ("plan", "plan"), ("general", "code"), ("explore", "read")] {
        assert_eq!(a[name]["model"], format!("zygo/{role}"), "{name}");
    }
    let mech = mechanics(&c.view);
    for (r, role) in &c.config.roles {
        let key = format!("zygo-{r}");
        if *r == c.config.main_role {
            assert!(a.get(&key).is_none(), "{key}");
            continue;
        }
        let mut want = json!({"mode": "subagent", "description": role.description, "prompt": format!("{} {mech}", role.prompt), "model": format!("zygo/{r}")});
        if !role.writes {
            want["permission"] = json!({"edit": "deny"});
        }
        assert_eq!(a[&key], want, "{key}");
    }
    assert_eq!(a["zygo-ro"]["permission"]["edit"], "deny");
    assert!(a["zygo-rw"].get("permission").is_none());
    let c = ctx(&fx, |c| {
        c.config.roles.remove("review");
    });
    let (_, cfg) = built(&c);
    assert!(cfg["agent"].get("zygo-review").is_none());
    assert!(cfg["provider"]["zygo"]["models"].get("review").is_none());
    let c = ctx(&fx, |c| {
        c.config.roles.remove("read");
    });
    assert!(built(&c).1["agent"].get("explore").is_none());
}

#[test]
fn lead_guard_and_default_agent_follow_main_role() {
    let fx = fx();
    let with = |main: &str, eff: Option<&str>| {
        let c = ctx(&fx, |c| {
            c.config.main_role = main.into();
            c.view.main_role = main.into();
            c.effective_role = eff.map(String::from);
        });
        built(&c)
    };
    let denied = |cfg: &Value| -> Vec<String> {
        ["build", "plan", "general", "explore"].into_iter().filter(|n| cfg["agent"][*n].get("permission").is_some()).map(String::from).collect()
    };
    let (_, cfg) = with("lead", Some("lead"));
    assert_eq!(cfg["default_agent"], "build");
    assert_eq!(cfg["agent"]["build"]["permission"], json!({"edit": "deny"}));
    assert_eq!(denied(&cfg), ["build"]);
    let (_, cfg) = with("plan", Some("plan"));
    assert_eq!(cfg["default_agent"], "plan");
    assert_eq!(cfg["agent"]["plan"]["permission"], json!({"edit": "deny"}));
    assert!(cfg["agent"]["build"].get("permission").is_none());
    let (spec, cfg) = with("code", Some("code"));
    assert!(cfg.get("default_agent").is_none());
    assert_eq!(cfg["agent"]["general"]["permission"], json!({"edit": "deny"}));
    assert!(spec.log.contains(&"lead guard: no primary OpenCode agent maps to main_role code".to_string()), "{:?}", spec.log);
    let (spec, cfg) = with("design", Some("design"));
    assert!(cfg.get("default_agent").is_none());
    assert!(denied(&cfg).is_empty());
    assert!(spec.log.contains(&"lead guard: no primary OpenCode agent maps to main_role design".to_string()), "{:?}", spec.log);
    for eff in [Some("code"), None] {
        let (_, cfg) = with("lead", eff);
        assert_eq!(cfg["default_agent"], "build");
        assert!(denied(&cfg).is_empty(), "{eff:?}");
    }
}

#[test]
fn a_server_main_role_cannot_forge_a_session_log_line() { // N70: opencode.rs:443
    let fx = fx();
    let c = ctx(&fx, |c| {
        c.config.main_role = "x\ny\u{1b}[31m".into();
        c.view.main_role = c.config.main_role.clone();
        c.effective_role = None;
    });
    let (spec, _) = built(&c);
    assert!(spec.log.contains(&"lead guard: no primary OpenCode agent maps to main_role x\\ny\\u{1b}[31m".to_string()), "{:?}", spec.log);
} // mutation: `esc` removed at :443

#[test]
fn env_rows_and_args() {
    let fx = fx();
    let c = ctx(&fx, |c| {
        c.yolo = true;
        c.args = vec!["run".into()];
    });
    let (spec, cfg) = built(&c);
    let get = |k: &str| spec.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("OPENCODE_CONFIG_DIR").unwrap(), c.session_dir.root.join("opencode").to_string_lossy());
    assert_eq!(spec.args, ["run", "--auto"]);
    assert_eq!(spec.lines, ["plugins: partial"]);
    assert_eq!(cfg["model"], "zygo/lead");
    let c = ctx(&fx, |c| c.args = vec!["run".into(), "x".into()]);
    assert_eq!(oc().build(&c).unwrap().args, c.args);
}

fn yolo_args(args: &[&str]) -> ChildSpec {
    let fx = fx();
    let c = ctx(&fx, |c| {
        c.yolo = true;
        c.args = args.iter().map(|s| s.to_string()).collect();
    });
    oc().build(&c).unwrap()
}

fn skipped(spec: &ChildSpec, why: &str) {
    assert!(spec.log.contains(&format!("degraded: --yolo ({why})")), "{:?}", spec.log);
    assert!(spec.lines.iter().any(|l| l.starts_with("zygo: --yolo has no effect for opencode")), "{:?}", spec.lines);
}

#[test]
fn yolo_after_run() {
    assert_eq!(yolo_args(&["run", "x"]).args, ["run", "--auto", "x"]);
}

#[test]
fn yolo_tui_no_args() {
    let spec = yolo_args(&[]);
    assert_eq!(spec.args, ["--auto"]);
    assert!(!spec.log.iter().any(|l| l.contains("--yolo")), "{:?}", spec.log);
}

#[test]
fn yolo_user_flags_before_run() {
    // Position only: OpenCode 1.18.16 rejects any flag before a subcommand, so this layout does not run there.
    assert_eq!(yolo_args(&["-m", "zygo/lead", "run", "hi"]).args, ["-m", "zygo/lead", "run", "--auto", "hi"]);
}

#[test]
fn yolo_tui_with_flags_or_project() {
    assert_eq!(yolo_args(&["-c", "proj"]).args, ["--auto", "-c", "proj"]);
}

#[test]
fn yolo_other_subcommand_skipped() {
    for name in ["serve", "attach", "models", "plug"] {
        let spec = yolo_args(&[name, "x"]);
        assert_eq!(spec.args, [name, "x"]);
        skipped(&spec, &format!("opencode {name} has no --auto"));
        assert!(spec.lines.contains(&format!("zygo: --yolo has no effect for opencode {name}")), "{:?}", spec.lines);
    }
}

#[test]
fn yolo_first_subcommand_wins() {
    assert_eq!(yolo_args(&["run", "serve"]).args, ["run", "--auto", "serve"]);
}

#[test]
fn yolo_scan_stops_at_double_dash() {
    assert_eq!(yolo_args(&["run", "--", "serve"]).args, ["run", "--auto", "--", "serve"]);
    let spec = yolo_args(&["--", "run", "x"]);
    assert_eq!(spec.args, ["--", "run", "x"]);
    skipped(&spec, "after --");
}

#[test]
fn build_fails_without_a_zygo_path() {
    let fx = fx();
    assert!(OpenCode { zygo: PathBuf::new() }.build(&ctx(&fx, |_| {})).is_err());
}

#[cfg(unix)]
#[test]
fn non_utf8_session_dir_is_refused_not_rewritten() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let fx = fx();
    let mut c = ctx(&fx, |_| {});
    let root = fx.root.join(OsStr::from_bytes(b"s\xff"));
    c.session_dir = SessionDir::new(root.clone());
    let e = OpenCode { zygo: PathBuf::from("/opt/zygo") }.build(&c).err().unwrap();
    assert_eq!(e.to_string(), "cannot use the session directory path: it is not valid UTF-8");
    assert!(!root.exists(), "build wrote nothing before the refusal");
}

fn has(v: &Value, path: &str) -> bool {
    let Some((head, rest)) = path.split_once('.').map(|(h, r)| (h, Some(r))).or(Some((path, None))) else { return false };
    let next = |c: &Value| rest.is_none_or(|r| has(c, r));
    match v.as_object() {
        Some(o) if head == "<k>" => o.values().any(next),
        Some(o) => o.get(head).is_some_and(next),
        None => false,
    }
}

#[test]
fn inline_config_matches_the_fixture_shape() {
    let fx = fx();
    let (_, cfg) = built(&ctx(&fx, |_| {}));
    let fix = fixture("inline-config.json");
    for p in [
        "$schema", "model", "provider.zygo.npm", "provider.zygo.options.baseURL", "provider.zygo.options.apiKey", "provider.zygo.options.headers",
        "provider.zygo.models.<k>.limit.context", "provider.zygo.models.<k>.limit.output", "agent.build.model", "agent.build.permission.edit",
        "agent.<k>.mode", "agent.<k>.description", "agent.<k>.prompt", "agent.<k>.model", "agent.<k>.permission.edit", "mcp.zygo.type", "mcp.zygo.command",
    ] {
        assert!(has(&fix, p), "fixture lacks {p}");
        assert!(has(&cfg, p), "config lacks {p}");
    }
}

#[test]
fn inline_config_validates_against_the_pinned_schema() {
    let fx = fx();
    assert_valid(&built(&ctx(&fx, |_| {})).1);
    let c = ctx(&fx, |c| {
        c.config.custom_roles.insert("ro".into(), CustomRole { model: "claude-sonnet-5-5".into(), description: "d".into(), prompt: "p".into(), writes: false });
    });
    assert_valid(&built(&c).1);
    let c = ctx(&fx, |c| lead_model(c).facts.as_mut().unwrap().max_output_tokens = 0);
    assert_valid(&built(&c).1);
}

// ---- Task 3: the plugin file ----

const TEMPLATE: &str = include_str!("../assets/opencode-plugin.js");

fn plugin_js(c: &LaunchCtx, zygo: &Path) -> String {
    OpenCode { zygo: zygo.to_path_buf() }.build(c).unwrap();
    std::fs::read_to_string(c.session_dir.root.join("opencode/plugins/zygo.js")).unwrap()
}

#[test]
fn plugin_text_matches_the_fixtures() {
    assert_eq!(TEMPLATE.matches("__ZYGO_PATH__").count(), 1);
    assert!(TEMPLATE.contains("export const ZygoPlugin = async"));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode/plugin-hooks");
    let mut hooks = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        let stem = name.strip_suffix(".json").unwrap();
        if stem == "plugin-init" {
            continue;
        }
        let hook = stem.strip_suffix("-bash").unwrap_or(stem);
        assert!(TEMPLATE.contains(&format!("\"{hook}\"")), "{hook}");
        hooks += 1;
    }
    assert_eq!(hooks, 4);
    for t in [
        "directory", "\"--agent\", \"opencode\"", "10000", "permissionDecision", "updatedToolOutput", "output.output =", "output.system.push",
        "output.context.push", "ZYGO_MAIN_ROLE", "input.model", "stdin.on(\"error\"", "\"guard\"", "\"trim\"", "\"work\"", "\"session-start\"", "\"pre-compact\"",
    ] {
        assert!(TEMPLATE.contains(t), "{t}");
    }
}

#[test]
fn generated_plugin_embeds_the_json_escaped_path() {
    let fx = fx();
    let path = r#"/tmp/a b/"q"\z"#;
    let js = plugin_js(&ctx(&fx, |_| {}), Path::new(path));
    assert!(js.contains(&format!("const ZYGO = {};", serde_json::to_string(path).unwrap())));
    assert!(!js.contains("__ZYGO_PATH__"));
    assert!(!js.contains("spawn(\""));
}

/// What one run of the driver under node saw.
#[cfg(unix)]
struct NodeRun {
    result: Option<Value>,
    calls: String,
    dir: PathBuf,
}

#[cfg(unix)]
fn node_run(fx: &Fx, name: &str, mode: &str, role: &str, big: bool) -> NodeRun {
    use std::os::unix::fs::PermissionsExt;
    let dir = fx.root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("zygo-stub");
    let script = r#"#!/bin/sh
d=$(dirname "$0")
echo "$*" >> "$d/calls.txt"
if [ "$STUB_MODE" = noread ]; then exit 0; fi
cat > "$d/stdin-$2.json"
if [ "$STUB_MODE" = fail ]; then exit 1; fi
case "$2:$STUB_MODE" in
  guard:deny) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"R1"}}' ;;
  trim:ok) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"stdout":"VIEW"}}}' ;;
  trim:utf8) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"stdout":"'; cat "$d/stdout.txt"; printf '%s' '"}}}' ;;
  session-start:ok) printf TEXT ;;
  pre-compact:ok) printf KEEP ;;
esac
"#;
    std::fs::write(&stub, script).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let zygo = if mode == "missing" { dir.join("no-such-zygo") } else { stub };
    std::fs::write(dir.join("plugin.mjs"), plugin_js(&ctx(fx, |_| {}), &zygo)).unwrap();
    std::fs::write(
        dir.join("driver.mjs"),
        r#"import { ZygoPlugin } from "./plugin.mjs";
const h = await ZygoPlugin({ directory: "/r" });
const res = {};
try { await h["tool.execute.before"]({ tool: "bash" }, { args: { command: "x" } }); res.threw = null; } catch (e) { res.threw = e.message; }
const after = { output: process.env.BIG ? "y".repeat(1 << 20) : "orig" };
await h["tool.execute.after"]({ tool: "bash", args: {} }, after);
res.after = process.env.BIG ? after.output.length : after.output;
res.system = {};
for (const [id, p] of [["lead", "zygo"], ["code", "zygo"], ["read", "zygo"], ["lead", "other"]]) {
  const out = { system: [] };
  await h["experimental.chat.system.transform"]({ model: { id, providerID: p } }, out);
  res.system[id + "/" + p] = out.system;
}
const c = { context: [] };
await h["experimental.session.compacting"]({}, c);
res.compact = c.context;
console.log(JSON.stringify(res));
"#,
    )
    .unwrap();
    let mut cmd = std::process::Command::new("node");
    cmd.arg("driver.mjs").current_dir(&dir).env("ZYGO_MAIN_ROLE", role).env("STUB_MODE", mode);
    if big {
        cmd.env("BIG", "1");
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    NodeRun {
        result: serde_json::from_slice(&out.stdout).ok(),
        calls: std::fs::read_to_string(dir.join("calls.txt")).unwrap_or_default(),
        dir,
    }
}

#[cfg(unix)]
#[test]
fn plugin_behaves_under_node() {
    if !std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("skipped plugin_behaves_under_node: node is not on PATH");
        return;
    }
    let fx = fx();
    let sys = |r: &Value, k: &str| r["system"][k].clone();

    let r = node_run(&fx, "deny", "deny", "lead", false);
    assert_eq!(r.result.unwrap()["threw"], "R1");

    let r = node_run(&fx, "ok", "ok", "lead", false);
    let v = r.result.clone().unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert_eq!(v["after"], "VIEW");
    assert_eq!(sys(&v, "lead/zygo"), json!(["TEXT"]));
    for k in ["code/zygo", "read/zygo", "lead/other"] {
        assert_eq!(sys(&v, k), json!([]), "{k}");
    }
    assert_eq!(v["compact"], json!(["KEEP"]));
    let calls: Vec<&str> = r.calls.lines().collect();
    assert_eq!(&calls[..3], ["hook guard --agent opencode", "hook trim --agent opencode", "hook work --agent opencode"]);
    assert!(calls.contains(&"hook session-start --agent opencode"));
    assert!(calls.contains(&"hook pre-compact --agent opencode"));
    let stdin: Value = serde_json::from_slice(&std::fs::read(r.dir.join("stdin-guard.json")).unwrap()).unwrap();
    assert_eq!(stdin["cwd"], "/r");
    assert_eq!(stdin["input"]["tool"], "bash");
    assert_eq!(stdin["output"]["args"]["command"], "x");

    let r = node_run(&fx, "norole", "ok", "", false);
    let v = r.result.unwrap();
    for k in ["lead/zygo", "code/zygo", "read/zygo", "lead/other"] {
        assert_eq!(sys(&v, k), json!([]), "{k}");
    }
    assert!(!r.calls.contains("session-start"));

    for (name, mode) in [("fail", "fail"), ("missing", "missing")] {
        let v = node_run(&fx, name, mode, "lead", false).result.unwrap();
        assert_eq!(v["threw"], Value::Null, "{mode}");
        assert_eq!(v["after"], "orig", "{mode}");
        assert_eq!(v["compact"], json!([]), "{mode}");
        for k in ["lead/zygo", "code/zygo", "read/zygo", "lead/other"] {
            assert_eq!(sys(&v, k), json!([]), "{mode} {k}");
        }
    }

    let v = node_run(&fx, "noread", "noread", "lead", true).result.unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert_eq!(v["after"], 1 << 20);

    // A multi-byte trim view over 128 KiB reaches node in many pipe chunks; no character may split.
    let euro = "\u{20ac}".repeat(50_000);
    std::fs::create_dir_all(fx.root.join("utf8")).unwrap();
    std::fs::write(fx.root.join("utf8/stdout.txt"), &euro).unwrap();
    let v = node_run(&fx, "utf8", "utf8", "lead", false).result.unwrap();
    let after = v["after"].as_str().unwrap();
    assert!(!after.contains('\u{fffd}'), "U+FFFD in the trim view");
    assert_eq!(after, euro);
}

// ---- Task 4: delivery ----

use std::time::{Duration, Instant};
use zygo_plugins::select::PluginId;
use zygo_plugins::snapshot::{Delivered, DeliveredPlugin};
use zygo_plugins::store;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn put_plugin(store: &Path, mk: &str, name: &str, files: &[(&str, &str)], exec: &[&str]) -> DeliveredPlugin {
    let t = tempfile::tempdir().unwrap();
    let manifest = format!(r#"{{"name":"{name}","version":"1.0.0","homepage":"https://example.invalid/{name}"}}"#);
    let mut all: Vec<(&str, String)> = files.iter().map(|(r, c)| (*r, c.to_string())).collect();
    if !files.iter().any(|(r, _)| *r == ".claude-plugin/plugin.json") { all.push((".claude-plugin/plugin.json", manifest)); }
    for (rel, text) in &all {
        let p = rel.split('/').fold(t.path().to_path_buf(), |p, s| p.join(s));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let copied: Vec<String> = all.iter().map(|(r, _)| r.to_string()).collect();
    let modes = exec.iter().map(|r| (r.to_string(), "100755".to_string())).collect();
    let staged = store::stage(store, mk, t.path(), &copied, &modes, deadline()).unwrap();
    store::swap_in(store, &staged).unwrap();
    DeliveredPlugin { id: PluginId { marketplace: mk.into(), plugin: name.into() }, dir: store::entry_dir(store, &staged.entry), entry: staged.entry }
}

fn delivered(store: &Path, plugins: Vec<DeliveredPlugin>, lines: &[&str], details: &[&str]) -> Delivered {
    Delivered {
        plugins,
        lines: lines.iter().map(|s| s.to_string()).collect(),
        details: details.iter().map(|s| s.to_string()).collect(),
        lock: store::lock_until(store, false, deadline()).unwrap().unwrap(),
    }
}

const MCP: &str = r#"{"mcpServers":{"far":{"command":"${CLAUDE_PLUGIN_ROOT}/other/tool"},"srv":{"command":"${CLAUDE_PLUGIN_ROOT}/bin/srv","args":["--root","${CLAUDE_PLUGIN_ROOT}"],"env":{"K":"${CLAUDE_PLUGIN_ROOT}/d"}},"gone":{"command":"zygo-test-missing-cmd"}}}"#;

fn acme(fx: &Fx) -> DeliveredPlugin {
    put_plugin(
        &fx.paths.plugin_store,
        "mk",
        "acme",
        &[
            ("skills/review/SKILL.md", "---\nname: review\ndescription: d\n---\nbody\n"),
            ("skills/review/ref.md", "ref"),
            ("agents/helper.md", "---\ndescription: \"say \\\"hi\\\"\"\n---\nYou help.\n"),
            ("agents/crlf.md", "---\r\ndescription: Crlf one\r\n---\r\nBody\r\n"),
            ("commands/ship.md", "---\ndescription: Ship it\n---\nShip $ARGUMENTS\n"),
            ("opencode/plugin.js", "export default {}\n"),
            ("bin/srv", "#!/bin/sh\n"),
            ("scripts/run.sh", "#!/bin/sh\n"),
            ("other/tool", "#!/bin/sh\n"),
            (".mcp.json", MCP),
        ],
        &["bin/srv", "scripts/run.sh", "other/tool"],
    )
}

fn with_plugins(fx: &Fx, plugins: Vec<DeliveredPlugin>, lines: &[&str], details: &[&str]) -> LaunchCtx {
    let d = delivered(&fx.paths.plugin_store, plugins, lines, details);
    ctx(fx, |c| c.plugins = Plugins::Delivered(Some(d)))
}

fn text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn delivery_writes_the_opencode_forms() {
    let fx = fx();
    let c = with_plugins(&fx, vec![acme(&fx)], &[], &[]);
    let (spec, cfg) = built(&c);
    let oc_dir = c.session_dir.root.join("opencode");
    let root = oc_dir.join("plugin-root/acme");
    assert_eq!(text(&oc_dir.join("skills/acme-review/SKILL.md")), "---\nname: acme-review\ndescription: d\n---\nbody\n");
    assert!(oc_dir.join("skills/acme-review/ref.md").exists());
    assert_eq!(text(&oc_dir.join("plugins/plugin-acme.js")), "export default {}\n");
    for f in ["bin/srv", "scripts/run.sh"] {
        let p = root.join(f);
        assert!(p.exists(), "{f}");
        #[cfg(unix)]
        assert!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&p).unwrap().permissions()) & 0o111 != 0, "{f}");
    }
    assert!(!root.join("other/tool").exists());
    let a = &cfg["agent"];
    assert_eq!(a["acme-helper"], json!({"mode": "subagent", "description": "say \"hi\"", "prompt": "You help.\n"}));
    assert_eq!(a["acme-crlf"], json!({"mode": "subagent", "description": "Crlf one", "prompt": "Body\r\n"}));
    assert_eq!(cfg["command"]["acme-ship"], json!({"description": "Ship it", "template": "Ship $ARGUMENTS\n"}));
    let r = zygo_core::paths::plain(&root).to_string_lossy().into_owned();
    assert_eq!(cfg["mcp"]["acme-srv"], json!({"type": "local", "command": [format!("{r}/bin/srv"), "--root", &r], "environment": {"K": format!("{r}/d")}}));
    assert!(cfg["mcp"].get("acme-gone").is_none());
    assert!(cfg["mcp"].get("acme-far").is_none());
    assert_eq!(
        spec.lines,
        [
            "plugin mk:acme: ${CLAUDE_PLUGIN_ROOT}/other/tool not found (https://example.invalid/acme)",
            "plugin mk:acme: zygo-test-missing-cmd not found (https://example.invalid/acme)"
        ]
    );
    assert_eq!(cfg["model"], "zygo/lead");
    assert!(cfg["agent"].get("zygo-plan").is_some());
    assert_valid(&cfg);
}

#[test]
fn embedded_skill_is_delivered_always() {
    let fx = fx();
    let c = ctx(&fx, |_| {});
    let (spec, cfg) = built(&c);
    let skill = text(&c.session_dir.root.join("opencode/skills/zygo-zygo-cli/SKILL.md"));
    assert!(skill.lines().any(|l| l == "name: zygo-zygo-cli"), "{skill}");
    assert!(cfg.get("command").is_none_or(|m| m.as_object().unwrap().keys().all(|k| !k.starts_with("zygo-"))));
    assert_eq!(spec.lines, ["plugins: partial"]);
}

#[test]
fn delivered_lines_and_details_are_relayed() {
    let fx = fx();
    let line = "plugin mk:x: code needs approval; run zygo plugins approve mk:x";
    let c = with_plugins(&fx, vec![], &[line], &["store lock failed: Other"]);
    let (spec, _) = built(&c);
    assert_eq!(spec.lines, [line, "plugins: partial"]);
    assert_eq!(&spec.log[..1], ["store lock failed: Other"]);
}

fn clash(store: &Path, mk: &str, name: &str, server: &str) -> DeliveredPlugin {
    let mcp = format!(r#"{{"mcpServers":{{"{server}":{{"command":"${{CLAUDE_PLUGIN_ROOT}}/bin/s"}}}}}}"#);
    let (sk, ag, cm) = (name_key(name), name_key(name), name_key(name));
    put_plugin(
        store,
        mk,
        name,
        &[
            (&format!("skills/{sk}/SKILL.md"), &format!("---\nname: {sk}\ndescription: d\n---\nbody of {mk}:{name}\n")),
            (&format!("agents/{ag}.md"), "---\ndescription: d\n---\nA\n"),
            (&format!("commands/{cm}.md"), "---\ndescription: d\n---\nC\n"),
            ("bin/s", "#!/bin/sh\n"),
            (".mcp.json", &mcp),
        ],
        &["bin/s"],
    )
}

/// The component name that makes the key `a-b-c`: `b-c` under plugin `a`, `c` under plugin `a-b`.
fn name_key(plugin: &str) -> &'static str {
    if plugin == "a" { "b-c" } else { "c" }
}

#[test]
fn duplicate_keys_deliver_the_first_in_id_order() {
    let fx = fx();
    let s = &fx.paths.plugin_store;
    let c = with_plugins(&fx, vec![clash(s, "mk", "a", "b-c"), clash(s, "mk", "a-b", "c")], &[], &[]);
    let (spec, cfg) = built(&c);
    assert_eq!(
        spec.lines,
        ["skill", "agent", "command", "mcp"].map(|k| format!("plugin mk:a-b: {k} a-b-c already delivered from mk:a")).to_vec()
    );
    assert!(text(&c.session_dir.root.join("opencode/skills/a-b-c/SKILL.md")).ends_with("body of mk:a\n"));
    assert_eq!(cfg["agent"]["a-b-c"]["prompt"], "A\n");
    assert_valid(&cfg);
}

#[test]
fn missing_command_line_is_escaped() {
    let fx = fx();
    let forged = "\nplugin mk:fx: forged\u{1b}[2K";
    let pj = json!({"name": "fx", "version": "1.0.0", "homepage": format!("https://example.invalid/fx{forged}")}).to_string();
    let mcp = json!({"mcpServers": {"gone": {"command": format!("zygo-test-missing-cmd{forged}")}}}).to_string();
    let p = put_plugin(&fx.paths.plugin_store, "mk", "fx", &[(".claude-plugin/plugin.json", pj.as_str()), (".mcp.json", mcp.as_str())], &[]);
    let (spec, _) = built(&with_plugins(&fx, vec![p], &[], &[]));
    let e = "\\nplugin mk:fx: forged\\u{1b}[2K";
    assert_eq!(spec.lines, [format!("plugin mk:fx: zygo-test-missing-cmd{e} not found (https://example.invalid/fx{e})")]);
}

#[test]
fn duplicate_key_line_is_escaped() {
    let fx = fx();
    let s = &fx.paths.plugin_store;
    let srv = |name: &str, server: &str| {
        let mcp = json!({"mcpServers": {server: {"command": "${CLAUDE_PLUGIN_ROOT}/bin/s"}}}).to_string();
        put_plugin(s, "mk", name, &[("bin/s", "#!/bin/sh\n"), (".mcp.json", mcp.as_str())], &["bin/s"])
    };
    let forged = "\u{1b}[2K\nplugin mk:a-b: forged";
    let c = with_plugins(&fx, vec![srv("a", &format!("b-c{forged}")), srv("a-b", &format!("c{forged}"))], &[], &[]);
    let (spec, _) = built(&c);
    assert_eq!(spec.lines, ["plugin mk:a-b: mcp a-b-c\\u{1b}[2K\\nplugin mk:a-b: forged already delivered from mk:a"]);
}

#[test]
fn a_store_copy_that_differs_is_skipped() {
    let fx = fx();
    let s = &fx.paths.plugin_store;
    let bad = acme(&fx);
    std::fs::write(bad.dir.join("skills/review/SKILL.md"), "other bytes").unwrap();
    let other = put_plugin(s, "mk", "other", &[("commands/go.md", "---\ndescription: g\n---\nGo\n")], &[]);
    let c = with_plugins(&fx, vec![bad, other], &[], &[]);
    let (spec, cfg) = built(&c);
    assert!(!c.session_dir.root.join("opencode/skills/acme-review").exists());
    assert!(!c.session_dir.root.join("opencode/plugin-root/acme").exists());
    let all = cfg.to_string();
    assert!(!all.contains("acme-"), "{all}");
    assert_eq!(spec.log, ["plugin mk:acme: store copy differs from its manifest; run zygo plugins sync"]);
    assert_eq!(spec.lines, ["plugins: partial"]);
    assert_eq!(cfg["command"]["other-go"]["template"], "Go\n");
}

#[test]
fn a_snapshot_is_refused() {
    let fx = fx();
    let c = ctx(&fx, |c| c.plugins = Plugins::Snapshot(zygo_plugins::snapshot::Snapshot::default()));
    assert!(oc().build(&c).is_err());
}

// ---- Task 5: preflight ----

use zygo_launch::Preflight;

fn put(fx: &Fx, rel: &str, text: &str) {
    let p = fx.root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A launch in `<root>/repo/sub` (`<root>/repo/.git` is a directory) with `HOME = <root>/home` and `extra` variables.
fn pf_ctx(fx: &Fx, extra: &[(&str, PathBuf)], f: impl FnOnce(&mut LaunchCtx)) -> LaunchCtx {
    std::fs::create_dir_all(fx.root.join("repo/sub")).unwrap();
    std::fs::create_dir_all(fx.root.join("repo/.git")).unwrap();
    ctx(fx, |c| {
        c.cwd = fx.root.join("repo/sub");
        c.env.0.extend(extra.iter().map(|(k, v)| (k.into(), v.clone().into_os_string())));
        f(c);
    })
}

#[test]
fn preflight_names_the_file_and_key() {
    let cases: Vec<(&str, &str, &str)> = vec![
        ("repo/.opencode/opencode.json", r#"{"provider":{"zygo":{}}}"#, "provider.zygo"),
        ("repo/sub/opencode.json", r#"{"model":"a/b"}"#, "model"),
        ("repo/sub/opencode.json", r#"{"small_model":"a/b"}"#, "small_model"),
        ("repo/sub/opencode.json", r#"{"provider":{"zygo":{"options":{"baseURL":"http://x/y"}}} /* c */}"#, "provider.zygo"),
        ("repo/sub/opencode.json", r#"{"a":"/* not a comment */","model":"m"}"#, "model"),
        ("repo/sub/opencode.json", r#"{"a":"q\"//x","model":"m"}"#, "model"),
        ("repo/opencode.jsonc", "// c\n{\"agent\":{\"build\":{\"model\":\"a/b\",},},}", "agent.build.model"),
        ("repo/opencode.json", "// c\n{\"agent\":{\"build\":{\"model\":\"a/b\",},},}", "agent.build.model"),
        ("repo/opencode.json", r#"{"agent":{"build":{"permission":{"edit":"allow"}}}}"#, "agent.build.permission.edit"),
        ("repo/opencode.json", r#"{"agent":{"acme-helper":{"model":"a/b"}}}"#, "agent.acme-helper.model"),
        ("home/.config/opencode/opencode.json", r#"{"provider":{"zygo":{}}}"#, "provider.zygo"),
    ];
    for (rel, text, key) in cases {
        let fx = fx();
        put(&fx, rel, text);
        let plugin = put_plugin(&fx.paths.plugin_store, "mk", "acme", &[("agents/helper.md", "---\ndescription: d\n---\nA\n")], &[]);
        let d = delivered(&fx.paths.plugin_store, vec![plugin], &[], &[]);
        let c = pf_ctx(&fx, &[], |c| c.plugins = Plugins::Delivered(Some(d)));
        let got = oc().preflight(&c);
        assert_eq!(got, Err(Preflight { file: fx.root.join(rel), key: key.into() }), "{rel} {text}");
    }
    let fx = fx();
    put(&fx, "xdg/opencode/opencode.json", r#"{"model":"a/b"}"#);
    put(&fx, "home/.config/opencode/opencode.json", r#"{"small_model":"a/b"}"#);
    let c = pf_ctx(&fx, &[("XDG_CONFIG_HOME", fx.root.join("xdg"))], |_| {});
    assert_eq!(oc().preflight(&c), Err(Preflight { file: fx.root.join("xdg/opencode/opencode.json"), key: "model".into() }));
}

#[test]
fn preflight_accepts_what_zygo_does_not_set() {
    let ok = |rel: &str, text: &str| {
        let fx = fx();
        put(&fx, rel, text);
        assert_eq!(oc().preflight(&pf_ctx(&fx, &[], |_| {})), Ok(()), "{rel} {text}");
    };
    assert_eq!(oc().preflight(&pf_ctx(&fx(), &[], |_| {})), Ok(()));
    ok("repo/opencode.json", r#"{"agent":{"mine":{"model":"a/b"}}}"#);
    ok("repo/opencode.json", r#"{"permission":{"edit":"allow"}}"#);
    ok("repo/opencode.json", r#"{"provider":{"other":{}}}"#);
    ok("repo/opencode.json", "{ not json");
    ok("opencode.json", r#"{"model":"a/b"}"#);
    ok("repo/opencode.json", r#"{"agent":{"zygo-ro":{"permission":{"edit":"allow"}}}}"#);
    let fx = fx();
    put(&fx, "repo/opencode.json", r#"{"agent":{"zygo-ro":{"permission":{"edit":"allow"}}}}"#);
    let c = pf_ctx(&fx, &[], |c| {
        c.config.custom_roles.insert("ro".into(), CustomRole { model: "claude-sonnet-5-5".into(), description: "d".into(), prompt: "p".into(), writes: false });
    });
    assert_eq!(oc().preflight(&c), Err(Preflight { file: fx.root.join("repo/opencode.json"), key: "agent.zygo-ro.permission.edit".into() }));
}

// ---- Task 6: the launcher end to end ----

use std::ffi::OsString;
use std::sync::Mutex;
use zygo_launch::{LaunchOpts, Launcher};

/// The stub agent is this test binary (`stub_child`, ignored unless `ZYGO_TEST_OUT` is set).
#[test]
#[ignore]
fn stub_child() {
    let Some(out) = std::env::var_os("ZYGO_TEST_OUT").map(PathBuf::from) else { return };
    let vars: BTreeMap<String, String> =
        std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    std::fs::write(out.join("env.json"), serde_json::to_vec(&vars).unwrap()).unwrap();
    let mut tree = BTreeMap::new();
    if let Some(dir) = std::env::var_os("OPENCODE_CONFIG_DIR").map(PathBuf::from) {
        let mut todo = vec![dir.clone()];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    todo.push(p);
                } else {
                    let rel = p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
                    tree.insert(rel, std::fs::read_to_string(&p).unwrap_or_default());
                }
            }
        }
    }
    std::fs::write(out.join("tree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let session = std::env::var_os("ZYGO_SESSION_DIR").map(PathBuf::from).map(|d| files_under(&d)).unwrap_or_default();
    std::fs::write(out.join("session.json"), serde_json::to_vec(&session).unwrap()).unwrap();
    std::fs::write(out.join("argv.json"), serde_json::to_vec(&std::env::args().collect::<Vec<_>>()).unwrap()).unwrap();
}

/// Every file under `dir`, keyed by its `/`-separated relative path, its bytes read lossily as UTF-8.
fn files_under(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.is_file()
                && let Ok(b) = std::fs::read(&p)
            {
                // A locked file (`pid`) cannot be read on Windows, where locks are mandatory.
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                files.insert(rel, String::from_utf8_lossy(&b).into_owned());
            }
        }
    }
    files
}

struct Launch {
    fx: Fx,
    out: PathBuf,
    bin: PathBuf,
}

fn launch_fx() -> Launch {
    let fx = fx();
    let (out, bin) = (fx.root.join("out"), fx.root.join("bin"));
    for d in [&out, &bin, &fx.root.join("work")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::copy(std::env::current_exe().unwrap(), bin.join(format!("opencode{}", std::env::consts::EXE_SUFFIX))).unwrap();
    Launch { fx, out, bin }
}

fn launch(l: &Launch, extra: &[(&str, &str)]) -> (i32, String) {
    let mut env = vec![(OsString::from("PATH"), OsString::from(&l.bin)), (OsString::from("ZYGO_TEST_OUT"), OsString::from(&l.out))];
    env.extend(extra.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
    if cfg!(windows)
        && let Some(v) = std::env::var_os("SystemRoot")
    {
        env.push(("SystemRoot".into(), v));
    }
    let mut o = LaunchOpts::new(Env(env), l.fx.root.join("work"), l.fx.paths.clone());
    o.builtin = Some(builtin());
    let buf: Arc<Mutex<Vec<u8>>> = Arc::default();
    o.stderr = Some(buf.clone());
    o.args = ["--exact", "stub_child", "--ignored", "--nocapture", "--test-threads=1"].map(String::from).to_vec();
    let code = Launcher::run(&OpenCode::default(), o);
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    (code, err)
}

#[test]
fn launch_sets_the_opencode_environment() {
    let l = launch_fx();
    let (code, err) = launch(&l, &[("GH_TOKEN", "gh-test-value"), ("ANTHROPIC_API_KEY", "k-a"), ("OPENAI_API_KEY", "k-o")]);
    assert_eq!(code, 0, "{err}");
    let env: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(l.out.join("env.json")).expect("the child ran")).unwrap();
    assert_eq!(env["ZYGO_AGENT"], "opencode");
    assert_eq!(env["GH_TOKEN"], "gh-test-value");
    assert!(!env.contains_key("ANTHROPIC_API_KEY") && !env.contains_key("OPENAI_API_KEY"));
    let tree: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(l.out.join("tree.json")).unwrap()).unwrap();
    assert!(tree.contains_key("plugins/zygo.js"), "{:?}", tree.keys());
    assert!(tree.contains_key("skills/zygo-zygo-cli/SKILL.md"));
    let cfg: Value = serde_json::from_str(&env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    let token = cfg["provider"]["zygo"]["options"]["apiKey"].as_str().unwrap();
    assert!(token.len() > 8);
    for (k, v) in &env {
        assert!(k == "OPENCODE_CONFIG_CONTENT" || !v.contains(token), "{k}");
    }
    // C3: the token lives only in the session's `token` file, never in another file, argv or stderr.
    let session: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(l.out.join("session.json")).unwrap()).unwrap();
    for f in ["token", "view.json", "session.log", "opencode/plugins/zygo.js", "opencode/skills/zygo-zygo-cli/SKILL.md"] {
        assert!(session.contains_key(f), "{f}: {:?}", session.keys());
    }
    let leaks: Vec<&String> = session.iter().filter(|(k, v)| *k != "token" && v.contains(token)).map(|(k, _)| k).collect();
    assert!(leaks.is_empty(), "the token is in {leaks:?}");
    let argv: Vec<String> = serde_json::from_slice(&std::fs::read(l.out.join("argv.json")).unwrap()).unwrap();
    assert!(argv.iter().all(|a| !a.contains(token)), "{argv:?}");
    assert!(!err.contains(token), "{err}");
}

#[test]
fn a_project_config_that_defines_provider_zygo_stops_the_launch() {
    let l = launch_fx();
    let file = l.fx.root.join("work/.opencode/opencode.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, r#"{"provider":{"zygo":{}}}"#).unwrap();
    let (code, err) = launch(&l, &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains(&file.display().to_string()) && err.contains("provider.zygo"), "{err}");
    assert!(!l.out.join("env.json").exists());
}
