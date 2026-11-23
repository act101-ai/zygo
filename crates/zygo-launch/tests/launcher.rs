//! N31: launcher core (§8.2, §8.4, §10.5, §16.1 A1). No process-environment change: every variable goes to a child.
//! The stub agent is this test binary itself (`stub_child`, ignored unless `ZYGO_TEST_OUT` is set).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zygo_config::{load_with_builtin, LoadContext, Merged};
use zygo_core::paths::Paths;
use zygo_core::session_view::{SessionView, WorkItemView};
use zygo_core::Agent;
use zygo_launch::claude_code::ClaudeCode;
use zygo_launch::env::{child_env_with, Env};
use zygo_launch::{AgentAdapter, ChildSpec, LaunchCtx, LaunchOpts, Launcher, Plugins, Preflight};
use zygo_proxy::client::{SessionClient, WindowArg};
use zygo_redact::act_lite_store::{store_path, Lookup};

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
    cwd: PathBuf,
    out: PathBuf,
    bin: PathBuf,
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
    let (cwd, out, bin) = (root.join("work"), root.join("out"), root.join("bin"));
    for d in [&h, &cwd, &out, &bin] {
        std::fs::create_dir_all(d).unwrap();
    }
    Fx { _t: t, root, paths, cwd, out, bin }
}

