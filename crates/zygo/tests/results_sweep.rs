//! N71: the results sweep (§10.1) runs at every `zygo` command except hooks and `zygo mcp`. Every run uses a scratch
//! HOME and cache; no agent is started.
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

fn set_mtime(p: &Path, t: SystemTime) {
    let mut o = std::fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.write(true).custom_flags(0x0200_0000); // FILE_FLAG_BACKUP_SEMANTICS: lets a directory be opened
    }
    #[cfg(not(windows))]
    o.read(true);
    o.open(p).unwrap().set_modified(t).unwrap();
}

fn sessions(home: &Path) -> PathBuf {
    // Paths::from_env: unix `$HOME/.cache/zygo`; windows `$LOCALAPPDATA/zygo/cache` (crates/zygo-core/src/paths.rs)
    if cfg!(windows) { home.join("zygo").join("cache").join("sessions") } else { home.join(".cache/zygo/sessions") }
}

/// A session directory with `token`, `pid` and one stored result, every entry last changed `hours` ago. No lock is
/// held on `pid`, so the session counts as dead (`zygo_core::session_dir::live`).
fn stale(home: &Path, name: &str, hours: u64) -> PathBuf {
    let d = sessions(home).join(name);
    std::fs::create_dir_all(d.join("results")).unwrap();
    std::fs::write(d.join("results/f0"), "x").unwrap();
    std::fs::write(d.join("token"), "t").unwrap();
    std::fs::write(d.join("pid"), "1").unwrap();
    let t = SystemTime::now() - Duration::from_secs(hours * 3600);
    for p in [d.join("results/f0"), d.join("token"), d.join("pid"), d.join("results"), d.clone()] { set_mtime(&p, t); }
    d
}

fn zygo_env(home: &Path, args: &[&str], stdin: &[u8], extra: &[(&str, &OsStr)]) -> std::process::Output {
    std::fs::write(home.join("config.toml"), "").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    cmd.args(args)
        .env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"))
        .env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT")
        .current_dir(home)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in extra { cmd.env(k, v); }
    let mut c = cmd.spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin).unwrap();
    c.wait_with_output().unwrap()
}

fn zygo(home: &Path, args: &[&str], stdin: &[u8]) -> std::process::Output {
    zygo_env(home, args, stdin, &[])
}

fn stderr_of(o: &std::process::Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }

#[test]
fn plain_command_sweeps_a_session_older_than_24_hours() {
    let h = tempfile::tempdir().unwrap();
    let old = stale(h.path(), "old-s", 25);
    let new = stale(h.path(), "new-s", 23);
    let o = zygo(h.path(), &["config", "check"], b"");
    assert_eq!(o.status.code(), Some(0), "{}", stderr_of(&o));
    assert!(!old.exists());
    assert!(new.join("results/f0").exists());
}

#[test] // launch path: `zygo claude` with no agent on PATH exits 13 right after the sweep point; no agent is started
fn zygo_claude_sweeps_before_it_looks_for_the_agent() {
    let h = tempfile::tempdir().unwrap();
    let old = stale(h.path(), "old-s", 25);
    let empty = h.path().join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    let o = zygo_env(h.path(), &["claude"], b"", &[("PATH", empty.as_os_str())]);
    assert_eq!(o.status.code(), Some(13), "{}", stderr_of(&o));
    assert!(!old.exists());
}

#[test] // only the lock on `pid` keeps a live session: token present, every entry 25 h old
fn a_session_with_a_locked_pid_is_kept() {
    let h = tempfile::tempdir().unwrap();
    let live = stale(h.path(), "live-s", 25);
    let held = std::fs::OpenOptions::new().write(true).open(live.join("pid")).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    let o = zygo(h.path(), &["config", "check"], b"");
    assert_eq!(o.status.code(), Some(0), "{}", stderr_of(&o));
    assert!(live.join("results/f0").exists() && live.join("token").exists());
    drop(held);
}

#[test] // spec §10.1 note (N71 plan review): a hook is time-bounded (§17.1, §8.7) and never sweeps; C6 still holds
fn hook_command_does_not_sweep_and_exits_zero() {
    let h = tempfile::tempdir().unwrap();
    let old = stale(h.path(), "old-s", 25);
    let o = zygo(h.path(), &["hook", "role-guard"], br#"{"tool_name":"Edit"}"#);
    assert_eq!(o.status.code(), Some(0), "{}", stderr_of(&o));
    assert!(old.join("results/f0").exists());
}

#[test]
fn mcp_command_does_not_sweep() {
    let h = tempfile::tempdir().unwrap();
    let old = stale(h.path(), "old-s", 25);
    let o = zygo(h.path(), &["mcp", "--agent", "claude-code"], b"");
    assert_eq!(o.status.code(), Some(0), "{}", stderr_of(&o));
    assert!(old.join("results/f0").exists());
}

#[test]
fn a_parse_error_is_not_a_command_and_sweeps_nothing() {
    let h = tempfile::tempdir().unwrap();
    let old = stale(h.path(), "old-s", 25);
    let o = zygo(h.path(), &["no-such-verb"], b"");
    assert_ne!(o.status.code(), Some(0));
    assert!(old.join("results/f0").exists());
}
