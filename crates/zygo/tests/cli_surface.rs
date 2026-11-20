use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;

fn zygo(args: &[&str]) -> std::process::Output {
    let h = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    cmd.args(args).stdin(Stdio::null()).output().expect("run zygo")
}

fn help_of(args: &[&str]) -> String {
    let mut v: Vec<&str> = args.to_vec();
    v.push("--help");
    let out = zygo(&v);
    assert!(out.status.success(), "{args:?} --help failed");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn help_lists_every_command() {
    let help = help_of(&[]);
    let listed: HashSet<&str> = help
        .lines()
        .skip_while(|l| l.trim() != "Commands:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    for c in [
        "claude", "opencode", "serve", "status", "doctor", "compact", "profile", "work", "usage",
        "marketplace", "plugins", "plugin", "keys", "config", "update", "help", "results",
        "redaction", "guards", "units", "compact-tool", "mcp", "hook",
    ] {
        assert!(listed.contains(c), "--help Commands lacks {c}: {listed:?}");
    }
}

#[test]
fn every_command_exists() {
    let cmds = "claude|opencode|serve|serve token new|serve reload|status|doctor|compact|profile list|profile show|profile use|work|work report|usage|marketplace add|marketplace list|marketplace update|marketplace remove|plugins list|plugins sync|plugins enable|plugins disable|plugins approve|plugin check|keys set|keys rm|keys list|config show|config check|update|help|results|redaction|guards|units|compact-tool|mcp|hook";
    for c in cmds.split('|') {
        let mut argv: Vec<&str> = c.split(' ').collect();
        argv.push("--help");
        let out = zygo(&argv);
        assert!(out.status.success(), "zygo {c} --help exited {:?}", out.status.code());
    }
}

#[test]
fn flags() {
    let h = help_of(&["claude"]);
    for f in ["--passthrough", "--profile", "--server", "--compact", "--yolo", "--config-dir"] {
        assert!(h.contains(f), "claude lacks {f}");
    }
    assert!(h.contains("Claude Code config directory (sets CLAUDE_CONFIG_DIR)"));
    let o = help_of(&["opencode"]);
    assert!(!o.contains("--passthrough"));
    assert!(!o.contains("--config-dir"));
    let h = help_of(&["marketplace", "add"]);
    for f in ["--name", "--ref", "--enable"] {
        assert!(h.contains(f), "marketplace add lacks {f}");
    }
    let h = help_of(&["marketplace", "update"]);
    for f in ["--all", "--accept"] {
        assert!(h.contains(f), "marketplace update lacks {f}");
    }
    let h = help_of(&["usage"]);
    for f in ["--session", "--since", "--sync"] {
        assert!(h.contains(f), "usage lacks {f}");
    }
}

#[test]
fn verbs_match_spec() {
    let want: [(&str, &str); 13] = [
        ("help", "[<verb>]"),
        ("status", ""),
        ("usage", ""),
        ("compact", "<tokens|on|off>"),
        ("results", "on|off|threshold <bytes>"),
        ("redaction", "[content on|off]"),
        ("guards", "[<guard> <level>]"),
        ("units", "on|off"),
        ("work", "[<id>|clear]"),
        ("plugins", "[list|enable|disable|approve <m>:<p>]"),
        ("marketplace", "[list|add|update|remove …]"),
        ("profile", "[<name>|none]"),
        ("compact-tool", "on|off"),
    ];
    let specs = zygo_core::verbs::VERB_SPECS;
    assert_eq!(specs.len(), want.len());
    for (s, (n, a)) in specs.iter().zip(want) {
        assert_eq!((s.name, s.args), (n, a));
    }
    let names: Vec<&str> = specs.iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        ["help", "status", "usage", "compact", "results", "redaction", "guards", "units", "work", "plugins", "marketplace", "profile", "compact-tool"]
    );
    let verbs = zygo::cmd::verbs::VERBS;
    assert_eq!(verbs.len(), specs.len());
    for (i, v) in verbs.iter().enumerate() {
        assert!(std::ptr::eq(v.spec, &specs[i]));
    }
}

#[test]
fn binary_exit_zero() {
    let run = |args: &[&str], input: &str| {
        let h = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, h.path());
        let mut c = cmd
            .args(args)
            .env_remove("ZYGO_SESSION_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        c.wait_with_output().unwrap()
    };
    let o = run(&["hook", "role-guard"], "not json");
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1);
    let o = run(&["hook", "role-guard"], "{}");
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty());
    let o = zygo(&["hook", "token"]);
    assert_eq!(o.status.code(), Some(0));
    let o = zygo(&["hook", "nope"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1);
    // `token` must not read stdin: an open pipe that is never written or closed.
    let h = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    let mut c = cmd
        .args(["hook", "token"])
        .env_remove("ZYGO_SESSION_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _held = c.stdin.take();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break Some(s);
        }
        if start.elapsed() > Duration::from_secs(2) {
            let _ = c.kill();
            let _ = c.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.and_then(|s| s.code()), Some(0), "hook token waited on stdin");
}

