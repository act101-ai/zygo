# FIX-1 — request_path tests: a refused port that no other test can take

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §7.3 (proxy row and note 2026-11-18: fallback retries only a connect error, a 5xx or a 429), §10.4, §20 #11; found defect, graph Log 2026-11-21 and `docs/work/phase2/nodes/HUD-10.md:41`
**Files:** `crates/zygo-proxy/tests/request_path.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `for i in $(seq 20); do cargo test -p zygo-proxy --locked --test request_path || exit 1; done` → 20 runs, each `0 failed`, each with `refused_port_cannot_go_to_another_listener ... ok`, `fallback_all_failed_removes_pin ... ok` and `fallback_429_and_connect_refused ... ok`

## Plan

Work in the node's checkout. Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-1`. In each commit, tick (`- [x]`) the steps of this file that are done.

Facts of the live code (read before you start):

- `refused_port()` (`request_path.rs:183-185`) binds `127.0.0.1:0` and releases the port. Callers: `:1067` and `:1134-1135`. The port is used later (`:1080`, `:1154`). In that time every other test of the binary binds `127.0.0.1:0` (`Upstream::start` `:142`, the proxy at `crates/zygo-proxy/src/server.rs:48`, `closing_upstream` `:189`, `raw_upstream` `:232`, `drip_upstream` `:257`, `fake_proxy` `:306-307`) and can get the released port. An `Upstream` then answers 200 with `SSE` (`:110`) for the "dead" provider.
- The attempt loop goes to the next provider only on `e.is_connect()` (`crates/zygo-proxy/src/forward.rs:451`). Any other send error ends the attempts (`forward.rs:452-456`; test `fallback_not_on_4xx_or_send_error`). So the dead endpoint must fail in the connect, and a listener that accepts and closes is not a replacement.
- A bound socket that does not listen is no replacement: macOS drops its SYN (comment at `:181-182`).
- Design (author's call): the fixed port 1. No OS gives a port below 1024 for a port 0 bind, so no listener of this binary can get it. A connect to a closed loopback port is the mechanism the tests use today.
- Residual assumption: nothing listens on TCP port 1 (tcpmux) on a development or CI machine. The proof test fails with the text `a service listens on the refused port` when something does.
- Two providers of one model may share a host and differ in the path (`base`, `:372-375`: `{u}/a`, `{u}/b`, `{u}/c`).

### Task 1: the proof test, seen to fail

- [x] **1.1** Add this test directly above `fn fallback_500_moves_pin` (`:1048`):

```rust
/// The refused port is outside the range that a port 0 bind gives, so no listener of this binary can take it, and a
/// connect to it is refused while other listeners are held.
#[test]
fn refused_port_cannot_go_to_another_listener() {
    let p = refused_port();
    assert!(p < 1024, "port {p} is in the range that a port 0 bind gives");
    let held: Vec<std::net::TcpListener> = (0..64).map(|_| std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap()).collect();
    for l in &held {
        let q = l.local_addr().unwrap().port();
        assert!(q >= 1024 && q != p, "a port 0 bind gave {q}");
    }
    let e = std::net::TcpStream::connect(("127.0.0.1", p)).expect_err("a service listens on the refused port");
    assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused, "{e}");
    drop(held);
}
```

- [x] **1.2** Run `cargo test -p zygo-proxy --locked --test request_path refused_port_cannot`. Expected: FAIL at the first assertion, `port <n> is in the range that a port 0 bind gives`, with `<n>` at or above 1024.

### Task 2: the fixed port

- [x] **2.1** Replace `:181-185` with:

```rust
/// A port that no test can take: every listener of this binary binds port 0, and no OS gives a port below 1024 for
/// port 0. Nothing listens on TCP port 1, so a connect is refused at once. Not a released port (another test's
/// listener can get it). Not a bound socket that does not listen (refused on Linux, but macOS drops its SYN and the
/// connect times out). Not a listener that accepts and closes (a send error: no fallback, §7.3 note).
fn refused_port() -> u16 {
    1
}
```

- [x] **2.2** In `fallback_all_failed_removes_pin`, give the two providers different paths on the one port: `prov("d1", &format!("http://127.0.0.1:{d1}/d1"), "x", None, "")` and `prov("d2", &format!("http://127.0.0.1:{d2}/d2"), "x", None, "")`. Change no assertion. `fallback_429_and_connect_refused` needs no change.
- [x] **2.3** Run `cargo test -p zygo-proxy --locked --test request_path refused_port_cannot`. Expected: `1 passed; 0 failed`.
- [x] **2.4** Run `cargo test -p zygo-proxy --locked --test request_path fallback_`. Expected: `0 failed`. If the config load refuses the provider URLs of step 2.2, stop and report the error text.
- [x] **2.5** Commit `request_path.rs` and this file: `test(proxy): the refused port of request_path is a fixed port that no listener can take`.

### Task 3: verification

- [ ] **3.1** Acceptance. Run `for i in $(seq 20); do cargo test -p zygo-proxy --locked --test request_path || exit 1; done`. Expected: 20 runs, each `0 failed`, each with the three tests that the contract names.
- [ ] **3.2** Commit this file with all steps ticked: `docs(work): FIX-1 plan steps done`.
