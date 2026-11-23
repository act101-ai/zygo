//! N32: Claude Code adapter (§8.3-§8.5, §12.3). No process-environment change.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::schema::{CustomRole, Role};
use zygo_config::{load_with_builtin, LoadContext};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_core::Agent;
use zygo_hooks::session_text::mechanics;
use zygo_launch::claude_code::{shell_quote, ClaudeCode};
use zygo_launch::env::{child_env, Env};
use zygo_launch::{AgentAdapter, LaunchCtx, LaunchOpts, Launcher, Plugins, Preflight};
use zygo_plugins::snapshot::{Snapshot, SnapshotPlugin};
use zygo_proxy::session::SessionConfig;
use zygo_proxy::Url;
use zygo_work::WorkItem;

struct Fx {
    _t: TempDir,
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    bin: PathBuf,
    managed: PathBuf,
    sd: SessionDir,
    paths: Paths,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let (home, cwd, bin) = (root.join("home"), root.join("work"), root.join("bin"));
    let sd = SessionDir::new(root.join("sessions").join("s-n32"));
    for d in [&home, &cwd, &bin, &sd.root] {
        std::fs::create_dir_all(d).unwrap();
    }
    let paths = Paths {
        config_file: home.join("config.toml"),
        cache_dir: home.join("cache"),
        data_dir: home.join("data"),
        sessions_dir: home.join("cache/sessions"),
        marketplaces_dir: home.join("cache/marketplaces"),
        plugin_store: home.join("data/plugins"),
        approvals_file: home.join("data/approvals.toml"),
        usage_db: home.join("data/usage.db"),
    };
    let managed = root.join("managed-settings.json");
    Fx { _t: t, root, home, cwd, bin, managed, sd, paths }
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

fn role(model: &str, d: &str, p: &str, writes: bool) -> Role {
    Role { model: model.into(), description: d.into(), prompt: p.into(), writes, ..Default::default() }
}

const HOME_VAR: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

fn ctx(fx: &Fx, cfg_text: &str) -> LaunchCtx {
    std::fs::write(&fx.paths.config_file, cfg_text).unwrap();
    let cfg = Arc::new(load_with_builtin(&LoadContext { paths: fx.paths.clone(), repo: None, team: None }, builtin()).unwrap());
    let roles = BTreeMap::from([
        ("code".to_string(), role("gpt-x-test", "D-code", "P-code", true)),
        ("design".to_string(), role("claude-fable-5", "D-design", "P-design", true)),
        ("lead".to_string(), role("claude-opus-5-5", "D-lead", "P-lead", false)),
        ("read".to_string(), role("claude-haiku-4-5", "D-read", "P-read", false)),
    ]);
    let custom = CustomRole { model: "claude-haiku-4-5".into(), description: "D-probe".into(), prompt: "P-probe".into(), writes: false };
    let config = SessionConfig {
        main_role: "lead".into(),
        roles,
        custom_roles: BTreeMap::from([("probe".to_string(), custom)]),
        models: vec![],
        guards: cfg.config.guards.clone(),
    };
    let view = SessionView {
        session: "s-n32".into(),
        agent: Agent::ClaudeCode,
        main_role: "lead".into(),
        effective_role: Some("lead".into()),
        role_prompt: None,
        user_prompt: None,
        profile: None,
        profile_prompt: None,
        work_item: None,
        legend: vec![],
        disabled_roles: vec![],
        work_units: true,
        results: true,
        compact_tool: true,
        navigation: false,
        trim_threshold_bytes: 0,
        head_lines: 0,
        tail_lines: 0,
        ..Default::default()
    };
    let zygo = SnapshotPlugin {
        marketplace: "embedded".into(),
        name: "zygo".into(),
        dir: Some(fx.sd.root.join("plugins/zygo")),
        skills: vec![],
        agents: vec![],
        commands: vec![],
        mcp: vec![],
    };
    LaunchCtx {
        agent: Agent::ClaudeCode,
        program: PathBuf::from("claude"),
        agent_version: "2.1.328".into(),
        cfg,
        profile: None,
        session: "s-n32".into(),
        token: ["session", "token", "n32"].join("-"),
        session_dir: fx.sd.clone(),
        proxy_url: Url::parse("http://127.0.0.1:4100/").unwrap(),
        config,
        view,
        effective_role: Some("lead".into()),
        repo: Some("example.com/acme/widgets".into()),
        work: Some(WorkItem {
            source: "branch".into(),
            id: "main".into(),
            title: None,
            url: None,
            status: None,
            check_hint: None,
            files_hint: vec![],
            lookup_failed: None,
        }),
        plugins: Plugins::Snapshot(Snapshot { plugins: vec![zygo], lines: vec![], details: vec![] }),
        args: vec![],
        passthrough: false,
        yolo: false,
        compact: None,
        cwd: fx.cwd.clone(),
        repo_root: None,
        env: Env(vec![(HOME_VAR.into(), fx.home.clone().into_os_string()), ("GH_TOKEN".into(), "gh-n32".into())]),
        paths: fx.paths.clone(),
    }
}

#[cfg(unix)]
fn script(fx: &Fx, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = fx.bin.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn launch_opts(fx: &Fx, args: &[&str], passthrough: bool) -> LaunchOpts {
    let mut env = vec![
        (OsString::from("PATH"), OsString::from(&fx.bin)),
        (OsString::from("HOME"), OsString::from(&fx.home)),
        (OsString::from("ACT_CONFIG_DIR"), OsString::from(fx.root.join("act"))),
    ];
    if cfg!(windows)
        && let Some(v) = std::env::var_os("SystemRoot")
    {
        env.push(("SystemRoot".into(), v));
    }
    let mut o = LaunchOpts::new(Env(env), fx.cwd.clone(), fx.paths.clone());
    o.builtin = Some(builtin());
    o.stderr = Some(Arc::default());
    o.args = strings(args);
    o.passthrough = passthrough;
    o
}

fn stderr_of(o: &LaunchOpts) -> Arc<std::sync::Mutex<Vec<u8>>> {
    o.stderr.clone().unwrap()
}

fn text_of(buf: &Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buf.lock().unwrap()).into_owned()
}

fn cc(fx: &Fx) -> ClaudeCode {
    ClaudeCode { program: "claude".into(), managed_settings: fx.managed.clone(), zygo: fx.root.join("bin").join("zygo") }
}

fn strings(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// `s` as the inside of a JSON string.
fn escaped(s: &str) -> String {
    let q = Value::String(s.into()).to_string();
    q[1..q.len() - 1].to_string()
}

fn fixture(name: &str, ctx: &LaunchCtx, cc: &ClaudeCode) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code").join(name);
    let text = std::fs::read_to_string(p).unwrap()
        .replace("@MECH@", &escaped(&mechanics(&ctx.view)))
        .replace("@TOKEN@", &escaped(&ctx.token))
        .replace("<ZYGO>", &escaped(&shell_quote(&cc.zygo)))
        .replace("<ZYGO_PATH>", &escaped(&cc.zygo.to_string_lossy()));
    serde_json::from_str(&text).unwrap()
}

/// The value after each `name`.
fn flag(args: &[String], name: &str) -> Vec<String> {
    args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
}

/// `(matcher, command)` pairs of `event`.
fn hook_commands(s: &Value, event: &str) -> Vec<(Option<String>, String)> {
    let mut out = vec![];
    for e in s["hooks"][event].as_array().into_iter().flatten() {
        let m = e.get("matcher").and_then(Value::as_str).map(str::to_string);
        for h in e["hooks"].as_array().unwrap() {
            out.push((m.clone(), h["command"].as_str().unwrap().to_string()));
        }
    }
    out
}

fn put(p: &Path, json: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, json).unwrap();
}