impl Fx {
    fn config(&self, text: &str) {
        std::fs::write(&self.paths.config_file, text).unwrap();
    }
    fn merged(&self) -> Merged {
        load_with_builtin(&LoadContext { paths: self.paths.clone(), repo: None, team: None }, builtin()).unwrap()
    }
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

fn env_of(rows: &[(&str, &str)]) -> Env {
    Env(rows.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect())
}

fn names(e: &Env) -> Vec<String> {
    let mut v: Vec<String> = e.0.iter().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

const PROVIDER_CONFIG: &str = r#"
[[model]]
id = "test-env"
[[model.provider]]
name = "p-env"
base_url = "https://example.invalid"
dialects = ["anthropic"]
upstream_model = "up-env"
key = "env:MYPROVIDER_KEY"
"#;

#[test]
fn denylist_removes_provider_and_routing_names() {
    let fx = fx();
    fx.config(PROVIDER_CONFIG);
    let denied = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "ZYGO_TEAM_TOKEN",
        "MYPROVIDER_KEY",
    ];
    let kept = ["GH_TOKEN", "PATH", "CLAUDE_CODE_MAX_OUTPUT_TOKENS", "anthropic_api_key"];
    let rows: Vec<(&str, &str)> = denied.iter().chain(&kept).map(|n| (*n, "v")).collect();
    let (child, removed) = child_env_with(&env_of(&rows), &fx.merged(), &[], false);
    assert_eq!(names(&child), ["CLAUDE_CODE_MAX_OUTPUT_TOKENS", "GH_TOKEN", "PATH", "anthropic_api_key"]);
    let mut want: Vec<String> = denied.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(removed, want);
}

#[test]
fn denylist_folds_case_with_windows_rules() {
    let fx = fx();
    fx.config(PROVIDER_CONFIG);
    let rows = [("Anthropic_Api_Key", "v"), ("openai_base_url", "v"), ("Zygo_Team_Token", "v"), ("myprovider_key", "v"), ("Gh_Token", "v")];
    let (child, removed) = child_env_with(&env_of(&rows), &fx.merged(), &[], true);
    assert_eq!(names(&child), ["Gh_Token"]);
    assert_eq!(removed, ["Anthropic_Api_Key", "Zygo_Team_Token", "myprovider_key", "openai_base_url"]);
}

#[test]
fn set_rows_replace_parent_values_and_are_not_reported() {
    let fx = fx();
    let parent = env_of(&[("ANTHROPIC_BASE_URL", "old"), ("zygo_session", "old")]);
    let set = [("ANTHROPIC_BASE_URL".to_string(), "new".to_string()), ("ZYGO_SESSION".to_string(), "s1".to_string())];
    let (child, removed) = child_env_with(&parent, &fx.merged(), &set, true);
    let rows: Vec<(String, String)> =
        child.0.iter().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    assert_eq!(rows.iter().filter(|(k, _)| k == "ANTHROPIC_BASE_URL").collect::<Vec<_>>(), [&("ANTHROPIC_BASE_URL".into(), "new".into())]);
    assert_eq!(rows.iter().filter(|(k, _)| k == "ZYGO_SESSION").collect::<Vec<_>>(), [&("ZYGO_SESSION".into(), "s1".into())]);
    assert!(!rows.iter().any(|(k, _)| k == "zygo_session"));
    assert!(removed.is_empty());
    let (child, removed) = child_env_with(&parent, &fx.merged(), &set, false);
    assert!(child.0.iter().any(|(k, v)| k == "zygo_session" && v == "old"));
    assert!(removed.is_empty());
}

#[test]
fn denylist_removes_the_claude_code_markers_and_the_member_names() { // AU64, AU58
    let fx = fx();
    let gone = ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_BRIDGE_SESSION_ID", "ZYGO_WORKSPACE", "ZYGO_PANE"];
    let kept = ["ZYGO_CONFIG", "CLAUDE_CODE_MAX_OUTPUT_TOKENS", "PATH"];
    let rows: Vec<(&str, &str)> = gone.iter().chain(&kept).map(|n| (*n, "v")).collect();
    let (child, removed) = child_env_with(&env_of(&rows), &fx.merged(), &[], false);
    assert_eq!(names(&child), ["CLAUDE_CODE_MAX_OUTPUT_TOKENS", "PATH", "ZYGO_CONFIG"]);
    assert_eq!(removed, ["CLAUDECODE", "CLAUDE_CODE_BRIDGE_SESSION_ID", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "ZYGO_PANE", "ZYGO_WORKSPACE"]);
    // A member launch sets both names: the set rows win and nothing is reported.
    let set = [("ZYGO_WORKSPACE".to_string(), "ws-1".to_string()), ("ZYGO_PANE".to_string(), "p-1".to_string())];
    let (child, removed) = child_env_with(&env_of(&[("ZYGO_WORKSPACE", "old"), ("zygo_pane", "old")]), &fx.merged(), &set, true);
    let rows: Vec<(String, String)> = child.0.iter().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    assert_eq!(rows, [("ZYGO_WORKSPACE".to_string(), "ws-1".to_string()), ("ZYGO_PANE".to_string(), "p-1".to_string())]);
    assert!(removed.is_empty());
}

#[test]
fn session_dir_log_is_next_to_the_token() {
    let p = PathBuf::from("s1");
    assert_eq!(zygo_core::session_dir::SessionDir::new(p.clone()).log(), p.join("session.log"));
}

// ---- launch fixture ----

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const STORE_VALUE: &str = "n31-store-value-1";

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn opts_at(fx: &Fx, out: &Path, mode: &str, extra: &[(&str, &str)]) -> LaunchOpts {
    let mut env = vec![
        (OsString::from("PATH"), OsString::from(&fx.bin)),
        (OsString::from("ACT_CONFIG_DIR"), OsString::from(fx.root.join("act"))),
        (OsString::from("ZYGO_TEST_OUT"), OsString::from(out)),
        (OsString::from("ZYGO_TEST_MODE"), OsString::from(mode)),
    ];
    env.extend(extra.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
    if cfg!(windows)
        && let Some(v) = std::env::var_os("SystemRoot")
    {
        env.push(("SystemRoot".into(), v));
    }
    let mut o = LaunchOpts::new(Env(env), fx.cwd.clone(), fx.paths.clone());
    o.builtin = Some(builtin());
    o.stderr = Some(Arc::default());
    o
}

fn opts(fx: &Fx, mode: &str, extra: &[(&str, &str)]) -> LaunchOpts {
    opts_at(fx, &fx.out, mode, extra)
}

#[derive(Debug, Clone, Default)]
struct Seen {
    effective_role: Option<String>,
    plugin_ids: Vec<String>,
    delivered: Option<bool>,
    compact: Option<WindowArg>,
    #[cfg_attr(not(unix), allow(dead_code))]
    agent_version: String,
    repo: Option<String>,
    work: Option<String>,
}

struct Stub {
    agent: Agent,
    program: String,
    picked: Option<String>,
    preflight: Option<Preflight>,
    panic_in_build: bool,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn stub() -> Stub {
    Stub {
        agent: Agent::ClaudeCode,
        program: std::env::current_exe().unwrap().display().to_string(),
        picked: None,
        preflight: None,
        panic_in_build: false,
        seen: Arc::default(),
    }
}

impl Stub {
    fn last(&self) -> Seen {
        self.seen.lock().unwrap().last().cloned().expect("build was called")
    }
}

impl AgentAdapter for Stub {
    fn agent(&self) -> Agent {
        self.agent
    }
    fn program(&self) -> &str {
        &self.program
    }
    fn install_hint(&self) -> &str {
        "install the stub agent: TEST-HINT"
    }
    fn picked_model(&self, _args: &[String]) -> Option<String> {
        self.picked.clone()
    }
    fn build(&self, ctx: &LaunchCtx) -> anyhow::Result<ChildSpec> {
        assert!(!self.panic_in_build, "stub: build panics");
        let (plugin_ids, delivered) = match &ctx.plugins {
            Plugins::Snapshot(s) => (s.plugins.iter().map(|p| format!("{}:{}", p.marketplace, p.name)).collect(), None),
            Plugins::Delivered(d) => {
                let ids: Vec<String> = d.iter().flat_map(|d| &d.plugins).map(|p| p.id.to_string()).collect();
                let has = ids.iter().any(|i| i == "act101:act-lite");
                (ids, Some(has))
            }
        };
        self.seen.lock().unwrap().push(Seen {
            effective_role: ctx.effective_role.clone(),
            plugin_ids,
            delivered,
            compact: ctx.compact,
            agent_version: ctx.agent_version.clone(),
            repo: ctx.repo.clone(),
            work: ctx.work.as_ref().map(|w| format!("{}:{}", w.source, w.id)),
        });
        let args = ["--exact", "stub_child", "--ignored", "--nocapture", "--test-threads=1"];
        Ok(ChildSpec {
            args: args.map(String::from).to_vec(),
            env: vec![("ANTHROPIC_BASE_URL".into(), "http://stub.invalid/anthropic".into())],
            lines: vec!["stub: built".into()],
            log: vec!["stub: degraded --bare".into()],
        })
    }
    fn preflight(&self, _ctx: &LaunchCtx) -> Result<(), Preflight> {
        self.preflight.clone().map_or(Ok(()), Err)
    }
}

/// Exit code and captured stderr; the out directory is emptied first.
fn run(stub: &Stub, o: LaunchOpts) -> (i32, String) {
    let out = PathBuf::from(o.env.get("ZYGO_TEST_OUT").unwrap());
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let buf = o.stderr.clone().unwrap();
    let code = Launcher::run(stub, o).code;
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    (code, err)
}

fn env_json_at(out: &Path) -> BTreeMap<String, String> {
    serde_json::from_slice(&std::fs::read(out.join("env.json")).expect("the child ran")).unwrap()
}
fn env_json(fx: &Fx) -> BTreeMap<String, String> {
    env_json_at(&fx.out)
}
fn view(fx: &Fx) -> SessionView {
    serde_json::from_slice(&std::fs::read(fx.out.join("view.json")).unwrap()).unwrap()
}
fn log(fx: &Fx) -> String {
    std::fs::read_to_string(fx.out.join("session.log")).unwrap_or_default()
}
fn status_at(out: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(out.join("status.json")).unwrap()).unwrap()
}
fn status(fx: &Fx) -> serde_json::Value {
    status_at(&fx.out)
}

/// A test-side git call: no system or global config.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", NULL)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git on PATH");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn git_repo(dir: &Path, branch: &str, remote: &str) {
    git(dir, &["init", "-q", "-b", branch]);
    git(dir, &["config", "remote.origin.url", remote]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
}

#[cfg(unix)]
fn script(fx: &Fx, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = fx.bin.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn write(p: &Path, bytes: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// A local `act101` marketplace with an `act-lite` plugin whose `.mcp.json` runs `act-lite mcp serve`.
fn act101_market(fx: &Fx) -> PathBuf {
    let r = fx.root.join("act101");
    write(
        &r.join(".claude-plugin/marketplace.json"),
        r#"{"name":"act101","owner":{"name":"Owner"},"plugins":[{"name":"act-lite","source":"./plugins/act-lite","description":"Demo","version":"1.2.3"}]}"#,
    );
    write(
        &r.join("plugins/act-lite/.claude-plugin/plugin.json"),
        r#"{"name":"act-lite","version":"1.2.3","description":"Demo plugin","license":"MIT"}"#,
    );
    write(&r.join("plugins/act-lite/.mcp.json"), r#"{"mcpServers":{"act-lite":{"command":"act-lite","args":["mcp","serve"]}}}"#);
    git(&r, &["init", "-q", "-b", "main"]);
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "first"]);
    git(&r, &["tag", "v1.0.0"]);
    r
}

/// User config selects `act101:act-lite`; the store is synced and the plugin's code approved.
fn with_act_lite(fx: &Fx, extra_config: &str) {
    let r = act101_market(fx);
    let src = toml::Value::String(r.display().to_string());
    fx.config(&format!("[[marketplace]]\nname = \"act101\"\nsource = {src}\nref = \"v1.0.0\"\ndefault_on = [\"act-lite\"]\n{extra_config}"));
    let rep = zygo_plugins::sync::sync_all_in(&fx.merged(), &fx.paths, zygo_plugins::sync::TOTAL);
    let st = zygo_plugins::store::state(&fx.paths.plugin_store);
    let p = st.plugins.iter().find(|p| p.entry.plugin == "act-lite").unwrap_or_else(|| panic!("synced: {:?}", rep.lines));
    let digest = p.entry.digest.clone().expect("act-lite has code");
    zygo_plugins::approve::approve_in(&fx.paths.approvals_file, "act101", "act-lite", &digest).unwrap();
}

fn stub_act_lite(fx: &Fx) {
    std::fs::copy(std::env::current_exe().unwrap(), fx.bin.join(format!("act-lite{}", std::env::consts::EXE_SUFFIX))).unwrap();
}

fn act_lite_lookup(fx: &Fx) -> Lookup {
    let dir = fx.root.join("act");
    Lookup {
        env: Box::new(move |k| (k == "ACT_CONFIG_DIR").then(|| dir.clone().into_os_string())),
        home: None,
        known_config: None,
        windows: cfg!(windows),
    }
}

fn store_value(fx: &Fx) {
    write(&store_path(&fx.cwd, &act_lite_lookup(fx)), &format!("KEY={STORE_VALUE}\n"));
}

// ---- the stub agent ----

#[test]
#[ignore]
fn stub_child() {
    let Some(out) = std::env::var_os("ZYGO_TEST_OUT").map(PathBuf::from) else { return };
    let mode = std::env::var("ZYGO_TEST_MODE").unwrap_or_default();
    let vars: BTreeMap<String, String> =
        std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    std::fs::write(out.join("env.json"), serde_json::to_vec(&vars).unwrap()).unwrap();
    let dir = PathBuf::from(std::env::var_os("ZYGO_SESSION_DIR").unwrap());
    let sd = zygo_core::session_dir::SessionDir::new(dir.clone());
    let token = sd.read_token().unwrap();
    std::fs::write(out.join("token_in_env"), vars.values().any(|v| *v == token).to_string()).unwrap();
    let _ = std::fs::copy(sd.view_json(), out.join("view.json"));
    let _ = std::fs::copy(sd.log(), out.join("session.log"));
    let _ = std::fs::copy(sd.url(), out.join("url"));
    if mode == "peer" {
        std::fs::write(std::env::var_os("ZYGO_TEST_READY").unwrap(), "").unwrap();
        let peer = PathBuf::from(std::env::var_os("ZYGO_TEST_PEER").unwrap());
        let end = Instant::now() + Duration::from_secs(30);
        while !peer.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let st = SessionClient::from_session_dir(&dir).unwrap().status().unwrap();
    std::fs::write(out.join("status.json"), serde_json::to_vec(&st).unwrap()).unwrap();
    match mode.as_str() {
        "unit" => child_units(&sd),
        "request" => child_request(&out, &token),
        #[cfg(unix)]
        // The signal can arrive after `kill` returns: wait for it, never reach the exit below.
        "sigkill" => loop {
            unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
            std::thread::sleep(Duration::from_secs(1));
        },
        #[cfg(unix)]
        "sigint" => child_sigint(&out, &dir),
        #[cfg(unix)]
        "forward" => child_forward(&out),
        #[cfg(unix)]
        "inherit" => std::fs::write(out.join("inherited"), SIGS.map(disposition).join(" ")).unwrap(),
        #[cfg(windows)]
        "ctrlbreak" => child_ctrlbreak(&out, &dir),
        _ => {}
    }
    if let Some(ms) = std::env::var("ZYGO_TEST_SLEEP_MS").ok().and_then(|v| v.parse().ok()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
    let code = std::env::var("ZYGO_TEST_EXIT").ok().and_then(|c| c.parse().ok()).unwrap_or(0);
    std::process::exit(code);
}

fn child_units(sd: &zygo_core::session_dir::SessionDir) {
    use zygo_work::log::{lock, Begin, Kind, LogLine};
    let work = sd.work();
    std::fs::create_dir_all(&work).unwrap();
    let session = std::env::var("ZYGO_SESSION").unwrap();
    // a1's log lines are old: its `ended_at` must be the exit time, not the time of its last log line.
    for (agent, begun, stopped_at) in [("a1", now() - 1000, None), ("a2", now(), Some(1_000))] {
        let l = lock(&work, agent).unwrap();
        let offset = l.append(&LogLine { n: 0, ts: begun, kind: Kind::Begin { unit: "gh:1".into(), round: 1 } }).unwrap().unwrap();
        l.append(&LogLine { n: 0, ts: begun, kind: Kind::Call { tool: "Bash".into(), h: "h1".into() } }).unwrap();
        l.write_begin(Some(&Begin {
            unit: "gh:1".into(),
            round: 1,
            check: "cargo test".into(),
            files: vec![],
            began_at: now(),
            offset,
            session: session.clone(),
            agent_type: Some("zygo-code".into()),
            stopped_at,
            edit_root: None,
        }))
        .unwrap();
    }
    write(&sd.results().join("f0"), "x");
}

fn child_request(out: &Path, token: &str) {
    let url = zygo_proxy::Url::parse(&std::env::var("ZYGO_URL").unwrap()).unwrap();
    let body = format!(
        r#"{{"model":"test-up","max_tokens":1,"messages":[{{"role":"user","content":"key {STORE_VALUE}"}}]}}"#
    );
    let mut s = std::net::TcpStream::connect((url.host_str().unwrap(), url.port().unwrap())).unwrap();
    let session = std::env::var("ZYGO_SESSION").unwrap();
    write!(
        s,
        "POST {}anthropic/v1/messages HTTP/1.1\r\nhost: {}\r\ncontent-type: application/json\r\nanthropic-version: 2023-06-01\r\nx-api-key: {token}\r\nx-zygo-session: {session}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        url.path(),
        url.host_str().unwrap(),
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
    std::fs::write(out.join("reply"), reply.lines().next().unwrap_or_default()).unwrap();
}

// ---- launch tests ----

#[test]
fn child_gets_gh_token_and_the_zygo_variables() {
    let fx = fx();
    let st = stub();
    let extra = [
        ("GH_TOKEN", "gh-test-value"),
        ("ANTHROPIC_API_KEY", "sk-test-anthropic"),
        ("OPENAI_API_KEY", "sk-test-openai"),
        ("ZYGO_TEAM_TOKEN", "team-test"),
        ("ANTHROPIC_BASE_URL", "http://evil.invalid"),
    ];
    let (code, err) = run(&st, opts(&fx, "dump", &extra));
    assert_eq!(code, 0, "{err}");
    let e = env_json(&fx);
    assert_eq!(e["GH_TOKEN"], "gh-test-value");
    for n in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "ZYGO_TEAM_TOKEN"] {
        assert!(!e.contains_key(n), "{n}");
    }
    assert_eq!(e["ANTHROPIC_BASE_URL"], "http://stub.invalid/anthropic");
    assert_eq!(PathBuf::from(&e["ZYGO_SESSION_DIR"]), fx.paths.sessions_dir.join(&e["ZYGO_SESSION"]));
    assert_eq!(e["ZYGO_AGENT"], "claude-code");
    assert!(e["ZYGO_URL"].starts_with("http://127.0.0.1:"), "{}", e["ZYGO_URL"]);
    assert_eq!(e["ZYGO_URL"], std::fs::read_to_string(fx.out.join("url")).unwrap());
    assert_eq!(e["ZYGO_REPO"], "");
    assert_eq!(e["ZYGO_WORK"], "");
    // Only `opts.env` reaches the agent, never the launcher's own process environment.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(!e.contains_key("CARGO_MANIFEST_DIR"));
    assert_eq!(std::fs::read_to_string(fx.out.join("token_in_env")).unwrap(), "false");
    let l = log(&fx);
    assert!(l.lines().any(|x| x == "env: removed ANTHROPIC_API_KEY, OPENAI_API_KEY, ZYGO_TEAM_TOKEN"), "{l}");
    assert!(l.lines().any(|x| x == "stub: degraded --bare"), "{l}");
    for v in ["sk-test-anthropic", "sk-test-openai", "team-test"] {
        assert!(!l.contains(v));
    }
    assert!(err.contains("stub: built"), "{err}");
}

#[test]
fn invalid_config_exits_11() {
    let fx = fx();
    fx.config("[[[");
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 11);
    assert!(err.starts_with("zygo: "), "{err}");
    assert!(!fx.paths.sessions_dir.exists());
    assert!(!fx.out.join("env.json").exists());
}

#[test]
fn missing_agent_exits_13_with_the_hint() {
    let fx = fx();
    let mut st = stub();
    st.program = "zygo-test-no-such-agent".into();
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 13);
    assert!(err.contains("install the stub agent: TEST-HINT"), "{err}");
    assert!(!fx.paths.sessions_dir.exists());
}

#[test]
fn failed_preflight_exits_2_and_starts_nothing() {
    let fx = fx();
    let mut st = stub();
    let file = fx.root.join("settings.json");
    st.preflight = Some(Preflight { file: file.clone(), key: "env.ANTHROPIC_API_KEY".into() });
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 2);
    assert!(err.contains(&format!("zygo: {}: sets env.ANTHROPIC_API_KEY; zygo cannot launch with it", file.display())), "{err}");
    assert!(!fx.out.join("env.json").exists());
    assert_eq!(std::fs::read_dir(&fx.paths.sessions_dir).unwrap().count(), 0);
}

#[test]
fn agent_exit_code_is_returned() {
    let fx = fx();
    let (code, err) = run(&stub(), opts(&fx, "dump", &[("ZYGO_TEST_EXIT", "7")]));
    assert_eq!(code, 7, "{err}");
}

#[test]
fn exit_folds_open_units_and_keeps_only_results() {
    let fx = fx();
    let t0 = now();
    let (code, err) = run(&stub(), opts(&fx, "unit", &[]));
    assert_eq!(code, 0, "{err}");
    let rows = zygo_usage::Store::open(&fx.paths.usage_db).unwrap().work_units(&zygo_usage::Filter::default()).unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let a1 = rows.iter().find(|r| r.agent_id == "a1").unwrap();
    assert_eq!(a1.unit, "gh:1");
    assert_eq!(a1.tool_calls, 1);
    assert!(a1.ended_at.unwrap() >= t0);
    let a2 = rows.iter().find(|r| r.agent_id == "a2").unwrap();
    assert_eq!(a2.ended_at, Some(1000));
    let sd = fx.paths.sessions_dir.join(&env_json(&fx)["ZYGO_SESSION"]);
    let entries: Vec<String> = std::fs::read_dir(&sd).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(entries, ["results"]);
    assert!(sd.join("results/f0").exists());
}

#[test]
fn launch_prunes_old_usage_rows() {
    let fx = fx();
    std::fs::create_dir_all(&fx.paths.data_dir).unwrap();
    {
        let s = zygo_usage::Store::open(&fx.paths.usage_db).unwrap();
        let row = |ts, session: &str| zygo_usage::RequestRow { ts, session: session.into(), ..Default::default() };
        s.insert_request(&row(now() - 100 * 86_400, "old-s")).unwrap();
        s.insert_request(&row(now() - 86_400, "new-s")).unwrap();
    }
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    let s = zygo_usage::Store::open(&fx.paths.usage_db).unwrap();
    assert!(s.requests_for_session("old-s").unwrap().is_empty());
    assert_eq!(s.requests_for_session("new-s").unwrap().len(), 1);
}

#[test]
fn invalid_profile_prints_first_violation_and_launches_without_it() {
    let fx = fx();
    fx.config("[profile.bad]\ndisabled = [\"lead\"]\n");
    let mut o = opts(&fx, "dump", &[]);
    o.profile = Some("bad".into());
    let (code, err) = run(&stub(), o);
    assert_eq!(code, 0, "{err}");
    let line = "zygo: profile.bad.disabled[0]: is the main role and cannot be disabled; launching without the profile";
    assert_eq!(err.matches(line).count(), 1, "{err}");
    assert!(status(&fx)["profile"].is_null());
    let v = view(&fx);
    assert_eq!(v.profile, None);
    assert!(v.disabled_roles.is_empty());
}

#[test]
fn unknown_profile_prints_not_found_and_launches_without_it() {
    let fx = fx();
    let mut o = opts(&fx, "dump", &[]);
    o.profile = Some("ghost".into());
    let (code, err) = run(&stub(), o);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("zygo: profile ghost not found; launching without it"), "{err}");
    assert!(status(&fx)["profile"].is_null());
}

#[test]
fn valid_profile_and_settings_reach_the_view() {
    let fx = fx();
    fx.config(
        "[profile.ok]\ndisabled = [\"design\"]\nprompt = \"PROFILE-P\"\nfeatures = { work_units = false }\n[results]\ntrim_threshold_bytes = 20000\nhead_lines = 10\ntail_lines = 5\n[features]\ncompact_tool = false\n",
    );
    let mut o = opts(&fx, "dump", &[]);
    o.profile = Some("ok".into());
    let (code, err) = run(&stub(), o);
    assert_eq!(code, 0, "{err}");
    assert_eq!(status(&fx)["profile"], "ok");
    let v = view(&fx);
    assert_eq!(v.profile.as_deref(), Some("ok"));
    assert_eq!(v.profile_prompt.as_deref(), Some("PROFILE-P"));
    assert_eq!(v.disabled_roles, ["design"]);
    assert!(!v.work_units);
    assert!(!v.compact_tool);
    assert!(v.results);
    assert_eq!((v.trim_threshold_bytes, v.head_lines, v.tail_lines), (20000, 10, 5));
    assert!(!v.legend.iter().any(|l| l.role == "design"));
    assert!(v.legend.iter().any(|l| l.role == "code" && !l.custom));
}

#[test]
fn statusline_style_of_the_user_config_reaches_the_view() {
    // Statusline HUD design H§8 row Launch, acceptance 35. Both agents get the same handling.
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let fx = fx();
        let (code, err) = run(&Stub { agent, ..stub() }, opts(&fx, "dump", &[]));
        assert_eq!(code, 0, "{err}");
        assert_eq!(view(&fx).statusline_style, "compact", "{agent:?}: the default");
        fx.config("[statusline]\nstyle = \"verbose\"\n");
        let (code, err) = run(&Stub { agent, ..stub() }, opts(&fx, "dump", &[]));
        assert_eq!(code, 0, "{err}");
        assert_eq!(view(&fx).statusline_style, "verbose", "{agent:?}");
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(fx.out.join("view.json")).unwrap()).unwrap();
        assert_eq!(raw["statusline_style"], "verbose", "{agent:?}");
    }
}

