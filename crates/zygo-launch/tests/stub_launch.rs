//! N34: launch tests with a stub agent on every CI OS (§8.1 A-21, §20 #1). No process-environment change.

#[path = "stub_agent/harness.rs"]
mod harness;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use harness::*;
use serde_json::{json, Value};
use zygo_launch::launcher::LaunchOutcome;
use zygo_launch::AgentAdapter;

const SET: [&str; 11] = [
    "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_FABLE_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_GATEWAY_HINT_HEADERS", "ANTHROPIC_CUSTOM_HEADERS", "DISABLE_LOGIN_COMMAND", "DISABLE_LOGOUT_COMMAND",
];

/// A branch work source and a catalog provider whose key variable (`MYPROVIDER_KEY`) is named by no denylist prefix.
const CATALOG_CONFIG: &str = concat!(
    "[work]\nsources = [\"branch\"]\n",
    "[[model]]\nid = \"test-env\"\n[[model.provider]]\nname = \"p-env\"\nbase_url = \"https://example.invalid\"\n",
    "dialects = [\"anthropic\"]\nupstream_model = \"up-env\"\nkey = \"env:MYPROVIDER_KEY\"\n"
);

fn assert_no_denied(env: &BTreeMap<String, String>, set: &[&str]) {
    for k in env.keys() {
        let n = if cfg!(windows) { k.to_ascii_uppercase() } else { k.clone() };
        let denied = ["ANTHROPIC_", "OPENAI_", "OPENROUTER_", "CLAUDE_CODE_USE_"].iter().any(|p| n.starts_with(p))
            || n == "CLAUDE_CODE_OAUTH_TOKEN"
            || n == "ZYGO_TEAM_TOKEN";
        assert!(!denied || set.contains(&n.as_str()), "denylisted name reached the agent: {k}");
    }
}

#[test]
fn claude_code_child_environment_has_the_spec_rows_and_no_denylisted_name() {
    let rig = rig();
    git_repo(&rig.cwd, "main", "https://example.com/acme/widgets.git");
    std::fs::write(&rig.paths.config_file, CATALOG_CONFIG).unwrap();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.env.0.extend(denied_parent()));
    assert_eq!(r.code, 0, "{}", r.stderr);
    let e = &r.cap.env;
    let url = e["ZYGO_URL"].trim_end_matches('/');
    let want = [
        ("ANTHROPIC_BASE_URL", format!("{url}/anthropic")),
        ("ANTHROPIC_MODEL", "lead".into()),
        ("ANTHROPIC_DEFAULT_FABLE_MODEL", "design[1m]".into()),
        ("ANTHROPIC_DEFAULT_OPUS_MODEL", "lead".into()),
        ("ANTHROPIC_DEFAULT_SONNET_MODEL", "code".into()),
        ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "read".into()),
        ("CLAUDE_CODE_SUBAGENT_MODEL", "code".into()),
        ("CLAUDE_CODE_GATEWAY_HINT_HEADERS", "1".into()),
        ("DISABLE_LOGIN_COMMAND", "1".into()),
        ("DISABLE_LOGOUT_COMMAND", "1".into()),
        (
            "ANTHROPIC_CUSTOM_HEADERS",
            format!("X-Zygo-Session: {}\nX-Zygo-Agent-Version: 2.1.328\nX-Zygo-Repo: example.com/acme/widgets\nX-Zygo-Work: branch:main", e["ZYGO_SESSION"]),
        ),
    ];
    for (k, v) in &want {
        assert_eq!(&e[*k], v, "{k}");
    }
    assert_eq!(e["ZYGO_AGENT"], "claude-code");
    assert_eq!(e["ZYGO_MAIN_ROLE"], "lead");
    assert_eq!(e["ZYGO_REPO"], "example.com/acme/widgets");
    assert_eq!(e["ZYGO_WORK"], "branch:main");
    assert_eq!(e["GH_TOKEN"], "gh-launch-1");
    assert!(!e.contains_key("MYPROVIDER_KEY"), "a catalog provider's key variable is denylisted (§8.4)");
    assert_no_denied(e, &SET);
    assert!(!e.contains_key("CARGO_MANIFEST_DIR"), "only opts.env reaches the agent");
    let leaked: Vec<&str> = r
        .cap
        .session
        .values()
        .map(String::as_str)
        .chain(e.values().map(String::as_str))
        .chain(r.cap.argv.iter().map(String::as_str))
        .chain([r.stderr.as_str()])
        .filter(|t| t.contains("denied-value-"))
        .collect();
    assert!(leaked.is_empty(), "a denylisted value reached {leaked:?}");
    let line = r.cap.session["session.log"].lines().find(|l| l.starts_with("env: removed ")).expect("the session log names the removed names");
    let mut names = vec!["ANTHROPIC_API_KEY", "MYPROVIDER_KEY", "ZYGO_TEAM_TOKEN"];
    if cfg!(windows) {
        names.push("Anthropic_Api_Key");
    }
    for n in names {
        assert!(line.contains(n), "{n} is not in {line}");
    }
}