fn settings(fx: &Fx) -> Value {
    serde_json::from_slice(&std::fs::read(fx.sd.settings_json()).unwrap()).unwrap()
}

fn env_map(rows: &[(String, String)]) -> Value {
    Value::Object(rows.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

fn agents_of(args: &[String]) -> Value {
    let a = flag(args, "--agents");
    assert_eq!(a.len(), 1, "{args:?}");
    serde_json::from_str(&a[0]).unwrap()
}

fn sample() -> String {
    ["n32", "value"].join("-")
}

fn picked(a: &[&str]) -> Option<String> {
    ClaudeCode::default().picked_model(&strings(a))
}

#[test]
fn picked_model_resolves_the_alias_table() {
    assert_eq!(picked(&[]), None);
    for (v, want) in [
        ("opus", "lead"),
        ("opusplan", "lead"),
        ("fable", "design"),
        ("best", "design"),
        ("sonnet", "code"),
        ("haiku", "read"),
        ("sonnet[1m]", "code"),
        ("code", "code"),
        ("claude-opus-5-5", "claude-opus-5-5"),
    ] {
        assert_eq!(picked(&["--model", v]).as_deref(), Some(want), "{v}");
    }
    assert_eq!(picked(&["--model=sonnet"]).as_deref(), Some("code"));
    assert_eq!(picked(&["--model", "x", "--model", "haiku"]).as_deref(), Some("read"));
    assert_eq!(picked(&["-p", "--model"]), None);
}

#[test]
fn shell_quote_keeps_one_word() {
    #[cfg(unix)]
    assert_eq!(shell_quote(Path::new("/tmp/a b/it's/zygo")), r"'/tmp/a b/it'\''s/zygo'");
    #[cfg(windows)]
    assert_eq!(shell_quote(Path::new(r"C:\a b\zygo.exe")), r#""C:\a b\zygo.exe""#);
}

#[test]
fn settings_env_agents_and_mcp_match_fixtures() {
    let fx = fx();
    let c = ctx(&fx, "");
    let cc = cc(&fx);
    let spec = cc.build(&c).unwrap();
    let mut s = settings(&fx);
    let env = s.as_object_mut().unwrap().remove("env").unwrap();
    assert_eq!(s, fixture("settings.json", &c, &cc));
    assert_eq!(env, fixture("env.json", &c, &cc));
    assert_eq!(env_map(&spec.env), fixture("env.json", &c, &cc));
    let names: BTreeSet<&String> = spec.env.iter().map(|(k, _)| k).collect();
    assert_eq!(names.len(), spec.env.len());
    assert!(!names.iter().any(|k| *k == "GH_TOKEN"));
    let agents = agents_of(&spec.args);
    assert_eq!(agents, fixture("agents.json", &c, &cc));
    assert!(agents.get("zygo-lead").is_none());
    let mcp = fx.sd.root.join("mcp.json");
    assert_eq!(spec.args[..2], ["--mcp-config".to_string(), mcp.to_string_lossy().into_owned()]);
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&mcp).unwrap()).unwrap(), fixture("mcp.json", &c, &cc));
    assert_eq!(spec.args[2], "--settings");
    assert_eq!(spec.args[3], fx.sd.settings_json().to_string_lossy());
    assert_eq!(flag(&spec.args, "--plugin-dir"), [fx.sd.root.join("plugins").to_string_lossy().into_owned()]);
    assert!(spec.lines.is_empty());
    assert!(spec.log.is_empty());
}

#[test]
fn user_prompt_is_not_taken_by_mcp_config() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.args = strings(&["fix the bug"]);
    c.yolo = true;
    let spec = cc(&fx).build(&c).unwrap();
    let a = &spec.args;
    assert_eq!(a.last().unwrap(), "fix the bug");
    let at = a.iter().position(|x| x == "fix the bug").unwrap();
    for f in ["--mcp-config", "--settings", "--agents", "--plugin-dir", "--dangerously-skip-permissions"] {
        assert!(a.iter().position(|x| x == f).unwrap() < at, "{f}");
    }
    let s = a.iter().position(|x| x == "--settings").unwrap();
    assert_eq!(s, 2);
    assert_eq!(a[s - 1], fx.sd.root.join("mcp.json").to_string_lossy());
}

#[test]
fn no_plugins_no_plugin_dir() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.plugins = Plugins::Snapshot(Snapshot::default());
    c.args = strings(&["fix the bug"]);
    let spec = cc(&fx).build(&c).unwrap();
    let a = &spec.args;
    assert!(!a.iter().any(|x| x == "--plugin-dir"));
    assert_eq!(a[a.len() - 3], "--agents");
    assert!(serde_json::from_str::<Value>(&a[a.len() - 2]).unwrap().is_object());
    assert_eq!(a[a.len() - 1], "fix the bug");
}