// ---- HUD-9: H§14 #35 (launcher part) and the P-HUD-1 `refreshInterval` seam ----

#[test]
fn h14_35_user_verbose_style_is_written_to_view_json() { // H§14 #35 (launcher part): H§8 row Launch
    let fx = fx();
    fx.config("[statusline]\nstyle = \"verbose\"\n");
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    // The file as the launcher wrote it into the session directory (the stub child copies it out).
    let raw = json(&fx.out.join("view.json"));
    assert_eq!(raw["statusline_style"], "verbose", "{raw}");
    assert_eq!(view(&fx).statusline_style, "verbose");
}

/// The stub agent with the settings file of the real Claude Code adapter: `build` runs `ClaudeCode::build` on the
/// launch context first and keeps the `statusLine` object of the settings file it wrote into the session directory.
struct WithSettings {
    stub: Stub,
    status_line: Arc<Mutex<Option<serde_json::Value>>>,
}

impl AgentAdapter for WithSettings {
    fn agent(&self) -> Agent {
        self.stub.agent()
    }
    fn program(&self) -> &str {
        self.stub.program()
    }
    fn install_hint(&self) -> &str {
        self.stub.install_hint()
    }
    fn picked_model(&self, args: &[String]) -> Option<String> {
        self.stub.picked_model(args)
    }
    fn build(&self, ctx: &LaunchCtx) -> anyhow::Result<ChildSpec> {
        ClaudeCode::default().build(ctx)?;
        *self.status_line.lock().unwrap() = Some(json(&ctx.session_dir.settings_json())["statusLine"].clone());
        self.stub.build(ctx)
    }
    fn preflight(&self, ctx: &LaunchCtx) -> Result<(), Preflight> {
        self.stub.preflight(ctx)
    }
}

