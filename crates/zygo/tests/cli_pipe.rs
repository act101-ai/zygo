//! N45: a command whose stdout reader is gone ends quietly (no panic line, no signal), with its normal exit status.
//! A pipe whose read end is closed before the spawn makes every stdout write fail with EPIPE.
//! N48: the same for stderr: a command that writes an error line there keeps its §8.2 exit code (not 101).
mod common;

use std::path::Path;
use std::process::{Command, Output, Stdio};

/// A write end whose read end is already closed.
fn broken() -> Stdio {
    let (r, w) = std::io::pipe().unwrap();
    drop(r);
    w.into()
}

fn spawn(args: &[&str], stdin: &[u8], home: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zygo"))
        .args(args)
        .env("HOME", home)
        .env("LOCALAPPDATA", home)
        .env("APPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"))
        .env_remove("ZYGO_SESSION_DIR")
        .env_remove("ZYGO_AGENT")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(broken())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    common::write_stdin(child.stdin.take().unwrap(), stdin);
    child.wait_with_output().unwrap()
}

fn spawn_err(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zygo"))
        .args(args)
        .env("HOME", home)
        .env("LOCALAPPDATA", home)
        .env("APPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"))
        .env_remove("ZYGO_SESSION_DIR")
        .env_remove("ZYGO_AGENT")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(broken())
        .output()
        .unwrap()
}

fn assert_code(o: &Output, code: i32) {
    assert_eq!(o.status.code(), Some(code));
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert!(o.status.signal().is_none());
    }
}

fn assert_quiet(o: &Output, code: i32) {
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(code), "{err}");
    assert!(o.stderr.is_empty(), "{err}");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert!(o.status.signal().is_none());
    }
}

fn home() -> tempfile::TempDir {
    let d = tempfile::TempDir::new().unwrap();
    std::fs::write(d.path().join("config.toml"), "").unwrap();
    d
}

#[test]
fn plugins_list_closed_stdout_ends_quietly() {
    let h = home();
    assert_quiet(&spawn(&["plugins", "list"], b"", h.path()), 0);
}

#[test]
fn plugin_check_closed_stdout_ends_quietly() {
    let h = home();
    let empty = h.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert_quiet(&spawn(&["plugin", "check", empty.to_str().unwrap()], b"", h.path()), 1);
}

#[test]
fn config_check_closed_stdout_ends_quietly() {
    let h = home();
    assert_quiet(&spawn(&["config", "check"], b"", h.path()), 0);
}

#[test] // C6: a hook never breaks the agent
fn hook_closed_stdout_exits_zero() {
    let h = home();
    assert_quiet(&spawn(&["hook", "role-guard"], br#"{"tool_name":"Edit"}"#, h.path()), 0);
}

#[test]
fn config_load_error_closed_stderr_exits_11() {
    let h = home();
    std::fs::write(h.path().join("config.toml"), "not toml [").unwrap();
    assert_code(&spawn_err(&["config", "check"], h.path()), 11);
}

#[test]
fn config_unknown_layer_closed_stderr_exits_2() {
    let h = home();
    assert_code(&spawn_err(&["config", "show", "--layer", "bogus"], h.path()), 2);
}

#[test]
fn mcp_missing_agent_closed_stderr_exits_2() {
    let h = home();
    assert_code(&spawn_err(&["mcp"], h.path()), 2);
}

#[test]
fn verb_usage_error_closed_stderr_exits_2() {
    let h = home();
    assert_code(&spawn_err(&["plugins", "approve", "x"], h.path()), 2);
}
