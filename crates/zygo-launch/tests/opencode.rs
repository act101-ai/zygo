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
        ..Default::default()
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

/// The `after` bash fixture whose output is large: distinct plain lines, so no O§7.3 step 1-4 changes it and the
/// §10.1 size view applies.
fn big_after() -> Value {
    let mut p = fixture("plugin-hooks/tool.execute.after-bash.json");
    p["output"]["output"] = json!((0..20_000).map(|i| format!("x {i:05}\n")).collect::<String>());
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
    for (r, role) in &c.config.roles {
        let key = format!("zygo-{r}");
        if *r == c.config.main_role {
            assert!(a.get(&key).is_none(), "{key}");
            continue;
        }
        // ST4 verified (SX-10, T§7.2 item 3): the prompt is the role prompt only.
        let mut want = json!({"mode": "subagent", "description": role.description, "prompt": role.prompt, "model": format!("zygo/{r}")});
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

/// T§17 check 14, ST4 verified (`docs/premises.md` row ST4, SX-10): each OpenCode `zygo-<role>` prompt, of a built-in
/// and of a custom role, equals the role prompt and holds no mechanics text; the system-prompt transform is the one
/// owner of that text (session text design T§7.2 item 3).
#[test]
fn st4_verified_zygo_role_prompt_is_the_role_prompt() {
    let fx = fx();
    let custom = CustomRole { model: "claude-sonnet-5-5".into(), description: "custom".into(), prompt: "C".into(), writes: true };
    let c = ctx(&fx, |c| {
        c.config.custom_roles.insert("cu".into(), custom.clone());
    });
    let (_, cfg) = built(&c);
    let mech = mechanics(&c.view);
    assert!(!mech.is_empty());
    let prompts = c.config.roles.iter().map(|(k, r)| (k, &r.prompt)).chain(c.config.custom_roles.iter().map(|(k, r)| (k, &r.prompt)));
    let mut n = 0;
    for (r, prompt) in prompts.filter(|(k, _)| **k != c.config.main_role) {
        let p = &cfg["agent"][format!("zygo-{r}")]["prompt"];
        assert_eq!(p, &json!(prompt), "zygo-{r}");
        assert!(!p.as_str().unwrap().contains(&mech), "zygo-{r}");
        n += 1;
    }
    assert!(n >= 2, "{n}");
    assert_eq!(cfg["agent"]["zygo-cu"]["prompt"], "C");
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
    let mut hooks = BTreeSet::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        let stem = name.strip_suffix(".json").unwrap();
        if stem == "plugin-init" {
            continue;
        }
        let hook = stem.strip_suffix("-bash").or_else(|| stem.strip_suffix("-bash-stderr")).unwrap_or(stem);
        assert!(TEMPLATE.contains(&format!("\"{hook}\"")), "{hook}");
        hooks.insert(hook.to_string());
    }
    assert_eq!(hooks.len(), 4);
    for t in [
        "directory", "\"--agent\", \"opencode\"", "10000", "permissionDecision", "updatedToolOutput", "output.output =", "output.system.push",
        "output.context.push", "ZYGO_MAIN_ROLE", "input.model", "stdin.on(\"error\"", "\"guard\"", "\"trim\"", "\"work\"", "\"session-start\"", "\"pre-compact\"", "\"subagent\"",
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
fn node_run(fx: &Fx, name: &str, mode: &str, role: Option<&str>, big: bool) -> NodeRun {
    node_run_edge(fx, name, mode, role, big, None)
}

/// [node_run]; with `edge`, the driver makes only the one transform call of that `EDGE` case.
#[cfg(unix)]
fn node_run_edge(fx: &Fx, name: &str, mode: &str, role: Option<&str>, big: bool, edge: Option<&str>) -> NodeRun {
    use std::os::unix::fs::PermissionsExt;
    let dir = fx.root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("zygo-stub");
    let script = r#"#!/bin/sh
d=$(dirname "$0")
echo "$*" >> "$d/calls.txt"
if [ "$STUB_MODE" = noread ]; then exit 0; fi
cat > "$d/stdin-$2.json"
{ cat "$d/stdin-$2.json"; echo; } >> "$d/stdin-$2.jsonl"
if [ "$STUB_MODE" = fail ]; then exit 1; fi
case "$2:$STUB_MODE" in
  guard:deny) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"R1"}}' ;;
  trim:ok) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"stdout":"VIEW"}}}' ;;
  trim:utf8) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"stdout":"'; cat "$d/stdout.txt"; printf '%s' '"}}}' ;;
  session-start:ok) printf 'TEXT%s' "$6" ;;
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
const EDGE = {
  "no-model": [{}, { system: [] }],
  "null-model": [{ model: null }, { system: [] }],
  "no-id": [{ model: { providerID: "zygo" } }, { system: [] }],
  "no-system": [{ model: { id: "read", providerID: "zygo" } }, {}],
};
if (process.env.EDGE) {
  const [input, out] = EDGE[process.env.EDGE];
  try { await h["experimental.chat.system.transform"](input, out); res.threw = null; } catch (e) { res.threw = e.message; }
  res.out = out;
  console.log(JSON.stringify(res));
  process.exit(0);
}
try { await h["tool.execute.before"]({ tool: "bash" }, { args: { command: "x" } }); res.threw = null; } catch (e) { res.threw = e.message; }
const after = { output: process.env.BIG ? "y".repeat(1 << 20) : "orig" };
await h["tool.execute.after"]({ tool: "bash", args: {} }, after);
res.after = process.env.BIG ? after.output.length : after.output;
res.system = {};
for (const [id, p] of [["lead", "zygo"], ["code", "zygo"], ["read", "zygo"], ["lead", "anthropic"]]) {
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
    cmd.arg("driver.mjs").current_dir(&dir).env("STUB_MODE", mode).env_remove("ZYGO_WORKSPACE");
    match role {
        Some(r) => cmd.env("ZYGO_MAIN_ROLE", r),
        None => cmd.env_remove("ZYGO_MAIN_ROLE"),
    };
    if big {
        cmd.env("BIG", "1");
    }
    if let Some(e) = edge {
        cmd.env("EDGE", e);
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
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_behaves_under_node() {
    if no_node("plugin_behaves_under_node") {
        return;
    }
    let fx = fx();
    let sys = |r: &Value, k: &str| r["system"][k].clone();

    let r = node_run(&fx, "deny", "deny", Some("lead"), false);
    assert_eq!(r.result.unwrap()["threw"], "R1");

    let r = node_run(&fx, "ok", "ok", Some("lead"), false);
    let v = r.result.clone().unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert_eq!(v["after"], "VIEW");
    assert_eq!(sys(&v, "lead/zygo"), json!(["TEXT"]));
    assert_eq!(sys(&v, "code/zygo"), json!(["TEXTsubagent"]));
    assert_eq!(sys(&v, "read/zygo"), json!(["TEXTsubagent"]));
    assert_eq!(sys(&v, "lead/anthropic"), json!([]));
    assert_eq!(v["compact"], json!(["KEEP"]));
    let calls: Vec<&str> = r.calls.lines().collect();
    assert_eq!(&calls[..3], ["hook guard --agent opencode", "hook trim --agent opencode", "hook work --agent opencode"]);
    assert!(calls.contains(&"hook session-start --agent opencode"));
    assert!(calls.contains(&"hook pre-compact --agent opencode"));
    let stdin: Value = serde_json::from_slice(&std::fs::read(r.dir.join("stdin-guard.json")).unwrap()).unwrap();
    assert_eq!(stdin["cwd"], "/r");
    assert_eq!(stdin["input"]["tool"], "bash");
    assert_eq!(stdin["output"]["args"]["command"], "x");

    for (name, mode) in [("fail", "fail"), ("missing", "missing")] {
        let v = node_run(&fx, name, mode, Some("lead"), false).result.unwrap();
        assert_eq!(v["threw"], Value::Null, "{mode}");
        assert_eq!(v["after"], "orig", "{mode}");
        assert_eq!(v["compact"], json!([]), "{mode}");
        for k in ["lead/zygo", "code/zygo", "read/zygo", "lead/anthropic"] {
            assert_eq!(sys(&v, k), json!([]), "{mode} {k}");
        }
    }

    let v = node_run(&fx, "noread", "noread", Some("lead"), true).result.unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert_eq!(v["after"], 1 << 20);

    // A multi-byte trim view over 128 KiB reaches node in many pipe chunks; no character may split.
    let euro = "\u{20ac}".repeat(50_000);
    std::fs::create_dir_all(fx.root.join("utf8")).unwrap();
    std::fs::write(fx.root.join("utf8/stdout.txt"), &euro).unwrap();
    let v = node_run(&fx, "utf8", "utf8", Some("lead"), false).result.unwrap();
    let after = v["after"].as_str().unwrap();
    assert!(!after.contains('\u{fffd}'), "U+FFFD in the trim view");
    assert_eq!(after, euro);
}

#[cfg(unix)]
const MAIN_CALL: &str = "hook session-start --agent opencode";
#[cfg(unix)]
const SUB_CALL: &str = "hook session-start --agent opencode --event subagent";

#[cfg(unix)]
fn session_starts(r: &NodeRun) -> Vec<&str> {
    r.calls.lines().filter(|l| l.starts_with("hook session-start")).collect()
}

/// Every `session-start` call's stdin, one record per call (the stub appends each to `stdin-session-start.jsonl`).
#[cfg(unix)]
fn session_start_stdins(r: &NodeRun) -> Vec<Value> {
    let text = std::fs::read_to_string(r.dir.join("stdin-session-start.jsonl")).unwrap_or_default();
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

// T§7.2 item 1, T§17 check 10
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_other_zygo_role_calls_event_subagent() {
    if no_node("subagent_transform_other_zygo_role_calls_event_subagent") {
        return;
    }
    let fx = fx();
    let r = node_run(&fx, "sx-sub", "ok", Some("lead"), false);
    let v = r.result.clone().unwrap();
    assert_eq!(v["system"]["code/zygo"], json!(["TEXTsubagent"]));
    assert_eq!(v["system"]["read/zygo"], json!(["TEXTsubagent"]));
    assert_eq!(session_starts(&r)[1..], [SUB_CALL, SUB_CALL]);
    let stdins = session_start_stdins(&r);
    assert_eq!(stdins.len(), 3);
    for id in ["code", "read"] {
        let want = json!({"input": {"model": {"id": id, "providerID": "zygo"}}, "output": {"system": []}, "cwd": "/r"});
        assert_eq!(stdins.iter().filter(|s| **s == want).count(), 1, "{id}");
    }
}

// T§7.2 item 1, T§17 check 10
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_main_role_calls_without_event() {
    if no_node("subagent_transform_main_role_calls_without_event") {
        return;
    }
    let fx = fx();
    let r = node_run(&fx, "sx-main", "ok", Some("lead"), false);
    assert_eq!(r.result.clone().unwrap()["system"]["lead/zygo"], json!(["TEXT"]));
    assert_eq!(session_starts(&r)[0], MAIN_CALL);
    assert_eq!(session_starts(&r).iter().filter(|l| **l == MAIN_CALL).count(), 1);
}

// T§7.2 item 1, T§17 check 10
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_anthropic_provider_calls_nothing() {
    if no_node("subagent_transform_anthropic_provider_calls_nothing") {
        return;
    }
    let fx = fx();
    let r = node_run(&fx, "sx-other", "ok", Some("lead"), false);
    assert_eq!(r.result.clone().unwrap()["system"]["lead/anthropic"], json!([]));
    assert_eq!(session_starts(&r), [MAIN_CALL, SUB_CALL, SUB_CALL]);
}

// T§7.2 item 1, T§17 check 10
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_unset_main_role_calls_event_subagent() {
    if no_node("subagent_transform_unset_main_role_calls_event_subagent") {
        return;
    }
    let fx = fx();
    for (name, role) in [("sx-unset", None), ("sx-empty", Some(""))] {
        let r = node_run(&fx, name, "ok", role, false);
        let v = r.result.clone().unwrap();
        for k in ["lead/zygo", "code/zygo", "read/zygo"] {
            assert_eq!(v["system"][k], json!(["TEXTsubagent"]), "{name} {k}");
        }
        assert_eq!(v["system"]["lead/anthropic"], json!([]), "{name}");
        assert_eq!(session_starts(&r), [SUB_CALL, SUB_CALL, SUB_CALL], "{name}");
    }
}

// T§7.2 item 1 (SX-6 review round 1, item 3): a missing or null `input.model` is not a `zygo/` model
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_missing_or_null_model_calls_nothing() {
    if no_node("subagent_transform_missing_or_null_model_calls_nothing") {
        return;
    }
    let fx = fx();
    for edge in ["no-model", "null-model"] {
        let r = node_run_edge(&fx, &format!("sx-{edge}"), "ok", Some("lead"), false, Some(edge));
        let v = r.result.clone().unwrap();
        assert_eq!(v["threw"], Value::Null, "{edge}");
        assert_eq!(v["out"], json!({"system": []}), "{edge}");
        assert_eq!(r.calls, "", "{edge}");
    }
}

// T§7.2 item 1 (SX-6 review round 1, item 3): a `zygo` model with no id is not the main role
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_zygo_model_without_id_calls_event_subagent() {
    if no_node("subagent_transform_zygo_model_without_id_calls_event_subagent") {
        return;
    }
    let fx = fx();
    let r = node_run_edge(&fx, "sx-no-id", "ok", Some("lead"), false, Some("no-id"));
    let v = r.result.clone().unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert_eq!(v["out"], json!({"system": ["TEXTsubagent"]}));
    assert_eq!(session_starts(&r), [SUB_CALL]);
}

// T-C1 (SX-6 review round 1, item 4): no `output.system` array, no zygo process and no throw
#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn subagent_transform_without_system_array_calls_nothing() {
    if no_node("subagent_transform_without_system_array_calls_nothing") {
        return;
    }
    let fx = fx();
    let r = node_run_edge(&fx, "sx-no-system", "ok", Some("lead"), false, Some("no-system"));
    let v = r.result.clone().unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert_eq!(v["out"], json!({}));
    assert_eq!(r.calls, "");
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
    let code = Launcher::run(&OpenCode::default(), o).code;
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

// ---- L2: session arguments and member events ----

fn fixture_text(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode").join(rel)).unwrap()
}

/// Option names of the pinned OpenCode help: after `Options:`, each line whose first non-space character is `-`;
/// the text before the first run of two spaces, split at `, ` (W4's test parser).
fn help_names() -> BTreeSet<String> {
    fixture_text("help.txt")
        .lines()
        .skip_while(|l| *l != "Options:")
        .skip(1)
        .map(str::trim_start)
        .filter(|t| t.starts_with('-'))
        .flat_map(|t| t.split("  ").next().unwrap().split(", ").map(str::to_string).collect::<Vec<_>>())
        .collect()
}

#[test] // W§8.3, AU3, AU30
fn session_args_follow_w4_and_the_pinned_help() {
    let help = help_names();
    let oc = OpenCode::default();
    let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    let names = zygo_workspace::args::session_flags(Agent::OpenCode);
    assert_eq!(names, ["-c", "--continue", "-s", "--session", "--fork"]);
    for f in names {
        assert!(help.contains(f), "{f} is not in the pinned --help");
        assert!(oc.has_session_arg(&s(&["--model", "zygo/lead", f, "ses_1"])), "{f}");
    }
    assert!(oc.has_session_arg(&s(&["--session=ses_1"])));
    assert!(!oc.has_session_arg(&s(&["run", "--model", "zygo/lead", "--print-logs"])));
    assert!(!oc.has_session_arg(&s(&["--", "-s"])));
    assert!(oc.preassign_args("ses_1").is_empty());
    assert_eq!(oc.resume_args("ses_1"), s(&["--session=ses_1"]));
    assert_eq!(oc.resume_args("-x"), s(&["--session=-x"])); // one token: an id that starts with `-` stays the value (AU3)
    assert!(oc.has_session_arg(&oc.resume_args("-x")));
    assert!(help.contains("--session"));
}

#[test] // W§9.1, AU104, W§15 §8.7: PW4 event names
fn plugin_workspace_events_match_pw4_fixtures() {
    let types: BTreeSet<String> = fixture_text("workspace/plugin-event-types.txt").lines().map(str::to_string).collect();
    for t in ["session.created", "session.idle", "session.status"] {
        assert!(types.contains(t), "{t} not seen by PW4");
        assert!(TEMPLATE.contains(&format!("\"{t}\"")), "{t}");
    }
    assert_eq!(fixture("workspace/tool.execute.before-task.json")["tool"], "task");
    // W§18 #91: both tool hooks carry the same `callID` and `sessionID` (AU27 tool count)
    let b = fixture("plugin-hooks/tool.execute.before-bash.json");
    let a = fixture("plugin-hooks/tool.execute.after-bash.json");
    assert!(b["input"]["callID"].is_string());
    assert_eq!(b["input"]["callID"], a["input"]["callID"]);
    assert_eq!(b["input"]["sessionID"], a["input"]["sessionID"]);
    for t in [
        "event: async", "ZYGO_WORKSPACE", "parentID", "\"workspace\"", "\"--event\"", "\"session\"", "\"prompt\"", "\"busy\"", "\"stop\"", "\"pre-tool\"",
        "input.tool === \"task\"", "tool_name: \"Agent\"", "\"tool-end\"", "input.callID", "tool_name: input.tool",
    ] {
        assert!(TEMPLATE.contains(t), "{t}");
    }
}

/// One run of the generated plugin under node, member mode or not.
#[cfg(unix)]
struct Ws<'a> {
    name: &'a str,
    /// The `ZYGO_WORKSPACE` value; `None` removes the name.
    member: Option<&'a str>,
    /// The stub mode: `ok`, `deny` (pre-tool denies), `slow` (session sleeps 0.3 s), `fail` (every call prints a
    /// deny and exits 1), `hang` (every workspace call sleeps 12 s, more than the 10 s timeout of the plugin),
    /// `hangpre` (only workspace `pre-tool` calls sleep 12 s), `slowpre` (only workspace `pre-tool` calls sleep
    /// 0.3 s), `guarddeny` (each `guard` call sleeps 0.3 s before it writes its line to `calls.txt`, then denies
    /// with the reason `R4` and exits 0).
    mode: &'a str,
    /// The session id of the `task` dispatch.
    main: &'a str,
    /// `[type, properties]` of each event, `["tool.execute.before", input]` / `["tool.execute.after", input]` of
    /// a tool-hook step, `["start", step]` (starts `step` and does not wait for it) or `["join", null]` (waits for
    /// every started step).
    events: Value,
    /// All events at once, without waiting.
    parallel: bool,
    /// The plugin names a zygo path that does not exist.
    missing: bool,
    /// The `directory` of the plugin.
    dir: &'a str,
    /// Run `tool.execute.before` for the PW4 `task` fixture after the events.
    tool: bool,
}

#[cfg(unix)]
fn ws_opts<'a>(name: &'a str, member: Option<&'a str>, mode: &'a str, main: &'a str, events: Value) -> Ws<'a> {
    Ws { name, member, mode, main, events, parallel: false, missing: false, dir: "/r", tool: true }
}

/// Runs the generated plugin under node: each step of `w.events` (in order, or all at once when `w.parallel`) goes
/// to the `event` hook, except a tool-hook step (`["tool.execute.before", input]` or `["tool.execute.after", input]`),
/// which calls that hook with `input` and an output; a `start` step runs its step without waiting, and a `join` step
/// waits for every started step; then, with `w.tool`, `tool.execute.before` runs for the PW4 `task` fixture in
/// session `w.main`. The result holds `threw` (the thrown message of the closing call or null), `stepThrew` (for each
/// tool-hook step, the thrown message or null, in the order the steps end), `eventMs` and `toolMs`.
#[cfg(unix)]
fn ws_node(fx: &Fx, w: Ws) -> NodeRun {
    use std::os::unix::fs::PermissionsExt;
    let dir = fx.root.join(w.name);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("zygo-stub");
    let script = r#"#!/bin/sh
d=$(dirname "$0")
case "$2:$STUB_MODE" in
  guard:guarddeny) sleep 0.3 ;;
esac
echo "$*" >> "$d/calls.txt"
in=$(cat)
printf '%s' "$in" > "$d/stdin-$2-$6.json"
if [ "$2" = workspace ]; then printf '%s %s\n' "$6" "$in" >> "$d/ws.log"; fi
case "$2:$6:$STUB_MODE" in
  workspace:session:slow) sleep 0.3 ;;
  workspace:pre-tool:deny) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"R2"}}' ;;
  *:*:fail) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"R3"}}'; exit 1 ;;
  workspace:pre-tool:hangpre) exec sleep 12 ;;
  workspace:pre-tool:slowpre) sleep 0.3 ;;
  guard::guarddeny) printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"R4"}}' ;;
  workspace:*:hang) exec sleep 12 ;;
