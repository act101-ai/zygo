# FX-6 — Dead-port test helpers: a freed loopback port can be taken by a parallel test's server (CI flake)

**Spec:** — (test harness defect, CI run 45557591675: `cmd::compact::tests::unreachable_is_compact_failed` got `400 "one\ntwo\e[31m"` from a `canned_session` server bound on the port that `dead_session` (`crates/zygo/src/cmd/compact.rs:188-198`) had read and freed); review focus 75 (no new test reads a clock). The spec does not change.
**Files:** `crates/zygo/tests/dead_port/mod.rs` (new, the one shared helper), `crates/zygo/src/cmd/compact.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo/tests/member_fixture/mod.rs`, `crates/zygo/tests/serve_cli.rs`, `crates/zygo/tests/workspace_member.rs`, `crates/zygo-launch/tests/serve_client.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact cmd::status cmd::verbs && env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test session_verbs --test serve_cli --test workspace_member && env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --locked --test serve_client` → all pass (ignored tests stay ignored). Checked in review, not by this command: every "unreachable" URL or port these helpers make (`dead_session`, the three `closed_url`, `dead_session_at`, `closed_port`) can never be answered by another test while it is in use; no helper reads the port of a listener and then frees it; the change adds no sleep and no retry.

## Plan

Cause: each helper binds `127.0.0.1:0`, reads the port and frees it (`compact.rs:193-195`, `compact_cmd.rs:85-87`, `member_fixture/mod.rs:116-118`, `serve_cli.rs:90-92`, `serve_client.rs:149-151`; `workspace_member.rs:293` frees it as a temporary). From then on the port is free, and a parallel `127.0.0.1:0` bind can get it: in CI run 45557591675, `canned_session` (`compact.rs:201`, called at `compact.rs:318`) got the port of `dead_session`, so `unreachable_is_compact_failed` (`compact.rs:292-300`) got a canned `400`.

Fix: one shared module, `crates/zygo/tests/dead_port/mod.rs`, gives port 1 of 127.0.0.1: `closed_port()` returns `1` and `closed_url()` returns `http://127.0.0.1:1/`. It holds no socket, reads no listener port and frees nothing. Port 1 meets the contract for these reasons:
- No `127.0.0.1:0` bind gets port 1. A port-0 bind takes a port from the ephemeral range of the OS, and on the three CI OSes (`.github/workflows/ci.yml:35`: ubuntu, macos, windows) that range holds no port below 1024. Linux: `net.ipv4.ip_local_port_range`, default `32768 60999` (kernel `Documentation/networking/ip-sysctl.rst`; ip(7) names it as the source of ephemeral ports). macOS: `net.inet.ip.portrange.first`/`last` are 49152/65535 (measured on this Mac with `sysctl`; 2000 port-0 binds gave ports 51380 to 53380). Windows: the default dynamic port range is 49152 to 65535, and the lowest start port that can be set is 1025 (Microsoft KB 929851, "The default dynamic port range for TCP/IP has changed since Windows Vista and in Windows Server 2008").
- No test binds a fixed port: every bind in `crates/` is to port 0 (step 9 checks this). Also, a process without root cannot bind port 1 on Linux (`ip_unprivileged_port_start`, default 1024, `ip-sysctl.rst`; capabilities(7) `CAP_NET_BIND_SERVICE`) or on macOS (measured: a bind of `127.0.0.1:1` gave `Permission denied (os error 13)`).
- Nothing listens on port 1, so a connect fails with `ConnectionRefused`. Measured on this Mac: std `connect` gives `Connection refused (os error 61)` in under 1 ms, and reqwest gives a connect error (`is_connect=true`) with `tcp connect error: Connection refused (os error 61)`. Linux connect(2): `ECONNREFUSED`, "found no one listening on the remote address". Win32 `connect`: `WSAECONNREFUSED`, "The attempt to connect was forcefully rejected". CI is the first to prove the `ConnectionRefused` kind on Linux and Windows. CI already proves that a client call to `http://127.0.0.1:1/` gives `ClientError::Unreachable` with a connect cause on all three OSes: `unreachable_text_keeps_its_cause` (`serve_client.rs:435-443`) uses that URL and passed on ubuntu, macos and windows in CI run 45559607672.
- Every caller needs only a transport failure with no HTTP reply; none asserts the error kind or the word "refused". The callers assert `ClientError::Unreachable(_)` (`compact.rs:297`, `serve_client.rs:429-432`), text that starts with `compact failed: zygo proxy unreachable` / `session: unreachable (zygo proxy unreachable` / `redaction failed: zygo proxy unreachable: ` / `zygo: server unreachable: ` / `zygo usage --sync: session A1: server unreachable: ` / `zygo compact-tool: server unreachable: ` / `<url> unreachable (`, or exit 12. Both clients map every non-builder transport error to `Unreachable` (`zygo-proxy/src/client.rs:231-237`, `zygo-launch/src/serve_client.rs:212`). Two calls give the same URL; no caller needs distinct URLs.

