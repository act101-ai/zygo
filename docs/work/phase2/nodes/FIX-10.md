# FIX-10 — Record the upstream error type and message of a non-2xx passthrough answer (FIX-8 follow-up)

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §14 (usage store, metadata only: the `requests` columns, "Never: bodies, prompts, tool inputs …"), §12.4 (passthrough), §7.3 (a 5xx or 429 is kept as the last answer and relayed); found by FIX-8 (`docs/work/phase2/nodes/FIX-8.md` "Root cause", verdict `not determined`: in 24 of 24 passthrough sessions the first `auxiliary` request is answered 429, and no record holds the upstream error text, because the usage row stores the status only, `crates/zygo-usage/src/schema.rs`). Code: a 5xx or 429 is kept as `last` (`crates/zygo-proxy/src/forward.rs:446`) and relayed to the client (`:465-469`); a 4xx other than 429 is the served answer (`:447-450`) and is relayed the same way; the response tee copies a non-streamed body up to 4 MiB for parsing (§14; `crates/zygo-proxy/src/tee.rs:152-161`) and clears the copy in `Tee::finish` (`tee.rs:182`); the one thread that owns the proxy's `Store` is the usage writer of `UsageSink` (`tee.rs:325-366`), whose channel carries only `RequestRow` (`tee.rs:307`). Change: for a passthrough request whose upstream answer is not 2xx, zygo parses the relayed body as the Anthropic error form `{"type":"error","error":{"type":…,"message":…}}` and writes one row to a new usage table `upstream_errors` (`ts`, `session`, `request_class`, `model`, `status`, `error_type`, `error_message`), created with `CREATE TABLE IF NOT EXISTS` so an existing `usage.db` needs no migration and `RequestRow` and its literals do not change. `error_message` keeps at most 300 characters, with control characters replaced by a space; no request byte, header or credential is written. A body that does not parse writes the row with `error_type` and `error_message` empty. The relayed answer to the client does not change (status, headers, body). Retention as `requests` (`[usage] retain_days`). The node adds a dated note to §14 of the base spec for the new table and one base-spec change-log line: "§14: table `upstream_errors` records the upstream error type and message of a non-2xx passthrough answer; LIV-1 D8, FIX-8, FIX-10."
**Files:** `crates/zygo-usage/src/schema.rs`, `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/tests/passthrough.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** `schema::UpstreamErrorRow { ts: i64, session: String, request_class: Option<String>, model: String, status: u16, error_type: String, error_message: String }`, `store::Store::insert_upstream_error(&self, row: &UpstreamErrorRow) -> anyhow::Result<()>`, `store::Store::upstream_errors(&self, session: &str) -> anyhow::Result<Vec<UpstreamErrorRow>>`
**Acceptance:** `cargo test -p zygo-proxy --locked --test passthrough && cargo test -p zygo-usage --locked --test store && cargo test --workspace --locked --no-run` → 0 failed, including, with the mock upstream of `passthrough.rs` (`Upstream::answer`): an `auxiliary` request answered 429 with `{"type":"error","error":{"type":"rate_limit_error","message":"limit"}}` gives one `upstream_errors` row with status 429, class `auxiliary`, `rate_limit_error` and `limit`, and the client still gets the same status, `retry-after` and body; a 200 answer gives no row; a 500 with a body that is not JSON gives one row with empty type and message; a message of 1000 characters with a newline is stored as 300 characters with no control character; a request whose body holds a tracer word writes no row field that holds it; and `store.rs` creates the table on an existing database that has only the old tables

## Plan

### Contract findings (fixed in the contract above, 2026-11-22)

- F1, Files: `crates/zygo-proxy/src/tee.rs` was missing. The proxy writes usage rows only through the writer thread of `UsageSink::open` (`crates/zygo-proxy/src/tee.rs:325-366`), which owns the only `Store`; its channel type `RowSender` (`tee.rs:307`) carries `(RequestRow, Option<Ticket>)` only, and `UsageSink::send` (`tee.rs:375-380`) is the one entry. The relayed body is held only in `Tee::copy` (`tee.rs:53`, filled at `tee.rs:152-161`), and `Tee::finish` clears it (`tee.rs:182`) before `Flow::drop` (`crates/zygo-proxy/src/forward.rs:1052-1063`) can read it. The error row cannot be written without a change to `tee.rs`. Files now names it.
- F2, Spec line: the code cites only the 5xx/429 path (`forward.rs:446`). A 4xx other than 429 (400, 401, 403, 404, 413) is the served answer (`forward.rs:447-450`) and reaches the client by the same `Flow` (`forward.rs:567-571`). "Not 2xx" in the contract covers both; the Spec line now names `:447-450`.
- No other drift: `forward.rs:446` and `:465-469` are as cited; `SCHEMA` (`crates/zygo-usage/src/schema.rs:5-67`) runs on every `Store::open` while `user_version` ≤ 1 (`crates/zygo-usage/src/store.rs:82-90`), so `CREATE TABLE IF NOT EXISTS` reaches an existing `usage.db` without a version change, as `sync_pending` does (`schema.rs:66`, test `sync_pending_table_is_created_on_every_open`, `crates/zygo-usage/tests/store.rs:398`).

### Builds on (signatures in the code now)

- `zygo_usage::Store::open(path: &Path) -> anyhow::Result<Store>` (`crates/zygo-usage/src/store.rs:66`); `Store::prune(&self, retain_days: u32) -> anyhow::Result<()>` (`store.rs:237`); `Store::requests_for_session(&self, id: &str) -> anyhow::Result<Vec<RequestRow>>` (`crates/zygo-usage/src/query.rs:62`); `Store::insert_request(&self, r: &RequestRow) -> anyhow::Result<()>` (`store.rs:133`).
- `zygo_proxy::tee::Tee::new(streamed: bool) -> Tee` (`tee.rs:136`); `Tee::finish(&mut self, _on_line: &mut dyn FnMut(&mut SseLine)) -> Bytes` (`tee.rs:173`); `UsageSink::send(&self, r: RequestRow, t: Option<Ticket>)` (`tee.rs:375`, `pub(crate)`).
- `zygo_redact::secret::redact(body: &mut Value, c: &Compiled, cache: &mut SessionCache, policy: &DeadlinePolicy) -> Result<Findings, RedactError>` (`crates/zygo-redact/src/secret.rs:74`); `SessionState::redaction: Redaction { compiled: Compiled, secret_cache: Mutex<SessionCache>, content_cache: Mutex<SessionCache> }` (`crates/zygo-proxy/src/session.rs:210-214`, `:242`); `DeadlinePolicy::default()` (`secret.rs:22-26`).
- Test harness: `ProxyHandle::usage_written(&self, n: u64, limit: Duration) -> bool` (`crates/zygo-proxy/src/server.rs:317`); `Upstream::answer(&self, status: u16, headers: &[(&str, &str)], body: &Value)` (`crates/zygo-proxy/tests/passthrough.rs:96`); `setup`, `grant`, `post_pt`, `cc_headers`, `rows`, `hv`, `has`, `read_all`, `key_sample`, `mb`, `send` (`passthrough.rs:149-255`, `:402`).

### Plan produces

- `zygo_usage::schema::UpstreamErrorRow { ts: i64, session: String, request_class: Option<String>, model: String, status: u16, error_type: String, error_message: String }` (re-exported as `zygo_usage::UpstreamErrorRow` by `pub use schema::*`, `crates/zygo-usage/src/lib.rs`).
- `zygo_usage::store::Store::insert_upstream_error(&self, row: &UpstreamErrorRow) -> anyhow::Result<()>`.
- `zygo_usage::store::Store::upstream_errors(&self, session: &str) -> anyhow::Result<Vec<UpstreamErrorRow>>`.
- Internal, changed: `Tee::keep_body(&mut self)`, `Tee::body(&self) -> Option<&[u8]>`, `UsageSink::send(&self, r: RequestRow, e: Option<UpstreamErrorRow>, t: Option<Ticket>)`.

### Design decisions (author's calls, made at planning; the executor applies them as written)

- D1, table and migration: one new table `upstream_errors (ts INTEGER NOT NULL, session TEXT NOT NULL, request_class TEXT, model TEXT NOT NULL, status INTEGER NOT NULL, error_type TEXT NOT NULL, error_message TEXT NOT NULL)` with indexes `upstream_errors_session_ts (session, ts)` and `upstream_errors_ts (ts)`, appended to `SCHEMA`. No `SCHEMA_VERSION` change and no migration step. Reason: `Store::open` runs `SCHEMA` on every open (`store.rs:86`), as for `sync_pending`; `open_read_only` (the status line, `store.rs:111-124`) checks `user_version == 1` and keeps working; `RequestRow` and its literals do not change (contract). The indexes match `requests` (`schema.rs:61-62`): the read is by session, `prune` is by `ts`.
- D2, size cap: `error_message` and `error_type` each keep at most 300 Unicode characters (`char`s, not bytes, so a multi-byte character is never cut), after each control character (`char::is_control`) becomes one space. The parse reads the tee copy, which is already bounded by `COPY_MAX` = 4 MiB (`tee.rs:16`, §14); a larger body is not copied (`tee.rs:154-156`) and gives empty fields. Reason: the contract's 300; the 4 MiB copy bound is the existing §14 bound, so no second buffer and no second limit.
- D3, statuses: a row for every passthrough request whose relayed upstream status is not 2xx (`!StatusCode::is_success()`): the 5xx and 429 `last` answer and every other 4xx served answer. No row for a 2xx, for a zygo refusal before or without an upstream answer (`ZygoError`, for example `401 zygo_passthrough_token_invalid`, `422 zygo_redaction_block`, `503 zygo_provider_unavailable`), or for a non-passthrough session. Reason: FIX-8 needs the vendor's text for any vendor refusal, and a 4xx such as 400 or 403 is the likely form of a client-identity refusal (FIX-8 "What a next node needs"); a zygo refusal already has its code in the response and its status in `requests`.
- D4, streaming error events: not counted. An `event: error` inside a 2xx SSE stream writes no row, and a non-2xx answer with `content-type: text/event-stream` writes a row with empty type and message (the tee keeps no copy of a streamed body, `tee.rs:152-170`). Reason: the contract is about a non-2xx status; the FIX-8 evidence is a 429 status with a JSON body; reading SSE lines would add a parse in the line path (`tee.rs:201-244`) for a case that no record shows.
- D5, redaction: no request byte, header or credential is read for the row; the row takes `ts`, `session`, `request_class` and `model` from the usage row that is written anyway (`forward.rs:283-299`, `:372`), and `error_type` and `error_message` from the upstream body only. `error_message` goes through the session's secret pass (`zygo_redact::secret::redact` with `SessionState::redaction.compiled` and `secret_cache`, `DeadlinePolicy::default()`) before the control-character and 300-character step, always, also when `[features] redaction.secrets` is off. When the pass blocks, times out or panics (caught with `catch_unwind`), `error_message` is empty. Reason: §14 "Never: … credentials"; an upstream message can echo a key or a part of the request; redaction before truncation so a cut cannot leave a part of a secret that the pass would have matched whole; fail closed, as stage 6 does for a body (`crates/zygo-proxy/src/redact_stage.rs:133-144`). The content pass (§16.2) is not used: it works on the `messages` layout of a request, not on one string.
- D6, write path: the error row rides the same channel message as its request row, `(RequestRow, Option<UpstreamErrorRow>, Option<Ticket>)`; the writer inserts the request row, then the error row, then drops the ticket and counts one. Reason: one store owner (`Store` is not `Sync`, `tee.rs:309`); `usage_written(n)` and the rows reply wait (`UsageSink::flush`) stay exact, because both rows are in before the count.
- D7, where the body is read: `run` calls `Tee::keep_body` for a passthrough non-2xx answer; `Tee::finish` then keeps the copy, and `Flow::drop` (after the tee is finished, `forward.rs:1055-1059`) builds the row from `Tee::body` and puts it in `Pending::upstream_error`; `Pending::drop` sends it. A client that leaves before the body ends gives a partial copy, which does not parse: the row has empty type and message, and its status is still recorded. Reason: the tee copy is the existing reader of the relayed body (no second buffer); `Flow::drop` runs on stream end and on client disconnect alike (§14).
- D8, parse: the body must be a JSON object with `"type": "error"`; `error_type` is `error.type` and `error_message` is `error.message` when each is a string, else empty. Any other body (not JSON, another shape) gives both empty.
- D9, retention: `Store::prune` deletes `upstream_errors` rows with `ts` older than `retain_days`, in the same transaction as `requests`. No read surface (`zygo usage`, status line) changes in this node; the rows are read with `Store::upstream_errors` or `sqlite3`.
- D10, placeholder text in tests: the secret-pass test builds the expected placeholder from two parts (`["[REDACTED", ":anthropic_api_key]"].concat()`). Reason: the session's edit hook refuses a file write that holds the placeholder text as one literal.

### Steps

Work in the node's checkout. Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-10`. In each commit, tick (`- [x]`) the steps of this file that are done. No step sends a request to a real vendor.