#[test]
fn empty_zygo_path_fails_the_build() {
    let fx = fx();
    let c = ctx(&fx, "");
    let e = ClaudeCode { zygo: PathBuf::new(), ..cc(&fx) }.build(&c).err().unwrap();
    assert_eq!(e.to_string(), "cannot find the path of the zygo binary");
}

#[cfg(unix)]
#[test]
fn non_utf8_zygo_path_is_refused_not_rewritten() {
    use std::os::unix::ffi::OsStrExt;
    let fx = fx();
    let c = ctx(&fx, "");
    let zygo = PathBuf::from(OsStr::from_bytes(b"/opt/zy\xffgo/zygo"));
    let e = ClaudeCode { zygo, ..cc(&fx) }.build(&c).err().unwrap();
    assert_eq!(e.to_string(), "cannot find the path of the zygo binary");
    assert!(!c.session_dir.settings_json().exists(), "build wrote nothing before the refusal");
}

#[cfg(unix)]
#[test]
fn non_utf8_session_dir_is_refused_not_rewritten() {
    use std::os::unix::ffi::OsStrExt;
    let fx = fx();
    let mut c = ctx(&fx, "");
    let root = fx.root.join(OsStr::from_bytes(b"s\xff"));
    c.session_dir = SessionDir::new(root.clone());
    let e = cc(&fx).build(&c).err().unwrap();
    assert_eq!(e.to_string(), "cannot use the session directory path: it is not valid UTF-8");
    assert!(!root.exists(), "build wrote nothing before the refusal");
}

#[test]
fn passthrough_drops_model_rows_and_adds_the_token_header() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.passthrough = true;
    let cc = cc(&fx);
    let spec = cc.build(&c).unwrap();
    let mut s = settings(&fx);
    let env = s.as_object_mut().unwrap().remove("env").unwrap();
    assert_eq!(s, fixture("settings-passthrough.json", &c, &cc));
    assert!(s.get("modelPicker").is_none());
    assert_eq!(env, fixture("env-passthrough.json", &c, &cc));
    assert_eq!(env_map(&spec.env), fixture("env-passthrough.json", &c, &cc));
    assert_eq!(agents_of(&spec.args), fixture("agents-passthrough.json", &c, &cc));
    for x in spec.lines.iter().chain(&spec.log).chain(&spec.args) {
        assert!(!x.contains(&c.token), "{x}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(fx.sd.settings_json()).unwrap().permissions().mode() & 0o777, 0o600);
    }
    c.args = strings(&["--model", "claude-opus-5-5"]);
    c.effective_role = None;
    assert!(cc.build(&c).unwrap().log.is_empty());
}

#[test]
fn agent_version_line_is_left_out_when_empty() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.agent_version = String::new();
    c.repo = None;
    c.work = None;
    let spec = cc(&fx).build(&c).unwrap();
    assert_eq!(env_map(&spec.env)["ANTHROPIC_CUSTOM_HEADERS"], "X-Zygo-Session: s-n32");
}

#[test]
fn role_guard_follows_the_effective_role() {
    let fx = fx();
    let cc = cc(&fx);
    let q = shell_quote(&cc.zygo);
    for (eff, guarded) in [(Some("code"), false), (Some("lead"), true), (None, false)] {
        let mut c = ctx(&fx, "");
        c.effective_role = eff.map(str::to_string);
        cc.build(&c).unwrap();
        let pre = hook_commands(&settings(&fx), "PreToolUse");
        let rg = pre.iter().filter(|(_, cmd)| cmd.ends_with("hook role-guard")).collect::<Vec<_>>();
        if guarded {
            assert_eq!(rg, [&(Some("Edit|Write|NotebookEdit".to_string()), format!("{q} hook role-guard"))]);
        } else {
            assert!(rg.is_empty(), "{eff:?}");
        }
        assert!(pre.iter().any(|(_, cmd)| *cmd == format!("{q} hook guard")), "{eff:?}");
    }
}

/// §8.6 item 7: the attribution sentence (operator, 2026-11-22).
const NO_ATTRIBUTION: &str = "Never add Claude attribution to commits, pull request titles or bodies, tags, release notes or comments: no `Co-Authored-By: Claude` trailer, no `noreply@anthropic.com` address, no `Generated with Claude Code` line and no Claude session link.";

/// A user `--settings` value that tries to turn attribution back on, with its own hooks and keys (FIX-21 round 2 F1).
const USER_SETTINGS: &str = r#"{"attribution": {"commit": "Co-Authored-By: Claude", "pr": "Generated", "sessionUrl": true},
    "includeCoAuthoredBy": true, "model": "user-model", "permissions": {"allow": ["Bash"]}, "env": {"USER_VAR": "1"},
    "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "user-pre"}]}],
              "Stop": [{"hooks": [{"type": "command", "command": "user-stop"}]}]}}"#;
/// §8.5 Flags: the line for a user `--settings` value that zygo merges.
const MERGE_WARN: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force";

#[test] // FIX-21, §8.5: kills: the attribution keys written only outside passthrough, only for members or only with work units;
        // a user `--settings` file or inline value that replaces zygo's file, drops the guard hook or the user's hooks and
        // keys, or turns attribution back on (round 2 F1)