/// C6: every `zygo hook …` exits 0 with at most one stderr line.
#[test]
fn hook_c6_exit_paths() {
    let cases: &[&[&str]] = &[
        &["hook"],
        &["hook", "nope"],
        &["hook", "role-guard", "--agent", "bogus"],
        &["hook", "role-guard", "--bogus-flag"],
        &["hook", "role-guard", "extra"],
        &["hook", "role-guard", "--event"],
    ];
    for args in cases {
        let o = zygo(args);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1, "{args:?}");
    }
    // With stderr a closed pipe, a write to stderr must not panic.
    for args in cases {
        let (r, w) = std::io::pipe().unwrap();
        drop(r);
        let h = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, h.path());
        let o = cmd
            .args(*args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(w)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{args:?} with closed stderr");
    }
}

#[test]
fn hook_table_names() {
    let names: Vec<&str> = zygo::cmd::hook::TABLE.iter().map(|h| h.name).collect();
    assert_eq!(
        names,
        ["token", "statusline", "trim", "session-start", "role-guard", "prompt", "pre-compact", "work", "guard"]
    );
    for h in zygo::cmd::hook::TABLE {
        assert_eq!(h.reads_stdin, h.name != "token", "reads_stdin of {}", h.name);
    }
}

/// `--config-dir` is checked before any agent launch: exit 2, one stderr line.
#[test]
fn claude_config_dir_missing_exits_2() {
    let h = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let absent = h.path().join("absent");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    let o = cmd.args(["claude", "--config-dir"]).arg(&absent).env("PATH", bin.path()).env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT")
        .current_dir(h.path()).stdin(Stdio::null()).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{err}");
    assert!(err.contains("not a directory") && err.lines().count() == 1, "{err:?}");
}

// ---- N72: verb and config errors are escaped ----

const FORGED_PROFILE: &str = "[profile.\"x\\u001b[2K\\nforged\"]\nprompt = 1\n";
const FORGED_LOAD_END: &str = "profile.\"x\\u{1b}[2K\\nforged\".prompt at line 2: expected a string, found an integer\n";

fn with_config(config: &str, args: &[&str]) -> std::process::Output {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.toml"), config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_zygo")).args(args).env("HOME", d.path()).env("LOCALAPPDATA", d.path()).env("APPDATA", d.path())
        .env("ZYGO_CONFIG", d.path().join("config.toml")).env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT")
        .env_remove("XDG_DATA_HOME").env_remove("XDG_CACHE_HOME")
        .current_dir(d.path()).stdin(Stdio::null()).output().unwrap()
}

#[test]
fn verb_error_is_escaped_on_stderr() {
    let o = with_config("", &["guards", "x\u{1b}[2K\nforged", "on", "--save"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "refused: unknown guard `x\\u{1b}[2K\\nforged`\n");
}

#[test]
fn config_check_violation_is_escaped() {
    let o = with_config("[role.\"x\\u001b[2K\\nforged\"]\nmodel = \"nope\"\n", &["config", "check"]);
    assert_eq!(o.status.code(), Some(11));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "error: role.\"x\\u{1b}[2K\\nforged\".model: unknown model\n");
}

#[test]
fn config_show_load_error_is_escaped() {
    let o = with_config(FORGED_PROFILE, &["config", "show"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo config: user config ") && err.ends_with(FORGED_LOAD_END), "{err:?}");
    assert!(!err.contains('\u{1b}') && err.lines().count() == 1, "{err:?}");
}

#[test]
fn mcp_load_error_is_escaped() {
    let o = with_config(FORGED_PROFILE, &["mcp", "--agent", "claude-code"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo mcp: user config ") && err.ends_with(FORGED_LOAD_END), "{err:?}");
    assert!(!err.contains('\u{1b}') && err.lines().count() == 1, "{err:?}");
}

/// Round 2 F4: the `--layer` echo is escaped.
#[test]
fn config_show_layer_echo_is_escaped() {
    let o = with_config("", &["config", "show", "--layer", "x\u{1b}[2K\nforged"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&o.stderr),
        "zygo config: unknown layer x\\u{1b}[2K\\nforged (builtin|team|user|repository|profile|flags|session)\n");
}
