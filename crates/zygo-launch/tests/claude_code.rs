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
use zygo_launch::claude_code::{shell_quote, simple_mode, ClaudeCode};
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
    ClaudeCode::new("claude", fx.managed.clone(), fx.root.join("bin").join("zygo"))
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
    let mut a = cc(&fx);
    a.zygo = PathBuf::new();
    let e = a.build(&c).err().unwrap();
    assert_eq!(e.to_string(), "cannot find the path of the zygo binary");
}

#[cfg(unix)]
#[test]
fn non_utf8_zygo_path_is_refused_not_rewritten() {
    use std::os::unix::ffi::OsStrExt;
    let fx = fx();
    let c = ctx(&fx, "");
    let zygo = PathBuf::from(OsStr::from_bytes(b"/opt/zy\xffgo/zygo"));
    let mut a = cc(&fx);
    a.zygo = zygo;
    let e = a.build(&c).err().unwrap();
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
/// §8.5 Flags (FIX-23): the merge line when the merged settings set `disableAllHooks: true`.
const MERGE_HOOKS_OFF: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included";

/// §8.5 Flags (FIX-23 D4): the line for a user `disableAllHooks` value that is not `true` or `false`; `kind` is its JSON
/// type with its article (`a string`, `a number`, `an object`, `an array`, `null`).
fn bad_hooks(kind: &str) -> String {
    format!("zygo: your --settings disableAllHooks value is {kind}, not true or false; it was removed")
}

/// Characters that `serde_json` leaves raw in a JSON string and that `zygo_core::esc` escapes (FIX-23 D6): the C1
/// controls CSI and NEL, DEL, the line separator and the Arabic letter mark.
const RAW: [char; 5] = ['\u{9b}', '\u{85}', '\u{7f}', '\u{2028}', '\u{61c}'];

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

#[cfg(unix)]
#[test] // FIX-26 (D10, D12), §8.5 Subagents: kills: the --agents file read again in build after refusal read it, so a pipe is empty there and the user's agents are dropped with no line; an --agents pipe refused
fn print_agents_from_a_pipe_are_used() {
    let fx = fx();
    let (_r, value) = pipe_with(r#"{"helper": {"description": "h", "prompt": "y"}}"#);
    let mut c = ctx(&fx, "");
    c.args = strings(&["--print", "--agents", &value]);
    let a = cc(&fx);
    assert_eq!(a.refusal(&c), None);
    let spec = a.build(&c).unwrap();
    assert_eq!(agents_of(&spec.args)["helper"]["prompt"], "y");
}

#[cfg(unix)]
#[test] // FIX-26 (D3, D10, D12), §8.5 Subagents: kills: the --print --agents pipe read with O_NONBLOCK or ended at once, so a writer that writes after the launch starts is dropped and the launch is refused; the file read twice
fn print_agents_from_a_pipe_with_a_late_writer_are_used() {
    let fx = Arc::new(fx());
    let f = Arc::clone(&fx);
    let (refusal, agents) = late_pipe(r#"{"helper": {"description": "h", "prompt": "y"}}"#, move |value| {
        let mut c = ctx(&f, "");
        c.args = strings(&["--print", "--agents", &value]);
        let a = cc(&f);
        let refusal = a.refusal(&c);
        let spec = a.build(&c).unwrap();
        (refusal, agents_of(&spec.args))
    });
    assert_eq!(refusal, None);
    assert_eq!(agents["helper"]["prompt"], "y");
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
    assert_eq!(spec.lines, [NO_FILE]);
    let c = ctx(&fx, "");
    let spec = cc(&fx).build(&c).unwrap();
    assert!(spec.log.is_empty());
    assert!(spec.lines.is_empty());
}

/// §8.5 Flags (FIX-26 D2): the line for a user `--settings` value that names an entry zygo cannot read as a JSON object;
/// `why` is the error.
fn unreadable(value: &str, why: &str) -> String {
    format!("zygo: your --settings value {} could not be read as a JSON object and was not used: {why}", json!(value))
}

/// §8.5 Flags (FIX-26 D2): the line for a user `--settings` value that is not a JSON object and has no entry at its
/// path (no file found, or on Windows a name the system refuses).
const NO_FILE: &str = "zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used";

/// §8.5 Flags (FIX-26 D2): the line for a value with no entry at its path for another reason than a missing file.
#[cfg(unix)]
fn not_opened(why: &str) -> String {
    format!("zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: {why}; it was not used")
}

/// The error in `unreadable` for a file that holds `not json` (FIX-26 D4).
const NOT_JSON: &str = "syntax error at line 1 column 2";
/// The error in `unreadable` for a file that holds JSON that is not an object (FIX-26 D7).
const NOT_OBJECT: &str = "the file holds JSON that is not an object";

/// The system error text for `code`.
#[cfg(unix)]
fn os_error(code: i32) -> String {
    std::io::Error::from_raw_os_error(code).to_string()
}

/// The lines of a build with the user arguments `args`.
fn lines_of(fx: &Fx, args: &[&str]) -> Vec<String> {
    let mut c = ctx(fx, "");
    c.args = strings(args);
    cc(fx).build(&c).unwrap().lines
}

/// `text` in an anonymous pipe whose write end is closed, and the `/dev/fd/<n>` path of its read end: the form of
/// bash `<(printf ...)`. The data is in the pipe before the build starts.
#[cfg(unix)]
fn pipe_with(text: &str) -> (std::io::PipeReader, String) {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    let (r, mut w) = std::io::pipe().unwrap();
    w.write_all(text.as_bytes()).unwrap();
    drop(w);
    let path = format!("/dev/fd/{}", r.as_raw_fd());
    (r, path)
}

/// Runs `f` on its own thread and gives its result, or fails the test: a timeout means a hang in the code under test,
/// a disconnect means the thread panicked in its setup. The 20 s bound is a guard of the test only; zygo has no time
/// limit.
#[cfg(unix)]
fn no_hang<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok(v) => v,
        Err(RecvTimeoutError::Timeout) => panic!("{what}: did not return within 20 s: it waits"),
        Err(RecvTimeoutError::Disconnected) => panic!("{what}: test setup failed: the thread ended with no result (its panic is above)"),
    }
}