/// The `statusLine` object of the Claude Code settings of one launch with `extra` in the launch environment.
fn status_line_of_a_launch(extra: &[(&str, &str)]) -> serde_json::Value {
    let fx = fx();
    let adapter = WithSettings { stub: stub(), status_line: Arc::default() };
    let o = opts(&fx, "dump", extra);
    let _ = std::fs::remove_dir_all(&fx.out);
    std::fs::create_dir_all(&fx.out).unwrap();
    let buf = o.stderr.clone().unwrap();
    let code = Launcher::run(&adapter, o).code;
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&buf.lock().unwrap()));
    adapter.status_line.lock().unwrap().clone().expect("build was called")
}

#[test]
fn refresh_interval_seam_overrides_the_status_line_interval() { // HUD-9: P-HUD-1 seam `ZYGO_TEST_REFRESH_INTERVAL`
    // No variable: the default of 30 s.
    let s = status_line_of_a_launch(&[]);
    assert_eq!(s["refreshInterval"], 30, "{s}");
    assert!(s["command"].as_str().is_some_and(|c| c.ends_with(" hook statusline")), "{s}");
    // The variable reaches the launch through its environment parameter; the test process environment is not changed.
    let s = status_line_of_a_launch(&[("ZYGO_TEST_REFRESH_INTERVAL", "2")]);
    assert_eq!(s["refreshInterval"], 2, "{s}");
    assert!(s["refreshInterval"].is_u64(), "a JSON integer, as the default: {s}");
    // A value that is not a positive integer keeps the default.
    for bad in ["0", "-1", "2s", "1.5", ""] {
        let s = status_line_of_a_launch(&[("ZYGO_TEST_REFRESH_INTERVAL", bad)]);
        assert_eq!(s["refreshInterval"], 30, "{bad:?}: {s}");
    }
}