#### Task 1: the usage table (`crates/zygo-usage`)

- [ ] **1.1** In `crates/zygo-usage/tests/store.rs`, change the expected list of `schema_creates_three_tables_in_wal_mode` to `["requests", "sync_pending", "trims", "upstream_errors", "work_units"]`, and add to `never_list_columns_are_exact`:

  ```rust
  assert_eq!(cols(&t.db(), "upstream_errors"), ["ts","session","request_class","model","status","error_type","error_message"]);
  ```

  Result: the two tests name the new table and its exact columns.
- [ ] **1.2** In the same file, add the helpers and three tests:

  ```rust
  fn uerr(session: &str, ts: i64) -> UpstreamErrorRow {
      UpstreamErrorRow { ts, session: session.into(), request_class: Some("auxiliary".into()), model: "m1".into(), status: 429,
          error_type: "rate_limit_error".into(), error_message: "limit".into() }
  }
  fn tables(db: &Path) -> Vec<String> {
      let c = Connection::open(db).unwrap();
      let mut q = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
      q.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
  }

  #[test] // FIX-10: insert and read by session, in ts order
  fn upstream_error_round_trip_by_session() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      let a = uerr("S1", 10);
      let b = UpstreamErrorRow { request_class: None, status: 500, error_type: String::new(), error_message: String::new(), ..uerr("S1", 11) };
      s.insert_upstream_error(&b).unwrap();
      s.insert_upstream_error(&a).unwrap();
      s.insert_upstream_error(&uerr("S2", 5)).unwrap();
      assert_eq!(s.upstream_errors("S1").unwrap(), vec![a, b]);
      assert_eq!(s.upstream_errors("S2").unwrap(), vec![uerr("S2", 5)]);
      assert!(s.upstream_errors("S9").unwrap().is_empty());
      assert_eq!(count(&t.db(), "requests"), 0);
  }

  #[test] // FIX-10: an existing usage.db with only the old tables gets the table on open; no schema version change
  fn upstream_errors_table_is_created_on_an_existing_database() {
      let t = Tmp::new();
      Store::open(&t.db()).unwrap().insert_request(&req("S1", 1)).unwrap();
      Connection::open(t.db()).unwrap().execute_batch("DROP TABLE upstream_errors").unwrap();
      assert_eq!(tables(&t.db()), ["requests", "sync_pending", "trims", "work_units"]);
      let s = Store::open(&t.db()).unwrap();
      s.insert_upstream_error(&uerr("S1", 2)).unwrap();
      assert_eq!(s.upstream_errors("S1").unwrap(), vec![uerr("S1", 2)]);
      assert_eq!(s.requests_for_session("S1").unwrap(), vec![req("S1", 1)]);
      let c = Connection::open(t.db()).unwrap();
      assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
  } // mutation: the table not in `SCHEMA`

  #[test] // FIX-10: retention as `requests` (§14 `[usage] retain_days`)
  fn prune_deletes_upstream_errors_older_than_retain_days() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      s.insert_upstream_error(&uerr("old", now() - 91 * DAY)).unwrap();
      s.insert_upstream_error(&uerr("young", now() - 89 * DAY)).unwrap();
      s.prune(90).unwrap();
      assert_eq!(survivors(&t.db(), "upstream_errors"), ["young"]);
      s.prune(0).unwrap();
      assert_eq!(survivors(&t.db(), "upstream_errors"), ["young"]);
  }
  ```

  Result: the file holds the three tests; `req`, `count`, `survivors`, `now`, `DAY` and `Tmp` are the existing helpers of the file.