fn settings_force_no_claude_attribution() {
    for name in ["default", "passthrough", "member", "units off", "user file", "user inline", "member user file", "user file twice"] {
        let fx = fx();
        let user_file = fx.cwd.join("user-settings.json");
        std::fs::write(&user_file, USER_SETTINGS).unwrap();
        let mut c = match name {
            "member" | "member user file" => member_ctx(&fx, true),
            _ => {
                let mut c = ctx(&fx, "");
                c.passthrough = name == "passthrough";
                c.view.work_units = name != "units off";
                c
            }
        };
        c.args = match name {
            "user file" | "member user file" => strings(&["--settings", "user-settings.json", "-p", "hi"]),
            "user inline" => strings(&[&format!("--settings={USER_SETTINGS}"), "-p", "hi"]),
            "user file twice" => strings(&["--settings", r#"{"model": "first", "outputStyle": "x"}"#, "--settings", user_file.to_str().unwrap()]),
            _ => vec![],
        };
        let cc = cc(&fx);
        let spec = cc.build(&c).unwrap();
        let s = settings(&fx);
        assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{name}");
        assert_eq!(s["includeCoAuthoredBy"], json!(false), "{name}");
        let guard = (Some("Edit|Write|NotebookEdit|Read|Bash".to_string()), format!("{} hook guard", shell_quote(&cc.zygo)));
        assert!(hook_commands(&s, "PreToolUse").contains(&guard), "{name}");
        if name.contains("user") {
            assert_eq!(flag(&spec.args, "--settings"), [fx.sd.settings_json().to_string_lossy().into_owned()], "{name}");
            assert!(!spec.args.iter().any(|a| a.starts_with("--settings=")), "{name}");
            assert!(hook_commands(&s, "PreToolUse").contains(&(Some("Bash".to_string()), "user-pre".to_string())), "{name}");
            assert!(hook_commands(&s, "Stop").contains(&(None, "user-stop".to_string())), "{name}");
            assert_eq!(s["model"], "user-model", "{name}");
            assert_eq!(s["permissions"], json!({"allow": ["Bash"]}), "{name}");
            assert_eq!(s["env"]["USER_VAR"], "1", "{name}");
            assert!(s["env"]["ANTHROPIC_BASE_URL"].as_str().is_some_and(|u| u.ends_with("/anthropic")), "{name}");
            assert_eq!(spec.lines, [MERGE_WARN], "{name}");
            assert_eq!(spec.log, ["degraded: --settings"], "{name}");
        }
        if name == "user file twice" {
            assert_eq!(s["outputStyle"], "x");
        }
        if name == "member user file" {
            let z = shell_quote(&cc.zygo);
            assert!(hook_commands(&s, "Stop").contains(&(None, format!("{z} hook workspace --event stop"))), "{name}");
        }
        let agents = agents_of(&spec.args);
        let zygo: Vec<(&String, &Value)> = agents.as_object().unwrap().iter().filter(|(k, _)| k.starts_with("zygo-")).collect();
        assert!(!zygo.is_empty(), "{name}");
        for (k, a) in zygo {
            assert!(a["prompt"].as_str().unwrap().contains(NO_ATTRIBUTION), "{name} {k}");
        }
    }
}

#[test]
fn work_units_off_keeps_only_the_redaction_guard() {
    let fx = fx();
    let cc = cc(&fx);
    let mut c = ctx(&fx, "");
    c.view.work_units = false;
    cc.build(&c).unwrap();
    let s = settings(&fx);
    for event in s["hooks"].as_object().unwrap().keys() {
        assert!(!hook_commands(&s, event).iter().any(|(_, cmd)| cmd.contains("hook work")), "{event}");
    }
    let guard = (Some("Edit|Write|NotebookEdit|Read|Bash".to_string()), format!("{} hook guard", shell_quote(&cc.zygo)));
    assert!(hook_commands(&s, "PreToolUse").contains(&guard));
}

#[test]
fn turn_cap_sets_max_turns() {
    let fx = fx();
    let c = ctx(&fx, "[limits]\nsubagent_max_turns = 7\n");
    let agents = agents_of(&cc(&fx).build(&c).unwrap().args);
    for (k, a) in agents.as_object().unwrap() {
        assert_eq!(a["maxTurns"], 7, "{k}");
    }
    let c = ctx(&fx, "");
    let agents = agents_of(&cc(&fx).build(&c).unwrap().args);
    assert!(agents.as_object().unwrap().values().all(|a| a.get("maxTurns").is_none()));
}

#[test]
fn disabled_alias_targets_are_redirected() {
    let fx = fx();
    const ROWS: [&str; 5] = [
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL",
    ];
    for (main, keep, want) in [
        ("lead", &["lead", "read"][..], ["lead", "lead", "lead", "read", "lead"]),
        ("plan", &["plan", "read"], ["read"; 5]),
        ("plan", &["plan"], ["plan"; 5]),
        ("plan", &["plan", "code", "read"], ["code", "code", "code", "read", "code"]),
    ] {
        let mut c = ctx(&fx, "");
        c.config.main_role = main.into();
        c.config.roles = keep.iter().map(|r| (r.to_string(), role("claude-opus-5-5", "d", "p", true))).collect();
        let spec = cc(&fx).build(&c).unwrap();
        let env = env_map(&spec.env);
        for (row, w) in ROWS.iter().zip(want) {
            assert_eq!(env[row], w, "{main} {keep:?} {row}");
        }
        assert_eq!(env["ANTHROPIC_MODEL"], main);
        assert!(agents_of(&spec.args).get(format!("zygo-{main}")).is_none());
    }
}

#[test]
fn user_agents_merge_and_win_by_name() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    let mine = r#"{"zygo-code":{"description":"mine","prompt":"x"},"helper":{"description":"h","prompt":"y"}}"#;
    c.args = strings(&["--agents", mine, "-p", "hi"]);
    let spec = cc(&fx).build(&c).unwrap();
    let a = agents_of(&spec.args);
    assert_eq!(a["zygo-code"], json!({"description":"mine","prompt":"x"}));
    assert!(a.get("helper").is_some());
    assert_eq!(a["zygo-read"]["description"], "D-read");
    assert_eq!(spec.args[spec.args.len() - 2..], ["-p", "hi"]);

    let file = fx.root.join("agents.json");
    put(&file, r#"{"helper":{"description":"h","prompt":"y"}}"#);
    c.args = vec!["-p".into(), format!("--agents={}", file.display())];
    let a = agents_of(&cc(&fx).build(&c).unwrap().args);
    assert!(a.get("helper").is_some());
}

const BAD_AGENTS: &str = "--agents: not a JSON object (or, with --print, a file that holds one)";

#[test]
fn bad_user_agents_are_refused() {
    let fx = fx();
    let (good, list) = (fx.root.join("good.json"), fx.root.join("list.json"));
    put(&good, r#"{"helper":{"description":"h","prompt":"y"}}"#);
    put(&list, "[1]");
    let (good, list) = (good.to_string_lossy().into_owned(), list.to_string_lossy().into_owned());
    let refusal = |args: &[&str]| {
        let mut c = ctx(&fx, "");
        c.args = strings(args);
        cc(&fx).refusal(&c)
    };
    assert_eq!(refusal(&["--agents", "not json"]).as_deref(), Some(BAD_AGENTS));
    assert_eq!(refusal(&["--agents", &good]).as_deref(), Some(BAD_AGENTS));
    assert_eq!(refusal(&["--print", "--agents", &list]).as_deref(), Some(BAD_AGENTS));
    assert_eq!(refusal(&["--print", "--agents", &good]), None);
}

#[test]
fn yolo_and_user_args_pass_through() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.yolo = true;
    c.args = strings(&["-p", "hi"]);
    let a = cc(&fx).build(&c).unwrap().args;
    assert_eq!(a.iter().filter(|x| *x == "--dangerously-skip-permissions").count(), 1);
    let y = a.iter().position(|x| x == "--dangerously-skip-permissions").unwrap();
    assert!(y < a.iter().position(|x| x == "-p").unwrap());
    assert_eq!(a[a.len() - 2..], ["-p", "hi"]);
}

#[test]
fn defeating_flags_are_logged_and_settings_warns() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.args = strings(&["--settings", "x.json", "--bare", "--model", "claude-opus-5-5", "--dangerously-skip-permissions"]);
    c.effective_role = None;
    let spec = cc(&fx).build(&c).unwrap();
    assert_eq!(spec.log, ["degraded: --settings", "degraded: --bare", "degraded: --model claude-opus-5-5"]);
    assert_eq!(spec.lines, ["zygo: your --settings replaces zygo's settings file; the lead guard is off"]);
    let c = ctx(&fx, "");
    let spec = cc(&fx).build(&c).unwrap();
    assert!(spec.log.is_empty());
    assert!(spec.lines.is_empty());
}

#[test] // FIX-21 round 2 F1: a user `--settings` value zygo cannot read as a JSON object keeps the replace path: every
        // user `--settings` passes unchanged after zygo's, with the replace line
fn unreadable_user_settings_pass_unchanged() {
    let fx = fx();
    std::fs::write(fx.cwd.join("bad.json"), "not json").unwrap();
    std::fs::write(fx.cwd.join("list.json"), "[1]").unwrap();
    std::fs::write(fx.cwd.join("good.json"), "{}").unwrap();
    for args in [vec!["--settings", "missing.json"], vec!["--settings", "bad.json"], vec!["--settings", "list.json"],
                 vec!["--settings", "{bad"], vec!["--settings=[1]"], vec!["--settings", "good.json", "--settings", "missing.json"]] {
        let mut c = ctx(&fx, "");
        c.args = strings(&args);
        let spec = cc(&fx).build(&c).unwrap();
        assert_eq!(spec.args[2..4], ["--settings".to_string(), fx.sd.settings_json().to_string_lossy().into_owned()], "{args:?}");
        assert!(spec.args.ends_with(&c.args), "{args:?}: {:?}", spec.args);
        assert_eq!(spec.lines, ["zygo: your --settings replaces zygo's settings file; the lead guard is off"], "{args:?}");
        assert_eq!(spec.log, ["degraded: --settings"], "{args:?}");
        assert_eq!(settings(&fx)["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{args:?}");
    }
}

fn pf(file: PathBuf, key: &str) -> Result<(), Preflight> {
    Err(Preflight { file, key: key.into() })
}

#[test]
fn preflight_names_the_file_and_key() {
    let v = sample();
    let user = |fx: &Fx| fx.home.join(".claude/settings.json");
    let project = |fx: &Fx| fx.cwd.join(".claude/settings.json");
    let local = |fx: &Fx| fx.cwd.join(".claude/settings.local.json");
    let managed = |fx: &Fx| fx.managed.clone();
    type Where = fn(&Fx) -> PathBuf;
    let rows: Vec<(Where, String, &str)> = vec![
        (user, format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{v}"}}}}"#), "env.ANTHROPIC_API_KEY"),
        (project, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#.into(), "env.CLAUDE_CODE_USE_BEDROCK"),
        (local, r#"{"ANTHROPIC_BASE_URL":"http://x.invalid"}"#.into(), "ANTHROPIC_BASE_URL"),
        (user, format!(r#"{{"env":{{"ANTHROPIC_AUTH_TOKEN":"{v}"}}}}"#), "env.ANTHROPIC_AUTH_TOKEN"),
        (user, r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#.into(), "env.CLAUDE_CODE_USE_VERTEX"),
        (user, r#"{"env":{"CLAUDE_CODE_USE_FOUNDRY":"1"}}"#.into(), "env.CLAUDE_CODE_USE_FOUNDRY"),
        (managed, r#"{"disableAllHooks":true}"#.into(), "disableAllHooks"),
        (managed, r#"{"allowManagedHooksOnly":true}"#.into(), "allowManagedHooksOnly"),
        (managed, r#"{"apiKeyHelper":"x"}"#.into(), "apiKeyHelper"),
    ];
    for (at, text, key) in rows {
        let fx = fx();
        let file = at(&fx);
        put(&file, &text);
        let c = ctx(&fx, "");
        assert_eq!(cc(&fx).preflight(&c), pf(file, key), "{key}");
    }
}

#[test]
fn preflight_reads_config_dir_repo_root_and_folds_case_on_windows() {
    let v = sample();
    let key = format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{v}"}}}}"#);

    let fx = fx();
    let ccd = fx.root.join("ccd");
    put(&ccd.join("settings.json"), &key);
    let mut c = ctx(&fx, "");
    c.env.0.push(("CLAUDE_CONFIG_DIR".into(), ccd.clone().into_os_string()));
    assert_eq!(cc(&fx).preflight(&c), pf(ccd.join("settings.json"), "env.ANTHROPIC_API_KEY"));

    let fx = crate::fx();
    put(&fx.home.join(".claude/settings.json"), &key);
    let mut c = ctx(&fx, "");
    c.env.0.push(("CLAUDE_CONFIG_DIR".into(), fx.root.join("ccd").into_os_string()));
    assert_eq!(cc(&fx).preflight(&c), Ok(()));

    let fx = crate::fx();
    let repo = fx.root.join("repo");
    let file = repo.join(".claude/settings.local.json");
    put(&file, r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#);
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    let mut c = ctx(&fx, "");
    c.repo_root = Some(repo.clone());
    c.cwd = repo.join("sub");
    assert_eq!(cc(&fx).preflight(&c), pf(file, "env.CLAUDE_CODE_USE_VERTEX"));

    let fx = crate::fx();
    let user = fx.home.join(".claude/settings.json");
    put(&user, &format!(r#"{{"env":{{"anthropic_api_key":"{v}"}}}}"#));
    let c = ctx(&fx, "");
    let want = if cfg!(windows) { pf(user.clone(), "env.anthropic_api_key") } else { Ok(()) };
    assert_eq!(cc(&fx).preflight(&c), want);
    put(&user, &format!(r#"{{"anthropic_api_key":"{v}"}}"#));
    assert_eq!(cc(&fx).preflight(&c), Ok(()));
}

#[test]
fn preflight_first_hit_order() {
    let v = sample();
    let fx = fx();
    let user = fx.home.join(".claude/settings.json");
    put(&user, r#"{"env":{"ANTHROPIC_BASE_URL":"x"}}"#);
    put(&fx.managed, r#"{"disableAllHooks":true}"#);
    assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), pf(user, "env.ANTHROPIC_BASE_URL"));

    let fx = crate::fx();
    let (project, local) = (fx.cwd.join(".claude/settings.json"), fx.cwd.join(".claude/settings.local.json"));
    put(&project, r#"{"env":{"ANTHROPIC_BASE_URL":"x"}}"#);
    put(&local, &format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{v}"}}}}"#));
    assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), pf(project, "env.ANTHROPIC_BASE_URL"));

    let fx = crate::fx();
    let local = fx.cwd.join(".claude/settings.local.json");
    put(&local, &format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{v}"}}}}"#));
    put(&fx.managed, r#"{"disableAllHooks":true}"#);
    assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), pf(local, "env.ANTHROPIC_API_KEY"));
}

#[test]
fn preflight_passes_clean_and_unrelated_files() {
    let fx = fx();
    assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), Ok(()));
    let user = fx.home.join(".claude/settings.json");
    put(&user, r#"{"apiKeyHelper":"x"}"#);
    let mut c = ctx(&fx, "");
    assert_eq!(cc(&fx).preflight(&c), Ok(()));
    c.passthrough = true;
    assert_eq!(cc(&fx).preflight(&c), pf(user.clone(), "apiKeyHelper"));
    for text in [r#"{"disableAllHooks":true}"#, "not json", r#"{"env":{"GH_TOKEN":"x"}}"#] {
        put(&user, text);
        assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), Ok(()), "{text}");
    }
    std::fs::remove_file(&user).unwrap();
    put(&fx.managed, r#"{"disableAllHooks":false}"#);
    assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), Ok(()));
}

#[test]
fn gh_token_reaches_the_child_env() {
    let fx = fx();
    let c = ctx(&fx, "");
    let spec = cc(&fx).build(&c).unwrap();
    assert_eq!(child_env(&c.env, &c.cfg, &spec.env).0.get("GH_TOKEN"), Some(OsStr::new("gh-n32")));
}

const PT_MODEL: &str = "--passthrough sends model names to the vendor; use a vendor model id";

#[test]
fn passthrough_refusals() {
    let v = sample();
    let fx = fx();
    let refusal = |args: &[&str], passthrough: bool| {
        let mut c = ctx(&fx, "");
        c.args = strings(args);
        c.passthrough = passthrough;
        cc(&fx).refusal(&c)
    };
    for a in [
        &["--model", "lead"][..],
        &["--model=lead"],
        &["--model", "lead[1m]"],
        &["--fallback-model", "claude-sonnet-4-5,code"],
        &["--agents", r#"{"x":{"description":"d","prompt":"p","model":"code"}}"#],
        &["--model", "probe"],
        &["--model", "triage"],
    ] {
        assert_eq!(refusal(a, true).as_deref(), Some(PT_MODEL), "{a:?}");
    }
    assert_eq!(refusal(&["--bare"], true).as_deref(), Some("--passthrough does not work with --bare: bare mode reads only API keys"));
    for text in [r#"{"apiKeyHelper":"x"}"#.to_string(), format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{v}"}}}}"#), format!(r#"{{"env":{{"ANTHROPIC_AUTH_TOKEN":"{v}"}}}}"#)] {
        put(&fx.managed, &text);
        assert_eq!(refusal(&[], true).as_deref(), Some("passthrough needs your claude.ai login; a managed setting replaces it"), "{text}");
    }
    std::fs::remove_file(&fx.managed).unwrap();
    assert_eq!(refusal(&["--model", "claude-opus-5-5"], true), None);
    assert_eq!(refusal(&["--model", "opus"], true), None);
    assert_eq!(refusal(&["--model", "lead"], false), None);
    assert_eq!(refusal(&["--bare"], false), None);
}

#[cfg(unix)]
#[test]
fn passthrough_role_model_exits_2_through_the_launcher() {
    let fx = fx();
    script(&fx, "claude", "exit 0");
    let o = launch_opts(&fx, &["--model", "lead"], true);
    let err = stderr_of(&o);
    let adapter = ClaudeCode { program: "claude".into(), managed_settings: fx.managed.clone(), zygo: fx.bin.join("zygo") };
    assert_eq!(Launcher::run(&adapter, o).code, 2);
    assert!(text_of(&err).lines().any(|l| l == format!("zygo: {PT_MODEL}")), "{}", text_of(&err));
    assert_eq!(std::fs::read_dir(&fx.paths.sessions_dir).map_or(0, |d| d.count()), 0);
}

#[cfg(unix)]
#[test]
fn passthrough_alias_turns_the_role_off() {
    for (passthrough, want_role, guarded) in [(true, "", false), (false, "lead", true)] {
        let fx = fx();
        let r = fx.root.display();
        script(
            &fx,
            "claude",
            &format!(
                "[ \"$1\" = --version ] && exit 0\nprintf '%s' \"$ZYGO_MAIN_ROLE\" > \"{r}/role\"; /bin/cp \"$ZYGO_SESSION_DIR/settings.json\" \"{r}/settings.json\""
            ),
        );
        let o = launch_opts(&fx, &["--model", "opus"], passthrough);
        let err = stderr_of(&o);
        assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
        assert_eq!(std::fs::read_to_string(fx.root.join("role")).unwrap(), want_role);
        let s: Value = serde_json::from_slice(&std::fs::read(fx.root.join("settings.json")).unwrap()).unwrap();
        let rg = hook_commands(&s, "PreToolUse").iter().any(|(_, c)| c.ends_with("hook role-guard"));
        assert_eq!(rg, guarded, "passthrough {passthrough}");
    }
}

#[cfg(unix)]
#[test]
fn alias_of_a_disabled_role_picks_the_redirect_role() {
    for (disabled, model) in [("design", "fable"), ("code", "sonnet")] {
        let fx = fx();
        let r = fx.root.display();
        std::fs::write(&fx.paths.config_file, format!("[profile.off]\ndisabled = [\"{disabled}\"]\n")).unwrap();
        script(
            &fx,
            "claude",
            &format!(
                "[ \"$1\" = --version ] && exit 0\nprintf '%s' \"$ZYGO_MAIN_ROLE\" > \"{r}/role\"; /bin/cp \"$ZYGO_SESSION_DIR/settings.json\" \"{r}/settings.json\"; /bin/cat \"$ZYGO_SESSION_DIR/session.log\" > \"{r}/log\"; exit 0"
            ),
        );
        let mut o = launch_opts(&fx, &["--model", model], false);
        o.profile = Some("off".into());
        let err = stderr_of(&o);
        assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
        assert_eq!(std::fs::read_to_string(fx.root.join("role")).unwrap(), "lead", "{model}");
        let s: Value = serde_json::from_slice(&std::fs::read(fx.root.join("settings.json")).unwrap()).unwrap();
        assert!(hook_commands(&s, "PreToolUse").iter().any(|(_, c)| c.ends_with("hook role-guard")), "{model}");
        let log = std::fs::read_to_string(fx.root.join("log")).unwrap();
        assert!(!log.is_empty(), "{model}: the session log was not read");
        assert!(!log.contains("degraded"), "{model}: {log}");
    }
}

/// A test-side git call: no system or global config.
#[cfg(unix)]
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git on PATH");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[cfg(unix)]
#[test]
fn repo_root_settings_stop_the_launch() {
    let fx = fx();
    git(&fx.cwd, &["init", "-q"]);
    let file = fx.cwd.join(".claude/settings.json");
    put(&file, r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#);
    let sub = fx.cwd.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    script(&fx, "claude", "exit 0");
    let mut o = launch_opts(&fx, &[], false);
    o.cwd = sub;
    let err = stderr_of(&o);
    assert_eq!(Launcher::run(&cc(&fx), o).code, 2, "{}", text_of(&err));
    let want = format!("zygo: {}: sets env.CLAUDE_CODE_USE_VERTEX; zygo cannot launch with it", file.display());
    assert!(text_of(&err).lines().any(|l| l == want), "{}", text_of(&err));
}

#[test]
fn missing_claude_exits_13_with_the_hint() {
    let fx = fx();
    let o = launch_opts(&fx, &[], false);
    let err = stderr_of(&o);
    assert_eq!(Launcher::run(&cc(&fx), o).code, 13);
    let hint = "zygo: claude not found on PATH; install Claude Code: https://code.claude.com/docs/en/setup";
    assert!(text_of(&err).lines().any(|l| l == hint), "{}", text_of(&err));
}

fn member_ctx(fx: &Fx, work_units: bool) -> LaunchCtx {
    let mut c = ctx(fx, "");
    c.view.work_units = work_units;
    c.view.workspace = Some(ViewWorkspace { member: Member { ws_id: "0123456789abcdef".into(), pane: "p1".into() }, resume_note: None });
    c
}

/// Option names of the pinned Claude Code help: rows between `Options:` and `Commands:` that start with `  -`;
/// each word before the first run of two spaces that starts with `-`, a trailing `,` cut (W4's test parser).
fn help_names() -> BTreeSet<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/help.txt")).unwrap();
    text.lines()
        .skip_while(|l| *l != "Options:")
        .take_while(|l| *l != "Commands:")
        .filter(|l| l.starts_with("  -"))
        .flat_map(|l| {
            l[2..].split("  ").next().unwrap().split_whitespace().filter(|w| w.starts_with('-')).map(|w| w.trim_end_matches(',').to_string()).collect::<Vec<_>>()
        })
        .collect()
}

fn no_workspace_hook(s: &Value) {
    for event in s["hooks"].as_object().unwrap().keys() {
        assert!(!hook_commands(s, event).iter().any(|(_, cmd)| cmd.contains("hook workspace")), "{event}");
    }
    for e in ["Stop", "StopFailure", "SubagentStart"] {
        assert!(s["hooks"].get(e).is_none(), "{e}");
    }
}

#[test] // W§9.1, AU57, W§15 §8.5: every member hook, with work units on and off
fn member_settings_hold_every_workspace_hook() {
    let schema = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/settings-schema.json")).unwrap();
    for e in ["Stop", "StopFailure", "SubagentStart", "SubagentStop"] {
        assert!(schema.contains(&format!("\"{e}\"")), "{e} not in the pinned settings schema");
    }
    let agent: Value = serde_json::from_slice(&std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks/PreToolUse-Agent-main.json")).unwrap()).unwrap();
    assert_eq!(agent["tool_name"], "Agent");
    for work_units in [true, false] {
        let fx = fx();
        let cc = cc(&fx);
        let spec = cc.build(&member_ctx(&fx, work_units)).unwrap();
        assert!(spec.lines.is_empty(), "{work_units}");
        let s = settings(&fx);
        let z = shell_quote(&cc.zygo);
        let ws = |e: &str| format!("{z} hook workspace --event {e}");
        assert!(hook_commands(&s, "PreToolUse").contains(&(Some("Agent".to_string()), ws("pre-tool"))), "{work_units}");
        assert_eq!(hook_commands(&s, "Stop"), [(None, ws("stop"))], "{work_units}");
        assert_eq!(hook_commands(&s, "StopFailure"), [(None, ws("stop"))], "{work_units}");
        assert_eq!(hook_commands(&s, "SubagentStart"), [(None, ws("subagent-start"))], "{work_units}");
        let sub: BTreeSet<(Option<String>, String)> = hook_commands(&s, "SubagentStop").into_iter().collect();
        let mut want = BTreeSet::from([(None, ws("subagent-stop"))]);
        if work_units {
            want.insert((None, format!("{z} hook work")));
        }
        assert_eq!(sub, want, "{work_units}");
        assert_eq!(hook_commands(&s, "SessionStart"), [(None, format!("{z} hook session-start"))]);
        assert_eq!(hook_commands(&s, "UserPromptSubmit"), [(None, format!("{z} hook prompt"))]);
    }
}

#[test] // W§9.1: registered only for members
fn non_member_settings_hold_no_workspace_hook() {
    for work_units in [true, false] {
        let fx = fx();
        let mut c = ctx(&fx, "");
        c.view.work_units = work_units;
        assert!(c.view.workspace.is_none());
        cc(&fx).build(&c).unwrap();
        no_workspace_hook(&settings(&fx));
    }
}

#[test] // D5: arguments after `--` are not flags
fn settings_after_double_dash_is_not_a_flag() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.args = strings(&["--", "--settings", "x.json"]);
    let spec = cc(&fx).build(&c).unwrap();
    assert!(spec.lines.is_empty(), "{:?}", spec.lines);
    assert!(!spec.log.iter().any(|l| l == "degraded: --settings"), "{:?}", spec.log);
    c.args = strings(&["--append-system-prompt", "--settings"]);
    assert!(cc(&fx).build(&c).unwrap().lines.is_empty());
}

#[test] // W§8.3, AU3, AU30
fn session_args_follow_w4_and_the_pinned_help() {
    let help = help_names();
    let cc = ClaudeCode::default();
    let names = zygo_workspace::args::session_flags(Agent::ClaudeCode);
    assert_eq!(names, ["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport"]);
    for f in names {
        assert!(help.contains(f), "{f} is not in the pinned --help");
        assert!(cc.has_session_arg(&strings(&["--model", "opus", f, "x"])), "{f}");
    }
    assert!(cc.has_session_arg(&strings(&["-pc"])));
    assert!(!cc.has_session_arg(&[]));
    assert!(!cc.has_session_arg(&strings(&["--", "--resume"])));
    assert!(!cc.has_session_arg(&strings(&["--append-system-prompt", "--teleport"])));
    // L2 round 2 item 8: the `=` form. The pinned help uses it for a required value (`--output-format=stream-json`)
    // and for an optional value (`--debug=hooks,mcp`, `--tmux=classic`) of the same parser.
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/help.txt")).unwrap();
    for t in ["--output-format=stream-json", "--debug=hooks,mcp", "--tmux=classic", "--session-id <uuid>", "-r, --resume [value]"] {
        assert!(text.contains(t), "{t}");
    }
    assert_eq!(cc.preassign_args("U1"), strings(&["--session-id=U1"]));
    assert_eq!(cc.resume_args("U1"), strings(&["--resume=U1"]));
    for (a, name) in [(cc.preassign_args("U1"), "--session-id"), (cc.resume_args("U1"), "--resume")] {
        assert!(help.contains(a[0].split('=').next().unwrap()), "{}", a[0]);
        // the parse check: W4's walk reads one session flag with the value `U1`
        assert!(cc.has_session_arg(&a), "{a:?}");
        let o = zygo_workspace::args::occurrences(Agent::ClaudeCode, &a);
        assert_eq!(o.iter().map(|o| (o.name, o.values.clone())).collect::<Vec<_>>(), [(name, strings(&["U1"]))]);
    }
}

#[test] // D5, L2 round 2 item 9: after `--`, `--agents`, `--bare` and `--model` are prompt words
fn flags_after_double_dash_are_prompt_words() {
    let fx = fx();
    let cc = cc(&fx);
    let mut c = ctx(&fx, "");
    c.effective_role = None;
    let tail = strings(&["--", "--agents", "x", "--bare", "--model", "opus", "--print"]);
    c.args = tail.clone();
    assert_eq!(cc.refusal(&c), None);
    let spec = cc.build(&c).unwrap();
    assert!(spec.args.ends_with(&tail), "{:?}", spec.args);
    assert_eq!(spec.args.iter().filter(|a| *a == "--agents").count(), 2, "{:?}", spec.args);
    assert!(spec.log.is_empty(), "{:?}", spec.log);
    assert_eq!(cc.picked_model(&c.args), None);
    // a flag value is not a flag
    c.args = strings(&["--append-system-prompt", "--bare", "--system-prompt", "--model", "--append-system-prompt", "--agents"]);
    assert_eq!(cc.refusal(&c), None);
    let spec = cc.build(&c).unwrap();
    assert!(spec.log.is_empty(), "{:?}", spec.log);
    assert!(spec.args.ends_with(&c.args), "{:?}", spec.args);
    assert_eq!(cc.picked_model(&c.args), None);
    // the user's `--agents` before `--` is still merged and stripped
    c.args = strings(&["--agents", r#"{"mine":{"description":"d","prompt":"p"}}"#, "--", "--agents"]);
    let spec = cc.build(&c).unwrap();
    assert_eq!(spec.args.iter().filter(|a| *a == "--agents").count(), 2, "{:?}", spec.args);
    assert!(agents_of(&spec.args[..spec.args.len() - 2]).get("mine").is_some());
    assert!(spec.args.ends_with(&strings(&["--", "--agents"])), "{:?}", spec.args);
}

#[test] // AU59, AU102: the cmd layer uses the launcher's rule
fn user_settings_rule_is_public_and_stops_at_double_dash() {
    use zygo_launch::claude_code::has_user_settings;
    assert!(has_user_settings(&strings(&["--settings", "f"])));
    assert!(has_user_settings(&strings(&["--model", "opus", "--settings=f"])));
    assert!(!has_user_settings(&strings(&["--", "--settings", "f"])));
    assert!(!has_user_settings(&strings(&["--append-system-prompt", "--settings"])));
    assert!(!has_user_settings(&[]));
}

struct Bare;
impl AgentAdapter for Bare {
    fn agent(&self) -> Agent {
        Agent::ClaudeCode
    }
    fn program(&self) -> &str {
        "bare"
    }
    fn install_hint(&self) -> &str {
        ""
    }
    fn picked_model(&self, _args: &[String]) -> Option<String> {
        None
    }
    fn build(&self, _ctx: &LaunchCtx) -> anyhow::Result<zygo_launch::ChildSpec> {
        anyhow::bail!("unused")
    }
    fn preflight(&self, _ctx: &LaunchCtx) -> Result<(), Preflight> {
        Ok(())
    }
}

#[test] // the default bodies: no session argument, empty pre-assign, empty resume
fn session_methods_default_to_none() {
    assert!(!Bare.has_session_arg(&strings(&["--resume", "x", "--session-id", "y"])));
    assert!(Bare.preassign_args("U1").is_empty());
    assert!(Bare.resume_args("U1").is_empty());
}