#[test]
fn concurrent_launches_get_separate_proxies() {
    let fx = fx();
    let (o1, o2) = (fx.root.join("out1"), fx.root.join("out2"));
    let (r1, r2) = (fx.root.join("ready-1"), fx.root.join("ready-2"));
    let (r1s, r2s) = (r1.display().to_string(), r2.display().to_string());
    let a = opts_at(&fx, &o1, "peer", &[("ZYGO_TEST_READY", &r1s), ("ZYGO_TEST_PEER", &r2s)]);
    let b = opts_at(&fx, &o2, "peer", &[("ZYGO_TEST_READY", &r2s), ("ZYGO_TEST_PEER", &r1s)]);
    let (sa, sb) = (stub(), stub());
    let (ca, cb) = std::thread::scope(|s| {
        let ta = s.spawn(|| run(&sa, a));
        let tb = s.spawn(|| run(&sb, b));
        (ta.join().unwrap(), tb.join().unwrap())
    });
    assert_eq!(ca.0, 0, "{}", ca.1);
    assert_eq!(cb.0, 0, "{}", cb.1);
    let (e1, e2) = (env_json_at(&o1), env_json_at(&o2));
    assert_ne!(e1["ZYGO_URL"], e2["ZYGO_URL"]);
    assert_ne!(e1["ZYGO_SESSION"], e2["ZYGO_SESSION"]);
    assert_eq!(status_at(&o1)["session"], e1["ZYGO_SESSION"].as_str());
    assert_eq!(status_at(&o2)["session"], e2["ZYGO_SESSION"].as_str());
}

#[test]
fn navigation_present_with_act_lite_selected_and_on_path() {
    let fx = fx();
    with_act_lite(&fx, "");
    stub_act_lite(&fx);
    let st = stub();
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(view(&fx).navigation, "{err}");
    assert!(st.last().plugin_ids.iter().any(|i| i == "act101:act-lite"));
}

#[test]
fn navigation_absent_without_act_lite_on_path() {
    let fx = fx();
    with_act_lite(&fx, "");
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(!view(&fx).navigation);
    assert!(err.contains("plugin act101:act-lite: act-lite not found"), "{err}");
}

#[test]
fn opencode_navigation_follows_the_delivered_plugin() {
    let fx = fx();
    with_act_lite(&fx, "");
    let mut st = stub();
    st.agent = Agent::OpenCode;
    stub_act_lite(&fx);
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(view(&fx).navigation);
    assert_eq!(st.last().delivered, Some(true));
    std::fs::remove_file(fx.bin.join(format!("act-lite{}", std::env::consts::EXE_SUFFIX))).unwrap();
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(!view(&fx).navigation);
}

#[test]
fn compact_flag_sets_the_override_before_the_agent_starts() {
    let fx = fx();
    let st = stub();
    let mut o = opts(&fx, "dump", &[]);
    o.compact = Some(WindowArg::Tokens(600_000));
    let (code, err) = run(&st, o);
    assert_eq!(code, 0, "{err}");
    assert_eq!(status(&fx)["compact"], 600000);
    assert_eq!(st.last().compact, Some(WindowArg::Tokens(600_000)));
}

#[test]
fn act_lite_store_missing_writes_one_session_log_line() {
    let fx = fx();
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    let l = log(&fx);
    assert_eq!(l.lines().filter(|x| x.starts_with("act-lite store")).count(), 1, "{l}");
    assert!(l.lines().any(|x| x.starts_with("act-lite store: no act-lite store at")), "{l}");
    store_value(&fx);
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    let l = log(&fx);
    assert!(!l.contains("act-lite store"), "{l}");
    assert!(!l.contains(STORE_VALUE));
    assert!(!std::fs::read_to_string(fx.out.join("env.json")).unwrap().contains(STORE_VALUE));
}

#[test]
fn effective_role_follows_the_picked_model() {
    let fx = fx();
    let lead_prompt = zygo_config::builtin::defaults().role["lead"].prompt.clone();
    let cases = [
        (None, "lead", Some("lead"), Some(lead_prompt)),
        (Some("code"), "code", Some("code"), Some(zygo_config::builtin::defaults().role["code"].prompt.clone())),
        (Some("claude-opus-5-5"), "", None, None),
    ];
    for (picked, env_role, eff, prompt) in cases {
        let mut st = stub();
        st.picked = picked.map(String::from);
        let (code, err) = run(&st, opts(&fx, "dump", &[]));
        assert_eq!(code, 0, "{err}");
        assert_eq!(env_json(&fx)["ZYGO_MAIN_ROLE"], env_role);
        let v = view(&fx);
        assert_eq!(v.effective_role.as_deref(), eff);
        assert_eq!(v.role_prompt, prompt);
        assert_eq!(st.last().effective_role.as_deref(), eff);
    }
}

#[test]
fn usage_db_that_does_not_open_prints_one_warning() {
    let fx = fx();
    std::fs::create_dir_all(&fx.paths.usage_db).unwrap();
    let o = opts(&fx, "dump", &[]);
    let (code, err) = run(&stub(), o);
    assert_eq!(code, 0, "{err}");
    assert_eq!(err.matches("zygo: usage rows off: the usage database did not open").count(), 1, "{err}");
}

#[cfg(unix)]
#[test]
fn agent_killed_by_a_signal_exits_128_plus_signal() {
    let fx = fx();
    let (code, err) = run(&stub(), opts(&fx, "sigkill", &[]));
    assert_eq!(code, 137, "{err}");
    assert!(err.trim_end().ends_with("stub: built"), "{err}");
}

/// One request on 127.0.0.1: its body is kept, the reply is a minimal Messages response.
fn mock_upstream() -> (u16, Arc<Mutex<Vec<u8>>>) {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let got: Arc<Mutex<Vec<u8>>> = Arc::default();
    let g = got.clone();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = l.accept() else { return };
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head_end, len) = loop {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                    .unwrap_or(0);
                break (i + 4, len);
            }
        };
        while buf.len() < head_end + len {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        *g.lock().unwrap() = buf[head_end..].to_vec();
        let reply = r#"{"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
    });
    (port, got)
}

#[test]
fn act_lite_store_values_are_redacted_by_the_session_proxy() {
    let fx = fx();
    let (port, got) = mock_upstream();
    fx.config(&format!(
        "[[model]]\nid = \"test-up\"\n[[model.provider]]\nname = \"mock\"\nbase_url = \"http://127.0.0.1:{port}\"\ndialects = [\"anthropic\"]\nupstream_model = \"test-up\"\n"
    ));
    store_value(&fx);
    let (code, err) = run(&stub(), opts(&fx, "request", &[]));
    assert_eq!(code, 0, "{err}");
    let reply = std::fs::read_to_string(fx.out.join("reply")).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    let body = String::from_utf8(got.lock().unwrap().clone()).unwrap();
    assert!(body.contains("[REDACTED:act_lite_secret]"), "{body}");
    assert!(!body.contains(STORE_VALUE));
}