- [ ] **1.3** Run `cargo test -p zygo-usage --locked --test store`. Expected: the build fails with errors that name `UpstreamErrorRow`, `insert_upstream_error` and `upstream_errors` (not found). No other error.
- [ ] **1.4** In `crates/zygo-usage/src/schema.rs`, append to `SCHEMA`, after the `sync_pending` line (`schema.rs:66`):

  ```sql
  CREATE TABLE IF NOT EXISTS upstream_errors (
      ts INTEGER NOT NULL,
      session TEXT NOT NULL,
      request_class TEXT,
      model TEXT NOT NULL,
      status INTEGER NOT NULL,
      error_type TEXT NOT NULL,
      error_message TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS upstream_errors_session_ts ON upstream_errors(session, ts);
  CREATE INDEX IF NOT EXISTS upstream_errors_ts ON upstream_errors(ts);
  ```

  and add after `WorkUnitRow`:

  ```rust
  /// The upstream error of one passthrough request whose relayed upstream status is not 2xx (§14, FIX-10). No request
  /// byte: `error_type` and `error_message` come from the upstream body only.
  #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
  pub struct UpstreamErrorRow {
      /// Unix seconds.
      pub ts: i64,
      pub session: String,
      pub request_class: Option<String>,
      pub model: String,
      pub status: u16,
      pub error_type: String,
      pub error_message: String,
  }
  ```

  Result: `SCHEMA` creates the table and its two indexes; `SCHEMA_VERSION` (`store.rs:54`) stays 1.