#[test]
fn non_member_child_has_neither_workspace_name_nor_a_marker() { // AU58, AU64, W-C6
    const GONE: [&str; 6] = ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_BRIDGE_SESSION_ID", "ZYGO_WORKSPACE", "ZYGO_PANE"];
    let rig = rig();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.env.0.extend(GONE.map(|n| (n.into(), "parent-1".into()))));
    assert_eq!(r.code, 0, "{}", r.stderr);
    for n in GONE {
        assert!(!r.cap.env.contains_key(n), "{n} reached the agent");
    }
    let log = &r.cap.session["session.log"];
    assert!(log.lines().any(|l| l == "env: removed CLAUDECODE, CLAUDE_CODE_BRIDGE_SESSION_ID, CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_SESSION_ID, ZYGO_PANE, ZYGO_WORKSPACE"), "{log}");
    let view: Value = serde_json::from_str(&r.cap.session["view.json"]).unwrap();
    assert!(view["workspace"].is_null(), "{view}");
}

#[test]
fn member_launch_saves_the_member_and_the_note_and_sets_both_names() { // AU58, AU72
    use zygo_core::session_view::{Member, ResumeNote, SessionView, SubagentRef, ViewWorkspace};
    let rig = rig();
    let member = Member { ws_id: "ws-1".into(), pane: "p-1".into() };
    let note = ResumeNote {
        clean_stop: true,
        interrupted: vec![SubagentRef { agent_id: "a-1".into(), agent_type: "zygo-code".into() }],
        closed_units: vec!["gh:1".into()],
    };
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| {
        o.env.0.extend([("ZYGO_WORKSPACE".into(), "old-ws".into()), ("ZYGO_PANE".into(), "old-pane".into())]);
        o.member = Some(member.clone());
        o.resume_note = Some(note.clone());
    });
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!((r.cap.env["ZYGO_WORKSPACE"].as_str(), r.cap.env["ZYGO_PANE"].as_str()), ("ws-1", "p-1"));
    let view: SessionView = serde_json::from_str(&r.cap.session["view.json"]).unwrap();
    assert_eq!(view.workspace, Some(ViewWorkspace { member, resume_note: Some(note) }));
    // Set, not removed: the session log does not name them.
    assert!(!r.cap.session.get("session.log").is_some_and(|l| l.contains("ZYGO_WORKSPACE") || l.contains("ZYGO_PANE")));
}

#[cfg(unix)]
#[test]
fn death_by_signal_sets_signal_and_an_exit_code_does_not() { // AU73
    let rig = rig();
    let control = |ctl: &std::path::Path| {
        let ctl = ctl.to_path_buf();
        move |o: &mut zygo_launch::LaunchOpts| o.env.0.push(("ZYGO_STUB_CONTROL".into(), ctl.into_os_string()))
    };
    // A real exit 137: a code, no signal.
    let ctl = rig.root.join("ctl-exit");
    stub_control(&ctl, "exit 137").unwrap();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, control(&ctl));
    assert_eq!(r.outcome, LaunchOutcome { code: 137, signal: None, spawned: true, received: false }, "{}", r.stderr);
    // SIGKILL to the agent: the same code, and the signal.
    let ctl = rig.root.join("ctl-kill");
    let r = std::thread::scope(|s| {
        let t = s.spawn(|| launch(&rig, &claude(&rig), CLAUDE_VERSION, control(&ctl)));
        let pid = stub_started(&ctl, std::time::Duration::from_secs(60));
        if let Some(pid) = pid {
            assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        }
        let r = t.join().unwrap();
        assert!(pid.is_some(), "the stub did not start: {}", r.stderr);
        r
    });
    assert_eq!(r.outcome, LaunchOutcome { code: 137, signal: Some(libc::SIGKILL), spawned: true, received: false }, "{}", r.stderr);
}

