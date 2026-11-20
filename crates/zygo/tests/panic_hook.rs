//! N12: the binary's panic hook (C3). The panic path is the `test-hooks` trigger: `ZYGO_TEST_PANIC` (set on the child
//! only) makes the binary panic right after the hook is installed, with the payload a stdout write failure would carry.
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Output, Stdio};

mod common;

/// A write end whose read end is already closed.
fn broken() -> Stdio {
    let (r, w) = std::io::pipe().unwrap();
    drop(r);
    w.into()
}

fn trigger(stderr: Stdio) -> Output {
    let h = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    cmd.arg("status")
        .env("ZYGO_TEST_PANIC", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .output()
        .unwrap()
}

#[test] // C3: the installed hook prints a location, never the payload
fn binary_panic_hook_prints_location_only() {
    let o = trigger(Stdio::piped());
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(101), "{err}");
    assert!(err.starts_with("zygo: internal error at "), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    for payload in ["failed printing", "Broken pipe", "panicked"] {
        assert!(!err.contains(payload), "{err}");
    }
}

#[test] // the hook's own write to a broken stderr must not panic again (a panic in the hook aborts the process)
fn binary_panic_hook_survives_broken_stderr() {
    let o = trigger(broken());
    #[cfg(unix)]
    assert_eq!(o.status.signal(), None);
    assert_eq!(o.status.code(), Some(101));
}