#[test]
fn repo_and_work_item_reach_env_session_and_view() {
    let fx = fx();
    git_repo(&fx.cwd, "feat/n31.x", "https://user@GitHub.com/Acme/Widgets.git");
    fx.config("[work]\nsources = [\"branch\"]\n");
    let st = stub();
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    let e = env_json(&fx);
    assert_eq!(e["ZYGO_REPO"], "github.com/acme/widgets");
    assert_eq!(e["ZYGO_WORK"], "branch:feat/n31.x");
    let seen = st.last();
    assert_eq!(seen.repo.as_deref(), Some("github.com/acme/widgets"));
    assert_eq!(seen.work.as_deref(), Some("branch:feat/n31.x"));
    assert_eq!(status(&fx)["work"], "branch:feat/n31.x");
    assert_eq!(view(&fx).work_item, Some(WorkItemView { unit: "branch:feat-n31.x".into(), title: None }));
    assert!(!err.lines().any(|l| l.starts_with("zygo: ") && l.contains("work")), "{err}");

    git(&fx.cwd, &["checkout", "-q", "-b", "n31+x"]);
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("zygo: branch name is not a usable work item id; no item"), "{err}");
    let e = env_json(&fx);
    assert_eq!(e["ZYGO_WORK"], "");
    assert_eq!(e["ZYGO_REPO"], "github.com/acme/widgets");
    assert_eq!(view(&fx).work_item, None);
}

#[cfg(unix)]
#[test]
fn act_lite_version_other_than_the_compiled_list_is_logged() {
    let fx = fx();
    with_act_lite(&fx, "");
    script(&fx, "act-lite", "echo 'act-lite 9.9.9'");
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(view(&fx).navigation);
    let l = log(&fx);
    assert_eq!(l.lines().filter(|x| x.starts_with("act-lite:")).collect::<Vec<_>>(), ["act-lite: installed 9.9.9, language list for 1.2.3"]);

    script(&fx, "act-lite", "echo 'act-lite 1.2.3'");
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(!log(&fx).lines().any(|x| x.starts_with("act-lite:")));

    // Without the 2 s bound the probe alone would take 5 s.
    script(&fx, "act-lite", "exec /bin/sleep 5");
    let t = Instant::now();
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(!log(&fx).lines().any(|x| x.starts_with("act-lite:")));
}

#[cfg(unix)]
#[test]
fn agent_version_is_parsed_from_the_stub() {
    let fx = fx();
    let exe = std::env::current_exe().unwrap();
    let probe_env = fx.out.join("probe_env");
    // A denylisted name in the launcher's parent environment, value built at run time.
    let denied_value = ["probe", "value"].join("-");
    for (out, want) in [("echo '2.1.328 (Claude Code)'", "2.1.328"), ("echo 'no version here'", "")] {
        let body = format!(
            "[ \"$1\" = --version ] && {{ /usr/bin/env > '{}'; {out}; exit 0; }}; exec '{}' \"$@\"",
            probe_env.display(),
            exe.display()
        );
        let p = script(&fx, "stub-agent", &body);
        let mut st = stub();
        st.program = p.display().to_string();
        let (code, err) = run(&st, opts(&fx, "dump", &[("ANTHROPIC_API_KEY", &denied_value)]));
        assert_eq!(code, 0, "{err}");
        assert_eq!(st.last().agent_version, want);
        // The probe gets the parent `Env` after the denylist, never the launcher's process environment.
        let seen = std::fs::read_to_string(&probe_env).unwrap();
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        assert!(!seen.lines().any(|l| l.starts_with("CARGO_MANIFEST_DIR=")), "{seen}");
        assert!(!seen.lines().any(|l| l.starts_with("ANTHROPIC_API_KEY=")), "{seen}");
        assert!(seen.lines().any(|l| l.starts_with("ZYGO_TEST_MODE=")), "{seen}");
    }
}

#[cfg(unix)]
#[test]
fn agent_version_probe_is_bounded() { // §7 note 2026-11-19: 2 s, as the act-lite probe
    let fx = fx();
    let exe = std::env::current_exe().unwrap();
    let p = script(&fx, "stub-agent", &format!("[ \"$1\" = --version ] && exec /bin/sleep 30; exec '{}' \"$@\"", exe.display()));
    let mut st = stub();
    st.program = p.display().to_string();
    let t = Instant::now();
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert_eq!(st.last().agent_version, "");
} // mutation: the unbounded `wait_with_output` (30 s); the 2 s bound has an 18 s margin