#[test]
fn claude_code_flags_settings_agents_and_mcp_match_the_spec() {
    let rig = rig();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| {
        o.yolo = true;
        o.args = vec!["--print".into(), "hello".into()];
    });
    assert_eq!(r.code, 0, "{}", r.stderr);
    let sd = PathBuf::from(&r.cap.env["ZYGO_SESSION_DIR"]);
    let a = &r.cap.argv;
    assert_eq!(a[0], "--mcp-config");
    assert_eq!(PathBuf::from(&a[1]), sd.join("mcp.json"));
    assert_eq!(a[2], "--settings");
    assert_eq!(PathBuf::from(&a[3]), sd.join("settings.json"));
    assert_eq!(a[4], "--agents");
    assert_eq!(a[6], "--plugin-dir");
    assert_eq!(PathBuf::from(&a[7]), sd.join("plugins"));
    assert_eq!(a[8..], ["--dangerously-skip-permissions", "--print", "hello"]);
    // settings (§8.5)
    let q = spec_quote(&rig.zygo);
    let roles = zygo_config::builtin::defaults().role; // BTreeMap, sorted by name
    let s = &r.cap.settings;
    assert_eq!(s["disableAllHooks"], false);
    assert_eq!(s["apiKeyHelper"], format!("{q} hook token"));
    assert_eq!(s["model"], "lead");
    assert_eq!(s["availableModels"], json!(roles.keys().collect::<Vec<_>>()));
    let rows: Vec<Value> =
        roles.iter().map(|(k, x)| json!({"model": k, "label": k, "description": x.description, "behavesAs": x.model})).collect();
    assert_eq!(s["modelPicker"], json!({"options": rows}));
    assert_eq!(s["statusLine"], json!({"type": "command", "command": format!("{q} hook statusline"), "refreshInterval": 30}));
    let h = |m: Option<&str>, c: &str| {
        let hooks = json!([{"type": "command", "command": format!("{q} hook {c}")}]);
        match m {
            Some(m) => json!({"matcher": m, "hooks": hooks}),
            None => json!({"hooks": hooks}),
        }
    };
    assert_eq!(
        s["hooks"],
        json!({
            "PreToolUse": [h(Some("Edit|Write|NotebookEdit"), "role-guard"), h(Some("Edit|Write|NotebookEdit|Read|Bash"), "guard"),
                           h(Some("mcp__zygo__work_begin"), "work --event begin"), h(Some("mcp__zygo__work_verdict"), "work --event verdict")],
            "PostToolUse": [h(Some("Bash|Read|Grep|Glob|WebFetch"), "trim"), h(Some("*"), "work")],
            "PostToolUseFailure": [h(Some("*"), "work")],
            "SubagentStop": [h(None, "work")],
            "SessionStart": [h(None, "session-start")], "UserPromptSubmit": [h(None, "prompt")], "PreCompact": [h(None, "pre-compact")]})
    );
    // env mirror: settings.env is exactly the eleven rows and equals the process environment rows
    let env = s["env"].as_object().unwrap();
    assert_eq!(env.keys().map(String::as_str).collect::<BTreeSet<_>>(), SET.into_iter().collect());
    for (k, v) in env {
        assert_eq!(v.as_str().unwrap(), r.cap.env[k], "{k}");
    }
    // agents: one per enabled role except main_role, model = the role, tool restriction by definition
    let agents: Value = serde_json::from_str(&a[5]).unwrap();
    let want: BTreeSet<String> = roles.keys().filter(|k| *k != "lead").map(|k| format!("zygo-{k}")).collect();
    assert_eq!(agents.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>(), want);
    for (k, x) in &roles {
        if k == "lead" {
            continue;
        }
        let ag = &agents[format!("zygo-{k}")];
        assert_eq!(ag["model"], k.as_str());
        assert!(ag["prompt"].as_str().unwrap().starts_with(&x.prompt), "{k}");
        assert_eq!(ag.get("disallowedTools"), (!x.writes).then(|| json!(["Edit", "Write", "NotebookEdit"])).as_ref(), "{k}");
    }
    // MCP (A-6): the plain path, not a shell string
    assert_eq!(r.cap.mcp, json!({"mcpServers": {"zygo": {"command": rig.zygo.to_string_lossy(), "args": ["mcp", "--agent", "claude-code"]}}}));
    // plugin snapshot (§15.3): the embedded plugin is on disk where --plugin-dir points
    let manifest: Value = serde_json::from_str(&r.cap.plugins["zygo/.claude-plugin/plugin.json"]).unwrap();
    assert_eq!(manifest["name"], "zygo");
    assert!(r.cap.plugins["zygo/skills/zygo-cli/SKILL.md"].starts_with("---\nname: zygo-cli\n"));
}

