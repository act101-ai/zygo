# FX-6 — Dead-port test helpers: a freed loopback port can be taken by a parallel test's server (CI flake)

**Spec:** — (test harness defect, CI run 45557591675: `cmd::compact::tests::unreachable_is_compact_failed` got `400 "one\ntwo\e[31m"` from a `canned_session` server bound on the port that `dead_session` (`crates/zygo/src/cmd/compact.rs:188-198`) had read and freed); review focus 75 (no new test reads a clock). The spec does not change.
**Files:** `crates/zygo/tests/dead_port/mod.rs` (new, the one shared helper), `crates/zygo/src/cmd/compact.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo/tests/member_fixture/mod.rs`, `crates/zygo/tests/serve_cli.rs`, `crates/zygo/tests/workspace_member.rs`, `crates/zygo-launch/tests/serve_client.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact cmd::status cmd::verbs && env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test session_verbs --test serve_cli --test workspace_member && env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --locked --test serve_client` → all pass (ignored tests stay ignored). Checked in review, not by this command: every "unreachable" URL or port these helpers make (`dead_session`, the three `closed_url`, `dead_session_at`, `closed_port`) can never be answered by another test while it is in use; no helper reads the port of a listener and then frees it; the change adds no sleep and no retry.

## Plan

Cause: each helper binds `127.0.0.1:0`, reads the port and frees it (`compact.rs:193-195`, `compact_cmd.rs:85-87`, `member_fixture/mod.rs:116-118`, `serve_cli.rs:90-92`, `serve_client.rs:149-151`; `workspace_member.rs:293` frees it as a temporary). From then on the port is free, and a parallel `127.0.0.1:0` bind can get it: in CI run 45557591675, `canned_session` (`compact.rs:201`, called at `compact.rs:318`) got the port of `dead_session`, so `unreachable_is_compact_failed` (`compact.rs:292-300`) got a canned `400`.

Fix: one shared module, `crates/zygo/tests/dead_port/mod.rs`, holds one socket for the life of the test process on the port that it gives. Nothing listens on that port, so every connect fails with `ConnectionRefused` (as before). The held socket has no `SO_REUSEADDR`, so no other bind can take the port, and a `127.0.0.1:0` bind does not get it. Unix: a TCP socket from `libc` (already a normal dependency of `zygo` and of `zygo-launch`), bound and never put in listen state, the same raw-socket pattern as `serve_client.rs:323-327`. Windows (the CI matrix in `.github/workflows/ci.yml:35` runs `compact.rs`, `compact_cmd.rs`, `serve_cli.rs` and `serve_client.rs` there): the client end of a loopback connection holds its local port. Rust std sets no `SO_REUSEADDR` on Windows, so no bind can share that port. The Unix path does not use the connection method because macOS lets a bind with `SO_REUSEADDR` (std sets it on Unix) share a port that only connected sockets hold. The six helpers keep their names and signatures and call the module. `status.rs:101` and `verbs.rs:335` import `dead_session` and do not change. No new crate, no sleep, no retry, no clock.

Module includes (a subdirectory of `tests/` is not a test target):
- `compact_cmd.rs`, `serve_cli.rs`, `workspace_member.rs`: `mod dead_port;`
- `member_fixture/mod.rs`: `#[path = "../dead_port/mod.rs"] mod dead_port;` (its own copy, so `session_verbs.rs` does not change; in `compact_cmd` the two copies hold two ports, which does no harm)
- `compact.rs`, at file level: `#[cfg(test)] #[path = "../../tests/dead_port/mod.rs"] mod dead_port;` (the path is relative to `src/cmd/`)
- `serve_client.rs`: `#[path = "../../zygo/tests/dead_port/mod.rs"] mod dead_port;` (precedent: `crates/zygo-launch/tests/stub_member.rs:4`)

Commits: each commit message starts with `FX-6:` and ends with the `Zygo-Session` and `Zygo-Work` trailers that the brief gives, each on its own line. No Claude attribution.

- [ ] 1. Write the failing test. Create `crates/zygo/tests/dead_port/mod.rs` with the old algorithm, so that the red result comes from behavior and not from a compile error:

```rust
//! A loopback port that refuses every connection (FX-6).
#![allow(dead_code)] // each test binary uses a part of this module

/// A port with no listener.
pub fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// `http://127.0.0.1:<closed_port>/`.
pub fn closed_url() -> String {
    format!("http://127.0.0.1:{}/", closed_port())
}
```

  Add the file-level `#[cfg(test)] #[path = "../../tests/dead_port/mod.rs"] mod dead_port;` to `crates/zygo/src/cmd/compact.rs`. Then add this test to `compact.rs` `mod tests`:

```rust
#[test]
fn closed_port_refuses_and_no_bind_can_take_it() { // FX-6, CI run 45557591675
    let port = super::dead_port::closed_port();
    let e = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused, "{e}");
    let e = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse, "{e}");
    assert_eq!(super::dead_port::closed_port(), port);
}
```

- [ ] 2. See it fail: `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact::tests::closed_port_refuses_and_no_bind_can_take_it`. The test fails at the `AddrInUse` assertion: the bind succeeds and `unwrap_err` panics on `Ok`. This is the CI flake, done on purpose.

- [ ] 3. Implement. Replace the body of `crates/zygo/tests/dead_port/mod.rs` with this text. The signatures do not change:

```rust
//! A loopback port that refuses every connection and that no other socket can take while the test process runs
//! (FX-6, CI run 45557591675). One socket per process holds the port. Nothing listens on it, so a connect gets
//! `ConnectionRefused`. The holder has no `SO_REUSEADDR`, so no bind gets the port, a parallel test's
//! `127.0.0.1:0` bind too.
#![allow(dead_code)] // each test binary uses a part of this module

use std::sync::OnceLock;

/// The held port: the same port for each call in one process.
pub fn closed_port() -> u16 {
    static HELD: OnceLock<(Held, u16)> = OnceLock::new();
    HELD.get_or_init(hold).1
}

/// `http://127.0.0.1:<closed_port>/`.
pub fn closed_url() -> String {
    format!("http://127.0.0.1:{}/", closed_port())
}

/// Unix: a TCP socket bound to 127.0.0.1 and never put in listen state. Not a connected socket, because macOS lets
/// a bind with `SO_REUSEADDR` (std sets it on unix) share a port that only connected sockets hold.
#[cfg(unix)]
type Held = std::os::fd::OwnedFd;

#[cfg(unix)]
fn hold() -> (Held, u16) {
    use std::os::fd::FromRawFd;
    // SAFETY: plain socket calls on a new descriptor; `OwnedFd` takes ownership of it.
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let held = std::os::fd::OwnedFd::from_raw_fd(fd);
        assert_eq!(libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC), 0); // no child process inherits it
        let mut a: libc::sockaddr_in = std::mem::zeroed();
        a.sin_family = libc::AF_INET as libc::sa_family_t;
        a.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
        let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        assert_eq!(libc::bind(fd, &a as *const libc::sockaddr_in as *const libc::sockaddr, len), 0);
        assert_eq!(libc::getsockname(fd, &mut a as *mut libc::sockaddr_in as *mut libc::sockaddr, &mut len), 0);
        (held, u16::from_be(a.sin_port))
    }
}

/// Not unix (Windows): the client end of a loopback connection holds its local port and nothing listens there.
/// Windows std sets no `SO_REUSEADDR`, so no bind shares the port.
#[cfg(not(unix))]
type Held = (std::net::TcpStream, std::net::TcpStream);

#[cfg(not(unix))]
fn hold() -> (Held, u16) {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let c = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (a, _) = l.accept().unwrap();
    let port = c.local_addr().unwrap().port();
    ((c, a), port)
}
```

- [ ] 4. See it pass: run the command from step 2. The test passes. Commit `crates/zygo/tests/dead_port/mod.rs` and `crates/zygo/src/cmd/compact.rs`.

- [ ] 5. Change `dead_session` in `crates/zygo/src/cmd/compact.rs:188-198` so that it writes `super::dead_port::closed_url()` to `sd.url()`. Delete the bind, `local_addr` and `drop` lines (193-195). The doc line becomes ``/// A session dir `id` whose url is the held closed port of `dead_port`.`` Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact cmd::status cmd::verbs`. All tests pass, `unreachable_is_compact_failed` too. Commit.

- [ ] 6. In `crates/zygo/tests/compact_cmd.rs`, add `mod dead_port;` after `mod common;` (line 19). Delete `fn closed_url` (lines 84-89). `dead_session` (line 80) writes `dead_port::closed_url()`. In `crates/zygo/tests/member_fixture/mod.rs`, add `#[path = "../dead_port/mod.rs"] mod dead_port;`. `dead_session_at` (lines 115-120) becomes `session_at(sessions, id, &dead_port::closed_url())`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test session_verbs`. All tests pass with no new warning. Commit.

- [ ] 7. In `crates/zygo/tests/serve_cli.rs`, add `mod dead_port;` and delete `fn closed_url` (lines 89-94). The callers at lines 214 and 253 call `dead_port::closed_url()`. In `crates/zygo/tests/workspace_member.rs`, add `mod dead_port;` after `mod common;` (line 3) and delete `fn closed_port` with its doc line (lines 291-294). The callers at lines 1242, 1324 and 1401 call `dead_port::closed_port()`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test serve_cli --test workspace_member`. All tests pass and the ignored tests stay ignored. Commit.

- [ ] 8. In `crates/zygo-launch/tests/serve_client.rs`, add `#[path = "../../zygo/tests/dead_port/mod.rs"] mod dead_port;` and delete `fn closed_url` with its doc line (lines 147-153). The callers at lines 430, 635, 994 and 1007 call `dead_port::closed_url()`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --locked --test serve_client`. All tests pass and the ignored tests stay ignored. Commit.

- [ ] 9. Review check: `grep -rnE "fn (closed_url|closed_port|dead_session|dead_session_at)\b" crates/ && grep -rn "drop(l)" crates/zygo/src/cmd/compact.rs crates/zygo/tests/compact_cmd.rs crates/zygo/tests/member_fixture/mod.rs crates/zygo/tests/serve_cli.rs crates/zygo-launch/tests/serve_client.rs`. The first grep lists only `dead_port/mod.rs` (`closed_port`, `closed_url`), `compact.rs` (`dead_session`), `compact_cmd.rs` (`dead_session`) and `member_fixture/mod.rs` (`dead_session_at`), and none of the last three reads a listener port. The second grep has no match. `git diff origin/phase-3 -- crates | grep -nE "sleep|Instant|retry|elapsed"` has no match.

- [ ] 10. Run the Acceptance command as written. All tests pass and the ignored tests stay ignored. Push `phase3/FX-6`.