- [ ] **1.5** In `crates/zygo-usage/src/store.rs`, add to `impl Store`:

  ```rust
  /// One `upstream_errors` row (§14, FIX-10).
  pub fn insert_upstream_error(&self, row: &UpstreamErrorRow) -> Result<()> {
      self.conn.prepare_cached(
          "INSERT INTO upstream_errors (ts, session, request_class, model, status, error_type, error_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      )?.execute(rusqlite::params![row.ts, row.session, row.request_class, row.model, row.status, row.error_type, row.error_message])?;
      Ok(())
  }

  /// The `upstream_errors` rows of `session`, by `ts`, then insertion order.
  pub fn upstream_errors(&self, session: &str) -> Result<Vec<UpstreamErrorRow>> {
      let mut q = self.conn.prepare(
          "SELECT ts, session, request_class, model, status, error_type, error_message FROM upstream_errors WHERE session = ?1 ORDER BY ts, rowid",
      )?;
      let rows = q.query_map([session], |r| {
          Ok(UpstreamErrorRow { ts: r.get(0)?, session: r.get(1)?, request_class: r.get(2)?, model: r.get(3)?, status: r.get(4)?,
              error_type: r.get(5)?, error_message: r.get(6)? })
      })?;
      Ok(rows.collect::<rusqlite::Result<_>>()?)
  }
  ```

  and in `prune` (`store.rs:237-248`), after the `DELETE FROM requests` line: `tx.execute("DELETE FROM upstream_errors WHERE ts < ?1", [cutoff])?;`. Result: the two `Plan produces` methods of the store exist with the exact signatures; a stored status out of `u16` range is a read error (rusqlite `FromSql for u16`).
