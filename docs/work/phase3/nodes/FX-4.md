# FX-4 — Proxy rows-bound test waits for the written row before the complete-flag GET (Windows flake)

**Spec:** `docs/specs/2026-11-18-zygo-design.md` :703, note "N37 plan review" (the rows reply carries `complete`, false when the wait of §7 ended with rows still queued); review focus 75 (no new test reads a clock). The spec does not change.
**Files:** `crates/zygo-proxy/tests/serve.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo-proxy --locked --test serve` → all pass; in `requests_reply_ends_at_the_rows_bound_incomplete`, a `usage_written(1, …)` assertion comes between `hold.release()` and the next `requests(` call; the change adds no sleep.

## Plan

### Defect (found in CI)

`requests_reply_ends_at_the_rows_bound_incomplete` (`crates/zygo-proxy/tests/serve.rs:807-821`) fails intermittently on windows-latest, for example CI run 45543172599: `left: (200, 1, Some(false)) right: (200, 1, Some(true))` at `serve.rs:818`. The order of events:

1. After `hold.release()` (`serve.rs:817`) the test sends the GET at once (`serve.rs:818`).
2. The route computes `complete = usage.flush(&id, rows_wait)` and only then reads the store (`crates/zygo-proxy/src/routes.rs:164-171`).
3. `flush` waits at most `rows_wait` for the session's queue entry to go (`crates/zygo-proxy/src/tee.rs:584-588`). The test sets `rows_wait` to 300 ms for the whole server (`serve.rs:812`).
4. The writer inserts the row, then drops its ticket (`tee.rs:532-536`); the ticket's `Drop` removes the queue entry (`tee.rs:427-437`). When the writer finishes after the 300 ms wait but before the store read, the reply is `(1 row, complete=false)`.

That reply is correct by the spec note at :703. The test races wall-clock time; the product is correct, so the fix goes in the test.

### Premises checked against the live code (branch `phase3/FX-4` at `457fcc39`)

- `serve.rs:817` is `hold.release();`, `serve.rs:818` is the GET `requests(&s, &id, &s.alice)` with the `(200, 1, Some(true))` assertion.
- `UsageServer::usage_written(n, limit)` (`crates/zygo-proxy/src/server.rs:320-324`, feature `test-hooks`, which the dev-dependency in `crates/zygo-proxy/Cargo.toml:43` turns on) waits on the `handled` count (`tee.rs:595-598`). The writer calls `count(&h)` (`tee.rs:486-489`) only after `drop(ticket)` (`tee.rs:536-537`), so once `usage_written(1, …)` returns true the queue entry is gone and `flush` returns true at once.
- `serve.rs` already uses this seam (for example `serve.rs:742`, `serve.rs:842`).

### Steps

- [ ] **1. Verify the premise.** Read `serve.rs:807-821`, `routes.rs:164-171`, `tee.rs:427-437`, `tee.rs:521-541`, `tee.rs:584-598` and `server.rs:320-324`; confirm each fact in the premises list. Check: every cited line matches.
- [ ] **2. Make the one-line change.** In `requests_reply_ends_at_the_rows_bound_incomplete`, between `hold.release();` and the `requests(` GET, insert:
  ```rust
  assert!(s.h.usage_written(1, Duration::from_secs(30)));
  ```
  The 30 s value is a failure bound, like the test's `recv_timeout`, not a margin. Check: `git diff --stat` lists only `crates/zygo-proxy/tests/serve.rs` (and this node file), one line added; the diff contains no `sleep`.
- [ ] **3. Run the acceptance.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-proxy --locked --test serve` → all pass. Check: the order `hold.release()` → `usage_written(1, …)` → `requests(` holds in the test body.
- [ ] **4. Commit.** `FX-4: rows-bound test waits for the written row before the complete-flag GET`, with the brief's `Zygo-Session` and `Zygo-Work` trailers, no attribution. Push the node branch.

### Note: no failing-first test

No test can fail deterministically before this fix without a new product test hook (a hold between the writer's insert and its ticket drop), so the plan adds none; this is the author's call. The change only removes the race: the GET now starts after the row is counted, so `flush` sees an empty queue whatever the scheduler does.