Rejected methods, with the evidence from a scratch program on this Mac (not in the repo; std and reqwest 0.13.5 with the workspace `Cargo.lock`):
- A held socket, bound and never put in listen state (the first plan): macOS drops the SYN, so `connect` gave `TimedOut` at the 3 s bound of the scratch program (the implementer saw `Operation timed out (os error 60)` after about 7.8 s).
- `listen(fd, 0)` and no `accept`: three connects succeeded on macOS, and reqwest got `operation timed out` at its 5 s bound. A test would wait for the client timeout.
- Port 0 in the URL: `Url::parse` keeps port 0, and macOS gives `AddrNotAvailable` (`Can't assign requested address (os error 49)`), not a refusal. The Linux and Windows results are not proved.
- A listener that accepts and drops each connection: it needs a thread for the life of the process, and EOF or reset depends on timing. A constant port is simpler.

The six helpers keep their names and signatures and call the module. `status.rs:101` and `verbs.rs:335` import `dead_session` and do not change. No new crate, no sleep, no retry, no clock.

Module includes (a subdirectory of `tests/` is not a test target):
- `compact_cmd.rs`, `serve_cli.rs`, `workspace_member.rs`: `mod dead_port;`
- `member_fixture/mod.rs`: `#[path = "../dead_port/mod.rs"] mod dead_port;` (its own copy, so `session_verbs.rs` does not change; in `compact_cmd` the two copies give the same port, which does no harm)
- `compact.rs`, at file level: `#[cfg(test)] #[path = "../../tests/dead_port/mod.rs"] mod dead_port;` (the path is relative to `src/cmd/`)
- `serve_client.rs`: `#[path = "../../zygo/tests/dead_port/mod.rs"] mod dead_port;` (precedent: `crates/zygo-launch/tests/stub_member.rs:4`)

Commits: each commit message starts with `FX-6:` and ends with the `Zygo-Session` and `Zygo-Work` trailers that the brief gives, each on its own line. No Claude attribution.

- [x] 1. Write the failing test. Create `crates/zygo/tests/dead_port/mod.rs` with the old algorithm, so that the red result comes from behavior and not from a compile error:

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
fn closed_port_refuses_and_no_port_0_bind_gets_it() { // FX-6, CI run 45557591675
    let port = super::dead_port::closed_port();
    let e = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused, "{e}");
    assert!(port < 1024, "{port} is in an ephemeral range, so a parallel `127.0.0.1:0` bind can get it");
    assert_eq!(super::dead_port::closed_port(), port);
}
```

  The uncommitted `closed_port_refuses_and_no_bind_can_take_it` from the first plan is replaced by this test: its `AddrInUse` assertion cannot hold for port 1, because no socket holds port 1 (on macOS a bind of `127.0.0.1:1` gives `PermissionDenied`, measured).

- [x] 2. See it fail: `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact::tests::closed_port_refuses_and_no_port_0_bind_gets_it`. The connect assertion passes (the freed port refuses). The test fails at the `port < 1024` assertion, because the old algorithm gives a port from the ephemeral range. That is the cause of the CI flake.

- [x] 3. Implement. Replace the body of `crates/zygo/tests/dead_port/mod.rs` with this text (the uncommitted held-socket version from the first plan goes). The signatures do not change:

```rust
//! A loopback port that refuses every connection and that no other test can get (FX-6, CI run 45557591675): port 1.
//! A `127.0.0.1:0` bind takes a port from the ephemeral range of the OS, and on Linux, macOS and Windows that range
//! holds no port below 1024. No test binds a fixed port. Nothing listens on port 1, so a connect gets
//! `ConnectionRefused`. No socket is held and no port is read and freed.
#![allow(dead_code)] // each test binary uses a part of this module