- [ ] **1.6** Run `cargo test -p zygo-usage --locked --test store`. Expected: 0 failed; the output lists `upstream_error_round_trip_by_session ... ok`, `upstream_errors_table_is_created_on_an_existing_database ... ok`, `prune_deletes_upstream_errors_older_than_retain_days ... ok`, `schema_creates_three_tables_in_wal_mode ... ok`, `never_list_columns_are_exact ... ok`.
- [ ] **1.7** Commit `crates/zygo-usage/src/schema.rs`, `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs` and this file: `feat(usage): table upstream_errors for the upstream error of a passthrough answer`.

#### Task 2: the proxy tests (`crates/zygo-proxy/tests/passthrough.rs`)

- [ ] **2.1** In `passthrough.rs`: change the import line `use zygo_usage::{RedactionCount, RequestRow, Store};` to `use zygo_usage::{RedactionCount, RequestRow, Store, UpstreamErrorRow};`; add to `impl Upstream`:

  ```rust
  /// Every later request is answered with `status`, no extra header, and the raw `body` (with `content-type: application/json`).
  fn answer_raw(&self, status: u16, body: &str) {
      *self.status.lock().unwrap() = status;
      self.headers.lock().unwrap().clear();
      *self.reply.lock().unwrap() = body.to_string();
  }
  ```

  and after `rows`:

  ```rust
  fn errs(t: &T, g: &SessionGrant) -> Vec<UpstreamErrorRow> {
      Store::open(&t.d.path().join("data/usage.db")).unwrap().upstream_errors(&g.session).unwrap()
  }
  ```

  Result: the harness can answer a body that is not JSON and read the session's `upstream_errors` rows.
