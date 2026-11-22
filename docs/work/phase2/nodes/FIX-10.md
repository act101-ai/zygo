# FIX-10 — Record the upstream error type and message of a non-2xx passthrough answer (FIX-8 follow-up)

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §14 (usage store, metadata only: the `requests` columns, "Never: bodies, prompts, tool inputs …"), §12.4 (passthrough), §7.3 (a 5xx or 429 is kept as the last answer and relayed); found by FIX-8 (`docs/work/phase2/nodes/FIX-8.md` "Root cause", verdict `not determined`: in 24 of 24 passthrough sessions the first `auxiliary` request is answered 429, and no record holds the upstream error text, because the usage row stores the status only, `crates/zygo-usage/src/schema.rs`). Code: a 5xx or 429 is kept as `last` (`crates/zygo-proxy/src/forward.rs:446`) and relayed to the client (`:465-469`); a 4xx other than 429 is the served answer (`:447-450`) and is relayed the same way; the response tee copies a non-streamed body up to 4 MiB for parsing (§14; `crates/zygo-proxy/src/tee.rs:152-161`) and clears the copy in `Tee::finish` (`tee.rs:182`); the one thread that owns the proxy's `Store` is the usage writer of `UsageSink` (`tee.rs:325-366`), whose channel carries only `RequestRow` (`tee.rs:307`). Change: for a passthrough request whose upstream answer is not 2xx, zygo parses the relayed body as the Anthropic error form `{"type":"error","error":{"type":…,"message":…}}` and writes one row to a new usage table `upstream_errors` (`ts`, `session`, `request_class`, `model`, `status`, `error_type`, `error_message`), created with `CREATE TABLE IF NOT EXISTS` so an existing `usage.db` needs no migration and `RequestRow` and its literals do not change. `error_message` keeps at most 300 characters, with control characters replaced by a space; no request byte, header or credential is written. A body that does not parse writes the row with `error_type` and `error_message` empty. The relayed answer to the client does not change (status, headers, body). Retention as `requests` (`[usage] retain_days`). The node adds a dated note to §14 of the base spec for the new table and one base-spec change-log line: "§14: table `upstream_errors` records the upstream error type and message of a non-2xx passthrough answer; LIV-1 D8, FIX-8, FIX-10."
**Files:** `crates/zygo-usage/src/schema.rs`, `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/src/redact_stage.rs`, `crates/zygo-proxy/src/server.rs`, `crates/zygo-proxy/tests/passthrough.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** `schema::UpstreamErrorRow { ts: i64, session: String, request_class: Option<String>, model: String, status: u16, error_type: String, error_message: String }`, `store::Store::insert_upstream_error(&self, row: &UpstreamErrorRow) -> anyhow::Result<()>`, `store::Store::insert_request_and_error(&self, r: &RequestRow, e: &UpstreamErrorRow) -> anyhow::Result<()>`, `store::Store::upstream_errors(&self, session: &str) -> anyhow::Result<Vec<UpstreamErrorRow>>`
**Acceptance:** `cargo test -p zygo-proxy --locked --test passthrough && cargo test -p zygo-usage --locked --test store && cargo test --workspace --locked --no-run` → 0 failed, including, with the mock upstream of `passthrough.rs` (`Upstream::answer`): an `auxiliary` request answered 429 with `{"type":"error","error":{"type":"rate_limit_error","message":"limit"}}` gives one `upstream_errors` row with status 429, class `auxiliary`, `rate_limit_error` and `limit`, and the client still gets the same status, `retry-after` and body; a 200 answer gives no row; a 500 with a body that is not JSON gives one row with empty type and message; a message of 1000 characters with a newline is stored as 300 characters with no control character; a request whose body holds a tracer word writes no row field that holds it; and `store.rs` creates the table on an existing database that has only the old tables

## Plan

### Contract findings (fixed in the contract above, 2026-11-22)

- F1, Files: `crates/zygo-proxy/src/tee.rs` was missing. The proxy writes usage rows only through the writer thread of `UsageSink::open` (`crates/zygo-proxy/src/tee.rs:325-366`), which owns the only `Store`; its channel type `RowSender` (`tee.rs:307`) carries `(RequestRow, Option<Ticket>)` only, and `UsageSink::send` (`tee.rs:375-380`) is the one entry. The relayed body is held only in `Tee::copy` (`tee.rs:53`, filled at `tee.rs:152-161`), and `Tee::finish` clears it (`tee.rs:182`) before `Flow::drop` (`crates/zygo-proxy/src/forward.rs:1052-1063`) can read it. The error row cannot be written without a change to `tee.rs`. Files now names it.
- F2, Spec line: the code cites only the 5xx/429 path (`forward.rs:446`). A 4xx other than 429 (400, 401, 403, 404, 413) is the served answer (`forward.rs:447-450`) and reaches the client by the same `Flow` (`forward.rs:567-571`). "Not 2xx" in the contract covers both; the Spec line now names `:447-450`.
- F3 (plan review round 1): Files gain `crates/zygo-proxy/src/redact_stage.rs` (the shared secret-pass helper, B1) and `crates/zygo-proxy/src/server.rs` (the writer gets the configured secret deadline, B1); Produces gains `Store::insert_request_and_error` (one transaction, M3). `crates/zygo-proxy/src/translate/mod.rs` is only read (`error_object`, `error_message`, `scrub` are reused as they are), so it is not in Files.
- No other drift: `forward.rs:446` and `:465-469` are as cited; `SCHEMA` (`crates/zygo-usage/src/schema.rs:5-67`) runs on every `Store::open` while `user_version` ≤ 1 (`crates/zygo-usage/src/store.rs:82-90`), so `CREATE TABLE IF NOT EXISTS` reaches an existing `usage.db` without a version change, as `sync_pending` does (`schema.rs:66`, test `sync_pending_table_is_created_on_every_open`, `crates/zygo-usage/tests/store.rs:398`).

### Plan review round 1 (2026-11-22): how each finding is handled

- B1: `Flow::drop` does no parse and no pass; it copies at most 16 KiB of the kept body (D7). The usage writer (a std thread) parses with `translate::error_object` / `translate::error_message`, cuts each field to 2 KiB, scrubs the login, runs the secret pass through the shared `redact_stage::secret_pass` (factored out of `forward::row_name`, which now calls it) at the configured `RedactLimits.secret`, fails closed, then `clean` (D2, D5, D6, D8). Test `upstream_large_error_body_does_not_wait_for_the_writer` (step 2.3).
- B2: `error_type` takes the same path as `error_message` (`error_field`, step 3.3). Test `upstream_error_type_goes_through_the_secret_pass`.
- M2: the writer parses again; reason in D8.
- M3: `Store::insert_request_and_error`, one `BEGIN IMMEDIATE` transaction; test `insert_request_and_error_is_one_transaction` forces both failures (a non-finite `estimated_usd`, `store.rs:18-20`; a dropped table) (D6, steps 1.2, 1.5).
- M4: the §14 "Never" line is rewritten as final text with the one exception (step 4.2).
- M5: tests `upstream_error_message_with_a_block_match_is_stored_empty`, `upstream_error_secret_pass_runs_with_secrets_off`, `upstream_error_with_an_sse_body_records_empty_fields`, `client_disconnect_before_the_body_ends_records_empty_fields` (step 2.3).
- M6: the review says the provider-key scrub covers it. Against the code it does not: on a passthrough request no provider key is read (§12.4 stage 5), and an exact scrub (`translate::scrub`, `translate/mod.rs:108-113`) does not match a part of a value. The writer scrubs the client's login (the `Authorization` token, held in memory only): the whole value with `translate::scrub`, then each run of 8 or more of its characters (`scrub_partial`, D5). Test `upstream_echo_of_a_partial_login_is_scrubbed`.
- M7: note in D2.
- M8: `run` is cited at `forward.rs:318` (step 3.6).
- M9: `replace_requests` deletes the session's `upstream_errors` rows; test `replace_requests_deletes_the_sessions_upstream_errors` (D9, steps 1.2, 1.5).

### Builds on (signatures in the code now)

- `zygo_usage::Store::open(path: &Path) -> anyhow::Result<Store>` (`crates/zygo-usage/src/store.rs:66`); `Store::prune(&self, retain_days: u32) -> anyhow::Result<()>` (`store.rs:237`); `Store::replace_requests(&self, session: &str, rows: &[RequestRow]) -> anyhow::Result<usize>` (`store.rs:181`); `Store::insert_request(&self, r: &RequestRow) -> anyhow::Result<()>` (`store.rs:133`); `fn bind_request(stmt: &mut rusqlite::Statement<'_>, r: &RequestRow) -> anyhow::Result<()>` (`store.rs:17`); `Store::requests_for_session(&self, id: &str) -> anyhow::Result<Vec<RequestRow>>` (`crates/zygo-usage/src/query.rs:62`).
- `zygo_proxy::tee::Tee::new(streamed: bool) -> Tee` (`tee.rs:136`); `Tee::finish(&mut self, _on_line: &mut dyn FnMut(&mut SseLine)) -> Bytes` (`tee.rs:173`); `UsageSink::open(db: Option<&Path>, hooks: WriterHooks) -> (UsageSink, Option<String>)` (`tee.rs:325`, `pub(crate)`); `UsageSink::send(&self, r: RequestRow, t: Option<Ticket>)` (`tee.rs:375`, `pub(crate)`); `UsageHold::new() -> Arc<UsageHold>`, `UsageHold::release(&self)` (`tee.rs:285-291`, test hooks).
- `translate::error_object(v: &Value) -> &Value`, `translate::error_message(v: &Value) -> Option<&str>`, `translate::scrub(m: &str, key: Option<&str>) -> String` (`crates/zygo-proxy/src/translate/mod.rs:95-113`, `pub(crate)`).
- `forward::row_name(s: &SessionState, name: &str) -> Option<String>` (`forward.rs:737-747`); `zygo_redact::secret::redact(body: &mut Value, c: &Compiled, cache: &mut SessionCache, policy: &DeadlinePolicy) -> Result<Findings, RedactError>` (`crates/zygo-redact/src/secret.rs:74`); `SessionState::redaction: Redaction { compiled: Compiled, secret_cache: Mutex<SessionCache>, content_cache: Mutex<SessionCache> }` (`crates/zygo-proxy/src/session.rs:210-214`, `:242`); `RedactLimits { secret: DeadlinePolicy, … }` (`crates/zygo-proxy/src/redact_stage.rs:19-28`); `StartOpts::redact_limits: Option<RedactLimits>` and `StartOpts::usage_hold: Option<Arc<UsageHold>>` (`crates/zygo-proxy/src/server.rs:173`, `:197`, test hooks); `zygo_config::keys::Secret::new(v: impl Into<String>) -> Secret`, `Secret::expose(&self) -> &str` (`crates/zygo-config/src/keys.rs:22-25`).
- Test harness: `ProxyHandle::usage_written(&self, n: u64, limit: Duration) -> bool` (`server.rs:317`); `Upstream` and `Upstream::answer(&self, status: u16, headers: &[(&str, &str)], body: &Value)` (`crates/zygo-proxy/tests/passthrough.rs:45-100`); `setup`, `setup_with`, `grant`, `post_pt`, `cc_headers`, `rows`, `hv`, `has`, `read_all`, `key_sample`, `mb`, `send` (`passthrough.rs:149-255`, `:402`); `OAT` = `"sk-ant-oat-x"`, `BEARER` (`passthrough.rs:35-36`).

### Plan produces

- `zygo_usage::schema::UpstreamErrorRow { ts: i64, session: String, request_class: Option<String>, model: String, status: u16, error_type: String, error_message: String }` (re-exported as `zygo_usage::UpstreamErrorRow` by `pub use schema::*`, `crates/zygo-usage/src/lib.rs`).
- `zygo_usage::store::Store::insert_upstream_error(&self, row: &UpstreamErrorRow) -> anyhow::Result<()>`.
- `zygo_usage::store::Store::insert_request_and_error(&self, r: &RequestRow, e: &UpstreamErrorRow) -> anyhow::Result<()>`.
- `zygo_usage::store::Store::upstream_errors(&self, session: &str) -> anyhow::Result<Vec<UpstreamErrorRow>>`.
- Internal: `redact_stage::secret_pass(s: &SessionState, text: &str, policy: &DeadlinePolicy) -> Option<String>`; `tee::ErrorCapture { head: Vec<u8>, credential: Option<Secret>, session: Arc<SessionState> }`; `tee::ERROR_BODY_MAX: usize`; `Tee::keep_body(&mut self)`; `Tee::body(&self) -> Option<&[u8]>`; changed: `UsageSink::open(db: Option<&Path>, hooks: WriterHooks, secret: DeadlinePolicy) -> (UsageSink, Option<String>)`, `UsageSink::send(&self, r: RequestRow, e: Option<ErrorCapture>, t: Option<Ticket>)`.

### Design decisions (author's calls; the executor applies them as written)

- D1, table and migration: one new table `upstream_errors (ts INTEGER NOT NULL, session TEXT NOT NULL, request_class TEXT, model TEXT NOT NULL, status INTEGER NOT NULL, error_type TEXT NOT NULL, error_message TEXT NOT NULL)` with indexes `upstream_errors_session_ts (session, ts)` and `upstream_errors_ts (ts)`, appended to `SCHEMA`. No `SCHEMA_VERSION` change and no migration step. Reason: `Store::open` runs `SCHEMA` on every open (`store.rs:86`), as for `sync_pending`; `open_read_only` (the status line, `store.rs:111-124`) checks `user_version == 1` and keeps working; `RequestRow` and its literals do not change (contract). The indexes match `requests` (`schema.rs:61-62`): the read is by session, `prune` is by `ts`.
- D2, size caps (three): (a) the response path copies at most `ERROR_BODY_MAX` = 16 KiB of the kept body; a body over 16 KiB, or over the 4 MiB tee copy (`tee.rs:16`, `:154-156`), does not parse and gives empty fields (Anthropic error bodies are far under 1 KiB). (b) The writer cuts each field to `ERROR_FIELD_IN_MAX` = 2 KiB (at a character boundary) before any scrub or pass. (c) After the pass, `clean` makes each control character a space and keeps at most 300 characters. Why (b) bounds the pass: the secret pass checks its deadline only before a second uncached string (`secret.rs:121-122`: `scans > 0 && …`), so the one string of a call is always scanned whole and no deadline can stop it; the input cap is the bound: one scan of the compiled pattern set over at most 2 KiB. Why (b) keeps the stored text whole: 300 characters are at most 1200 bytes, so the pass input reaches at least 848 bytes past the last stored character, more than the span of any secret pattern; a secret that reaches the stored text is whole in the pass input. M7: the pass records each scanned string in the session's shared secret cache (`secret.rs:127-129`); the 2 KiB cap also bounds how much one upstream error can add to that cache.
- D3, statuses: a row for every passthrough request whose relayed upstream status is not 2xx (`!StatusCode::is_success()`): the 5xx and 429 `last` answer and every other 4xx served answer. No row for a 2xx, for a zygo refusal before or without an upstream answer (`ZygoError`, for example `401 zygo_passthrough_token_invalid`, `422 zygo_redaction_block`, `503 zygo_provider_unavailable`), or for a non-passthrough session. Reason: FIX-8 needs the vendor's text for any vendor refusal, and a 4xx such as 400 or 403 is the likely form of a client-identity refusal (FIX-8 "What a next node needs"); a zygo refusal already has its code in the response and its status in `requests`.
- D4, streaming error events: not counted. An `event: error` inside a 2xx SSE stream writes no row, and a non-2xx answer with `content-type: text/event-stream` writes a row with empty type and message (the tee keeps no copy of a streamed body, `tee.rs:152-170`). Reason: the contract is about a non-2xx status; the FIX-8 evidence is a 429 status with a JSON body; reading SSE lines would add a parse in the line path (`tee.rs:201-244`) for a case that no record shows.
- D5, redaction: no request byte or header is read for the row except the client's login, which is held in memory only to remove it; `ts`, `session`, `request_class`, `model` and `status` come from the usage row that is written anyway (`forward.rs:283-299`, `:372`, `:549`), the two text fields from the upstream body only. Each text field, after the 2 KiB cut: (1) the client's login (the `Authorization` value after its scheme, from `ResponseCtx::headers`) is removed whole with `translate::scrub`; on a passthrough request no provider key is read (§12.4 stage 5), so the login is the one credential that went upstream; (2) each run of 8 or more characters that is also a part of the login is removed (`scrub_partial`, M6); (3) the secret pass runs through `redact_stage::secret_pass` at the configured `RedactLimits.secret` (`StartOpts::redact_limits`, else the spec default), always, also when `[features] redaction.secrets` is off; a `block` match, a timeout or a panic gives an empty field (fail closed, as stage 6 does for a body, `redact_stage.rs:133-144`). Reason: §14 "Never: … credentials"; an upstream message can echo the login, a key or a part of the request. The content pass (§16.2) is not used: it works on the `messages` layout of a request, not on one string.
- D6, write path: the channel message is `(RequestRow, Option<ErrorCapture>, Option<Ticket>)`. The writer thread builds the `UpstreamErrorRow` from the capture (parse, D2, D5, `clean`), then writes the request row and the error row with `Store::insert_request_and_error`, one `BEGIN IMMEDIATE` transaction (M3): a failed request insert leaves no error row, and a failed error insert leaves no request row. Without a capture it calls `insert_request` as before. Then it drops the ticket and counts one. Reason: one store owner (`Store` is not `Sync`, `tee.rs:309`); the parse and the pass run on a std thread, never on a tokio worker (B1); `usage_written(n)` and the rows-reply wait (`UsageSink::flush`) stay exact, because both rows are in before the count.
- D7, response path (B1): `run` calls `Tee::keep_body` for a passthrough non-2xx answer; `Tee::finish` then keeps the copy; `Flow::drop` (after the tee is finished, `forward.rs:1055-1059`) copies at most 16 KiB of `Tee::body` as raw bytes, takes the login from `self.rctx.headers` into a `Secret`, and puts an `ErrorCapture` in `Pending::upstream_error`; `Pending::drop` sends it. No parse, no lock and no pass on the response path. A client that leaves before the body ends gives a partial copy, which does not parse: empty fields, status recorded.
- D8, parse (M2): the writer parses the 16 KiB head again, with `serde_json::from_slice`, then `error_type` = `translate::error_object(v)["type"]` and `error_message` = `translate::error_message(v)` when each is a string, else empty. Any other body gives both empty. Why not the parse of `Tee::finish` (`tee.rs:175-181`): it parses the whole copy (up to 4 MiB) for `usage` and drops the value at once; keeping that value until `Flow::drop` would hold the parsed tree of a large body, and taking fields from it there is the work B1 moves off the response path. A parse of at most 16 KiB on the writer is cheap.
- D9, retention and sync: `Store::prune` deletes `upstream_errors` rows with `ts` older than `retain_days`, in the same transaction as `requests`. `Store::replace_requests` (`zygo usage --sync`) deletes the session's `upstream_errors` rows in its transaction (M9). No read surface (`zygo usage`, status line) changes in this node; the rows are read with `Store::upstream_errors` or `sqlite3`.
- D10, placeholder text in tests: the tests build the redaction placeholder from two parts (`["[REDACTED", ":anthropic_api_key]"].concat()`, and `["[REDACTED", "]"].concat()` for the scrub marker), and `scrub_partial` takes its marker from `translate::scrub` (`scrub(cred, Some(cred))`). Reason: the session's edit hook refuses a file write that holds the placeholder text as one literal.

### Steps

Work in the node's checkout. Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-10`. In each commit, tick (`- [x]`) the steps of this file that are done. No step sends a request to a real vendor.

#### Task 1: the usage table (`crates/zygo-usage`)

- [x] **1.1** In `crates/zygo-usage/tests/store.rs`, change the expected list of `schema_creates_three_tables_in_wal_mode` to `["requests", "sync_pending", "trims", "upstream_errors", "work_units"]`, and add to `never_list_columns_are_exact`:

  ```rust
  assert_eq!(cols(&t.db(), "upstream_errors"), ["ts","session","request_class","model","status","error_type","error_message"]);
  ```

  Result: the two tests name the new table and its exact columns.
- [x] **1.2** In the same file, add the helpers and five tests:

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

  #[test] // FIX-10 M3: the request row and its error row, both or neither
  fn insert_request_and_error_is_one_transaction() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      s.insert_request_and_error(&req("S1", 1), &uerr("S1", 1)).unwrap();
      assert_eq!((s.requests_for_session("S1").unwrap().len(), s.upstream_errors("S1").unwrap().len()), (1, 1));
      let bad = RequestRow { estimated_usd: f64::NAN, ..req("S2", 1) };
      assert!(s.insert_request_and_error(&bad, &uerr("S2", 1)).is_err());
      assert!(s.upstream_errors("S2").unwrap().is_empty());
      Connection::open(t.db()).unwrap().execute_batch("DROP TABLE upstream_errors").unwrap();
      assert!(s.insert_request_and_error(&req("S3", 1), &uerr("S3", 1)).is_err());
      assert!(s.requests_for_session("S3").unwrap().is_empty());
  } // mutations: two separate inserts; commit before the error insert

  #[test] // FIX-10 M9: `zygo usage --sync` replaces the session's rows, its upstream_errors rows included
  fn replace_requests_deletes_the_sessions_upstream_errors() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      s.insert_upstream_error(&uerr("S1", 1)).unwrap();
      s.insert_upstream_error(&uerr("S2", 1)).unwrap();
      assert_eq!(s.replace_requests("S1", &[req("S1", 2)]).unwrap(), 1);
      assert!(s.upstream_errors("S1").unwrap().is_empty());
      assert_eq!(s.upstream_errors("S2").unwrap(), vec![uerr("S2", 1)]);
  }
  ```

  Result: the file holds the five tests; `req`, `count`, `survivors`, `now`, `DAY` and `Tmp` are the existing helpers of the file.
- [x] **1.3** Run `cargo test -p zygo-usage --locked --test store`. Expected: the build fails with errors that name `UpstreamErrorRow`, `insert_upstream_error`, `insert_request_and_error` and `upstream_errors` (not found). No other error.
- [x] **1.4** In `crates/zygo-usage/src/schema.rs`, append to `SCHEMA`, after the `sync_pending` line (`schema.rs:66`):

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
  /// byte: `error_type` and `error_message` come from the upstream body only, scrubbed and capped.
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
- [x] **1.5** In `crates/zygo-usage/src/store.rs`, add after `bind_request` (`store.rs:17-51`):

  ```rust
  /// One `upstream_errors` row on `conn` (a connection or an open transaction).
  fn insert_error(conn: &Connection, e: &UpstreamErrorRow) -> Result<()> {
      conn.prepare_cached(
          "INSERT INTO upstream_errors (ts, session, request_class, model, status, error_type, error_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      )?.execute(rusqlite::params![e.ts, e.session, e.request_class, e.model, e.status, e.error_type, e.error_message])?;
      Ok(())
  }
  ```

  add to `impl Store`:

  ```rust
  /// One `upstream_errors` row (§14, FIX-10).
  pub fn insert_upstream_error(&self, row: &UpstreamErrorRow) -> Result<()> {
      insert_error(&self.conn, row)
  }

  /// The request row and its `upstream_errors` row in one `BEGIN IMMEDIATE` transaction: both or neither (FIX-10 M3).
  pub fn insert_request_and_error(&self, r: &RequestRow, e: &UpstreamErrorRow) -> Result<()> {
      let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
      bind_request(&mut *tx.prepare_cached(INSERT_REQUEST)?, r)?;
      insert_error(&tx, e)?;
      tx.commit()?;
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

  in `prune` (`store.rs:237-248`), after the `DELETE FROM requests` line: `tx.execute("DELETE FROM upstream_errors WHERE ts < ?1", [cutoff])?;`; in `replace_requests` (`store.rs:186`), after the `DELETE FROM requests` line: `tx.execute("DELETE FROM upstream_errors WHERE session = ?1", [session])?;`. Result: the store methods of `Plan produces` exist with the exact signatures; a stored status out of `u16` range is a read error (rusqlite `FromSql for u16`); a dropped transaction rolls back (rusqlite `Transaction` drop).
- [x] **1.6** Run `cargo test -p zygo-usage --locked --test store`. Expected: 0 failed; the output lists `... ok` for `upstream_error_round_trip_by_session`, `upstream_errors_table_is_created_on_an_existing_database`, `prune_deletes_upstream_errors_older_than_retain_days`, `insert_request_and_error_is_one_transaction`, `replace_requests_deletes_the_sessions_upstream_errors`, `schema_creates_three_tables_in_wal_mode`, `never_list_columns_are_exact`, `replace_requests_swaps_a_session_in_one_transaction`.
- [x] **1.7** Commit `crates/zygo-usage/src/schema.rs`, `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs` and this file: `feat(usage): table upstream_errors for the upstream error of a passthrough answer`.

#### Task 2: the proxy tests (`crates/zygo-proxy/tests/passthrough.rs`)

- [x] **2.1** Harness changes in `passthrough.rs`:
  - Imports: `use zygo_usage::{RedactionCount, RequestRow, Store, UpstreamErrorRow};` in place of the `zygo_usage` line; add `use futures_util::StreamExt;` and `use zygo_proxy::tee::UsageHold;`.
  - `struct Upstream`: add the fields `ctype: Arc<Mutex<String>>` (`"application/json"` at start) and `hang: Arc<Mutex<bool>>` (false at start). In `start`, create them, clone them into the reply closure next to `r2, s2, h2`, and build the reply as:

    ```rust
    let ct = c3.lock().unwrap().clone();
    let mut b = Response::builder().status(*s3.lock().unwrap()).header("content-type", ct);
    for (k, v) in h3.lock().unwrap().iter() {
        b = b.header(k.as_str(), v.as_str());
    }
    let body = r3.lock().unwrap().clone();
    let body = if *k3.lock().unwrap() {
        let first = futures_util::stream::once(async move { Ok::<Bytes, std::io::Error>(Bytes::from(body)) });
        axum::body::Body::from_stream(first.chain(futures_util::stream::pending()))
    } else {
        axum::body::Body::from(body)
    };
    b.body(body).unwrap()
    ```

    (`c3`, `k3` are the clones of `ctype`, `hang`), and return `Upstream { addr, got, reply, status, headers, ctype, hang, _stop: tx }`.
  - Add to `impl Upstream`:

    ```rust
    /// Every later request is answered with `status`, no extra header, content type `ctype` and the raw `body`.
    fn answer_raw(&self, status: u16, ctype: &str, body: &str) {
        *self.status.lock().unwrap() = status;
        self.headers.lock().unwrap().clear();
        *self.ctype.lock().unwrap() = ctype.to_string();
        *self.reply.lock().unwrap() = body.to_string();
    }
    /// Every later request is answered with `status` and the JSON content type; the body is `head`, then the stream
    /// stays open (no end).
    fn answer_hang(&self, status: u16, head: &str) {
        self.answer_raw(status, "application/json", head);
        *self.hang.lock().unwrap() = true;
    }
    ```

  - Move the body of `setup_with` into a new `fn setup_full(extra: &str, pipeline: Option<Pipeline>, hold: Option<Arc<UsageHold>>) -> T` (doc: "`setup_with`, and `hold` as the usage writer's hold: no row is written until `release`."), with `usage_hold: hold,` added to its `StartOpts` literal; `setup_with` becomes `{ setup_full(extra, pipeline, None) }`.
  - After `rows`, add:

    ```rust
    fn errs(t: &T, g: &SessionGrant) -> Vec<UpstreamErrorRow> {
        Store::open(&t.d.path().join("data/usage.db")).unwrap().upstream_errors(&g.session).unwrap()
    }
    ```

  Result: the harness answers any content type, can leave a body open, can hold the usage writer, and reads `upstream_errors` rows; every existing test still compiles unchanged.
- [x] **2.2** Add, after `upstream_429_reaches_the_client_once_and_unchanged`, these seven tests (the contract cases):

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
      t.anth.answer_raw(500, "text/plain", "upstream connect error");
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

  Result: the seven tests are in the file.
- [x] **2.3** Add after them these seven tests (plan review round 1):

  ```rust
  #[test] // FIX-10 B1: a 4 MiB error body reaches the client whole while the usage writer is held; the row follows
  fn upstream_large_error_body_does_not_wait_for_the_writer() {
      let hold = UsageHold::new();
      let t = setup_full("", None, Some(hold.clone()));
      let g = grant(&t, Agent::ClaudeCode, true);
      let e = json!({"type":"error","error":{"type":"invalid_request_error","message": "x".repeat(4_000_000)}});
      t.anth.answer(400, &[], &e);
      let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main"));
      assert_eq!((r.0, r.2.len()), (400, e.to_string().len()));
      assert!(!t.h.usage_written(1, Duration::from_millis(200)), "the writer ran while held");
      hold.release();
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!(w.len(), 1);
      assert_eq!((w[0].status, w[0].error_type.as_str(), w[0].error_message.as_str()), (400, "", "")); // a 16 KiB head does not parse
  }

  #[test] // FIX-10 B2: error.type gets the scrub, the secret pass and the cap too
  fn upstream_error_type_goes_through_the_secret_pass() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let key = key_sample();
      t.anth.answer(400, &[], &json!({"type":"error","error":{"type": format!("bad {key}"), "message":"m"}}));
      assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main")).0, 400);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      let placeholder = ["[REDACTED", ":anthropic_api_key]"].concat();
      assert_eq!((w.len(), w[0].error_type.clone(), w[0].error_message.as_str()), (1, format!("bad {placeholder}"), "m"));
      assert!(read_all(t.d.path()).iter().all(|f| !has(f, key.as_bytes())), "a file holds the key");
  }

  #[test] // FIX-10 M5: a message that matches a `block` pattern is stored empty (fail closed)
  fn upstream_error_message_with_a_block_match_is_stored_empty() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let blocked = format!("TRACERBLOCK_{}", 123456);
      t.anth.answer(400, &[], &json!({"type":"error","error":{"type":"invalid_request_error","message": format!("saw {blocked}")}}));
      assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main")).0, 400);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!((w.len(), w[0].error_type.as_str(), w[0].error_message.as_str()), (1, "invalid_request_error", ""));
      assert!(read_all(t.d.path()).iter().all(|f| !has(f, blocked.as_bytes())), "a file holds the blocked word");
  }

  #[test] // FIX-10 M5: with `redaction.secrets` off the pass still runs on the stored fields
  fn upstream_error_secret_pass_runs_with_secrets_off() {
      let t = setup("\n[features.redaction]\nsecrets = false\n");
      let g = grant(&t, Agent::ClaudeCode, true);
      let key = key_sample();
      t.anth.answer(401, &[], &json!({"type":"error","error":{"type":"authentication_error","message": format!("bad key {key} here")}}));
      assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main")).0, 401);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      let placeholder = ["[REDACTED", ":anthropic_api_key]"].concat();
      assert_eq!((w.len(), w[0].error_message.clone()), (1, format!("bad key {placeholder} here")));
      assert!(read_all(t.d.path()).iter().all(|f| !has(f, key.as_bytes())), "a file holds the key");
  }

  #[test] // FIX-10 M5 (D4): a non-2xx answer with an SSE body gives empty fields; the body reaches the client unchanged
  fn upstream_error_with_an_sse_body_records_empty_fields() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let sse = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"boom\"}}\n\n";
      t.anth.answer_raw(500, "text/event-stream", sse);
      let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main"));
      assert_eq!((r.0, r.2.as_slice()), (500, sse.as_bytes()));
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!((w.len(), w[0].status, w[0].error_type.as_str(), w[0].error_message.as_str()), (1, 500, "", ""));
  }

  #[test] // FIX-10 M5 (D7): a client that leaves before the body ends gives a row with empty fields
  fn client_disconnect_before_the_body_ends_records_empty_fields() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      t.anth.answer_hang(500, r#"{"type":"error","error":{"type":"api_error""#);
      let _ = rustls::crypto::ring::default_provider().install_default();
      let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
      rt.block_on(async {
          let c = reqwest::Client::builder().no_proxy().build().unwrap();
          let mut r = c.post(t.h.url().join(M).unwrap())
              .header("x-zygo-token", g.token.as_str()).header("x-zygo-session", g.session.as_str())
              .header("authorization", BEARER).header("content-type", "application/json")
              .body(mb("claude-test-1")).send().await.unwrap();
          assert_eq!(r.status().as_u16(), 500);
          assert!(r.chunk().await.unwrap().is_some());
          drop(r);
      });
      drop(rt);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      assert_eq!((w.len(), w[0].status, w[0].error_type.as_str(), w[0].error_message.as_str()), (1, 500, "", ""));
  }

  #[test] // FIX-10 M6 (D5): an upstream echo of the login, whole or a part of it, is never stored
  fn upstream_echo_of_a_partial_login_is_scrubbed() {
      let t = setup("");
      let g = grant(&t, Agent::ClaudeCode, true);
      let part = &OAT[3..]; // 9 characters of the login
      t.anth.answer(401, &[], &json!({"type":"error","error":{"type":"authentication_error","message": format!("token {OAT} or {part} rejected")}}));
      assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("main")).0, 401);
      assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
      let w = errs(&t, &g);
      let mark = ["[REDACTED", "]"].concat();
      assert_eq!((w.len(), w[0].error_message.clone()), (1, format!("token {mark} or {mark} rejected")));
      assert!(!format!("{w:?}").contains(part));
      assert!(read_all(t.d.path()).iter().all(|f| !has(f, part.as_bytes())), "a file holds a part of the login");
  }
  ```

  Result: the seven tests are in the file.
- [x] **2.4** Run `cargo test -p zygo-proxy --locked --test passthrough`. Expected: the build passes; `upstream_200_records_no_error`, `normal_session_error_records_no_row` and every test that existed before pass; the other twelve new tests fail on their first row-count assertion (left 0). `upstream_large_error_body_does_not_wait_for_the_writer` fails only after `hold.release()`.
- [x] **2.5** Commit `crates/zygo-proxy/tests/passthrough.rs` and this file: `test(proxy): upstream error rows of a non-2xx passthrough answer`.

#### Task 3: the proxy code (`crates/zygo-proxy/src`)

- [x] **3.1** In `crates/zygo-proxy/src/redact_stage.rs`: change the import to `use zygo_redact::secret::{redact, redact_after_wait, RedactError};`, add `use serde_json::Value;` and `use crate::session::SessionState;`, and add:

  ```rust
  /// `text` through the session's secret pass under `policy`; `None` on a `block` match, a timeout or a panic. Shared by
  /// the usage row's model name (`forward::row_name`) and the `upstream_errors` fields (§14, FIX-10).
  pub(crate) fn secret_pass(s: &SessionState, text: &str, policy: &DeadlinePolicy) -> Option<String> {
      let mut v = Value::String(text.to_string());
      let r = catch_unwind(AssertUnwindSafe(|| {
          let mut cache = lock(&s.redaction.secret_cache);
          redact(&mut v, &s.redaction.compiled, &mut cache, policy)
      }));
      match (r, v) {
          (Ok(Ok(_)), Value::String(t)) => Some(t),
          _ => None,
      }
  }
  ```

  In `crates/zygo-proxy/src/forward.rs`, replace the body of `row_name` (`forward.rs:737-747`) with `crate::redact_stage::secret_pass(s, name, &DeadlinePolicy::default()).map(|n| strip_1m(&n))`. Result: `row_name` keeps its behavior (the same `Ok(Ok(_))` rule, the same deadline); the pass code exists once.
- [x] **3.2** In `crates/zygo-proxy/src/tee.rs`, `Tee`: add the field `/// Keep the non-streamed copy after `finish`, for `body` (FIX-10). keep: bool,` after `copy_over`; set `keep: false` in `Tee::new` (`tee.rs:137`); in `Tee::finish` change `self.copy = Vec::new();` (`tee.rs:182`) to `if !self.keep { self.copy = Vec::new(); }`; add:

  ```rust
  /// `body` returns the non-streamed copy after `finish` (FIX-10). No effect on a streamed response.
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
- [x] **3.3** In `tee.rs`, add the imports `use zygo_config::keys::Secret;`, `use zygo_redact::DeadlinePolicy;`, `use crate::redact_stage::secret_pass;`, `use crate::session::SessionState;`, `use crate::translate::{error_message, error_object, scrub};`, change `use zygo_usage::{RequestRow, Store};` to `use zygo_usage::{RequestRow, Store, UpstreamErrorRow};`, and add before `struct Ticket`:

  ```rust
  /// The most bytes of the relayed body that `Flow::drop` copies for the usage writer (§14, FIX-10 B1).
  pub(crate) const ERROR_BODY_MAX: usize = 16 << 10;
  /// The most bytes of `error.type` and `error.message` that reach the scrub and the secret pass (FIX-10 B1, M7).
  const ERROR_FIELD_IN_MAX: usize = 2 << 10;
  /// The most characters stored in each field (§14, FIX-10).
  const ERROR_TEXT_MAX: usize = 300;
  /// The shortest run of the login that `scrub_partial` removes (FIX-10 M6).
  const PARTIAL_MIN: usize = 8;

  /// What the usage writer needs for a passthrough request's `upstream_errors` row (§14, FIX-10). Built on the response
  /// path with no parse and no pass (B1).
  pub(crate) struct ErrorCapture {
      /// At most ERROR_BODY_MAX bytes of the relayed body; empty for a streamed body or one over COPY_MAX.
      pub head: Vec<u8>,
      /// The client's login (the `Authorization` value after its scheme): removed from both fields, never stored.
      pub credential: Option<Secret>,
      pub session: Arc<SessionState>,
  }

  /// The longest prefix of `t` of at most `max` bytes that ends at a character boundary.
  fn head(t: &str, max: usize) -> &str {
      let mut end = t.len().min(max);
      while !t.is_char_boundary(end) {
          end -= 1;
      }
      &t[..end]
  }

  /// Each control character as a space; at most ERROR_TEXT_MAX characters.
  fn clean(t: &str) -> String {
      t.chars().map(|c| if c.is_control() { ' ' } else { c }).take(ERROR_TEXT_MAX).collect()
  }

  /// Each run of at least PARTIAL_MIN characters of `m` that is also a part of `cred` becomes the marker of
  /// `translate::scrub` (M6: an upstream that echoes a part of the login). Runs on at most ERROR_FIELD_IN_MAX bytes.
  fn scrub_partial(m: &str, cred: &str) -> String {
      let mark = scrub(cred, Some(cred));
      let c: Vec<char> = m.chars().collect();
      let (mut out, mut i) = (String::with_capacity(m.len()), 0);
      while i < c.len() {
          let (mut run, mut n) = (String::new(), 0);
          while i + n < c.len() {
              run.push(c[i + n]);
              if !cred.contains(run.as_str()) {
                  break;
              }
              n += 1;
          }
          if n >= PARTIAL_MIN {
              out.push_str(&mark);
              i += n;
          } else {
              out.push(c[i]);
              i += 1;
          }
      }
      out
  }

  /// One stored field (D2, D5): the 2 KiB cut, the login removed (whole, then each run), the secret pass under
  /// `policy` (empty on a block, a timeout or a panic), then `clean`.
  fn error_field(raw: &str, cred: Option<&str>, s: &SessionState, policy: &DeadlinePolicy) -> String {
      let mut t = scrub(head(raw, ERROR_FIELD_IN_MAX), cred);
      if let Some(c) = cred.filter(|c| !c.is_empty()) {
          t = scrub_partial(&t, c);
      }
      secret_pass(s, &t, policy).map(|t| clean(&t)).unwrap_or_default()
  }

  /// The `upstream_errors` row of `row` from its capture (D8): `error.type` and `error.message` of the head, each
  /// through `error_field`; empty when the head is not such JSON.
  fn error_row(row: &RequestRow, c: &ErrorCapture, policy: &DeadlinePolicy) -> UpstreamErrorRow {
      let v = serde_json::from_slice::<Value>(&c.head).ok();
      let cred = c.credential.as_ref().map(Secret::expose);
      let field = |x: Option<&str>| x.map(|x| error_field(x, cred, &c.session, policy)).unwrap_or_default();
      UpstreamErrorRow {
          ts: row.ts,
          session: row.session.clone(),
          request_class: row.request_class.clone(),
          model: row.model.clone(),
          status: row.status,
          error_type: field(v.as_ref().and_then(|v| error_object(v).get("type")).and_then(Value::as_str)),
          error_message: field(v.as_ref().and_then(error_message)),
      }
  }
  ```

  Result: the parse, the scrubs, the pass and the cuts live in `tee.rs` and run only where `error_row` is called (the writer, step 3.4).
- [x] **3.4** In `tee.rs`, `UsageSink`: `type RowSender = mpsc::Sender<(RequestRow, Option<ErrorCapture>, Option<Ticket>)>;` (`tee.rs:307`); `UsageSink::open` gets the parameter `secret: DeadlinePolicy` (signature `pub(crate) fn open(db: Option<&Path>, #[allow(unused_variables)] hooks: WriterHooks, secret: DeadlinePolicy) -> (UsageSink, Option<String>)`); the channel becomes `mpsc::channel::<(RequestRow, Option<ErrorCapture>, Option<Ticket>)>()` (`tee.rs:339`); the writer loop becomes `for (row, error, ticket) in rx {` with the test hooks as they are, then:

  ```rust
  let e = error.map(|c| error_row(&row, &c, &secret));
  let _ = match &e {
      Some(e) => store.insert_request_and_error(&row, e),
      None => store.insert_request(&row),
  };
  ```

  in place of `let _ = store.insert_request(&row);` (`drop(ticket); count(&h);` stay); and `send` becomes:

  ```rust
  /// No store or closed: the row and its capture are counted and dropped (with the ticket). `e`: the request's
  /// `upstream_errors` capture (FIX-10).
  pub(crate) fn send(&self, r: RequestRow, e: Option<ErrorCapture>, t: Option<Ticket>) {
      let sent = lock(&self.tx).as_ref().is_some_and(|tx| tx.send((r, e, t)).is_ok());
      if !sent {
          count(&self.handled);
      }
  }
  ```

  Result: the error row is built and written on the writer thread, in one transaction with its request row (D6).
- [x] **3.5** In `crates/zygo-proxy/src/server.rs`, `app` (`server.rs:80-88`), before `UsageSink::open`:

  ```rust
  // The secret deadline of stage 6 for the upstream_errors fields (§14, FIX-10).
  #[cfg(feature = "test-hooks")]
  let secret = opts.redact_limits.as_ref().map(|l| l.secret.clone()).unwrap_or_default();
  #[cfg(not(feature = "test-hooks"))]
  let secret = zygo_redact::DeadlinePolicy::default();
  ```

  and call `UsageSink::open(opts.usage_db.as_deref(), hooks, secret)`. Result: the writer uses the configured `RedactLimits.secret` (the spec default outside tests).
- [x] **3.6** In `crates/zygo-proxy/src/forward.rs`: keep the existing `use zygo_config::keys::Secret;` (`forward.rs:21`), add `AUTHORIZATION` to the `http::header` import (`forward.rs:16`), and change the `crate::tee` import (`forward.rs:34`) to add `ErrorCapture, ERROR_BODY_MAX`. In `struct Pending` (`forward.rs:208-225`) add `/// Passthrough, an upstream status that is not 2xx: the capture for the `upstream_errors` row (§14, FIX-10). upstream_error: Option<ErrorCapture>,`; in the `Pending` literal of `handle` (`forward.rs:301`) add `upstream_error: None`; in `Pending::drop` change the last line (`forward.rs:266`) to `self.sink.send(std::mem::take(&mut self.row), self.upstream_error.take(), self.ticket.take());`. In `run` (`forward.rs:318`), after `let mut tee = Tee::new(streamed);` (`forward.rs:539`), add `if s.passthrough && !status.is_success() { tee.keep_body(); }`. In `Flow::drop` (`forward.rs:1052-1063`), after `self.pending.split = self.tee.cache_split();`, add:

  ```rust
  // Passthrough, not 2xx: raw bytes and the login for the usage writer; no parse and no pass here (§14, FIX-10 B1).
  if self.pending.row.passthrough && !self.rctx.status.is_success() {
      let head = self.tee.body().map(|b| b[..b.len().min(ERROR_BODY_MAX)].to_vec()).unwrap_or_default();
      let credential = self.rctx.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
          .map(|v| v.split_once(' ').map_or(v, |(_, t)| t).trim())
          .filter(|t| !t.is_empty())
          .map(Secret::new);
      self.pending.upstream_error = Some(ErrorCapture { head, credential, session: self.pending.session.clone() });
  }
  ```

  Result: `RequestRow` and its literals are unchanged; a refusal (no `Flow`) sends `None`; the relayed status, headers and bytes are not touched (the tee returns each chunk unchanged, `tee.rs:161`); the response path copies at most 16 KiB and does no parse, lock or pass.
- [x] **3.7** Run `cargo check -p zygo-proxy --locked --all-targets`. Expected: exit 0, no warning in `forward.rs`, `tee.rs`, `redact_stage.rs` or `server.rs`.
- [x] **3.8** Run `cargo test -p zygo-proxy --locked --test passthrough`. Expected: 0 failed; the output lists `... ok` for the fourteen tests of steps 2.2 and 2.3, for `upstream_429_reaches_the_client_once_and_unchanged`, `cover_model_secret_not_in_row` and `cover_model_secret_redacted_upstream` (the `row_name` refactor).
- [x] **3.9** Run `cargo test -p zygo-proxy --locked --test request_path --test session_api --test redaction_e2e --lib`. Expected: 0 failed (the `Tee` and usage-writer tests, `usage_written_on_client_disconnect` among them, and the redaction tests are unchanged by `keep_body`, the new channel item and `secret_pass`).
- [x] **3.10** Commit `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/redact_stage.rs`, `crates/zygo-proxy/src/server.rs` and this file: `fix(proxy): record the upstream error type and message of a non-2xx passthrough answer`.

#### Task 4: the base spec (`docs/specs/2026-11-18-zygo-design.md`)

- [ ] **4.1** In §14, after the bullet `  - \`work_units\`: §13.1 facts.`, add the bullet:

  ```markdown
    - `upstream_errors`: ts, session, request_class, model, status, error_type, error_message. *(Note 2026-11-22, FIX-10: LIV-1 D8, FIX-8. One row for each passthrough request whose upstream answer reaches the client with a status that is not 2xx; a refusal by zygo itself, a non-passthrough session and an error event inside a 2xx stream write no row. The response path copies at most the first 16 KiB of the relayed body and does no parse; the usage writer reads `error.type` and `error.message` from it (`error` as an object, else the body). Both are empty when the copy is not such JSON (not JSON, an SSE body, a body over 16 KiB, or cut off by a client disconnect). Each field is cut to 2 KiB, the client's login is removed (whole, and each run of 8 or more of its characters), the secret pass of §16.1 runs on it at the stage-6 deadline (an empty field when the pass blocks or fails, also when `[features] redaction.secrets` is off), each control character becomes a space, and at most 300 characters are kept. The row and its `requests` row are written in one transaction. The table is created with `CREATE TABLE IF NOT EXISTS` on every open, without a schema version change; `retain_days` and `zygo usage --sync` (which replaces a session's rows) apply as for `requests`.)*
  ```

  Result: `rg -n 'upstream_errors' docs/specs/2026-11-18-zygo-design.md` prints the new line in §14 (between lines 670 and 683 of the current file).
- [ ] **4.2** In §14, replace the line `- Never: bodies, prompts, tool inputs, file paths outside the repository-relative \`files\` of a unit (kept only in the local session directory), redacted spans or words, credentials.` with:

  ```markdown
  - Never: bodies, prompts, tool inputs, file paths outside the repository-relative `files` of a unit (kept only in the local session directory), redacted spans or words, credentials. One exception: the upstream error `type` and `message` of a non-2xx passthrough answer (`upstream_errors`), scrubbed and capped as that table's note says. *(2026-11-22, FIX-10)*
  ```

  Result: `rg -n '^- Never: .*One exception' docs/specs/2026-11-18-zygo-design.md` prints one line.
- [ ] **4.3** At the end of the table under `## Change log`, add the row:

  ```markdown
  | 2026-11-22 | §14 (`upstream_errors`, "Never" line) | author's call (FIX-10, from FIX-8) | §14: table `upstream_errors` records the upstream error type and message of a non-2xx passthrough answer; LIV-1 D8, FIX-8, FIX-10. |
  ```

  Result: `rg -n 'LIV-1 D8, FIX-8, FIX-10' docs/specs/2026-11-18-zygo-design.md` prints one line, the change-log row.
- [ ] **4.4** Commit `docs/specs/2026-11-18-zygo-design.md` and this file: `docs(spec): §14 table upstream_errors (FIX-10)`.

#### Task 5: verification

- [ ] **5.1** Run the Acceptance of the contract: `cargo test -p zygo-proxy --locked --test passthrough && cargo test -p zygo-usage --locked --test store && cargo test --workspace --locked --no-run`. Expected: exit 0; 0 failed in both test binaries; the output lists `upstream_429_error_type_and_message_recorded ... ok`, `upstream_200_records_no_error ... ok`, `upstream_500_not_json_records_empty_fields ... ok`, `upstream_error_message_capped_and_without_control_characters ... ok`, `upstream_error_row_holds_no_request_content ... ok`, `upstream_errors_table_is_created_on_an_existing_database ... ok`; the workspace build of every test target ends without an error. Then commit this file with every step ticked: `docs(work): FIX-10 acceptance`.
