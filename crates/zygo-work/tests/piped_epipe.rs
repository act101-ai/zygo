//! DR-11 :147: `Piped::send` does `write_all` + `flush`, and a write error (EPIPE) gives `Failed`.
//!
//! This file must hold exactly one test. Cargo builds each integration-test file as its own process, and one test
//! means one test thread. On Linux a pipe write gets EPIPE only when no process holds the read end; a child that
//! another test thread spawns holds a copy of this test's stdin read end until its `execve`. With no other thread,
//! no such child exists, so the EPIPE is deterministic. Add no second test here.

#![cfg(unix)]

use std::process::Command;
use std::time::{Duration, Instant};
use zygo_work::repo::{run_piped, RunError};

#[test]
fn piped_send_after_the_child_closed_stdin_fails_with_epipe() {
    // The child closes its stdin, then says so; it exits at once, and nothing else holds the read end.
    let mut c = Command::new("sh");
    c.args(["-c", "exec 0<&-; printf 'closed\\n'"]);
    let mut p = run_piped(c, Instant::now() + Duration::from_secs(3600)).unwrap();
    assert_eq!(p.read_line(16).unwrap(), b"closed");
    assert!(matches!(p.send(b"x\n"), Err(RunError::Failed)));
}