esac
"#;
    std::fs::write(&stub, script).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let zygo = if w.missing { dir.join("no-such-zygo") } else { stub };
    std::fs::write(dir.join("plugin.mjs"), plugin_js(&ctx(fx, |_| {}), &zygo)).unwrap();
    std::fs::write(
        dir.join("driver.mjs"),
        r#"import { readFileSync } from "node:fs";
import { ZygoPlugin } from "./plugin.mjs";
const h = await ZygoPlugin({ directory: process.env.DIR });
const task = JSON.parse(readFileSync(process.env.TASK_FX, "utf8"));
const steps = JSON.parse(process.env.EVENTS);
const res = { threw: null, stepThrew: [] };
const started = [];
const one = async ([type, properties]) => {
  if (type === "start") {
    started.push(one(properties));
  } else if (type === "join") {
    await Promise.all(started.splice(0));
  } else if (type === "tool.execute.before" || type === "tool.execute.after") {
    const output = type === "tool.execute.before" ? { args: {} } : { title: "", output: "o", metadata: {} };
    try {
      await h[type](properties, output);
      res.stepThrew.push(null);
    } catch (e) { res.stepThrew.push(e.message); }
  } else {
    await h.event({ event: { type, properties } });
  }
};
let t = Date.now();
if (process.env.PARALLEL === "1") {
  await Promise.all(steps.map(one));
} else {
  for (const s of steps) await one(s);
}
res.eventMs = Date.now() - t;
if (process.env.TOOL === "1") {
  t = Date.now();
  try {
    await h["tool.execute.before"]({ tool: task.tool, sessionID: process.env.MAIN, callID: "c" }, { args: { description: "d", prompt: "p", subagent_type: "s" } });
  } catch (e) { res.threw = e.message; }
  res.toolMs = Date.now() - t;
}
process.stdout.write(JSON.stringify(res) + "\n", () => process.exit(0));
"#,
    )
    .unwrap();
    let mut cmd = std::process::Command::new("node");
    cmd.arg("driver.mjs")
        .current_dir(&dir)
        .env("STUB_MODE", w.mode)
        .env("MAIN", w.main)
        .env("DIR", w.dir)
        .env("EVENTS", w.events.to_string())
        .env("PARALLEL", if w.parallel { "1" } else { "0" })
        .env("TOOL", if w.tool { "1" } else { "0" })
        .env("TASK_FX", Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode/workspace/tool.execute.before-task.json"))
        .env_remove("ZYGO_WORKSPACE");
    if let Some(m) = w.member {
        cmd.env("ZYGO_WORKSPACE", m);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    NodeRun { result: serde_json::from_slice(&out.stdout).ok(), calls: std::fs::read_to_string(dir.join("calls.txt")).unwrap_or_default(), dir }
}

#[cfg(unix)]
fn ws_node_run(fx: &Fx, name: &str, member: Option<&str>, mode: &str, main: &str, events: Value, parallel: bool) -> NodeRun {
    ws_node(fx, Ws { parallel, ..ws_opts(name, member, mode, main, events) })
}

/// True when `node` is not on `PATH` and the test is skipped. With the variable `CI` set, a missing `node` fails the
/// test instead.
#[cfg(unix)]
fn no_node(test: &str) -> bool {
    let missing = !std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success());
    if missing {
        assert!(std::env::var_os("CI").is_none(), "{test}: node is not on PATH and CI is set");
        eprintln!("skipped {test}: node is not on PATH");
    }
    missing
}