/// Runs `f` with the `/dev/fd/<n>` path of an anonymous pipe whose write end the test keeps open, checks that `f` has
/// not returned after 1 s (it waits for the writer), then writes `text`, closes the write end and gives the result of
/// `f`: a writer that writes after the build starts (FIX-26 D3). The bounds are guards of the test only.
#[cfg(unix)]
fn late_pipe<T: Send + std::fmt::Debug + 'static>(text: &str, f: impl FnOnce(String) -> T + Send + 'static) -> T {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Duration;
    let (r, mut w) = std::io::pipe().unwrap();
    let path = format!("/dev/fd/{}", r.as_raw_fd());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(f(path));
    });
    match rx.recv_timeout(Duration::from_secs(1)) {
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => panic!("test setup failed: the thread ended with no result (its panic is above)"),
        Ok(v) => panic!("returned before the writer wrote, so a late writer is dropped: {v:?}"),
    }
    w.write_all(text.as_bytes()).unwrap();
    drop(w);
    let v = match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(v) => v,
        Err(RecvTimeoutError::Timeout) => panic!("did not return within 20 s after the writer closed the pipe"),
        Err(RecvTimeoutError::Disconnected) => panic!("test setup failed: the thread ended with no result (its panic is above)"),
    };
    drop(r);
    v
}

/// Makes a FIFO at `p` (test setup).
#[cfg(unix)]
fn mkfifo(p: &Path) {
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: `c` is a NUL-terminated path that lives for the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "test setup: mkfifo {}", p.display());
}

#[test] // FIX-21 round 3, §8.5 Flags: kills: an unreadable user `--settings` value passed on after zygo's file (Claude
        // Code then exits 1 or runs with neither file), the attribution keys or the guard hook lost, no line or a line
        // that does not name the value, a readable value next to it not merged; FIX-26: kills: a value with no entry printed
fn unreadable_user_settings_keep_only_zygos_file() {
    let fx = fx();
    std::fs::write(fx.cwd.join("bad.json"), "not json").unwrap();
    std::fs::write(fx.cwd.join("list.json"), "[1]").unwrap();
    std::fs::write(fx.cwd.join("good.json"), r#"{"outputStyle": "x"}"#).unwrap();
    let cases: [(Vec<&str>, Vec<String>); 7] = [
        (vec!["--settings", "missing.json", "-p", "hi"], vec![NO_FILE.to_string()]),
        (vec!["--settings", "bad.json", "-p", "hi"], vec![unreadable("bad.json", NOT_JSON)]),
        (vec!["--settings", "list.json", "-p", "hi"], vec![unreadable("list.json", NOT_OBJECT)]),
        (vec!["--settings", "{bad", "-p", "hi"], vec![NO_FILE.to_string()]),
        (vec!["--settings=[1]", "-p", "hi"], vec![NO_FILE.to_string()]),
        (vec!["--settings", "\"two\nlines\"", "-p", "hi"], vec![NO_FILE.to_string()]),
        (vec!["--settings", "good.json", "--settings", "missing.json", "-p", "hi"], vec![MERGE_WARN.to_string(), NO_FILE.to_string()]),
    ];
    for (args, lines) in cases {
        let mut c = ctx(&fx, "");
        c.args = strings(&args);
        let cc = cc(&fx);
        let spec = cc.build(&c).unwrap();
        assert_eq!(flag(&spec.args, "--settings"), [fx.sd.settings_json().to_string_lossy().into_owned()], "{args:?}");
        assert!(!spec.args.iter().any(|a| a.starts_with("--settings=")), "{args:?}");
        assert!(spec.args.ends_with(&strings(&["-p", "hi"])), "{args:?}: {:?}", spec.args);
        assert_eq!(spec.lines, lines, "{args:?}");
        assert!(spec.lines.iter().all(|l| !l.contains('\n')), "{args:?}");
        assert_eq!(spec.log, ["degraded: --settings"], "{args:?}");
        let s = settings(&fx);
        assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{args:?}");
        assert_eq!(s["includeCoAuthoredBy"], json!(false), "{args:?}");
        let guard = (Some("Edit|Write|NotebookEdit|Read|Bash".to_string()), format!("{} hook guard", shell_quote(&cc.zygo)));
        assert!(hook_commands(&s, "PreToolUse").contains(&guard), "{args:?}");
        assert_eq!(s.get("outputStyle").is_some(), args.contains(&"good.json"), "{args:?}");
    }
}

#[test] // FIX-21 round 3, FIX-23, §8.5 Flags: kills: a user disableAllHooks that does not win; an attribution key lost; the "hooks stay in force" line for settings that stop every hook
fn user_disable_all_hooks_wins_and_attribution_keys_stay() {
    let fx = fx();
    let mut c = ctx(&fx, "");
    c.args = strings(&[r#"--settings={"disableAllHooks": true, "attribution": {"commit": "x"}, "includeCoAuthoredBy": true}"#]);
    let spec = cc(&fx).build(&c).unwrap();
    let s = settings(&fx);
    assert_eq!(s["disableAllHooks"], json!(true));
    assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}));
    assert_eq!(s["includeCoAuthoredBy"], json!(false));
    assert_eq!(spec.lines, [MERGE_HOOKS_OFF]);
    assert!(!spec.lines.iter().any(|l| l.contains("hooks stay in force")), "{:?}", spec.lines);
}