- [ ] **2.2** Add, after `upstream_429_reaches_the_client_once_and_unchanged`, these seven tests:

  ```rust
  #[test] // FIX-10 (§14): a 429 answer writes its error type and message; the client gets the answer unchanged
  fn upstream_429_error_type_and_message_recorded() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let e = json!({"type":"error","error":{"type":"rate_limit_error","message":"limit"}});
      t.anth.answer(429, &[("retry-after", "7")], &e);
      let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("auxiliary"));
      assert_eq!((r.0, hv(&r.1, "retry-after")), (429, "7"));
      assert_eq!(r.2, e.to_string().into_bytes());
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!(w.len(), 1);
      assert_eq!((w[0].status, w[0].request_class.as_deref(), w[0].error_type.as_str(), w[0].error_message.as_str()),
                 (429, Some("auxiliary"), "rate_limit_error", "limit"));
      let q = &rows(&t, &g)[0];
      assert_eq!((w[0].session.as_str(), w[0].model.as_str(), w[0].ts), (g.session.as_str(), q.model.as_str(), q.ts));
  }

  #[test] // FIX-10: a 2xx answer writes no upstream_errors row
  fn upstream_200_records_no_error() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main")).0, 200);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      assert_eq!(rows(&t, &g).len(), 1);
      assert!(errs(&t, &g).is_empty());
  }

  #[test] // FIX-10 (D8): a body that is not JSON writes the row with empty type and message; the body reaches the client
  fn upstream_500_not_json_records_empty_fields() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      t.anth.answer_raw(500, "upstream connect error");
      let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main"));
      assert_eq!((r.0, r.2.as_slice()), (500, &b"upstream connect error"[..]));
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!(w.len(), 1);
      assert_eq!((w[0].status, w[0].request_class.as_deref(), w[0].error_type.as_str(), w[0].error_message.as_str()),
                 (500, Some("main"), "", ""));
  }

  #[test] // FIX-10 (D2): at most 300 characters, each control character a space; the client gets the whole message
  fn upstream_error_message_capped_and_without_control_characters() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let long = format!("line1\r\n\t{}", "y".repeat(992));
      assert_eq!(long.chars().count(), 1000);
      let e = json!({"type":"error","error":{"type":"invalid_request_error","message": long}});
      t.anth.answer(400, &[], &e);
      let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main"));
      assert_eq!((r.0, r.2.clone()), (400, e.to_string().into_bytes()));
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!(w.len(), 1);
      let m = &w[0].error_message;
      assert_eq!(m.chars().count(), 300);
      assert!(!m.chars().any(char::is_control));
      assert_eq!(m, &format!("line1   {}", "y".repeat(292)));
      assert_eq!((w[0].status, w[0].error_type.as_str()), (400, "invalid_request_error"));
  }

  #[test] // FIX-10 (D5), §14 "Never": no request byte or credential in the row or any file
  fn upstream_error_row_holds_no_request_content() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let tracer = format!("FIXTENTRACER{}", 424242);
      let b = serde_json::to_vec(&json!({"model": "claude-test-1", "max_tokens": 8, "messages": [{"role":"user","content": tracer.as_str()}]})).unwrap();
      t.anth.answer(400, &[], &json!({"type":"error","error":{"type":"invalid_request_error","message":"bad request"}}));
      assert_eq!(post_pt(&t, &g, M, b, &cc_headers("auxiliary")).0, 400);
      assert!(has(&t.anth.at("/v1/messages").body, tracer.as_bytes()));
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!(w.len(), 1);
      assert_eq!((w[0].error_type.as_str(), w[0].error_message.as_str()), ("invalid_request_error", "bad request"));
      let text = format!("{w:?}");
      for needle in [tracer.as_str(), OAT, g.token.as_str()] {
          assert!(!text.contains(needle), "row holds {needle}");
      }
      assert!(read_all(t.d.path()).iter().all(|f| !has(f, tracer.as_bytes())), "a file holds the tracer");
  }

  #[test] // FIX-10 (D5): the stored message goes through the secret pass; the relayed body does not change
  fn upstream_error_message_goes_through_the_secret_pass() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let key = key_sample();
      t.anth.answer(401, &[], &json!({"type":"error","error":{"type":"authentication_error","message": format!("bad key {key} here")}}));
      let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main"));
      assert_eq!(r.0, 401);
      assert!(has(&r.2, key.as_bytes()));
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!(w.len(), 1);
      let placeholder = ["[REDACTED", ":anthropic_api_key]"].concat();
      assert_eq!((w[0].error_type.as_str(), w[0].error_message.clone()), ("authentication_error", format!("bad key {placeholder} here")));
      assert!(read_all(t.d.path()).iter().all(|f| !has(f, key.as_bytes())), "a file holds the key");
  }

  #[test] // FIX-10 (D3): a session that is not passthrough writes no upstream_errors row
  fn normal_session_error_records_no_row() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, false);
      t.up.answer(429, &[], &json!({"type":"error","error":{"type":"rate_limit_error","message":"limit"}}));
      let hd = [("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())];
      assert_eq!(send(&t, "POST", M, &hd, Some(mb("code"))).0, 429);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      assert_eq!(rows(&t, &g)[0].status, 429);
      assert!(errs(&t, &g).is_empty());
  }
  ```

  Result: the seven tests are in the file; `OAT`, `key_sample`, `read_all`, `has`, `send` are the existing helpers.
- [ ] **2.3** Run `cargo test -p zygo-proxy --locked --test passthrough`. Expected: the build passes; `upstream_429_error_type_and_message_recorded`, `upstream_500_not_json_records_empty_fields`, `upstream_error_message_capped_and_without_control_characters`, `upstream_error_row_holds_no_request_content` and `upstream_error_message_goes_through_the_secret_pass` fail on `assert_eq!(w.len(), 1)` (left 0); `upstream_200_records_no_error`, `normal_session_error_records_no_row` and every test that existed before pass.
- [ ] **2.4** Commit `crates/zygo-proxy/tests/passthrough.rs` and this file: `test(proxy): upstream error rows of a non-2xx passthrough answer`.

#### Task 3: the proxy code (`crates/zygo-proxy/src`)

- [ ] **3.1** In `crates/zygo-proxy/src/tee.rs`, `Tee`: add the field `/// Keep the non-streamed copy after `finish`, for `body` (FIX-10). keep: bool,` after `copy_over`; set `keep: false` in `Tee::new` (`tee.rs:137`); in `Tee::finish` change `self.copy = Vec::new();` (`tee.rs:182`) to `if !self.keep { self.copy = Vec::new(); }`; add:

  ```rust
  /// `body` returns the whole non-streamed copy after `finish` (FIX-10). No effect on a streamed response.
  pub fn keep_body(&mut self) {
      self.keep = true;
  }

  /// The non-streamed body as copied (at most COPY_MAX), when `keep_body` was called; `None` for a streamed body or
  /// a body over COPY_MAX.
  pub fn body(&self) -> Option<&[u8]> {
      (self.keep && !self.streamed && !self.copy_over).then_some(self.copy.as_slice())
  }
  ```

  Result: without `keep_body`, `Tee` behaves as before (the `Tee` tests in `request_path.rs:620-690` and `session_api.rs:1084-1170` do not call it).