#[cfg(unix)]
fn ws(e: &str) -> String {
    format!("hook workspace --agent opencode --event {e}")
}

#[cfg(unix)]
fn ws_calls(r: &NodeRun) -> Vec<String> {
    r.calls.lines().filter(|l| l.starts_with("hook workspace")).map(str::to_string).collect()
}

#[cfg(unix)]
fn stdin_of(r: &NodeRun, f: &str) -> Value {
    serde_json::from_slice(&std::fs::read(r.dir.join(f)).unwrap()).unwrap()
}

#[cfg(unix)]
fn busy(s: &str) -> Value {
    json!(["session.status", {"sessionID": s, "status": {"type": "busy"}}])
}

#[cfg(unix)]
fn idle(s: &str) -> Value {
    json!(["session.idle", {"sessionID": s}])
}

#[cfg(unix)]
fn status_idle(s: &str) -> Value {
    json!(["session.status", {"sessionID": s, "status": {"type": "idle"}}])
}

#[cfg(unix)]
fn created(s: &str) -> Value {
    json!(["session.created", {"info": {"id": s}}])
}

#[cfg(unix)]
fn created_child(s: &str, parent: &str) -> Value {
    json!(["session.created", {"info": {"id": s, "parentID": parent}}])
}

#[cfg(unix)]
fn updated(s: &str) -> Value {
    json!(["session.updated", {"info": {"id": s}}])
}

#[cfg(unix)]
fn deleted(s: &str) -> Value {
    json!(["session.deleted", {"info": {"id": s}}])
}

/// A `tool.execute.before` step of tool `tool` in session `s` with `callID` `call`.
#[cfg(unix)]
fn tool_before(tool: &str, s: &str, call: &str) -> Value {
    json!(["tool.execute.before", {"tool": tool, "sessionID": s, "callID": call}])
}