#[test] // FIX-26 (D1, D2), §8.5 Flags: kills: a value with no entry at its path printed (a BOM or U+200B prefix, an array, a broken array, a JSON string, broken inline JSON on a member launch or in the = form), so a secret reaches stderr; a first-character rule that takes the file "{x" as inline; on Windows an invalid name given the open-error line in place of NO_FILE; a printed path with a raw bidi control
fn settings_value_text_is_printed_only_for_a_file() {
    let fx = fx();
    const BAD_INLINE: &str = r#"{"env": {"TOKEN":"s3cret"}, "disableAllHooks": tru}"#;
    let rlo = "a\u{202e}b.json";
    std::fs::write(fx.cwd.join("{x"), "not json").unwrap();
    std::fs::write(fx.cwd.join(rlo), "not json").unwrap();
    let bom = "\u{feff}{\"env\": {\"TOKEN\": \"s3cret\"}}";
    let zwsp = "\u{200b}{\"env\": {\"TOKEN\": \"s3cret\"}}";
    let eq_form = format!("--settings={BAD_INLINE}");
    type Row<'a> = (&'a str, bool, Vec<&'a str>, Vec<String>);
    let rows: [Row; 10] = [
        ("BOM prefix", false, vec!["--settings", bom], vec![NO_FILE.to_string()]),
        ("U+200B prefix", false, vec!["--settings", zwsp], vec![NO_FILE.to_string()]),
        ("array", false, vec!["--settings", r#"["s3cret"]"#], vec![NO_FILE.to_string()]),
        ("broken array", false, vec!["--settings", r#"["s3cret""#], vec![NO_FILE.to_string()]),
        ("JSON string", false, vec!["--settings", r#""s3cret""#], vec![NO_FILE.to_string()]),
        ("missing path", false, vec!["--settings", "nofile.json"], vec![NO_FILE.to_string()]),
        ("broken inline, member", true, vec!["--settings", BAD_INLINE], vec![NO_FILE.to_string()]),
        ("broken inline, = form", false, vec![&eq_form], vec![NO_FILE.to_string()]),
        ("file named {x", false, vec!["--settings", "{x"], vec![unreadable("{x", NOT_JSON)]),
        ("U+202E path", false, vec!["--settings", rlo], vec![r#"zygo: your --settings value "a\u{202e}b.json" could not be read as a JSON object and was not used: syntax error at line 1 column 2"#.to_string()]),
    ];
    let mut failed = vec![];
    for (name, member, args, lines) in rows {
        let mut c = if member { member_ctx(&fx, true) } else { ctx(&fx, "") };
        c.args = strings(&args);
        let spec = cc(&fx).build(&c).unwrap();
        let ok = spec.lines == lines && !spec.lines.iter().any(|l| l.contains("s3cret") || l.contains('\u{202e}'));
        if !ok {
            failed.push(format!("{name}: {:?}", spec.lines));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[cfg(unix)]
#[test] // FIX-26 (D2, D6), §8.5 Flags: kills: an error with no entry at the path (ENOTDIR, ELOOP in a folder part) given NO_FILE or printed with the value; an entry at the path (a link loop at the last part, a dangling link, a directory) given NO_FILE or the open-error line
fn settings_path_errors_show_the_value_only_for_an_entry() {
    let fx = fx();
    std::fs::write(fx.cwd.join("file.json"), "{}").unwrap();
    std::os::unix::fs::symlink("loop.json", fx.cwd.join("loop.json")).unwrap();
    std::os::unix::fs::symlink("nowhere.json", fx.cwd.join("dangle.json")).unwrap();
    std::fs::create_dir(fx.cwd.join("dir.json")).unwrap();
    let rows: [(&str, Vec<String>, &str); 5] = [
        ("file.json/x", vec![not_opened(&os_error(libc::ENOTDIR))], "file.json"),
        ("loop.json/x", vec![not_opened(&os_error(libc::ELOOP))], "loop.json"),
        ("loop.json", vec![unreadable("loop.json", &os_error(libc::ELOOP))], ""),
        ("dangle.json", vec![unreadable("dangle.json", &os_error(libc::ENOENT))], ""),
        ("dir.json", vec![unreadable("dir.json", &os_error(libc::EISDIR))], ""),
    ];
    for (value, lines, hidden) in rows {
        let got = lines_of(&fx, &["--settings", value]);
        assert_eq!(got, lines, "{value}");
        assert!(hidden.is_empty() || !got.iter().any(|l| l.contains(hidden)), "{value}: {got:?}");
    }
}

#[cfg(unix)]
#[test] // FIX-26 (D3), §8.5 Flags: kills: a user --settings link not followed (O_NOFOLLOW, or a type check on the link itself)
fn settings_link_to_a_file_is_followed() {
    let fx = fx();
    std::fs::write(fx.cwd.join("good.json"), r#"{"outputStyle": "x"}"#).unwrap();
    std::os::unix::fs::symlink("good.json", fx.cwd.join("link.json")).unwrap();
    assert_eq!(lines_of(&fx, &["--settings", "link.json"]), [MERGE_WARN]);
    assert_eq!(settings(&fx)["outputStyle"], "x");
}

#[cfg(unix)]
#[test] // FIX-26 (D3), §8.5 Flags: kills: a pipe (the <(…) form /dev/fd/<n>) refused as not a regular file, or a link to one not followed, so process substitution no longer works
fn settings_from_a_pipe_are_used() {
    let fx = fx();
    let (_r1, direct) = pipe_with(r#"{"outputStyle": "x"}"#);
    let (_r2, target) = pipe_with(r#"{"theme": "dark"}"#);
    std::os::unix::fs::symlink(&target, fx.cwd.join("pipelink.json")).unwrap();
    assert_eq!(lines_of(&fx, &["--settings", &direct, "--settings", "pipelink.json"]), [MERGE_WARN]);
    let s = settings(&fx);
    assert_eq!((s["outputStyle"].as_str(), s["theme"].as_str()), (Some("x"), Some("dark")));
}

#[cfg(unix)]
#[test] // FIX-26 (D3), §8.5 Flags: kills: a user --settings pipe read with O_NONBLOCK or with a type check that ends the read at once, so a writer that writes after the build starts is dropped (an eof error line in place of the merge)
fn settings_from_a_pipe_with_a_late_writer_are_used() {
    let fx = Arc::new(fx());
    let f = Arc::clone(&fx);
    let lines = late_pipe(r#"{"outputStyle": "x"}"#, move |value| lines_of(&f, &["--settings", &value]));
    assert_eq!(lines, [MERGE_WARN]);
    assert_eq!(settings(&fx)["outputStyle"], "x");
}

#[cfg(unix)]
#[test] // FIX-26 (D2), §8.5 Flags: kills: EACCES on a parent folder (no entry found) given NO_FILE, or printed with the value
fn settings_under_unsearchable_folder_are_not_shown() {
    use std::os::unix::fs::PermissionsExt;
    /// Gives the folder its search right back, also when an assertion fails, so the test folder can be removed.
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        }
    }
    let fx = fx();
    let locked = fx.cwd.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("x.json"), "{}").unwrap();
    let _restore = Restore(locked.clone());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
    if std::fs::symlink_metadata(locked.join("x.json")).is_ok() {
        eprintln!("note: settings_under_unsearchable_folder_are_not_shown skipped: the test runs as root, so the folder can be searched");
        return;
    }
    let lines = lines_of(&fx, &["--settings", "locked/x.json"]);
    assert_eq!(lines, [not_opened(&os_error(libc::EACCES))]);
    assert!(!lines.iter().any(|l| l.contains("locked") || l.contains("x.json")), "{lines:?}");
}

#[cfg(unix)]
#[test] // FIX-26 (D2, D6), §8.5 Flags: kills: a file that exists but cannot be opened given NO_FILE or the open-error line with no path
fn settings_file_with_mode_0_names_the_path_and_error() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fx();
    let p = fx.cwd.join("noperm.json");
    std::fs::write(&p, "{}").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&p).is_ok() {
        eprintln!("note: settings_file_with_mode_0_names_the_path_and_error skipped: the test runs as root, so mode 0 does not stop an open");
        return;
    }
    assert_eq!(lines_of(&fx, &["--settings", "noperm.json"]), [unreadable("noperm.json", &os_error(libc::EACCES))]);
}

#[cfg(unix)]
#[test] // FIX-26 (D2), §8.5 Flags: kills: ENAMETOOLONG (ErrorKind::InvalidFilename on unix) given NO_FILE, that is the Windows invalid-name rule applied on unix; broken inline JSON longer than NAME_MAX printed
fn long_broken_inline_settings_are_not_shown() {
    let fx = fx();
    let head = r#"{"env": {"TOKEN": "s3cret"}, "pad": ""#;
    let tail = r#"", tru}"#;
    let value = format!("{head}{}{tail}", "x".repeat(400 - head.len() - tail.len()));
    assert_eq!(value.len(), 400);
    let lines = lines_of(&fx, &["--settings", &value]);
    assert_eq!(lines, [not_opened(&os_error(libc::ENAMETOOLONG))]);
    assert!(!lines.iter().any(|l| l.contains("s3cret") || l.contains("TOKEN")), "{lines:?}");
}

#[test] // FIX-26 (D4), §8.5 Flags: kills: serde_json's error text printed (for a data error it holds the literal 1234567890123 through the $serde_json::private::Number key); a category written as the wrong word; the position lost
fn settings_file_parse_errors_name_only_category_and_position() {
    let fx = fx();
    let cases = [
        ("syntax.json", "not json", "syntax error at line 1 column 2"),
        ("data.json", r#"{"$serde_json::private::Number": 1234567890123}"#, "data error at line 1 column 46"),
        ("eof.json", r#"{"a": 1"#, "eof error at line 1 column 7"),
    ];
    for (file, text, error) in cases {
        std::fs::write(fx.cwd.join(file), text).unwrap();
        let lines = lines_of(&fx, &["--settings", file]);
        assert_eq!(lines, [unreadable(file, error)], "{file}");
        assert!(!lines.iter().any(|l| l.contains("1234567890123") || l.contains("expected") || l.contains("EOF while")), "{lines:?}");
    }
}

#[test] // FIX-23 (D1, D3, D4), §8.5 Flags: kills: the "hooks stay in force" line for merged settings that stop every hook; the new line for a user value that a later false overrides; the line for other user settings changed; a non-boolean disableAllHooks passed on, not named by its type, or printed with its value; a lost unreadable-value line
fn merge_line_is_true_when_disable_all_hooks_is_set() {
    let fx = fx();
    std::fs::write(fx.cwd.join("off.json"), r#"{"disableAllHooks": true}"#).unwrap();
    type Row<'a> = (&'a str, bool, &'a [&'a str], Vec<String>, bool, &'a str);
    let rows: [Row; 14] = [
        ("inline true", false, &[r#"--settings={"disableAllHooks": true}"#], vec![MERGE_HOOKS_OFF.to_string()], true, ""),
        ("file true", false, &["--settings", "off.json", "-p", "hi"], vec![MERGE_HOOKS_OFF.to_string()], true, ""),
        ("inline false", false, &["--settings", r#"{"disableAllHooks": false}"#], vec![MERGE_WARN.to_string()], false, ""),
        (
            "true then false",
            false,
            &["--settings", r#"{"disableAllHooks": true}"#, "--settings", r#"{"disableAllHooks": false}"#],
            vec![MERGE_WARN.to_string()],
            false,
            "",
        ),
        ("true then other", false, &["--settings", "off.json", "--settings", r#"{"outputStyle": "x"}"#], vec![MERGE_HOOKS_OFF.to_string()], true, ""),
        ("other only", false, &["--settings", r#"{"outputStyle": "x"}"#], vec![MERGE_WARN.to_string()], false, ""),
        (
            "true and unreadable",
            false,
            &["--settings", "off.json", "--settings", "missing.json"],
            vec![MERGE_HOOKS_OFF.to_string(), NO_FILE.to_string()],
            true,
            "",
        ),
        ("member true", true, &["--settings", "off.json"], vec![MERGE_HOOKS_OFF.to_string()], true, ""),
        ("string true", false, &["--settings", r#"{"disableAllHooks": "true"}"#], vec![MERGE_WARN.to_string(), bad_hooks("a string")], false, r#""true""#),
        ("number", false, &["--settings", r#"{"disableAllHooks": 17}"#], vec![MERGE_WARN.to_string(), bad_hooks("a number")], false, "17"),
        ("null", false, &["--settings", r#"{"disableAllHooks": null}"#], vec![MERGE_WARN.to_string(), bad_hooks("null")], false, ""),
        (
            "object",
            false,
            &["--settings", "{\"disableAllHooks\": {\"token\": \"s3cret\\ny\"}}"],
            vec![MERGE_WARN.to_string(), bad_hooks("an object")],
            false,
            "s3cret",
        ),
        (
            "true then string",
            false,
            &["--settings", "off.json", "--settings", r#"{"disableAllHooks": "s3cret"}"#],
            vec![MERGE_HOOKS_OFF.to_string(), bad_hooks("a string")],
            true,
            "s3cret",
        ),
        (
            "number and unreadable",
            false,
            &["--settings", r#"{"disableAllHooks": 17}"#, "--settings", "missing.json"],
            vec![MERGE_WARN.to_string(), bad_hooks("a number"), NO_FILE.to_string()],
            false,
            "17",
        ),
    ];
    for (name, member, args, lines, off, hidden) in rows {
        let mut c = if member { member_ctx(&fx, true) } else { ctx(&fx, "") };
        c.args = strings(args);
        let spec = cc(&fx).build(&c).unwrap();
        let s = settings(&fx);
        assert_eq!(spec.lines, lines, "{name}");
        assert!(spec.lines.iter().all(|l| !l.contains('\n')), "{name}");
        assert!(hidden.is_empty() || !spec.lines.iter().any(|l| l.contains(hidden)), "{name}: {:?}", spec.lines);
        assert_eq!(s["disableAllHooks"], json!(off), "{name}");
        assert_eq!(spec.lines.iter().any(|l| l.contains("hooks stay in force")), !off, "{name}");
        assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{name}");
        assert_eq!(s["includeCoAuthoredBy"], json!(false), "{name}");
        assert_eq!(spec.log, ["degraded: --settings"], "{name}");
    }
}

#[test] // FIX-23 (D3, D4), §8.5 Flags: kills: a non-boolean value from a file, an array or a long string passed on, not named by its type, or printed with its value; a non-boolean value that hides a later boolean; two values out of order or merged into one line; a member launch that passes a non-boolean value on
fn non_boolean_disable_all_hooks_edges() {
    let fx = fx();
    std::fs::write(fx.cwd.join("off.json"), r#"{"disableAllHooks": true}"#).unwrap();
    std::fs::write(fx.cwd.join("yes.json"), r#"{"disableAllHooks": "s3cret-yes"}"#).unwrap();
    let big = "x".repeat(1 << 20);
    let big_inline = format!("{{\"disableAllHooks\": \"{big}\"}}");
    type Row<'a> = (&'a str, bool, Vec<&'a str>, Vec<String>, bool, &'a str);
    let rows: [Row; 8] = [
        ("file string", false, vec!["--settings", "yes.json"], vec![MERGE_WARN.to_string(), bad_hooks("a string")], false, "s3cret"),
        ("array", false, vec!["--settings", r#"{"disableAllHooks": [true, "s3cret"]}"#], vec![MERGE_WARN.to_string(), bad_hooks("an array")], false, "s3cret"),
        ("1 MB string", false, vec!["--settings", &big_inline], vec![MERGE_WARN.to_string(), bad_hooks("a string")], false, "xxxx"),
        (
            "number then true",
            false,
            vec!["--settings", r#"{"disableAllHooks": 17}"#, "--settings", "off.json"],
            vec![MERGE_HOOKS_OFF.to_string(), bad_hooks("a number")],
            true,
            "17",
        ),
        (
            "number then false",
            false,
            vec!["--settings", r#"{"disableAllHooks": 17}"#, "--settings", r#"{"disableAllHooks": false}"#],
            vec![MERGE_WARN.to_string(), bad_hooks("a number")],
            false,
            "17",
        ),
        (
            "true, number, false",
            false,
            vec!["--settings", "off.json", "--settings", r#"{"disableAllHooks": 17}"#, "--settings", r#"{"disableAllHooks": false}"#],
            vec![MERGE_WARN.to_string(), bad_hooks("a number")],
            false,
            "17",
        ),
        (
            "two non-boolean",
            false,
            vec!["--settings", r#"{"disableAllHooks": 17}"#, "--settings", r#"{"disableAllHooks": "s3cret-two"}"#],
            vec![MERGE_WARN.to_string(), bad_hooks("a number"), bad_hooks("a string")],
            false,
            "s3cret",
        ),
        ("member number", true, vec!["--settings", r#"{"disableAllHooks": 17}"#], vec![MERGE_WARN.to_string(), bad_hooks("a number")], false, "17"),
    ];
    for (name, member, args, lines, off, hidden) in rows {
        let mut c = if member { member_ctx(&fx, true) } else { ctx(&fx, "") };
        c.args = strings(&args);
        let spec = cc(&fx).build(&c).unwrap();
        let s = settings(&fx);
        assert!(spec.lines == lines, "{name}: {} lines", spec.lines.len());
        assert!(spec.lines.iter().all(|l| !l.contains('\n')), "{name}");
        assert!(!spec.lines.iter().any(|l| l.contains(hidden)), "{name}: a line holds the value");
        assert_eq!(spec.lines.iter().filter(|l| l.contains("disableAllHooks value")).count(), lines.len() - 1, "{name}");
        assert_eq!(s["disableAllHooks"], json!(off), "{name}");
        assert_eq!(spec.lines[0] == MERGE_HOOKS_OFF, off, "{name}: the merge line and the merged file disagree");
        assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{name}");
    }
}

#[test] // FIX-23 (D4, D6), §8.5 Flags: kills: an unreadable value printed with a raw DEL, C1 control, line separator or bidi control, so one line can look like two or move the cursor; a non-boolean disableAllHooks printed with its value
fn settings_lines_escape_every_control_character() {
    let fx = fx();
    let hooks = format!("{{\"disableAllHooks\": \"a{}b{}c{}d{}e{}f\"}}", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
    let path = format!("m{}i{}s{}s{}x{}.json", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
    std::fs::write(fx.cwd.join(&path), "not json").unwrap();
    let mut c = ctx(&fx, "");
    c.args = strings(&["--settings", &hooks, "--settings", &path]);
    let spec = cc(&fx).build(&c).unwrap();
    assert_eq!(
        spec.lines,
        [
            MERGE_WARN.to_string(),
            bad_hooks("a string"),
            r#"zygo: your --settings value "m\u{9b}i\u{85}s\u{7f}s\u{2028}x\u{61c}.json" could not be read as a JSON object and was not used: syntax error at line 1 column 2"#.to_string(),
        ]
    );
    for l in &spec.lines {
        assert!(!l.contains('\n') && !l.contains(RAW), "{l:?}");
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

/// FIX-24 diff review round 3, finding 1: Claude Code 2.1.331's own rule for `CLAUDE_CODE_SIMPLE`
/// (`De(process.env.CLAUDE_CODE_SIMPLE)`: lower case, trim, one of 1, true, yes, on).
#[test]
fn simple_mode_is_claude_codes_rule() {
    for v in ["1", "TRUE", " yes ", "on", "True", "\tON\n"] {
        assert!(simple_mode(Some(OsStr::new(v))), "{v:?}");
    }
    for v in ["false", "no", "off", "2", " 0", "0", "", " ", "y", "1 1"] {
        assert!(!simple_mode(Some(OsStr::new(v))), "{v:?}");
    }
    assert!(!simple_mode(None), "unset");
}

/// A catalog provider whose key variable is `CLAUDE_CODE_SIMPLE`: the deny list removes that name from the agent.
const SIMPLE_AS_KEY: &str = concat!(
    "[[model]]\nid = \"test-env\"\n[[model.provider]]\nname = \"p-env\"\nbase_url = \"https://example.invalid\"\n",
    "dialects = [\"anthropic\"]\nupstream_model = \"up-env\"\nkey = \"env:CLAUDE_CODE_SIMPLE\"\n"
);

/// FIX-24 diff review round 3, finding 4: bare mode from the environment that the agent gets is refused with
/// passthrough and logged as degraded, as `--bare` is.
#[test]
fn bare_mode_from_the_environment_is_refused_with_passthrough_and_logged() {
    let fx = fx();
    let run = |cfg: &str, value: Option<&str>, passthrough: bool| {
        let mut c = ctx(&fx, cfg);
        if let Some(v) = value {
            c.env.0.push(("CLAUDE_CODE_SIMPLE".into(), v.into()));
        }
        c.passthrough = passthrough;
        let refused = cc(&fx).refusal(&c);
        c.passthrough = false;
        (refused, cc(&fx).build(&c).unwrap().log)
    };
    for v in ["1", " yes ", "TRUE", "on"] {
        let (refused, log) = run("", Some(v), true);
        assert_eq!(refused.as_deref(), Some(PT_BARE_ENV), "{v:?}");
        assert!(log.contains(&"degraded: CLAUDE_CODE_SIMPLE".to_string()), "{v:?}: {log:?}");
    }
    for v in [Some("0"), Some("false"), Some(""), None] {
        let (refused, log) = run("", v, true);
        assert_eq!(refused, None, "{v:?}");
        assert!(!log.iter().any(|l| l.contains("CLAUDE_CODE_SIMPLE")), "{v:?}: {log:?}");
    }
    // The deny list removes a provider key variable, so the agent gets no CLAUDE_CODE_SIMPLE.
    let (refused, log) = run(SIMPLE_AS_KEY, Some("1"), true);
    assert_eq!(refused, None);
    assert!(!log.iter().any(|l| l.contains("CLAUDE_CODE_SIMPLE")), "{log:?}");
}

const PT_BARE_ENV: &str = "--passthrough does not work with CLAUDE_CODE_SIMPLE: bare mode reads only API keys";

#[cfg(unix)]
#[test]
fn passthrough_role_model_exits_2_through_the_launcher() {
    let fx = fx();
    script(&fx, "claude", "exit 0");
    let o = launch_opts(&fx, &["--model", "lead"], true);
    let err = stderr_of(&o);
    let adapter = ClaudeCode::new("claude", fx.managed.clone(), fx.bin.join("zygo"));
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
#[test] // FIX-23 (D1, D2, D4), §8.5 Flags: kills: the merge line or the disableAllHooks line lost or changed between build and stderr
fn disable_all_hooks_merge_line_reaches_stderr() {
    for (user, want, not) in [
        (r#"{"disableAllHooks": true}"#, MERGE_HOOKS_OFF.to_string(), "hooks stay in force"),
        (r#"{"outputStyle": "x"}"#, MERGE_WARN.to_string(), "disableAllHooks"),
        (r#"{"disableAllHooks": "s3cret"}"#, bad_hooks("a string"), "s3cret"),
    ] {
        let fx = fx();
        script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");
        let o = launch_opts(&fx, &["--settings", user], false);
        let err = stderr_of(&o);
        assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
        assert_eq!(text_of(&err).lines().filter(|l| *l == want).count(), 1, "{}", text_of(&err));
        assert!(!text_of(&err).contains(not), "{}", text_of(&err));
    }
}

#[cfg(unix)]
#[test] // FIX-23 (D3, D6), §8.5 Flags: kills: settings lines on stderr in value order and not grouped as merge line, disableAllHooks lines, unreadable lines; a raw control or bidi character on stderr
fn settings_lines_on_stderr_are_safe_and_in_order() {
    let fx = fx();
    script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");
    let hooks = format!("{{\"disableAllHooks\": \"a{}b{}c{}d{}e{}f\"}}", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
    let path = format!("m{}i{}s{}s{}x{}.json", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
    std::fs::write(fx.cwd.join(&path), "not json").unwrap();
    let args = ["--settings", &path, "--settings", &hooks, "--settings", "missing.json", "--settings", r#"{"disableAllHooks": 2}"#, "--settings", r#"{"outputStyle": "x"}"#];
    let o = launch_opts(&fx, &args, false);
    let err = stderr_of(&o);
    assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
    let text = text_of(&err);
    assert!(!text.contains(RAW), "{text:?}");
    let lines: Vec<&str> = text.lines().collect();
    let at = |want: &str| lines.iter().position(|l| *l == want).unwrap_or_else(|| panic!("{want:?} not in {text:?}"));
    let merge = at(MERGE_WARN);
    let bad1 = at(&bad_hooks("a string"));
    let bad2 = at(&bad_hooks("a number"));
    let path_line = at(r#"zygo: your --settings value "m\u{9b}i\u{85}s\u{7f}s\u{2028}x\u{61c}.json" could not be read as a JSON object and was not used: syntax error at line 1 column 2"#);
    let missing = at(NO_FILE);
    assert_eq!([merge + 1, merge + 2, merge + 3, merge + 4], [bad1, bad2, path_line, missing], "{text:?}");
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

#[cfg(unix)]
#[test] // FIX-26 (D9), §8.4 Preflight, §12.3: kills: a found settings file or the managed file opened with a blocking open, so a FIFO with no writer at <cwd>/.claude/settings.json or at the managed path makes the launch, the preflight or the passthrough refusal wait for ever
fn preflight_skips_a_found_fifo_with_no_hang() {
    let fx = Arc::new(fx());
    script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");
    std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
    mkfifo(&fx.cwd.join(".claude/settings.json"));
    mkfifo(&fx.managed);
    let f = Arc::clone(&fx);
    let (code, err) = no_hang("launch", move || {
        let o = launch_opts(&f, &[], false);
        let err = stderr_of(&o);
        (Launcher::run(&cc(&f), o).code, text_of(&err))
    });
    assert_eq!(code, 0, "{err}");
    let f = Arc::clone(&fx);
    let (preflight, refusal) = no_hang("preflight and passthrough refusal", move || {
        let mut c = ctx(&f, "");
        c.passthrough = true;
        let a = cc(&f);
        (a.preflight(&c), a.refusal(&c))
    });
    assert_eq!((preflight, refusal), (Ok(()), None));
}

#[cfg(target_os = "linux")]
#[test] // FIX-26 (D9), §8.4 Preflight, §12.3: kills: a found FIFO, a found link to a FIFO or a found device (here the managed path is a pseudo-terminal slave) opened before the type check, with O_NONBLOCK too: an IN_OPEN event on a path that only this test can open
fn preflight_does_not_open_a_found_fifo_link_or_device() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let fx = Arc::new(fx());
    std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
    let fifo = fx.cwd.join(".claude/settings.json");
    let other = fx.cwd.join("other-fifo");
    mkfifo(&fifo);
    mkfifo(&other);
    std::os::unix::fs::symlink(&other, fx.cwd.join(".claude/settings.local.json")).unwrap();
    // SAFETY: plain libc calls; every descriptor is owned below and closed on drop.
    let m = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if m < 0 {
        eprintln!("note: preflight_does_not_open_a_found_fifo_link_or_device skipped: posix_openpt failed ({})", std::io::Error::last_os_error());
        return;
    }
    // SAFETY: `m` is a new descriptor that nothing else owns.
    let master = unsafe { OwnedFd::from_raw_fd(m) };
    // SAFETY: `m` is open for the calls.
    if unsafe { libc::grantpt(m) } != 0 || unsafe { libc::unlockpt(m) } != 0 {
        eprintln!("note: preflight_does_not_open_a_found_fifo_link_or_device skipped: grantpt or unlockpt failed ({})", std::io::Error::last_os_error());
        return;
    }
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: `name` is a buffer of the given length.
    assert_eq!(unsafe { libc::ptsname_r(m, name.as_mut_ptr(), name.len()) }, 0, "test setup: ptsname_r");
    // SAFETY: `ptsname_r` wrote a NUL-terminated name.
    let slave = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_str().unwrap().to_string();
    // SAFETY: plain libc call.
    let i = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    assert!(i >= 0, "test setup: inotify_init1");
    // SAFETY: `i` is a new descriptor that nothing else owns.
    let ino = unsafe { OwnedFd::from_raw_fd(i) };
    for p in [fifo.clone(), other.clone(), PathBuf::from(&slave)] {
        let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `c` is a NUL-terminated path that lives for the call.
        if unsafe { libc::inotify_add_watch(i, c.as_ptr(), libc::IN_OPEN) } < 0 {
            eprintln!("note: preflight_does_not_open_a_found_fifo_link_or_device skipped: no inotify watch on {} ({})", p.display(), std::io::Error::last_os_error());
            return;
        }
    }
    let (f, s) = (Arc::clone(&fx), slave.clone());
    let (preflight, refusal) = no_hang("preflight and passthrough refusal", move || {
        let a = ClaudeCode::new("claude", PathBuf::from(s), f.root.join("bin").join("zygo"));
        let mut c = ctx(&f, "");
        let preflight = a.preflight(&c);
        c.passthrough = true;
        (preflight, a.refusal(&c))
    });
    assert_eq!((preflight, refusal), (Ok(()), None));
    let mut ev = [0u8; 256];
    // SAFETY: `ev` is a buffer of the given length; `i` is open (owned by `ino`).
    let n = unsafe { libc::read(i, ev.as_mut_ptr().cast(), ev.len()) };
    assert_eq!(n, -1, "a found FIFO, a link to a FIFO or the device {slave} was opened: inotify reported IN_OPEN");
    drop((ino, master));
}

#[cfg(unix)]
#[test] // FIX-26 (D9), §8.4 Preflight: kills: a found settings file that is a link not followed (O_NOFOLLOW, or a type check on the link itself), so its credential key does not stop the launch
fn preflight_follows_a_link_to_a_settings_file() {
    let fx = fx();
    put(&fx.cwd.join("real.json"), r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#);
    std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
    let link = fx.cwd.join(".claude/settings.json");
    std::os::unix::fs::symlink(fx.cwd.join("real.json"), &link).unwrap();
    assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), pf(link, "env.CLAUDE_CODE_USE_VERTEX"));
}

#[cfg(target_os = "linux")]
#[test] // FIX-26 (D9), §8.4 Preflight: kills: a found regular file skipped when the O_NONBLOCK open fails with EWOULDBLOCK (a lease) and is not opened again without O_NONBLOCK, so its credential key does not stop the launch
fn preflight_reads_a_leased_settings_file_after_a_blocking_open() {
    use std::os::unix::io::AsRawFd;
    use std::time::{Duration, Instant};
    let fx = fx();
    std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
    let p = fx.cwd.join(".claude/settings.json");
    std::fs::write(&p, r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#).unwrap();
    let holder = std::fs::File::open(&p).unwrap();
    let fd = holder.as_raw_fd();
    // The kernel sends SIGIO to a lease holder when another open waits; the test ignores it and restores it at the end.
    // SAFETY: a plain signal disposition change; no handler runs.
    let old = unsafe { libc::signal(libc::SIGIO, libc::SIG_IGN) };
    // SAFETY: `fd` is the open descriptor of `holder`, which lives for the test.
    if unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_WRLCK) } != 0 {
        let e = std::io::Error::last_os_error();
        // SAFETY: restores the disposition read above.
        unsafe { libc::signal(libc::SIGIO, old) };
        eprintln!("note: preflight_reads_a_leased_settings_file_after_a_blocking_open skipped: no write lease on the test folder ({e})");
        return;
    }
    let result = std::thread::scope(|s| {
        let pre = s.spawn(|| cc(&fx).preflight(&ctx(&fx, "")));
        // The O_NONBLOCK open fails with EWOULDBLOCK and starts the lease break, so the lease then reads F_RDLCK.
        let start = Instant::now();
        // SAFETY: as above.
        while unsafe { libc::fcntl(fd, libc::F_GETLEASE) } != libc::F_RDLCK && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(10));
        }
        // SAFETY: as above.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_UNLCK) }, 0, "release the lease");
        pre.join().unwrap()
    });
    // SAFETY: restores the disposition read above.
    unsafe { libc::signal(libc::SIGIO, old) };
    assert_eq!(result, pf(p, "env.CLAUDE_CODE_USE_VERTEX"));
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
    c.view.workspace = Some(ViewWorkspace { member: Member { ws_id: "0123456789abcdef".into(), pane: "p1".into(), ..Default::default() }, resume_note: None, ..Default::default() });
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

/// Option names of the Claude Code 2.1.331 binary (`fixtures/claude-code/options-2.1.331.txt`, FIX-31): each word that
/// starts with `-` of the third tab-separated field of a row that is not a `#` comment, a trailing `,` cut.
fn binary_option_names() -> BTreeSet<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/options-2.1.331.txt")).unwrap();
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .flat_map(|l| {
            l.split('\t').nth(2).unwrap().split_whitespace().filter(|w| w.starts_with('-')).map(|w| w.trim_end_matches(',').to_string()).collect::<Vec<_>>()
        })
        .collect()
}

fn no_workspace_hook(s: &Value) {
    for event in s["hooks"].as_object().unwrap().keys() {
        assert!(!hook_commands(s, event).iter().any(|(_, cmd)| cmd.contains("hook workspace")), "{event}");
    }
    for e in ["Stop", "StopFailure", "SubagentStart", "SessionEnd"] {
        assert!(s["hooks"].get(e).is_none(), "{e}");
    }
}

#[test] // W§9.1, AU57, W§15 §8.5: every member hook, with work units on and off
fn member_settings_hold_every_workspace_hook() {
    let schema = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/settings-schema.json")).unwrap();
    for e in ["Stop", "StopFailure", "SubagentStart", "SubagentStop", "PostToolUseFailure", "SessionEnd"] {
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
        // The tool count (W10 round 2): every tool call of a member, at its start and at its end.
        assert!(hook_commands(&s, "PreToolUse").contains(&(Some("*".to_string()), ws("pre-tool"))), "{work_units}");
        assert!(!hook_commands(&s, "PreToolUse").iter().any(|(m, c)| m.as_deref() == Some("Agent") && *c == ws("pre-tool")), "{work_units}");
        assert!(hook_commands(&s, "PostToolUse").contains(&(Some("*".to_string()), ws("tool-end"))), "{work_units}");
        let failure: BTreeSet<(Option<String>, String)> = hook_commands(&s, "PostToolUseFailure").into_iter().collect();
        let mut want = BTreeSet::from([(Some("*".to_string()), ws("tool-end"))]);
        if work_units {
            want.insert((Some("*".to_string()), format!("{z} hook work")));
        }
        assert_eq!(failure, want, "{work_units}");
        assert_eq!(hook_commands(&s, "SessionEnd"), [(None, ws("session-end"))], "{work_units}");
        // D4: the member part of `UserPromptSubmit` is its own entry; D3: a member's `SessionStart` carries the flag.
        assert_eq!(hook_commands(&s, "UserPromptSubmit"), [(None, format!("{z} hook prompt")), (None, ws("prompt"))], "{work_units}");
        assert_eq!(hook_commands(&s, "SessionStart"), [(None, format!("{z} hook session-start --event member"))], "{work_units}");
        assert_eq!(hook_commands(&s, "Stop"), [(None, ws("stop"))], "{work_units}");
        assert_eq!(hook_commands(&s, "StopFailure"), [(None, ws("stop"))], "{work_units}");
        assert_eq!(hook_commands(&s, "SubagentStart"), [(None, ws("subagent-start"))], "{work_units}");
        let sub: BTreeSet<(Option<String>, String)> = hook_commands(&s, "SubagentStop").into_iter().collect();
        let mut want = BTreeSet::from([(None, ws("subagent-stop"))]);
        if work_units {
            want.insert((None, format!("{z} hook work")));
        }
        assert_eq!(sub, want, "{work_units}");
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
    assert_eq!(names, ["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport", "--remote"]);
    let binary = binary_option_names();
    for f in names {
        assert!(help.contains(f) || binary.contains(f), "{f} is in neither the pinned --help nor the 2.1.331 option list");
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

#[test] // M5 round 3 finding 1: `for_zygo` keeps the default program and managed path and takes the given zygo binary
fn for_zygo_takes_the_given_binary() {
    let z = PathBuf::from("/opt/zygo/bin/zygo");
    let a = ClaudeCode::for_zygo(z.clone());
    let d = ClaudeCode::default();
    assert_eq!((a.program.as_str(), &a.managed_settings, &a.zygo), (d.program.as_str(), &d.managed_settings, &z));
}