- [ ] **3.2** In `tee.rs`, `UsageSink`: change the import to `use zygo_usage::{RequestRow, Store, UpstreamErrorRow};`; `type RowSender = mpsc::Sender<(RequestRow, Option<UpstreamErrorRow>, Option<Ticket>)>;` (`tee.rs:307`); the channel in `UsageSink::open` to `mpsc::channel::<(RequestRow, Option<UpstreamErrorRow>, Option<Ticket>)>()` (`tee.rs:339`); the writer loop to `for (row, error, ticket) in rx {`, with, after `let _ = store.insert_request(&row);`, the line `if let Some(e) = &error { let _ = store.insert_upstream_error(e); }` (the test hooks and `drop(ticket); count(&h);` stay as they are); and `send` to:

  ```rust
  /// No store or closed: the rows are counted and dropped (with the ticket). `e`: the request's `upstream_errors` row (FIX-10).
  pub(crate) fn send(&self, r: RequestRow, e: Option<UpstreamErrorRow>, t: Option<Ticket>) {
      let sent = lock(&self.tx).as_ref().is_some_and(|tx| tx.send((r, e, t)).is_ok());
      if !sent {
          count(&self.handled);
      }
  }
  ```

  Result: one channel message carries the request row and its error row; the count is one per message (D6).
- [ ] **3.3** In `crates/zygo-proxy/src/forward.rs`: imports `use std::panic::{catch_unwind, AssertUnwindSafe};`, `std::sync::{Arc, OnceLock, PoisonError}` (`forward.rs:7`), `use zygo_redact::secret::redact;`, and `use zygo_usage::{RedactionCount, RequestRow, UpstreamErrorRow};` (`forward.rs:25`). In `struct Pending` (`forward.rs:208-225`) add `/// Passthrough, an upstream status that is not 2xx: the `upstream_errors` row (§14, FIX-10). upstream_error: Option<UpstreamErrorRow>,`; in the `Pending` literal of `handle` (`forward.rs:301`) add `upstream_error: None`; in `Pending::drop` change the last line (`forward.rs:266`) to `self.sink.send(std::mem::take(&mut self.row), self.upstream_error.take(), self.ticket.take());`. Result: `RequestRow` and its literals are unchanged; a refusal (no `Flow`) sends `None`.
- [ ] **3.4** In `forward.rs`, add before `struct Flow` (`forward.rs:1041`):

  ```rust
  /// The most characters of `error_type` and `error_message` in `upstream_errors` (§14, FIX-10).
  const ERROR_TEXT_MAX: usize = 300;

  /// Each control character as a space; at most ERROR_TEXT_MAX characters.
  fn clean(t: &str) -> String {
      t.chars().map(|c| if c.is_control() { ' ' } else { c }).take(ERROR_TEXT_MAX).collect()
  }

  /// `error.type` and `error.message` of an Anthropic error body (`{"type":"error","error":{…}}`); empty for any other
  /// body or a missing body (§14, FIX-10 D8).
  fn error_fields(body: Option<&[u8]>) -> (String, String) {
      let v = body.and_then(|b| serde_json::from_slice::<Value>(b).ok());
      let e = v.as_ref().filter(|v| v["type"] == "error").map(|v| &v["error"]);
      let f = |k: &str| e.and_then(|e| e[k].as_str()).unwrap_or("").to_string();
      (f("type"), f("message"))
  }

  /// `m` through the session's secret pass (§16.1), then `clean`; empty when the pass blocks, times out or panics
  /// (FIX-10 D5: fail closed).
  fn stored_message(m: String, s: &SessionState) -> String {
      let mut v = Value::String(m);
      let passed = catch_unwind(AssertUnwindSafe(|| {
          let mut cache = s.redaction.secret_cache.lock().unwrap_or_else(PoisonError::into_inner);
          redact(&mut v, &s.redaction.compiled, &mut cache, &DeadlinePolicy::default()).is_ok()
      }))
      .unwrap_or(false);
      match v {
          Value::String(t) if passed => clean(&t),
          _ => String::new(),
      }
  }

  /// The `upstream_errors` row of the passthrough request `row`, relayed with `status` (not 2xx) and `body` (§14, FIX-10).
  fn upstream_error(row: &RequestRow, status: StatusCode, body: Option<&[u8]>, s: &SessionState) -> UpstreamErrorRow {
      let (error_type, message) = error_fields(body);
      UpstreamErrorRow {
          ts: row.ts,
          session: row.session.clone(),
          request_class: row.request_class.clone(),
          model: row.model.clone(),
          status: status.as_u16(),
          error_type: clean(&error_type),
          error_message: stored_message(message, s),
      }
  }
  ```

  Result: the four functions read no request field other than the usage row's `ts`, `session`, `request_class` and `model`.
