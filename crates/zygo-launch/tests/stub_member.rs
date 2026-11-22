//! L3: one test per stub behavior of the W§18 preamble. No process-environment change.
#[path = "stub_agent/harness.rs"]
mod harness;
#[path = "../../zygo/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use serde_json::{json, Value};
use tempfile::TempDir;

const ID: &str = "11111111-1111-4111-8111-111111111111";
const BOUND: Duration = Duration::from_secs(10);

struct Stub { child: Child, out: PathBuf, ctl: PathBuf, t: TempDir }
impl Drop for Stub { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

/// Starts `stub-agent` directly in `t`: capture `t/out`, control `t/ctl`, cwd `t/work`, session dir `t/sd` with `token` = `tok-1`.
fn spawn_in(t: TempDir, args: &[&str], control: bool, url: &str) -> Stub {
    let d = |n: &str| { let p = t.path().join(n); std::fs::create_dir_all(&p).unwrap(); p };
    let (out, ctl, work, sd) = (d("out"), d("ctl"), d("work"), d("sd"));
    std::fs::write(sd.join("token"), "tok-1\n").unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_stub-agent"));
    c.args(args).current_dir(&work).stdin(Stdio::null())
        .env("ZYGO_STUB_OUT", &out).env("ZYGO_STUB_SHELL", harness::shell())
        .env("ZYGO_SESSION", "s-1").env("ZYGO_SESSION_DIR", &sd).env("ZYGO_URL", url)
        .env_remove("ZYGO_STUB_EXIT").env_remove("ZYGO_STUB_CONTROL").env_remove("OPENCODE_CONFIG_DIR").env_remove("ANTHROPIC_MODEL");
    if control { c.env("ZYGO_STUB_CONTROL", &ctl); }
    #[cfg(unix)] // a test run with SIGINT ignored (a background job) must not give the stub an ignored signal
    unsafe {
        use std::os::unix::process::CommandExt;
        c.pre_exec(|| { libc::signal(libc::SIGINT, libc::SIG_DFL); libc::signal(libc::SIGTERM, libc::SIG_DFL); Ok(()) });
    }
    Stub { child: c.spawn().unwrap(), out, ctl, t }
}
fn spawn(args: &[&str]) -> Stub { spawn_in(tempfile::tempdir().unwrap(), args, true, "") }

/// Polls every 10 ms until `ctl/done` has `n` lines; panics after `BOUND`.
fn wait_done(ctl: &Path, n: usize) -> Vec<String> {
    let end = Instant::now() + BOUND;
    loop {
        let l: Vec<String> = std::fs::read_to_string(ctl.join("done")).unwrap_or_default().lines().map(String::from).collect();
        if l.len() >= n { return l; }
        assert!(Instant::now() < end, "done has {l:?}, want {n} lines");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// Polls `try_wait` every 10 ms; panics after `BOUND`.
fn wait_exit(child: &mut Child) -> ExitStatus {
    let end = Instant::now() + BOUND;
    loop {
        if let Some(s) = child.try_wait().unwrap() { return s; }
        assert!(Instant::now() < end, "the stub did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// The JSON lines of a file; empty when the file is missing.
fn jsonl(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn exit_step_ends_the_stub_with_that_code() {
    let t = tempfile::tempdir().unwrap();
    harness::stub_control(&t.path().join("ctl"), "exit 7").unwrap(); // written before the start
    let mut s = spawn_in(t, &[], true, "");
    let pid = s.child.id();
    assert_eq!(wait_exit(&mut s.child).code(), Some(7));
    assert_eq!(wait_done(&s.ctl, 2), [format!("start {pid}"), "exit 7".to_string()]);
    // a second process on the same directory does not run the finished step again
    let t = std::mem::replace(&mut s.t, tempfile::tempdir().unwrap());
    let mut s2 = spawn_in(t, &[], true, "");
    assert_eq!(wait_done(&s2.ctl, 3)[2], format!("start {}", s2.child.id()));
    harness::stub_control(&s2.ctl, "exit 0").unwrap();
    assert_eq!(wait_exit(&mut s2.child).code(), Some(0));
    assert_eq!(wait_done(&s2.ctl, 4)[3], "exit 0");
}

#[test]
fn stub_stays_alive_until_a_signal() {
    #[cfg(unix)]
    let sigs = [libc::SIGTERM, libc::SIGINT];
    #[cfg(not(unix))]
    let sigs = [0];
    for sig in sigs {
        let mut s = spawn(&[]);
        wait_done(&s.ctl, 1);
        harness::stub_control(&s.ctl, "hook Stop").unwrap(); // alive: it still serves a step after the start
        wait_done(&s.ctl, 2);
        assert!(s.child.try_wait().unwrap().is_none());
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(unsafe { libc::kill(s.child.id() as i32, sig) }, 0);
            let st = wait_exit(&mut s.child);
            assert_eq!((st.signal(), st.code()), (Some(sig), None)); // death by the signal, not an exit code (AU73)
        }
        #[cfg(not(unix))]
        {
            let _ = sig;
            s.child.kill().unwrap();
            assert!(!wait_exit(&mut s.child).success());
        }
    }
}

#[test]
fn a_step_that_cannot_run_exits_2() {
    for step in ["jump", "hook SessionStart", "hook Stop [1]", "exit x", "request 5"] { // `request`: ZYGO_URL is empty
        let mut s = spawn(&[]);
        harness::stub_control(&s.ctl, step).unwrap();
        assert_eq!(wait_exit(&mut s.child).code(), Some(2), "{step}");
        assert_eq!(wait_done(&s.ctl, 1).len(), 1, "{step}"); // only the start line
    }
}

#[test]
fn session_argument_is_recorded() {
    for flag in ["--session-id", "--resume", "-s"] {
        let mut s = spawn_in(tempfile::tempdir().unwrap(), &["--model", "x", flag, ID], false, "");
        assert_eq!(wait_exit(&mut s.child).code(), Some(0));
        let v: Value = serde_json::from_slice(&std::fs::read(s.out.join("session-arg.json")).unwrap()).unwrap();
        assert_eq!(v, json!({"flag": flag, "id": ID}));
    }
    let mut s = spawn_in(tempfile::tempdir().unwrap(), &["--model", "x"], false, "");
    assert_eq!(wait_exit(&mut s.child).code(), Some(0));
    assert!(!s.out.join("session-arg.json").exists());
}