/// Port 1 of 127.0.0.1, for each call.
pub fn closed_port() -> u16 {
    1
}

/// `http://127.0.0.1:<closed_port>/`.
pub fn closed_url() -> String {
    format!("http://127.0.0.1:{}/", closed_port())
}
```

- [x] 4. See it pass: run the command from step 2. The test passes. Commit `crates/zygo/tests/dead_port/mod.rs` and `crates/zygo/src/cmd/compact.rs`.

- [x] 5. Change `dead_session` in `crates/zygo/src/cmd/compact.rs:188-198` so that it writes `super::dead_port::closed_url()` to `sd.url()`. Delete the bind, `local_addr` and `drop` lines (193-195). The doc line becomes ``/// A session dir `id` whose url is the closed port of `dead_port`.`` Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact cmd::status cmd::verbs`. All tests pass, `unreachable_is_compact_failed` too. Commit.

- [x] 6. In `crates/zygo/tests/compact_cmd.rs`, add `mod dead_port;` after `mod common;` (line 19). Delete `fn closed_url` (lines 84-89). `dead_session` (line 80) writes `dead_port::closed_url()`. In `crates/zygo/tests/member_fixture/mod.rs`, add `#[path = "../dead_port/mod.rs"] mod dead_port;`. `dead_session_at` (lines 115-120) becomes `session_at(sessions, id, &dead_port::closed_url())`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test session_verbs`. All tests pass with no new warning. Commit.

- [ ] 7. In `crates/zygo/tests/serve_cli.rs`, add `mod dead_port;` and delete `fn closed_url` (lines 89-94). The callers at lines 214 and 253 call `dead_port::closed_url()`. In `crates/zygo/tests/workspace_member.rs`, add `mod dead_port;` after `mod common;` (line 3) and delete `fn closed_port` with its doc line (lines 291-294). The callers at lines 1242, 1324 and 1401 call `dead_port::closed_port()`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test serve_cli --test workspace_member`. All tests pass and the ignored tests stay ignored. Commit.

- [ ] 8. In `crates/zygo-launch/tests/serve_client.rs`, add `#[path = "../../zygo/tests/dead_port/mod.rs"] mod dead_port;` and delete `fn closed_url` with its doc line (lines 147-153). The callers at lines 430, 635, 994 and 1007 call `dead_port::closed_url()`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --locked --test serve_client`. All tests pass and the ignored tests stay ignored. Commit.

- [ ] 9. Review check: `grep -rnE "fn (closed_url|closed_port|dead_session|dead_session_at)\b" crates/ && grep -rn "drop(l)" crates/zygo/src/cmd/compact.rs crates/zygo/tests/compact_cmd.rs crates/zygo/tests/member_fixture/mod.rs crates/zygo/tests/serve_cli.rs crates/zygo-launch/tests/serve_client.rs`. The first grep lists only `dead_port/mod.rs` (`closed_port`, `closed_url`), `compact.rs` (`dead_session`), `compact_cmd.rs` (`dead_session`) and `member_fixture/mod.rs` (`dead_session_at`), and none of the last three reads a listener port. The second grep has no match. `grep -rnE "(TcpListener|TcpSocket|UdpSocket)::bind|libc::bind" crates/`: each match binds port 0 (`127.0.0.1:0`, `("127.0.0.1", 0)` or a zeroed `sockaddr_in`), or is `zygo-proxy/src/serve.rs` binding its `listen` value, which every test sets to a port-0 address; no match binds port 1. `git diff origin/phase-3 -- crates | grep -nE "sleep|Instant|retry|elapsed"` has no match.

- [ ] 10. Run the Acceptance command as written. All tests pass and the ignored tests stay ignored. Push `phase3/FX-6`.