/// A `tool.execute.after` step of tool `tool` in session `s` with `callID` `call`.
#[cfg(unix)]
fn tool_after(tool: &str, s: &str, call: &str) -> Value {
    json!(["tool.execute.after", {"tool": tool, "sessionID": s, "callID": call, "args": {}}])
}

/// A step that starts `step` and does not wait for it.
#[cfg(unix)]
fn start(step: Value) -> Value {
    json!(["start", step])
}

/// A step that waits for every started step.
#[cfg(unix)]
fn join() -> Value {
    json!(["join", null])
}

/// The `zygo hook workspace` calls of a run as `ws_seq` gives them, with the `tool_name` of each `pre-tool`.
#[cfg(unix)]
fn tool_seq(r: &NodeRun) -> Vec<String> {
    seq_of(r, true)
}

/// The tool-order call list of a run whose closing `task` call is in session `A`.
#[cfg(unix)]
fn tool_order(fx: &Fx, name: &str, member: Option<&str>, mode: &str, events: Value) -> Vec<String> {
    tool_seq(&ws_node(fx, ws_opts(name, member, mode, "A", events)))
}

/// The `zygo hook workspace` calls of a run, one line each: the event, a space, the stdin JSON.
#[cfg(unix)]
fn ws_log(r: &NodeRun) -> Vec<String> {
    std::fs::read_to_string(r.dir.join("ws.log")).unwrap_or_default().lines().map(str::to_string).collect()
}

/// The `zygo hook workspace` calls of a run as `<event> <sessionID>`, or `<event>{}` with no `sessionID`; each
/// payload's `cwd` is `/r` (plan D7).
#[cfg(unix)]
fn ws_seq(r: &NodeRun) -> Vec<String> {
    seq_of(r, false)
}

/// The body of `ws_seq`; with `tool_names`, a `pre-tool` line is `pre-tool <sessionID> <tool_name>`.
#[cfg(unix)]
fn seq_of(r: &NodeRun, tool_names: bool) -> Vec<String> {
    ws_log(r)
        .iter()
        .map(|line| {
            let (event, payload) = line.split_once(' ').unwrap_or_else(|| panic!("{line}"));
            let v: Value = serde_json::from_str(payload).unwrap_or_else(|e| panic!("{line}: {e}"));
            assert_eq!(v["cwd"], "/r", "{line}");
            match v["sessionID"].as_str() {
                Some(id) if tool_names && event == "pre-tool" => {
                    format!("{event} {id} {}", v["tool_name"].as_str().unwrap_or_else(|| panic!("{line}")))
                }
                Some(id) => format!("{event} {id}"),
                None => format!("{event}{{}}"),
            }
        })
        .collect()
}

/// The call list of a member run (tool on, so the closing `pre-tool A` drains the chain, plan D7).
#[cfg(unix)]
fn order(fx: &Fx, name: &str, member: Option<&str>, events: Value) -> Vec<String> {
    ws_seq(&ws_node(fx, ws_opts(name, member, "ok", "A", events)))
}

#[cfg(unix)]
fn p1_events() -> Vec<Value> {
    vec![created("A"), busy("A"), idle("A")]
}

/// The event calls of P1, without the closing `pre-tool A`.
#[cfg(unix)]
fn p1_calls() -> Vec<String> {
    ["session{}", "session A", "prompt A", "stop A"].map(str::to_string).to_vec()
}

#[cfg(unix)]
const M: &str = "0123456789abcdef";

#[cfg(unix)]
#[test] // W§9.1, AU104: the Spec payloads, the member gate, the pre-tool deny
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_sends_member_events_under_node() {
    if no_node("plugin_sends_member_events_under_node") {
        return;
    }
    let fx = fx();
    let sid = fixture("workspace/tool.execute.before-task.json")["sessionID"].as_str().unwrap().to_string();
    let ev = json!([
        created(&sid),
        ["session.updated", {"info": {"id": sid}}],
        created_child("ses_child", &sid),
        busy(&sid),
        busy("ses_child"),
        idle("ses_child"),
        idle(&sid),
        status_idle(&sid)
    ]);
    let r = ws_node_run(&fx, "ws-ok", Some(M), "ok", &sid, ev.clone(), false);
    assert_eq!(r.result.clone().unwrap()["threw"], Value::Null);
    assert_eq!(
        r.calls.lines().collect::<Vec<_>>(),
        [ws("session"), ws("session"), ws("prompt"), ws("stop"), ws("pre-tool"), "hook guard --agent opencode".to_string()]
    );
    assert_eq!(stdin_of(&r, "stdin-workspace-session.json"), json!({"sessionID": sid, "cwd": "/r"}));
    assert_eq!(stdin_of(&r, "stdin-workspace-stop.json"), json!({"sessionID": sid, "cwd": "/r"}));
    let pre = std::fs::read(r.dir.join("stdin-workspace-pre-tool.json")).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&pre).unwrap(), json!({"sessionID": sid, "tool_name": "Agent", "cwd": "/r"}));
    let i = HookInput::parse(Agent::OpenCode, &pre).unwrap();
    assert_eq!(i.tool_name.as_deref(), Some("Agent"));
    assert!(!i.has_agent_id);

    let r = ws_node_run(&fx, "ws-deny", Some(M), "deny", &sid, ev.clone(), false);
    assert_eq!(r.result.clone().unwrap()["threw"], "R2");
    assert_eq!(r.calls.lines().last(), Some(ws("pre-tool").as_str()));
    assert!(!r.calls.contains("hook guard"));

    let resume = json!([["message.part.updated", {"part": {"sessionID": "ses_resume"}}], busy("ses_resume"), idle("ses_resume")]);
    let r = ws_node_run(&fx, "ws-resume", Some(M), "ok", "ses_resume", resume, false);
    assert_eq!(ws_calls(&r), [ws("session"), ws("prompt"), ws("stop"), ws("pre-tool")]);
    assert_eq!(stdin_of(&r, "stdin-workspace-session.json"), json!({"cwd": "/r"}));
    assert_eq!(ws_seq(&r), ["session{}", "prompt ses_resume", "stop ses_resume", "pre-tool ses_resume"]);

    for (name, member) in [("ws-none", None), ("ws-empty", Some(""))] {
        let r = ws_node_run(&fx, name, member, "deny", &sid, ev.clone(), false);
        assert_eq!(r.result.clone().unwrap()["threw"], Value::Null, "{name}");
        assert_eq!(r.calls.lines().collect::<Vec<_>>(), ["hook guard --agent opencode"], "{name}");
    }
}

#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_sends_prompt_for_a_turn_of_the_main_session_only() { // AU27 as amended
    if no_node("plugin_sends_prompt_for_a_turn_of_the_main_session_only") {
        return;
    }
    let fx = fx();
    let main = "ses_main";
    let events = json!([
        created(main),
        busy(main),
        busy(main),
        ["session.status", {"sessionID": main, "status": {"type": "retry", "attempt": 1, "message": "m", "next": 0}}],
        created_child("ses_child", main),
        busy("ses_child"),
        idle("ses_child"),
        busy("ses_child"),
        idle(main),
        busy(main),
        status_idle(main)
    ]);
    let r = ws_node_run(&fx, "ws-prompt", Some(M), "ok", main, events, false);
    assert_eq!(ws_calls(&r), [ws("session"), ws("session"), ws("prompt"), ws("stop"), ws("prompt"), ws("stop"), ws("pre-tool")]);
    assert_eq!(stdin_of(&r, "stdin-workspace-prompt.json"), json!({"sessionID": main, "cwd": "/r"}));

    // a child's busy sends no prompt, and an idle with no turn sends no stop
    let r = ws_node_run(&fx, "ws-prompt-sub", Some(M), "ok", main, json!([created(main), idle(main), created_child("ses_child", main), busy("ses_child"), idle("ses_child")]), false);
    assert_eq!(ws_calls(&r), [ws("session"), ws("session"), ws("pre-tool")]);

    let r = ws_node_run(&fx, "ws-prompt-none", None, "ok", main, json!([created(main), busy(main)]), false);
    assert!(ws_calls(&r).is_empty());
}

#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_sends_one_stop_per_turn_end() { // L2 round 2 item 5
    if no_node("plugin_sends_one_stop_per_turn_end") {
        return;
    }
    let fx = fx();
    let m = "ses_m";
    for (name, end) in [("ws-one-stop-a", json!([status_idle(m), idle(m)])), ("ws-one-stop-b", json!([idle(m), status_idle(m), idle(m)]))] {
        let mut events = vec![created(m), busy(m), busy(m)];
        events.extend(end.as_array().unwrap().iter().cloned());
        let r = ws_node_run(&fx, name, Some(M), "ok", m, Value::Array(events), false);
        assert_eq!(ws_calls(&r), [ws("session"), ws("session"), ws("prompt"), ws("stop"), ws("pre-tool")], "{name}");
    }
}