#[cfg(unix)]
#[test]
fn act_lite_probe_does_not_wait_for_a_background_child() {
    let fx = fx();
    with_act_lite(&fx, "");
    // A background grandchild keeps stdout open for 8 s: the first line is the version, the whole probe is bounded by
    // 2 s (§10.5), and the grandchild is stopped with the probe. The pid is written before the version line, so the
    // probe cannot stop the script before it is written.
    let gc = fx.root.join("gc.pid");
    script(&fx, "act-lite", &format!("/bin/sleep 8 & echo $! > '{}'; echo 'act-lite 9.9.9'", gc.display()));
    let t = Instant::now();
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(view(&fx).navigation);
    let l = log(&fx);
    assert_eq!(l.lines().filter(|x| x.starts_with("act-lite:")).collect::<Vec<_>>(), ["act-lite: installed 9.9.9, language list for 1.2.3"]);
    let pid: i32 = std::fs::read_to_string(&gc).unwrap().trim().parse().unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    let gone = loop {
        let dead = unsafe { libc::kill(pid, 0) } == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if dead || Instant::now() >= end {
            break dead;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !gone {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(gone, "the probe's background child {pid} survived");
}

#[test]
fn panic_after_the_session_exists_removes_the_session_dir() {
    let fx = fx();
    let mut st = stub();
    st.panic_in_build = true;
    let o = opts(&fx, "dump", &[]);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Launcher::run(&st, o)));
    assert!(r.is_err());
    assert_eq!(std::fs::read_dir(&fx.paths.sessions_dir).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn act_lite_probe_that_leaves_its_group_is_still_stopped() {
    let perl = Path::new("/usr/bin/perl");
    if !perl.exists() {
        eprintln!("skipped: /usr/bin/perl is absent; this test needs it to leave the probe's process group");
        return;
    }
    let fx = fx();
    with_act_lite(&fx, "");
    // The probe moves itself into its parent's group, so the group kill misses it; it keeps running for 8 s.
    script(
        &fx,
        "act-lite",
        &format!(r#"exec {} -e '$|=1; setpgrp(0, getpgrp(getppid())); print "act-lite 9.9.9\n"; sleep 8'"#, perl.display()),
    );
    let t = Instant::now();
    let (code, err) = run(&stub(), opts(&fx, "dump", &[]));
    assert_eq!(code, 0, "{err}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(log(&fx).lines().any(|x| x == "act-lite: installed 9.9.9, language list for 1.2.3"), "{}", log(&fx));
}

// ---- signals (N57) ----

/// Each wait in the signal tests; generous, launches are slow under machine load (N58).
const SIG_BOUND: Duration = Duration::from_secs(60);

/// Polls `done` every 10 ms until it holds or `bound` passes; returns whether it held.
fn until(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + bound;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn json(p: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// A launch in its own process (`harness_launch`); unix: its own process group; Windows: its own console and group.
struct Harness {
    child: std::process::Child,
    #[cfg_attr(not(unix), allow(dead_code))]
    pid: u32,
    root: PathBuf,
    reaped: bool,
}

/// `ignored`: unix signals the harness starts with ignored; every other one of `SIGS` starts at default (F1).
fn harness(root: &Path, mode: &str, ignored: &[i32]) -> Harness {
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "harness_launch", "--ignored", "--nocapture", "--test-threads=1"])
        .env("ZYGO_TEST_HARNESS", root)
        .env("ZYGO_TEST_HARNESS_MODE", mode)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(root.join("harness-stderr")).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let ign = ignored.to_vec();
        c.process_group(0);
        // SAFETY: between fork and exec the closure calls only `signal`, which is async-signal-safe; this test
        // process is not changed.
        unsafe {
            c.pre_exec(move || {
                for s in SIGS {
                    libc::signal(s, libc::SIG_DFL);
                }
                for &s in &ign {
                    libc::signal(s, libc::SIG_IGN);
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = ignored;
        c.creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP);
    }
    let child = c.spawn().unwrap();
    Harness { pid: child.id(), child, root: root.to_path_buf(), reaped: false }
}

impl Harness {
    fn finish(&mut self) -> std::process::ExitStatus {
        let end = Instant::now() + 3 * SIG_BOUND;
        loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                self.reaped = true;
                return st;
            }
            assert!(Instant::now() < end, "the harness did not end; {}", self.log());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> String {
        let r = |n: &str| read_text(&self.root.join(n));
        format!("harness stderr:\n{}\nlauncher stderr:\n{}", r("harness-stderr"), r("launcher-stderr"))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(self.pid) {
            // SAFETY: `kill` takes plain integers; the harness leads its own group (`process_group(0)`) and is not
            // reaped yet, so the group id cannot name another process group.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The launch for the signal tests, in its own process; runs only when `ZYGO_TEST_HARNESS` is set.
#[test]
#[ignore]
fn harness_launch() {
    let Some(root) = std::env::var_os("ZYGO_TEST_HARNESS").map(PathBuf::from) else { return };
    #[cfg(unix)]
    std::fs::write(root.join("before"), SIGS.map(disposition).join(" ")).unwrap();
    let mode = std::env::var("ZYGO_TEST_HARNESS_MODE").unwrap();
    let fx = fx();
    let group = std::process::id().to_string();
    // `control`: the agent is the stub agent in control mode (it has no signal handler); every other mode: this binary.
    let (stub_out, ctl) = (root.join("stub-out"), root.join("ctl"));
    let (adapter, o) = if mode == "control" {
        let extra = [("ZYGO_TEST_GROUP", group.as_str()), ("ZYGO_STUB_OUT", stub_out.to_str().unwrap()), ("ZYGO_STUB_CONTROL", ctl.to_str().unwrap())];
        (Stub { program: env!("CARGO_BIN_EXE_stub-agent").to_string(), ..stub() }, opts_at(&fx, &root, &mode, &extra))
    } else {
        (stub(), opts_at(&fx, &root, &mode, &[("ZYGO_TEST_GROUP", &group)]))
    };
    let buf = o.stderr.clone().unwrap();
    let out = Launcher::run(&adapter, o);
    std::fs::write(root.join("launcher-stderr"), &*buf.lock().unwrap()).unwrap();
    #[cfg(unix)]
    std::fs::write(root.join("after"), SIGS.map(disposition).join(" ")).unwrap();
    std::fs::write(root.join("launcher-outcome"), format!("{} {:?} {} {}", out.code, out.signal, out.spawned, out.received)).unwrap();
    std::fs::write(root.join("launcher-exit"), out.code.to_string()).unwrap();
}

#[cfg(unix)]
const SIGS: [libc::c_int; 4] = [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP];
#[cfg(unix)]
static SEEN: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];

#[cfg(unix)]
extern "C" fn seen(sig: libc::c_int) {
    if let Some(i) = SIGS.iter().position(|&s| s == sig) {
        SEEN[i].fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(unix)]
fn seen_of(sig: libc::c_int) -> u32 {
    SEEN[SIGS.iter().position(|&s| s == sig).unwrap()].load(Ordering::SeqCst)
}

/// "default", "ignored" or "caught": this process's handling of `sig` now (sigaction with a null new action).
#[cfg(unix)]
fn disposition(sig: libc::c_int) -> &'static str {
    let mut a: libc::sigaction = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::sigaction(sig, std::ptr::null(), &mut a) }, 0);
    match a.sa_sigaction {
        libc::SIG_DFL => "default",
        libc::SIG_IGN => "ignored",
        _ => "caught",
    }
}

/// sigaction(sig, seen, SA_RESTART, empty mask) for each.
#[cfg(unix)]
fn catch(sigs: &[libc::c_int]) {
    for &s in sigs {
        let mut a: libc::sigaction = unsafe { std::mem::zeroed() };
        a.sa_sigaction = seen as extern "C" fn(libc::c_int) as libc::sighandler_t;
        a.sa_flags = libc::SA_RESTART;
        unsafe { libc::sigemptyset(&mut a.sa_mask) };
        assert_eq!(unsafe { libc::sigaction(s, &a, std::ptr::null_mut()) }, 0);
    }
}

/// `kill(target, sig)` == 0; `target` is the harness pid or its negation (the harness's group).
#[cfg(unix)]
fn send(target: i32, sig: libc::c_int) {
    assert_ne!(target.abs(), unsafe { libc::getpgrp() }, "a signal to the test runner's group");
    assert_eq!(unsafe { libc::kill(target, sig) }, 0, "kill({target}, {sig}): {}", std::io::Error::last_os_error());
}

#[cfg(unix)]
fn child_sigint(out: &Path, dir: &Path) {
    let w = |name: &str, text: String| std::fs::write(out.join(name), text).unwrap();
    w("inherited", format!("{} {}", disposition(libc::SIGINT), disposition(libc::SIGQUIT)));
    catch(&[libc::SIGINT, libc::SIGQUIT]);
    w("ready", String::new());
    // Phase 1: SIGINT and SIGQUIT to the launcher alone. Nothing may arrive here; the 2 s window lets a
    // wrongly forwarded signal arrive (load-bearing check a), and never decides a pass of correct code.
    until(SIG_BOUND, || out.join("sent1").exists());
    until(Duration::from_secs(2), || seen_of(libc::SIGINT) + seen_of(libc::SIGQUIT) > 0);
    w("direct", format!("{} {}", seen_of(libc::SIGINT), seen_of(libc::SIGQUIT)));
    w("ready2", String::new());
    // Phase 2: the terminal's Ctrl+C and Ctrl+\ to the whole group.
    until(SIG_BOUND, || seen_of(libc::SIGINT) >= 1 && seen_of(libc::SIGQUIT) >= 1);
    let st = SessionClient::from_session_dir(dir).unwrap().status().unwrap();
    std::fs::write(out.join("status-after.json"), serde_json::to_vec(&st).unwrap()).unwrap();
    w("counts", format!("{} {}", seen_of(libc::SIGINT), seen_of(libc::SIGQUIT)));
}

#[cfg(unix)]
fn child_forward(out: &Path) -> ! {
    catch(&[libc::SIGTERM, libc::SIGHUP]);
    std::fs::write(out.join("ready"), "").unwrap();
    // `seen` = "<term> <hup>", rewritten on each change, so the parent can see each receipt (F5).
    let mut last = String::new();
    until(SIG_BOUND, || {
        let now = format!("{} {}", seen_of(libc::SIGTERM), seen_of(libc::SIGHUP));
        if now != last {
            std::fs::write(out.join("seen"), &now).unwrap();
            last = now;
        }
        seen_of(libc::SIGTERM) >= 1 && seen_of(libc::SIGHUP) >= 1
    });
    std::fs::write(out.join("forward"), format!("{} {}", seen_of(libc::SIGTERM), seen_of(libc::SIGHUP))).unwrap();
    std::process::exit(3);
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(routine: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
}

#[cfg(windows)]
static BREAKS: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
extern "system" fn on_break(ctrl: u32) -> i32 {
    if ctrl == 1 {
        BREAKS.fetch_add(1, Ordering::SeqCst);
        1
    } else {
        0
    }
}

#[cfg(windows)]
fn child_ctrlbreak(out: &Path, dir: &Path) {
    unsafe { SetConsoleCtrlHandler(Some(on_break), 1) };
    let group: u32 = std::env::var("ZYGO_TEST_GROUP").unwrap().parse().unwrap();
    // CTRL_BREAK_EVENT (1) to the harness's group (the launcher and this child) on the harness's own console.
    assert_ne!(unsafe { GenerateConsoleCtrlEvent(1, group) }, 0, "{}", std::io::Error::last_os_error());
    until(SIG_BOUND, || BREAKS.load(Ordering::SeqCst) >= 1);
    let st = SessionClient::from_session_dir(dir).unwrap().status().unwrap();
    std::fs::write(out.join("status-after.json"), serde_json::to_vec(&st).unwrap()).unwrap();
    std::fs::write(out.join("ctrlbreak"), BREAKS.load(Ordering::SeqCst).to_string()).unwrap();
}

/// A temp root for one harness, canonical (the child compares paths).
fn sig_root() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    (t, root)
}

#[cfg(unix)]
#[test]
fn ctrl_c_reaches_only_the_child() {
    let (_t, root) = sig_root();
    let r = |n: &str| read_text(&root.join(n));
    let mut h = harness(&root, "sigint", &[]);
    let pid = i32::try_from(h.pid).unwrap();
    assert!(until(SIG_BOUND, || root.join("ready").exists()), "{}", h.log());
    send(pid, libc::SIGINT);
    send(pid, libc::SIGQUIT);
    std::fs::write(root.join("sent1"), "").unwrap();
    assert!(until(SIG_BOUND, || root.join("ready2").exists()), "{}", h.log());
    send(-pid, libc::SIGINT);
    send(-pid, libc::SIGQUIT);
    let st = h.finish();
    assert!(st.success(), "harness: {st}; {}", h.log());
    assert_eq!(r("launcher-exit"), "0", "{}", h.log());
    assert_eq!(r("launcher-outcome"), "0 None true false", "{}", h.log());
    assert_eq!(r("inherited"), "default default");
    assert_eq!(r("direct"), "0 0");
    assert_eq!(r("counts"), "1 1");
    assert_eq!(json(&root.join("status-after.json"))["session"], env_json_at(&root)["ZYGO_SESSION"].as_str());
    assert_eq!(r("before"), "default default default default");
    assert_eq!(r("after"), r("before"));
}

#[cfg(unix)]
#[test]
fn sigterm_and_sighup_are_forwarded() {
    let (_t, root) = sig_root();
    let r = |n: &str| read_text(&root.join(n));
    let mut h = harness(&root, "forward", &[]);
    let pid = i32::try_from(h.pid).unwrap();
    assert!(until(SIG_BOUND, || root.join("ready").exists()), "{}", h.log());
    let mut sends = [0u32; 2];
    for (i, sig) in [(0, libc::SIGTERM), (1, libc::SIGHUP)] {
        // A resend covers the window before the launcher stores the child pid (Q1).
        let got = || r("seen").split(' ').nth(i).and_then(|f| f.parse::<u32>().ok()).is_some_and(|n| n >= 1);
        loop {
            send(pid, sig);
            sends[i] += 1;
            if until(Duration::from_secs(5), got) {
                break;
            }
            assert!(sends[i] < 12, "signal {sig}: no receipt after {} sends; {}", sends[i], h.log());
        }
    }
    eprintln!("S2 sends: term {} hup {}", sends[0], sends[1]);
    let st = h.finish();
    assert!(st.success(), "harness: {st}; {}", h.log());
    assert_eq!(r("launcher-exit"), "3", "{}", h.log());
    assert_eq!(r("launcher-outcome"), "3 None true true", "{}", h.log());
    let f: Vec<u32> = r("forward").split(' ').map(|x| x.parse().unwrap()).collect();
    assert!((1..=sends[0]).contains(&f[0]), "term {} of {} sends", f[0], sends[0]);
    assert!((1..=sends[1]).contains(&f[1]), "hup {} of {} sends", f[1], sends[1]);
    assert_eq!(r("before"), "default default default default");
    assert_eq!(r("after"), r("before"));
}

#[test]
fn exits_before_the_spawn_have_spawned_false() { // AU60
    use zygo_launch::launcher::LaunchOutcome;
    let fx = fx();
    let no_spawn = |code| LaunchOutcome { code, signal: None, spawned: false, received: false, ran: None };
    let missing = Stub { program: "zygo-test-no-such-agent".into(), ..stub() };
    assert_eq!(Launcher::run(&missing, opts(&fx, "dump", &[])), no_spawn(13)); // before the session
    let refused = Stub { preflight: Some(Preflight { file: fx.root.join("settings.json"), key: "env.ANTHROPIC_API_KEY".into() }), ..stub() };
    assert_eq!(Launcher::run(&refused, opts(&fx, "dump", &[])), no_spawn(2)); // after the session directory
    let ran = Launcher::run(&stub(), opts(&fx, "dump", &[("ZYGO_TEST_EXIT", "7")]));
    assert_eq!(LaunchOutcome { ran: None, ..ran }, LaunchOutcome { code: 7, signal: None, spawned: true, received: false, ran: None });
    assert!(ran.ran.is_some());
}

/// W§8.2 step 7, controller M2: `ran` is the agent's own run time. The stub sleeps 300 ms and exits 3, so `ran` is
/// at least 300 ms (an implementation that gives `Some(Duration::ZERO)` fails) and at most the wall time of the
/// whole launch (an implementation that counts from before the spawn can still pass this bound, so the
/// slow-setup test of step 31 checks the other side).
#[test]
fn ran_is_the_time_from_the_spawn_to_the_exit() {
    let fx = fx();
    let t0 = std::time::Instant::now();
    let o = Launcher::run(&stub(), opts(&fx, "dump", &[("ZYGO_TEST_SLEEP_MS", "300"), ("ZYGO_TEST_EXIT", "3")]));
    let wall = t0.elapsed();
    assert_eq!((o.code, o.spawned), (3, true));
    let ran = o.ran.unwrap();
    assert!(ran >= Duration::from_millis(300) && ran <= wall, "{ran:?} {wall:?}");
}

#[cfg(unix)]
#[test]
fn sigterm_to_the_launcher_sets_received() { // AU34, AU17
    let (_t, root) = sig_root();
    let r = |n: &str| read_text(&root.join(n));
    let mut h = harness(&root, "control", &[]);
    let pid = i32::try_from(h.pid).unwrap();
    // The stub writes `start <pid>` after the launcher spawned it, so the handling is installed.
    assert!(until(SIG_BOUND, || r("ctl/done").starts_with("start ") && r("ctl/done").ends_with('\n')), "{}", h.log());
    send(pid, libc::SIGTERM);
    let st = h.finish();
    assert!(st.success(), "harness: {st}; {}", h.log());
    // The launcher sent SIGTERM on; the stub has no handler and died by it.
    assert_eq!(r("launcher-outcome"), "143 Some(15) true true", "{}", h.log());
    assert_eq!(r("after"), r("before"));
}

#[cfg(unix)]
#[test]
fn ignored_sighup_stays_ignored_for_the_agent() {
    let (_t, root) = sig_root();
    let r = |n: &str| read_text(&root.join(n));
    let mut h = harness(&root, "inherit", &[libc::SIGHUP]);
    let st = h.finish();
    assert!(st.success(), "harness: {st}; {}", h.log());
    assert_eq!(r("launcher-exit"), "0", "{}", h.log());
    assert_eq!(r("before"), "default default default ignored");
    assert_eq!(r("inherited"), "default default default ignored");
    assert_eq!(r("after"), r("before"));
}

#[cfg(windows)]
#[test]
fn ctrl_break_reaches_only_the_child() {
    let (_t, root) = sig_root();
    let r = |n: &str| read_text(&root.join(n));
    let mut h = harness(&root, "ctrlbreak", &[]);
    let st = h.finish();
    assert!(st.success(), "harness: {st}; {}", h.log());
    assert_eq!(r("launcher-exit"), "0", "{}", h.log());
    assert_eq!(r("ctrlbreak"), "1");
    assert_eq!(json(&root.join("status-after.json"))["session"], env_json_at(&root)["ZYGO_SESSION"].as_str());
}