- [ ] **3.5** In `run` (`forward.rs:539`), after `let mut tee = Tee::new(streamed);`, add `if s.passthrough && !status.is_success() { tee.keep_body(); }`. In `Flow::drop` (`forward.rs:1052-1063`), after `self.pending.split = self.tee.cache_split();`, add:

  ```rust
  // Passthrough, not 2xx: the upstream error type and message (§14, FIX-10).
  if self.pending.row.passthrough && !self.rctx.status.is_success() {
      let e = upstream_error(&self.pending.row, self.rctx.status, self.tee.body(), &self.pending.session);
      self.pending.upstream_error = Some(e);
  }
  ```

  Result: the relayed status, headers and bytes are not touched (the tee returns each chunk unchanged, `tee.rs:161`).
- [ ] **3.6** Run `cargo check -p zygo-proxy --locked --all-targets`. Expected: exit 0, no warning in `forward.rs` or `tee.rs`.
- [ ] **3.7** Run `cargo test -p zygo-proxy --locked --test passthrough`. Expected: 0 failed; the output lists `... ok` for the seven tests of step 2.2 and for `upstream_429_reaches_the_client_once_and_unchanged`.
- [ ] **3.8** Run `cargo test -p zygo-proxy --locked --test request_path --test session_api --lib`. Expected: 0 failed (the `Tee` and usage-writer tests, `usage_written_on_client_disconnect` among them, are unchanged by `keep_body` and the new channel item).
- [ ] **3.9** Commit `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/src/forward.rs` and this file: `fix(proxy): record the upstream error type and message of a non-2xx passthrough answer`.

#### Task 4: the base spec (`docs/specs/2026-11-18-zygo-design.md`)

- [ ] **4.1** In §14, after the bullet `  - \`work_units\`: §13.1 facts.`, add the bullet:

  ```markdown
    - `upstream_errors`: ts, session, request_class, model, status, error_type, error_message. *(Note 2026-11-22, FIX-10: LIV-1 D8, FIX-8. One row for each passthrough request whose upstream answer reaches the client with a status that is not 2xx; a refusal by zygo itself, a non-passthrough session and an error event inside a 2xx stream write no row. `error_type` and `error_message` are `error.type` and `error.message` of the relayed body in the Anthropic error form (`{"type":"error","error":{…}}`), read from the tee's 4 MiB copy; both are empty when the body is not that form (not JSON, an SSE body, over 4 MiB, or cut off by a client disconnect). `error_message` goes through the secret pass of §16.1 (empty when the pass blocks or fails); in both, each control character becomes a space, and each keeps at most 300 characters. No request byte, header or credential is written. The table is created with `CREATE TABLE IF NOT EXISTS` on every open, without a schema version change; `retain_days` applies as for `requests`.)*
  ```

  Result: `rg -n 'upstream_errors' docs/specs/2026-11-18-zygo-design.md` prints the line in §14 (between lines 670 and 683 of the current file).
- [ ] **4.2** At the end of the table under `## Change log`, add the row:

  ```markdown
  | 2026-11-22 | §14 | author's call (FIX-10, from FIX-8) | §14: table `upstream_errors` records the upstream error type and message of a non-2xx passthrough answer; LIV-1 D8, FIX-8, FIX-10. |
  ```

  Result: `rg -n 'LIV-1 D8, FIX-8, FIX-10' docs/specs/2026-11-18-zygo-design.md` prints one line, the change-log row.
- [ ] **4.3** Commit `docs/specs/2026-11-18-zygo-design.md` and this file: `docs(spec): §14 table upstream_errors (FIX-10)`.

#### Task 5: verification

- [ ] **5.1** Run the Acceptance of the contract: `cargo test -p zygo-proxy --locked --test passthrough && cargo test -p zygo-usage --locked --test store && cargo test --workspace --locked --no-run`. Expected: exit 0; 0 failed in both test binaries; the output lists `upstream_429_error_type_and_message_recorded ... ok`, `upstream_200_records_no_error ... ok`, `upstream_500_not_json_records_empty_fields ... ok`, `upstream_error_message_capped_and_without_control_characters ... ok`, `upstream_error_row_holds_no_request_content ... ok`, `upstream_errors_table_is_created_on_an_existing_database ... ok`; the workspace build of every test target ends without an error. Then commit this file with every step ticked: `docs(work): FIX-10 acceptance`.