#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_main_session_follows_the_active_top_session_and_never_a_child() { // review minor 4, 5; round 2 item 4
    if no_node("plugin_main_session_follows_the_active_top_session_and_never_a_child") {
        return;
    }
    let fx = fx();
    let events = json!([
        created("ses_a"),
        busy("ses_a"),
        created_child("ses_kid", "ses_a"),
        ["session.updated", {"info": {"id": "ses_kid", "parentID": "ses_a"}}],
        busy("ses_kid"),
        idle("ses_a"),
        created("ses_b"),
        busy("ses_b"),
        idle("ses_b"),
        busy("ses_a"),
        status_idle("ses_a")
    ]);
    let r = ws_node_run(&fx, "ws-switch", Some(M), "ok", "ses_a", events, false);
    assert_eq!(
        ws_calls(&r),
        [
            ws("session"),
            ws("session"),
            ws("prompt"),
            ws("stop"),
            ws("session"),
            ws("prompt"),
            ws("stop"),
            ws("prompt"),
            ws("session"),
            ws("stop"),
            ws("pre-tool")
        ]
    );
    assert_eq!(stdin_of(&r, "stdin-workspace-session.json"), json!({"sessionID": "ses_a", "cwd": "/r"}));
    assert_eq!(stdin_of(&r, "stdin-workspace-stop.json"), json!({"sessionID": "ses_a", "cwd": "/r"}));

    // A child's events before any top session: the child never becomes the main session.
    let kid_first = json!([created_child("ses_kid", "ses_x"), busy("ses_kid"), idle("ses_kid")]);
    let r = ws_node_run(&fx, "ws-kid-first", Some(M), "ok", "ses_kid", kid_first, false);
    assert_eq!(ws_calls(&r), [ws("session"), ws("pre-tool")]);
}

#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_follows_a_tui_switch_to_an_existing_session() { // L2 round 2 item 4
    if no_node("plugin_follows_a_tui_switch_to_an_existing_session") {
        return;
    }
    let fx = fx();
    // `ses_old` was made before this OpenCode started: no `session.created` event for it
    let events = json!([created("ses_new"), busy("ses_new"), idle("ses_new"), busy("ses_old"), busy("ses_old"), idle("ses_old"), status_idle("ses_old")]);
    let r = ws_node_run(&fx, "ws-tui-switch", Some(M), "ok", "ses_old", events, false);
    assert_eq!(ws_calls(&r), [ws("session"), ws("session"), ws("prompt"), ws("stop"), ws("prompt"), ws("stop"), ws("pre-tool")]);
    assert_eq!(stdin_of(&r, "stdin-workspace-session.json"), json!({"sessionID": "ses_new", "cwd": "/r"}));
    assert_eq!(stdin_of(&r, "stdin-workspace-prompt.json"), json!({"sessionID": "ses_old", "cwd": "/r"}));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_decides_before_it_waits() { // review B2
    if no_node("plugin_decides_before_it_waits") {
        return;
    }
    let fx = fx();
    let events = json!([busy("ses_resume"), idle("ses_resume")]);
    let r = ws_node_run(&fx, "ws-parallel", Some(M), "slow", "ses_resume", events, true);
    assert_eq!(ws_calls(&r), [ws("session"), ws("prompt"), ws("stop"), ws("pre-tool")]);
    let calls = ws_calls(&r);
    assert_eq!(calls.iter().rposition(|c| *c == ws("stop")), Some(2), "the last turn call is stop: not busy");
}

#[cfg(unix)]
#[test]
#[ignore = "timing: asserts elapsed time against the plugin's 10 s timeout"]
fn plugin_member_calls_survive_a_missing_failing_or_hung_zygo() { // L2 round 2 item 6
    if no_node("plugin_member_calls_survive_a_missing_failing_or_hung_zygo") {
        return;
    }
    let fx = fx();
    let turn = json!([created("ses_m"), busy("ses_m"), idle("ses_m")]);
    let guard = "hook guard --agent opencode".to_string();

    let r = ws_node(&fx, Ws { missing: true, ..ws_opts("ws-missing", Some(M), "ok", "ses_m", turn.clone()) });
    let v = r.result.clone().unwrap();
    assert_eq!(v["threw"], Value::Null);
    assert!(v["eventMs"].as_u64().unwrap() < 2000, "{v}");
    assert!(r.calls.is_empty());

    // a deny printed by a zygo that exits 1 is ignored, on `event` and on `pre-tool`
    let r = ws_node(&fx, ws_opts("ws-fail", Some(M), "fail", "ses_m", turn.clone()));
    assert_eq!(r.result.clone().unwrap()["threw"], Value::Null);
    assert_eq!(r.calls.lines().collect::<Vec<_>>(), [ws("session"), ws("session"), ws("prompt"), ws("stop"), ws("pre-tool"), guard.clone()]);

    // a hung zygo on `event`: the handler never waits for it
    let r = ws_node(&fx, Ws { tool: false, ..ws_opts("ws-hang-event", Some(M), "hang", "ses_m", turn.clone()) });
    let v = r.result.clone().unwrap();
    assert!(v["eventMs"].as_u64().unwrap() < 2000, "{v}");

    // a hung zygo on the load call and on `pre-tool`: the 10 s timeout of each queued call ends the wait, no deny,
    // the guard runs
    let r = ws_node(&fx, ws_opts("ws-hang-tool", Some(M), "hang", "ses_m", json!([])));
    let v = r.result.clone().unwrap();
    assert_eq!(v["threw"], Value::Null);
    let ms = v["toolMs"].as_u64().unwrap();
    assert!((19_000..30_000).contains(&ms), "{v}");
    assert_eq!(r.calls.lines().collect::<Vec<_>>(), [ws("session"), ws("pre-tool"), guard]);
}

#[cfg(unix)]
#[test]
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_passes_hostile_ids_as_exact_json() { // L2 round 2 item 6
    if no_node("plugin_passes_hostile_ids_as_exact_json") {
        return;
    }
    let fx = fx();
    let dir = fx.root.join("ws-hostile");
    let pwned = |n: &str| dir.join(format!("pwned-{n}"));
    let sid = format!("ses\"q'u\nn$(touch {})`touch {}`\u{2028}e\\z", pwned("sid1").display(), pwned("sid2").display());
    let cwd = format!("/r \"d\"\n$(touch {})\u{2028}`touch {}`", pwned("cwd1").display(), pwned("cwd2").display());
    let r = ws_node(&fx, Ws { dir: &cwd, ..ws_opts("ws-hostile", Some(M), "ok", &sid, json!([created(&sid), busy(&sid), idle(&sid)])) });
    assert_eq!(r.result.clone().unwrap()["threw"], Value::Null);
    assert_eq!(ws_calls(&r), [ws("session"), ws("session"), ws("prompt"), ws("stop"), ws("pre-tool")]);
    for e in ["session", "prompt", "stop"] {
        assert_eq!(stdin_of(&r, &format!("stdin-workspace-{e}.json")), json!({"sessionID": sid, "cwd": cwd}), "{e}");
    }
    assert_eq!(stdin_of(&r, "stdin-workspace-pre-tool.json"), json!({"sessionID": sid, "tool_name": "Agent", "cwd": cwd}));
    for n in ["sid1", "sid2", "cwd1", "cwd2"] {
        assert!(!pwned(n).exists(), "{n}");
    }
}

/// `p1_calls()`, then `more`, then the closing `pre-tool A` (plan D7).
#[cfg(unix)]
fn after_p1(more: &[&str]) -> Vec<String> {
    let mut v = p1_calls();
    v.extend(more.iter().map(|s| s.to_string()));
    v.push("pre-tool A".to_string());
    v
}

/// `p1_events()`, then `more`.
#[cfg(unix)]
fn p1_then(more: Vec<Value>) -> Value {
    let mut v = p1_events();
    v.extend(more);
    json!(v)
}

