//! M2: `zygo ws run`, the member wrapper and the workspace test harness (W§8.2, W§8.3, W§9.4 inputs, W§7.3, W§7.4,
//! W§18 preamble). Every test runs a workspace verb, so the file is `cfg(unix)` (OP5).
#![cfg(unix)]
mod common;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::workspace::{output_child, ready_exe, wait_child, Harness, WS_ID};
use serde_json::{json, Value};
use zygo_core::Agent;
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind};

const KEY: &str = "kaaaaaaaa";
const KEY2: &str = "kbbbbbbbb";
const OLD_ZS: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn json_file(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

/// Polls `f` every 20 ms until it is true or `bound` is over (a test bound).
fn until(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = std::time::Instant::now();
    loop {
        if f() {
            return true;
        }
        if t0.elapsed() >= bound {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Writes `body` to `path` with mode 0755, then `ready_exe(path)` (D14).
fn write_exe(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    ready_exe(path);
}

// ---- Harness (W§18 preamble) ----

#[test]
fn harness_new_at_a_missing_tmux_is_none() {
    assert!(Harness::new_at(Path::new("/nonexistent/zygo-m2/tmux")).is_none());
}

#[test]
fn harness_seed_writes_where_the_binary_reads() {
    let h = Harness::no_tmux();
    h.seed(WS_ID, &mut |_| {});
    // W§7.1: the binary with HOME = root reads `<root>/.local/state/zygo/workspaces/<id>/workspace.json`.
    assert!(h.root().join(".local/state/zygo/workspaces").join(WS_ID).join("workspace.json").is_file());
    assert_eq!(h.store(WS_ID).read().unwrap().unwrap().name, "app");
}

#[test]
fn harness_new_starts_a_private_server() {
    let Some(h) = Harness::new() else { return };
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "cat"]);
    assert_eq!(
        h.tmux(&["display-message", "-p", "-t", "=zygo-app:", "#{socket_path}"]),
        h.socket().display().to_string()
    );
}

#[test]
fn harness_new_at_starts_the_server_with_that_tmux() {
    let Some(real) = zygo_workspace::mux::testing::require_tmux() else { return };
    let d = tempfile::tempdir().unwrap();
    let calls = d.path().join("calls");
    let rec = d.path().join("tmux");
    write_exe(&rec, &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n", calls.display(), real.display()));
    let h = Harness::new_at(&rec).unwrap();
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "cat"]);
    assert!(std::fs::read_to_string(&calls).unwrap().lines().any(|l| l.contains("new-session")));
    assert_eq!(std::fs::read_link(h.bin.join("tmux")).unwrap(), rec);
}