/// Runs, through `ZYGO_STUB_SHELL`, every command string in the generated settings. On Linux and macOS the shell is `/bin/sh`
/// (single-quoted path). On the CI windows job it is Git Bash, the shell Claude Code uses on Windows when Git Bash is installed
/// (hooks docs); it proves the double-quoted path with a space and an apostrophe is one word there. It does not prove PowerShell or cmd.
#[test]
fn every_command_zygo_writes_for_claude_code_runs_in_the_agents_shell() {
    let rig = rig();
    // Only this test runs the commands, so only this test needs the shell (Git for Windows on the CI windows job).
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.env.0.push(("ZYGO_STUB_SHELL".into(), shell().into_os_string())));
    assert_eq!(r.code, 0, "{}", r.stderr);
    let q = spec_quote(&rig.zygo);
    let cmds: BTreeSet<String> = [
        "token", "statusline", "role-guard", "guard", "work --event begin", "work --event verdict", "trim", "work", "session-start", "prompt",
        "pre-compact",
    ]
    .iter()
    .map(|n| format!("{q} hook {n}"))
    .collect();
    assert_eq!(r.cap.shell.keys().cloned().collect::<BTreeSet<_>>(), cmds);
    for (c, v) in &r.cap.shell {
        assert_eq!(v["code"], 0, "{c}: {v}");
        assert_eq!(v["out"].as_str().unwrap(), c.strip_prefix(&q).unwrap().trim(), "{c}");
    }
    assert_eq!(r.cap.mcp_run["zygo"], "mcp --agent claude-code");
}