#[cfg(unix)]
#[test] // W§18 #90 (1), AU27, AU104: the load call, then the main session and its turn
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p01_run_fresh() {
    if no_node("plugin_order_p01_run_fresh") {
        return;
    }
    let fx = fx();
    let r = ws_node(&fx, ws_opts("p01", Some(M), "ok", "A", json!(p1_events())));
    assert_eq!(ws_seq(&r), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
    assert_eq!(ws_log(&r)[0], r#"session {"cwd":"/r"}"#);
    // the load call needs no event
    assert_eq!(order(&fx, "p01-empty", Some(M), json!([])), ["session{}", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (2): a resumed session with no `info` is never sent
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p02_resume_no_info() {
    if no_node("plugin_order_p02_resume_no_info") {
        return;
    }
    let fx = fx();
    assert_eq!(order(&fx, "p02", Some(M), json!([busy("A"), idle("A")])), ["session{}", "prompt A", "stop A", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (3): an `info` in the turn classifies the active session as top
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p03_resume_info_in_turn() {
    if no_node("plugin_order_p03_resume_info_in_turn") {
        return;
    }
    let fx = fx();
    assert_eq!(
        order(&fx, "p03", Some(M), json!([busy("A"), updated("A"), idle("A")])),
        ["session{}", "prompt A", "session A", "stop A", "pre-tool A"]
    );
}

#[cfg(unix)]
#[test] // W§18 #90 (4)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p04_child_normal_order() {
    if no_node("plugin_order_p04_child_normal_order") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), created_child("K", "A"), busy("K"), idle("K"), idle("A")]);
    assert_eq!(order(&fx, "p04", Some(M), ev), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (5): a child whose `busy` comes before its `info`
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p05_late_child() {
    if no_node("plugin_order_p05_late_child") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), busy("K"), created_child("K", "A"), idle("K"), idle("A")]);
    assert_eq!(order(&fx, "p05", Some(M), ev), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (6)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p06_two_late_children() {
    if no_node("plugin_order_p06_two_late_children") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), busy("K1"), busy("K2"), created_child("K1", "A"), created_child("K2", "A"), idle("A")]);
    assert_eq!(order(&fx, "p06", Some(M), ev), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (7)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p07_child_after_its_idle() {
    if no_node("plugin_order_p07_child_after_its_idle") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), busy("K"), idle("K"), created_child("K", "A"), idle("A")]);
    assert_eq!(order(&fx, "p07", Some(M), ev), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (8): a child is never sent in a `session` call
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p08_late_child_alone() {
    if no_node("plugin_order_p08_late_child_alone") {
        return;
    }
    let fx = fx();
    assert_eq!(order(&fx, "p08", Some(M), json!([busy("K"), created_child("K", "P")])), ["session{}", "prompt K", "stop K", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (9)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p09_tui_switch_with_info() {
    if no_node("plugin_order_p09_tui_switch_with_info") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![busy("B"), updated("B"), idle("B")]);
    assert_eq!(order(&fx, "p09", Some(M), ev), after_p1(&["prompt B", "session B", "stop B"]));
}

#[cfg(unix)]
#[test] // W§18 #90 (10), AU27 known limit 1
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p10_tui_switch_no_info() {
    if no_node("plugin_order_p10_tui_switch_no_info") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![busy("B"), idle("B")]);
    assert_eq!(order(&fx, "p10", Some(M), ev), after_p1(&["prompt B", "stop B"]));
}

#[cfg(unix)]
#[test] // W§18 #90 (11): one turn set over two top sessions
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p11_overlapping_turns() {
    if no_node("plugin_order_p11_overlapping_turns") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), created("B"), busy("B"), idle("A"), idle("B")]);
    assert_eq!(order(&fx, "p11", Some(M), ev), ["session{}", "session A", "prompt A", "session B", "stop B", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (12): a late `info` of an older session does not take the main session
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p12_late_info_of_an_older_session() {
    if no_node("plugin_order_p12_late_info_of_an_older_session") {
        return;
    }
    let fx = fx();
    let ev = json!([busy("Y"), created("X"), busy("X"), updated("Y")]);
    assert_eq!(order(&fx, "p12", Some(M), ev), ["session{}", "prompt Y", "session X", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (13): an `info` event does not activate
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p13_info_event_does_not_activate() {
    if no_node("plugin_order_p13_info_event_does_not_activate") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![created("B"), deleted("A")]);
    assert_eq!(order(&fx, "p13", Some(M), ev), after_p1(&["session B"]));
}

#[cfg(unix)]
#[test] // W§18 #90 (14), W-C6: not a member, no workspace call
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p14_not_a_member() {
    if no_node("plugin_order_p14_not_a_member") {
        return;
    }
    let fx = fx();
    let r = ws_node(&fx, ws_opts("p14", None, "ok", "A", json!(p1_events())));
    assert!(ws_seq(&r).is_empty());
    assert_eq!(r.calls.lines().collect::<Vec<_>>(), ["hook guard --agent opencode"]);
}

#[cfg(unix)]
#[test] // W§18 #90 (15): decides before any wait
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_order_p15_decides_before_waits() {
    if no_node("plugin_order_p15_decides_before_waits") {
        return;
    }
    let fx = fx();
    let r = ws_node(&fx, Ws { parallel: true, ..ws_opts("p15", Some(M), "slow", "A", json!(p1_events())) });
    assert_eq!(ws_seq(&r), after_p1(&[]));
}

#[cfg(unix)]
#[test] // AU27, W§9.1 (H3 review round 1, F1): a session with no `info` until its `session.deleted` is never recorded
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_session_never_takes_the_record() {
    if no_node("plugin_deleted_session_never_takes_the_record") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![busy("Y"), idle("Y"), deleted("Y")]);
    assert_eq!(order(&fx, "del-y", Some(M), ev), after_p1(&["prompt Y", "stop Y"]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 1, F1): a deleted main session moves the record to the newest known top session
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_main_moves_the_record_to_the_newest_top_session() {
    if no_node("plugin_deleted_main_moves_the_record_to_the_newest_top_session") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![created("B"), busy("B"), idle("B"), deleted("B")]);
    assert_eq!(order(&fx, "del-main", Some(M), ev), after_p1(&["session B", "prompt B", "stop B", "session A"]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 1, F1): a deleted main session with no other known top session sends nothing
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_main_with_no_other_top_session_sends_nothing() {
    if no_node("plugin_deleted_main_with_no_other_top_session_sends_nothing") {
        return;
    }
    let fx = fx();
    assert_eq!(order(&fx, "del-only", Some(M), p1_then(vec![deleted("A")])), after_p1(&[]));
    // a later top session is still taken and sent
    let ev = p1_then(vec![deleted("A"), created("B"), busy("B"), idle("B")]);
    assert_eq!(order(&fx, "del-only-new", Some(M), ev), after_p1(&["session B", "prompt B", "stop B"]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 1, F1): a deleted session leaves the turn set; `stop` when the set becomes empty
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_session_leaves_the_turn_set() {
    if no_node("plugin_deleted_session_leaves_the_turn_set") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), deleted("A")]);
    assert_eq!(order(&fx, "del-turn", Some(M), ev), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
    let ev = json!([created("A"), busy("A"), created("B"), busy("B"), deleted("B"), idle("A")]);
    assert_eq!(
        order(&fx, "del-turn-two", Some(M), ev),
        ["session{}", "session A", "prompt A", "session B", "session A", "stop A", "pre-tool A"]
    );
}

#[cfg(unix)]
#[test] // AU27, W§7.2 (H3 review round 1, F3): any present, non-null `parentID` makes a child
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_child_by_present_parent_id() {
    if no_node("plugin_child_by_present_parent_id") {
        return;
    }
    let fx = fx();
    for (name, parent) in
        [("child-empty", json!("")), ("child-zero", json!(0)), ("child-false", json!(false)), ("child-object", json!({}))]
    {
        let ev = json!([["session.created", {"info": {"id": "K", "parentID": parent}}], busy("K"), idle("K")]);
        assert_eq!(order(&fx, name, Some(M), ev), ["session{}", "pre-tool A"], "{name}");
    }
    // a null or a missing `parentID` is a top session
    let ev = json!([["session.created", {"info": {"id": "K", "parentID": null}}]]);
    assert_eq!(order(&fx, "child-null", Some(M), ev), ["session{}", "session K", "pre-tool A"]);
    assert_eq!(order(&fx, "child-missing", Some(M), json!([created("K")])), ["session{}", "session K", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 2, F-e): the delete of an older top session that is not the main session sends nothing
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_older_top_session_sends_nothing() {
    if no_node("plugin_deleted_older_top_session_sends_nothing") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![created("B"), busy("B"), idle("B"), deleted("A")]);
    assert_eq!(order(&fx, "del-older", Some(M), ev), after_p1(&["session B", "prompt B", "stop B"]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 2, F-e): the delete of a known child sends nothing
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_child_sends_nothing() {
    if no_node("plugin_deleted_child_sends_nothing") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), created_child("K", "A"), busy("K"), idle("K"), deleted("K"), idle("A")]);
    assert_eq!(order(&fx, "del-child", Some(M), ev), ["session{}", "session A", "prompt A", "stop A", "pre-tool A"]);
    // a deleted child stays a child: a later `busy` of it is not a turn
    let ev = p1_then(vec![created_child("K", "A"), deleted("K"), busy("K"), idle("K")]);
    assert_eq!(order(&fx, "del-child-busy", Some(M), ev), after_p1(&[]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 2, F-e): a second `session.deleted` of the same session sends nothing more
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_twice_sends_once() {
    if no_node("plugin_deleted_twice_sends_once") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![created("B"), busy("B"), idle("B"), deleted("B"), deleted("B")]);
    assert_eq!(order(&fx, "del-twice", Some(M), ev), after_p1(&["session B", "prompt B", "stop B", "session A"]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 2, F-e): a session deleted before any other event is never sent; its turn signals work
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_deleted_before_any_event_is_never_sent() {
    if no_node("plugin_deleted_before_any_event_is_never_sent") {
        return;
    }
    let fx = fx();
    let ev = json!([deleted("A"), created("A"), busy("A"), idle("A")]);
    assert_eq!(order(&fx, "del-first", Some(M), ev), ["session{}", "prompt A", "stop A", "pre-tool A"]);
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 2, F-e): `busy` and idle after the delete send `prompt` and `stop`, never `session`
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_turn_signals_of_a_deleted_session_still_work() {
    if no_node("plugin_turn_signals_of_a_deleted_session_still_work") {
        return;
    }
    let fx = fx();
    let ev = p1_then(vec![deleted("A"), busy("A"), idle("A")]);
    assert_eq!(order(&fx, "del-busy", Some(M), ev), after_p1(&["prompt A", "stop A"]));
}

#[cfg(unix)]
#[test] // AU27 (H3 review round 2, F-e): a deleted main session falls back past a session with no kind yet; that
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
// session becomes the main session when its `info` comes
fn plugin_deleted_main_skips_an_unclassified_session() {
    if no_node("plugin_deleted_main_skips_an_unclassified_session") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("X"), created("B"), deleted("B"), updated("X")]);
    assert_eq!(
        order(&fx, "del-unclassified", Some(M), ev),
        ["session{}", "session A", "prompt X", "session B", "session A", "session X", "pre-tool A"]
    );
}

/// The steps of W§18 #91 (1): a tool of a top session in its turn.
#[cfg(unix)]
fn t1_steps() -> Value {
    json!([created("A"), busy("A"), tool_before("bash", "A", "a1"), tool_after("bash", "A", "a1"), idle("A")])
}

/// The call list of W§18 #91 (1).
#[cfg(unix)]
const T1_CALLS: [&str; 7] = ["session{}", "session A", "prompt A", "pre-tool A bash", "tool-end A", "stop A", "pre-tool A Agent"];

/// The guard, trim, work and closing guard calls of the steps of W§18 #91 (1).
#[cfg(unix)]
const T1_OTHER: [&str; 4] =
    ["hook guard --agent opencode", "hook trim --agent opencode", "hook work --agent opencode", "hook guard --agent opencode"];

#[cfg(unix)]
#[test] // W§18 #91 (1)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_top_session_counts_in_its_turn() {
    if no_node("plugin_tool_top_session_counts_in_its_turn") {
        return;
    }
    let fx = fx();
    let r = ws_node(&fx, ws_opts("t01", Some(M), "ok", "A", t1_steps()));
    assert_eq!(tool_seq(&r), T1_CALLS);
    let log = ws_log(&r);
    assert!(log.contains(&r#"pre-tool {"sessionID":"A","tool_name":"bash","cwd":"/r"}"#.to_string()), "{log:?}");
    assert!(log.contains(&r#"tool-end {"sessionID":"A","cwd":"/r"}"#.to_string()), "{log:?}");
    assert_eq!(r.calls.lines().filter(|l| !l.starts_with("hook workspace")).collect::<Vec<_>>(), T1_OTHER);
    assert_eq!(r.result.unwrap()["stepThrew"], json!([null, null]));
}

#[cfg(unix)]
#[test] // W§18 #91 (2)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_unclassified_session_counts() {
    if no_node("plugin_tool_unclassified_session_counts") {
        return;
    }
    let fx = fx();
    let ev = json!([busy("X"), tool_before("bash", "X", "x1"), tool_after("bash", "X", "x1"), idle("X")]);
    assert_eq!(
        tool_order(&fx, "t02", Some(M), "ok", ev),
        ["session{}", "prompt X", "pre-tool X bash", "tool-end X", "stop X", "pre-tool A Agent"]
    );
}

#[cfg(unix)]
#[test] // W§18 #91 (3)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_known_child_sends_nothing() {
    if no_node("plugin_tool_known_child_sends_nothing") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), created_child("K", "A"), tool_before("bash", "K", "k1"), tool_after("bash", "K", "k1")]);
    assert_eq!(tool_order(&fx, "t03", Some(M), "ok", ev), ["session{}", "session A", "pre-tool A Agent"]);
}

#[cfg(unix)]
#[test] // W§18 #91 (4)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_end_after_the_session_becomes_a_child() {
    if no_node("plugin_tool_end_after_the_session_becomes_a_child") {
        return;
    }
    let fx = fx();
    let ev = json!([tool_before("bash", "K", "k1"), created_child("K", "A"), tool_after("bash", "K", "k1")]);
    assert_eq!(tool_order(&fx, "t04", Some(M), "ok", ev), ["session{}", "pre-tool K bash", "tool-end K", "pre-tool A Agent"]);
}

#[cfg(unix)]
#[test] // W§18 #91 (5)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_end_needs_a_counted_call() {
    if no_node("plugin_tool_end_needs_a_counted_call") {
        return;
    }
    let fx = fx();
    let ev = json!([tool_after("bash", "A", "z9")]);
    assert_eq!(tool_order(&fx, "t05-none", Some(M), "ok", ev), ["session{}", "pre-tool A Agent"]);
    let ev = json!([tool_before("bash", "A", ""), tool_after("bash", "A", "")]);
    assert_eq!(tool_order(&fx, "t05-empty", Some(M), "ok", ev), ["session{}", "pre-tool A bash", "pre-tool A Agent"]);
    let ev = json!([tool_before("bash", "A", "a1"), tool_after("bash", "A", "a1"), tool_after("bash", "A", "a1")]);
    assert_eq!(
        tool_order(&fx, "t05-twice", Some(M), "ok", ev),
        ["session{}", "pre-tool A bash", "tool-end A", "pre-tool A Agent"]
    );
}

#[cfg(unix)]
#[test] // W§18 #91 (6)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_task_counts_and_ends_in_any_session() {
    if no_node("plugin_tool_task_counts_and_ends_in_any_session") {
        return;
    }
    let fx = fx();
    let ev = json!([
        tool_before("task", "A", "t1"),
        tool_after("task", "A", "t1"),
        created("A"),
        created_child("K", "A"),
        tool_before("task", "K", "t2"),
        tool_after("task", "K", "t2")
    ]);
    let r = ws_node(&fx, ws_opts("t06-ok", Some(M), "ok", "A", ev));
    assert_eq!(
        tool_seq(&r),
        ["session{}", "pre-tool A Agent", "tool-end A", "session A", "pre-tool K Agent", "tool-end K", "pre-tool A Agent"]
    );
    assert_eq!(r.result.unwrap()["stepThrew"], json!([null, null, null, null]));
    let ev = json!([
        tool_before("task", "A", "t1"),
        tool_after("task", "A", "t1"),
        tool_before("bash", "A", "b1"),
        tool_after("bash", "A", "b1")
    ]);
    let r = ws_node(&fx, ws_opts("t06-deny", Some(M), "deny", "A", ev));
    assert_eq!(tool_seq(&r), ["session{}", "pre-tool A Agent", "pre-tool A bash", "tool-end A", "pre-tool A Agent"]);
    let res = r.result.unwrap();
    assert_eq!(res["stepThrew"], json!(["R2", null, null, null]));
    assert_eq!(res["threw"], "R2");
}

#[cfg(unix)]
#[test] // W§18 #91 (7)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_prompt_after_a_tool_with_no_end() {
    if no_node("plugin_tool_prompt_after_a_tool_with_no_end") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), busy("A"), tool_before("bash", "A", "a1"), idle("A"), busy("A")]);
    assert_eq!(
        tool_order(&fx, "t07", Some(M), "ok", ev),
        ["session{}", "session A", "prompt A", "pre-tool A bash", "stop A", "prompt A", "pre-tool A Agent"]
    );
}

#[cfg(unix)]
#[test] // W§18 #91 (8)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_events_need_a_member() {
    if no_node("plugin_tool_events_need_a_member") {
        return;
    }
    let fx = fx();
    for (name, member) in [("t08-none", None), ("t08-empty", Some(""))] {
        let r = ws_node(&fx, ws_opts(name, member, "ok", "A", t1_steps()));
        assert!(tool_seq(&r).is_empty(), "{name}");
        assert_eq!(r.calls.lines().collect::<Vec<_>>(), T1_OTHER, "{name}");
    }
}

#[cfg(unix)]
#[test] // W§18 #91 (9)
#[ignore = "timing: asserts elapsed time against the plugin's 10 s timeout"]
fn plugin_tool_calls_keep_order_when_zygo_fails_or_hangs() {
    if no_node("plugin_tool_calls_keep_order_when_zygo_fails_or_hangs") {
        return;
    }
    let fx = fx();
    let r = ws_node(&fx, ws_opts("t09-fail", Some(M), "fail", "A", t1_steps()));
    assert_eq!(tool_seq(&r), T1_CALLS);
    let res = r.result.unwrap();
    assert_eq!(res["stepThrew"], json!([null, null]));
    assert_eq!(res["threw"], Value::Null);
    let r = ws_node(&fx, ws_opts("t09-hang", Some(M), "hangpre", "A", t1_steps()));
    assert_eq!(tool_seq(&r), T1_CALLS);
    let res = r.result.unwrap();
    assert_eq!(res["threw"], Value::Null);
    let event_ms = res["eventMs"].as_u64().unwrap();
    assert!(event_ms < 2000, "eventMs {event_ms}");
    let tool_ms = res["toolMs"].as_u64().unwrap();
    assert!((19_000..30_000).contains(&tool_ms), "toolMs {tool_ms}");
}