#[test]
fn launch_leaves_the_users_own_files_untouched() {
    let rig = rig();
    let put = |p: PathBuf, t: &str| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    };
    put(rig.user.join(".claude/settings.json"), r#"{"theme":"dark"}"#);
    put(rig.cwd.join(".claude/settings.local.json"), r#"{"theme":"dark"}"#);
    put(rig.user.join(".config/opencode/opencode.json"), r#"{"theme":"tokyonight"}"#);
    put(rig.cwd.join(".opencode/opencode.json"), r#"{"theme":"tokyonight"}"#);
    let before = (tree(&rig.user), tree(&rig.cwd));
    for (adapter, v) in [(&claude(&rig) as &dyn AgentAdapter, CLAUDE_VERSION), (&opencode(&rig), OPENCODE_VERSION)] {
        let r = launch(&rig, adapter, v, |_| {});
        assert_eq!(r.code, 0, "{}", r.stderr);
        assert_eq!((tree(&rig.user), tree(&rig.cwd)), before);
    }
}

#[test]
fn inline_agents_json_with_shell_metacharacters_reaches_the_agent_intact() {
    const NASTY: &str = "say \"hi\" & 100% | <x> ^ !y, a=b; C:\\dir\\sub é";
    const TAIL: &str = "ends with a backslash \\";
    let rig = rig();
    let mine = json!({"mine": {"description": NASTY, "prompt": format!("{NASTY} {TAIL}")}});
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.args = vec!["--agents".into(), mine.to_string()]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.cap.argv.iter().filter(|a| *a == "--agents").count(), 1);
    let agents: Value = serde_json::from_str(&r.cap.argv[r.cap.argv.iter().position(|a| a == "--agents").unwrap() + 1]).unwrap();
    assert_eq!(agents["mine"], mine["mine"]);
    assert!(agents.get("zygo-code").is_some());
}

#[test]
fn claude_code_session_token_stays_in_the_token_file_and_in_passthrough_in_settings_and_custom_headers() {
    for passthrough in [false, true] {
        let rig = rig();
        let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.passthrough = passthrough);
        assert_eq!(r.code, 0, "{}", r.stderr);
        let token = r.cap.session["token"].trim().to_string();
        assert!(token.len() > 8);
        let line = format!("X-Zygo-Token: {token}");
        let files: Vec<&String> = r
            .cap
            .session
            .iter()
            .filter(|(k, v)| !(*k == "token" || (passthrough && *k == "settings.json")) && v.contains(&token))
            .map(|(k, _)| k)
            .collect();
        assert!(files.is_empty(), "passthrough {passthrough}: the token is in {files:?}");
        let vars: Vec<&String> = r
            .cap
            .env
            .iter()
            .filter(|(k, v)| !(passthrough && *k == "ANTHROPIC_CUSTOM_HEADERS") && v.contains(&token))
            .map(|(k, _)| k)
            .collect();
        assert!(vars.is_empty(), "passthrough {passthrough}: the token is in {vars:?}");
        assert_eq!(r.cap.env["ANTHROPIC_CUSTOM_HEADERS"].lines().any(|l| l == line), passthrough);
        assert_eq!(r.cap.settings["env"]["ANTHROPIC_CUSTOM_HEADERS"].as_str().unwrap().lines().any(|l| l == line), passthrough);
        assert!(r.cap.argv.iter().all(|a| !a.contains(&token)), "argv");
        assert!(!r.stderr.contains(&token), "stderr");
        assert!(!r.cap.session["session.log"].contains(&token), "session log");
        #[cfg(unix)]
        assert_eq!(r.cap.settings_mode, Some(0o600), "settings.json mode");
    }
}

#[test]
fn opencode_child_environment_flags_and_inline_config_match_the_spec() {
    let rig = rig();
    git_repo(&rig.cwd, "main", "https://example.com/acme/widgets.git");
    std::fs::write(&rig.paths.config_file, CATALOG_CONFIG).unwrap();
    let r = launch(&rig, &opencode(&rig), OPENCODE_VERSION, |o| {
        o.yolo = true;
        o.args = vec!["run".into(), "hello".into()];
        o.env.0.extend(denied_parent());
    });
    assert_eq!(r.code, 0, "{}", r.stderr);
    let e = &r.cap.env;
    assert_eq!(r.cap.argv, ["run", "--auto", "hello"]);
    assert_eq!(e["ZYGO_AGENT"], "opencode");
    assert_eq!(e["GH_TOKEN"], "gh-launch-1");
    assert_no_denied(e, &[]); // OpenCode gets no ANTHROPIC_* row: the parent's ANTHROPIC_BASE_URL is removed too
    let sd = PathBuf::from(&e["ZYGO_SESSION_DIR"]);
    assert_eq!(PathBuf::from(&e["OPENCODE_CONFIG_DIR"]), sd.join("opencode"));
    let cfg: Value = serde_json::from_str(&e["OPENCODE_CONFIG_CONTENT"]).unwrap();
    let url = e["ZYGO_URL"].trim_end_matches('/');
    let token = r.cap.session["token"].trim();
    assert_eq!(cfg["model"], "zygo/lead");
    assert_eq!(cfg["provider"]["zygo"]["npm"], "@ai-sdk/anthropic");
    assert_eq!(cfg["provider"]["zygo"]["options"]["baseURL"], format!("{url}/anthropic/v1"));
    assert_eq!(cfg["provider"]["zygo"]["options"]["apiKey"], token);
    assert_eq!(
        cfg["provider"]["zygo"]["options"]["headers"],
        json!({"X-Zygo-Session": e["ZYGO_SESSION"], "X-Zygo-Agent-Version": "1.18.16", "X-Zygo-Repo": "example.com/acme/widgets", "X-Zygo-Work": "branch:main"})
    );
    let roles = zygo_config::builtin::defaults().role;
    assert_eq!(cfg["provider"]["zygo"]["models"].as_object().unwrap().keys().collect::<Vec<_>>(), roles.keys().collect::<Vec<_>>());
    assert_eq!(cfg["agent"]["build"]["permission"], json!({"edit": "deny"})); // lead guard, §8.3: build maps to main_role lead
    assert_eq!(cfg["mcp"]["zygo"], json!({"type": "local", "command": [rig.zygo.to_string_lossy(), "mcp", "--agent", "opencode"]}));
    assert!(r.cap.ocdir.contains_key("plugins/zygo.js") && r.cap.ocdir.contains_key("skills/zygo-zygo-cli/SKILL.md"));
    // C3: the token is only in the `token` file of the session dir and in the inline config row
    let leaks: Vec<&String> = r.cap.session.iter().filter(|(k, v)| *k != "token" && v.contains(token)).map(|(k, _)| k).collect();
    assert!(leaks.is_empty(), "{leaks:?}");
    assert!(e.iter().all(|(k, v)| k == "OPENCODE_CONFIG_CONTENT" || !v.contains(token)));
    assert!(r.cap.argv.iter().all(|a| !a.contains(token)) && !r.stderr.contains(token));
    let hit: Vec<&String> = r.cap.session.iter().chain(e.iter()).filter(|(_, t)| t.contains("denied-value-")).map(|(k, _)| k).collect();
    assert!(hit.is_empty(), "{hit:?}");
}

/// Runs only on the CI windows job. Proves: a Claude Code program that is an npm-style `claude.cmd` shim
/// (`cmd.exe` runs the batch file, which forwards `%*`) receives the inline `--agents` JSON with quotes, `%`, `&`, `|`, `<`, `>`, `^`
/// and `!` intact, the raw command line it receives is under cmd.exe's 8191-character limit, and the launcher returns the program's exit code.
#[cfg(windows)]
#[test]
fn windows_cmd_shim_receives_the_inline_agents_json_intact() {
    let rig = rig();
    std::fs::remove_file(rig.bin.join("claude.exe")).unwrap();
    std::fs::copy(&rig.zygo, rig.bin.join("claude-real.exe")).unwrap();
    std::fs::write(rig.bin.join("claude.cmd"), "@echo off\r\n\"%~dp0claude-real.exe\" %*\r\n").unwrap();
    let nasty = "say \"hi\" & 100% | <x> ^ !y, a=b; C:\\dir\\sub é";
    let mine = json!({"mine": {"description": nasty, "prompt": format!("%PATH% {nasty}")}});
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| {
        o.args = vec!["--agents".into(), mine.to_string()];
        o.env.0.push(("ZYGO_STUB_EXIT".into(), "7".into()));
    });
    let raw = r.cap.cmdline.clone().expect("the stub records its raw command line on Windows");
    let n = raw.encode_utf16().count();
    println!("raw command line received through claude.cmd: {n} UTF-16 units");
    assert_eq!(r.code, 7, "{}\nraw command line ({n} units): {raw}", r.stderr);
    assert!(n < 8191, "raw command line is {n} units, cmd.exe's limit is 8191: {raw}");
    let i = r.cap.argv.iter().position(|a| a == "--agents").unwrap();
    let agents: Value = serde_json::from_str(&r.cap.argv[i + 1]).unwrap_or_else(|e| panic!("{e}; raw command line: {raw}"));
    assert_eq!(agents["mine"], mine["mine"], "raw command line: {raw}");
    assert!(agents.get("zygo-code").is_some());
}