#[cfg(unix)]
#[test] // W§18 #91 (10)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_two_open_calls_each_end() {
    if no_node("plugin_tool_two_open_calls_each_end") {
        return;
    }
    let fx = fx();
    let want = ["session{}", "pre-tool A bash", "pre-tool A read", "tool-end A", "tool-end A", "pre-tool A Agent"];
    let ev = json!([
        tool_before("bash", "A", "a1"),
        tool_before("read", "A", "a2"),
        tool_after("bash", "A", "a1"),
        tool_after("read", "A", "a2")
    ]);
    assert_eq!(tool_order(&fx, "t10-in-order", Some(M), "ok", ev), want);
    let ev = json!([
        tool_before("bash", "A", "a1"),
        tool_before("read", "A", "a2"),
        tool_after("read", "A", "a2"),
        tool_after("bash", "A", "a1")
    ]);
    assert_eq!(tool_order(&fx, "t10-reversed", Some(M), "ok", ev), want);
}

#[cfg(unix)]
#[test] // W§18 #91 (11)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_reset_forgets_the_session_calls() {
    if no_node("plugin_tool_reset_forgets_the_session_calls") {
        return;
    }
    let fx = fx();
    let ev = json!([
        busy("A"),
        tool_before("bash", "A", "a1"),
        idle("A"),
        busy("A"),
        tool_before("bash", "A", "a2"),
        tool_after("bash", "A", "a1"),
        tool_after("bash", "A", "a2")
    ]);
    assert_eq!(
        tool_order(&fx, "t11-stop", Some(M), "ok", ev),
        ["session{}", "prompt A", "pre-tool A bash", "stop A", "prompt A", "pre-tool A bash", "tool-end A", "pre-tool A Agent"]
    );
    let ev = json!([tool_before("bash", "A", "a1"), busy("A"), tool_after("bash", "A", "a1")]);
    assert_eq!(
        tool_order(&fx, "t11-prompt", Some(M), "ok", ev),
        ["session{}", "pre-tool A bash", "prompt A", "pre-tool A Agent"]
    );
    // plan review 7(c): a `stop` forgets the counted calls of every session
    let ev = json!([
        busy("A"),
        tool_before("bash", "K", "k1"),
        idle("A"),
        busy("A"),
        tool_before("bash", "A", "a2"),
        tool_after("bash", "K", "k1"),
        tool_after("bash", "A", "a2")
    ]);
    assert_eq!(
        tool_order(&fx, "t11-other", Some(M), "ok", ev),
        ["session{}", "prompt A", "pre-tool K bash", "stop A", "prompt A", "pre-tool A bash", "tool-end A", "pre-tool A Agent"]
    );
}

#[cfg(unix)]
#[test] // W§18 #91 (12)
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_same_call_id_in_two_sessions() {
    if no_node("plugin_tool_same_call_id_in_two_sessions") {
        return;
    }
    let fx = fx();
    let ev = json!([
        tool_before("bash", "A", "c1"),
        tool_before("bash", "B", "c1"),
        tool_after("bash", "B", "c1"),
        tool_after("bash", "A", "c1")
    ]);
    assert_eq!(
        tool_order(&fx, "t12", Some(M), "ok", ev),
        ["session{}", "pre-tool A bash", "pre-tool B bash", "tool-end B", "tool-end A", "pre-tool A Agent"]
    );
}

#[cfg(unix)]
#[test] // AU27 tool count: a `task` call is counted when its `pre-tool` is queued, before the wait
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_task_counted_before_its_wait() {
    if no_node("plugin_tool_task_counted_before_its_wait") {
        return;
    }
    let fx = fx();
    // The `stop` is queued while the `task` waits for its `pre-tool` answer; it forgets the `task` call.
    let ev = json!([
        busy("A"),
        tool_before("bash", "A", "x1"),
        start(tool_before("task", "A", "t1")),
        idle("A"),
        join(),
        tool_after("task", "A", "t1")
    ]);
    let r = ws_node(&fx, ws_opts("t13", Some(M), "slowpre", "A", ev));
    assert_eq!(
        tool_seq(&r),
        ["session{}", "prompt A", "pre-tool A bash", "pre-tool A Agent", "stop A", "pre-tool A Agent"]
    );
    assert_eq!(r.result.unwrap()["stepThrew"], json!([null, null, null]));
}

#[cfg(unix)]
#[test] // AU27 tool count: a `stop` forgets the counted calls with no `prompt` after it
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_stop_forgets_the_calls() {
    if no_node("plugin_tool_stop_forgets_the_calls") {
        return;
    }
    let fx = fx();
    let ev = json!([busy("A"), tool_before("bash", "A", "a1"), idle("A"), tool_after("bash", "A", "a1")]);
    assert_eq!(
        tool_order(&fx, "t14", Some(M), "ok", ev),
        ["session{}", "prompt A", "pre-tool A bash", "stop A", "pre-tool A Agent"]
    );
}

#[cfg(unix)]
#[test] // AU27 tool count, W§9.1: the `pre-tool` of a tool is queued before the guard runs
#[ignore = "timing: a 0.3 s sleep orders two concurrent processes"]
fn plugin_tool_pre_tool_queued_before_the_guard() {
    if no_node("plugin_tool_pre_tool_queued_before_the_guard") {
        return;
    }
    let fx = fx();
    // The guard of `guarddeny` writes its line 0.3 s after it starts, so a `pre-tool` queued before the guard runs
    // writes its line first; a `pre-tool` queued after the guard answers writes its line after the guard line.
    let r = ws_node(&fx, ws_opts("t15", Some(M), "guarddeny", "A", json!([tool_before("bash", "A", "b1")])));
    assert_eq!(
        r.calls.lines().collect::<Vec<_>>(),
        [ws("session"), ws("pre-tool"), "hook guard --agent opencode".to_string(), ws("pre-tool"), "hook guard --agent opencode".to_string()]
    );
    let res = r.result.unwrap();
    assert_eq!(res["stepThrew"], json!(["R4"]));
    assert_eq!(res["threw"], "R4");
}

#[cfg(unix)]
#[test] // AU27 known limit (6): a call that the guard denies sends no `tool-end`; the count stays until the `stop`
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_guard_deny_sends_no_tool_end() {
    if no_node("plugin_tool_guard_deny_sends_no_tool_end") {
        return;
    }
    let fx = fx();
    let r = ws_node(&fx, ws_opts("t16", Some(M), "guarddeny", "A", json!([busy("A"), tool_before("bash", "A", "b1"), idle("A")])));
    assert_eq!(tool_seq(&r), ["session{}", "prompt A", "pre-tool A bash", "stop A", "pre-tool A Agent"]);
    assert_eq!(r.result.unwrap()["stepThrew"], json!(["R4"]));
}

#[cfg(unix)]
#[test] // AU27 tool count: a denied `task` of a known child is not counted
#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]
fn plugin_tool_denied_task_of_a_child_is_not_counted() {
    if no_node("plugin_tool_denied_task_of_a_child_is_not_counted") {
        return;
    }
    let fx = fx();
    let ev = json!([created("A"), created_child("K", "A"), tool_before("task", "K", "t2"), tool_after("task", "K", "t2")]);
    let r = ws_node(&fx, ws_opts("t17", Some(M), "deny", "A", ev));
    assert_eq!(tool_seq(&r), ["session{}", "session A", "pre-tool K Agent", "pre-tool A Agent"]);
    let res = r.result.unwrap();
    assert_eq!(res["stepThrew"], json!(["R2", null]));
    assert_eq!(res["threw"], "R2");
}

#[test] // M5 round 3 finding 1: `new` takes the zygo binary that the plugin and the hooks run
fn new_takes_the_given_binary() {
    let z = PathBuf::from("/opt/zygo/bin/zygo");
    assert_eq!(OpenCode::new(z.clone()).zygo, z);
}
