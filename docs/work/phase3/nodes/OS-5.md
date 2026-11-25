# OS-5 — MCP result tools record one read-back row per successful reply

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.4, O§6.5 item 2, O§7.11 last line, O§10 row "Usage store busy or broken (MCP read-back)", O§16 item 14
**Files:** `crates/zygo-mcp/src/tools/results.rs`, `crates/zygo-mcp/tests/readbacks.rs`, `crates/zygo-mcp/Cargo.toml`, `Cargo.lock`, `crates/zygo-results/tests/results.rs`
**Consumes:** `Stored.session` (OS-1), `ReadbackRow` (OS-2), `Store::insert_readback` (OS-2), `Store::open_bounded` (OS-3), `HOOK_DB_BOUND` (OS-3)
**Produces:** `Readbacks::new(usage_db: PathBuf, read_in: String, bound: Duration) -> Readbacks` (opens the store at the first read-back and keeps it; a failed open is tried again at the next read-back), `results::tools_with(dir: PathBuf, rb: Option<Readbacks>) -> Vec<Box<dyn Tool>>`; `results::tools(ctx: &McpCtx)` builds `Readbacks` from the session view's `session`, `Paths::from_env().usage_db` and `HOOK_DB_BOUND`
**Acceptance:** `cargo test -p zygo-mcp --test readbacks && cargo test -p zygo-mcp && cargo test -p zygo --test mcp_stdio` → all pass, including new tests in `readbacks.rs`: `result_view` on a stored id writes one row with `bytes` = the reply's byte count, `result_id` = the id, `session` = the meta's session, `read_in` = the reading session; a result whose meta has no `session` takes the reading session; `result_search` and `result_tree` each write one row; a bad id, a bad range and a bad pattern write no row; with the write lock held by a second connection and bound 0, the reply is byte-equal to the reply with no lock and no row is written

## Plan

### Files-line correction (premise false in the live code)

The header first listed only `results.rs` and `readbacks.rs`. Two premises were false at 882a7bdd:

1. **Test dependency.** The tests must hold the write lock in a second connection (`BEGIN IMMEDIATE`) and read the `readbacks` rows. No `zygo_usage` API does either: `Store` has only `insert_readback` (`crates/zygo-usage/src/store.rs:326`) and no reader of `readbacks`. `zygo-mcp` has no `rusqlite` dependency (`crates/zygo-mcp/Cargo.toml:7-23`; `[dev-dependencies]` has only `tempfile`). Thus `crates/zygo-mcp/Cargo.toml` gets `rusqlite.workspace = true` in `[dev-dependencies]` (workspace entry: `Cargo.toml:57`), and `Cargo.lock` gets `"rusqlite"` in the `zygo-mcp` package entry. `Cargo.lock` lists dev-dependencies too (`tempfile` is in that entry now).
2. **An existing test would write to the real usage store.** `tools_acceptance_100_kib_bash_output_is_trimmed_and_the_three_tools_read_the_stored_bytes` (`crates/zygo-results/tests/results.rs:857-875`) calls `zygo_mcp::tools::results::tools` (`:866`) in-process, with the real `HOME`, and gets 4 successful replies. After this node, `results::tools` takes `Paths::from_env().usage_db` (`crates/zygo-core/src/paths.rs:29,51`: `$HOME/.local/share/zygo/usage.db`). On a machine where that database exists, each `cargo test` run would add 4 `readbacks` rows with a temporary-directory session to the user's real figures. On CI the file is absent, `open_bounded` creates nothing (`store.rs:196-199`, `SQLITE_OPEN_READ_WRITE` without `CREATE`), and the test passes. That is why `crates/zygo-results/tests/results.rs` is in **Files**: Step 9 changes one line of it.

### Premise check (live code at 882a7bdd)

| Consumes | Location | Signature now | As the contract says |
|---|---|---|---|
| `Stored.session` (OS-1) | `crates/zygo-results/src/store.rs:53` | `pub session: Option<String>`; `Store::get` fills it from the meta (`:96`), `None` for a meta without the field (`:18-19`, `serde(default)`) | yes |
| `ReadbackRow` (OS-2) | `crates/zygo-usage/src/schema.rs:156` | `pub struct ReadbackRow { pub ts: i64, pub session: String, pub read_in: String, pub result_id: String, pub tool: String, pub bytes: u64 }`, reached as `zygo_usage::ReadbackRow` (`crates/zygo-usage/src/lib.rs:7`) | yes |
| `Store::insert_readback` (OS-2) | `crates/zygo-usage/src/store.rs:326` | `pub fn insert_readback(&self, r: &ReadbackRow) -> anyhow::Result<()>` | yes |
| `Store::open_bounded` (OS-3) | `crates/zygo-usage/src/store.rs:195` | `pub fn open_bounded(path: &Path, bound: Duration) -> anyhow::Result<Store>`: no file or schema is created; error unless `user_version` = 1; opens under a held write lock (`crates/zygo-usage/tests/store.rs:836-844`); an insert at bound 0 under the lock is `DatabaseBusy` (`:847-858`) | yes |
| `HOOK_DB_BOUND` (OS-3) | `crates/zygo-usage/src/store.rs:69` | `pub const HOOK_DB_BOUND: Duration = Duration::from_millis(100);`, reached as `zygo_usage::store::HOOK_DB_BOUND` | yes |

Other code the plan uses:

- `results::tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>>` (`crates/zygo-mcp/src/tools/results.rs:8-12`): empty without `ctx.session_dir`; else the three tools on `sd.results()`. `tools::build` passes it to `build_with` (`crates/zygo-mcp/src/tools/mod.rs:10-12`), and `zygo mcp` calls `build` (`crates/zygo/src/cmd/mcp.rs:13`).
- `McpCtx` (`crates/zygo-mcp/src/tool.rs:48-53`) has `agent`, `features` and `session_dir` only, with no session id. The existing helper `zygo_work::hooks::session_id(sd: &SessionDir) -> String` (`crates/zygo-work/src/hooks.rs:571-576`) gives `SessionView.session`, else the session directory's file name. `work::tools` (`crates/zygo-mcp/src/tools/work.rs:22-23`) and `workspace_down::tools` (`workspace_down.rs:45`) use it. Use it here; do not write a new loader.
- `Paths::from_env().usage_db` (`crates/zygo-core/src/paths.rs:26,51`).
- The reply text: the server sends a `Value::String(s)` reply as the `text` of the content unchanged (`crates/zygo-mcp/src/server.rs:123,132`). Thus "the reply's byte count" is `s.len()` of the `String` that the tool returns.
- Error replies: `load` (`results.rs:18-21`) gives `InvalidParams` for a bad id and `Failed` for an unknown id; `lines` and `Stored::range` give `InvalidParams` (`:48,50`); a missing or bad `pattern` gives `InvalidParams` (`:67,69`); `kind::tree` gives `Failed` (`:87`).
- The stderr line: write it with `let _ = writeln!(std::io::stderr().lock(), …)`. `eprintln!` panics on a closed stderr (`server.rs:29-31`; tests `closed_stderr_*`, `crates/zygo-mcp/tests/mcp_server.rs:483-485`).

### Produces (exact)

In `crates/zygo-mcp/src/tools/results.rs`:

- `pub struct Readbacks { usage_db: PathBuf, read_in: String, bound: Duration, store: Mutex<Option<zygo_usage::Store>> }`.
- `pub fn Readbacks::new(usage_db: PathBuf, read_in: String, bound: Duration) -> Readbacks` (`store` starts as `None`).
- `fn Readbacks::record(&self, stored: &Stored, tool: &str, reply: &str)` (private). It takes the lock (on a poisoned lock, `into_inner`). If the kept store is `None`, it calls `zygo_usage::Store::open_bounded(&self.usage_db, self.bound)`. It inserts `ReadbackRow { ts: <Unix seconds now>, session: stored.session.clone().unwrap_or_else(|| self.read_in.clone()), read_in: self.read_in.clone(), result_id: stored.id.clone(), tool: tool.into(), bytes: reply.len() as u64 }`. On an error of the open or the insert: no row, the kept store becomes `None` (so the next read-back opens again: O§6.4), and one stderr line `zygo mcp: usage store unavailable; read-back not recorded`. It never returns an error and never panics.
- `pub fn tools_with(dir: PathBuf, rb: Option<Readbacks>) -> Vec<Box<dyn Tool>>`: the three tools on `dir`, all three with one shared `Option<Arc<Readbacks>>`.
- `pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>>` keeps its signature. Without `ctx.session_dir` it returns `Vec::new()`, as now. Else it returns `tools_with(sd.results(), Some(Readbacks::new(Paths::from_env().usage_db, zygo_work::hooks::session_id(sd), zygo_usage::store::HOOK_DB_BOUND)))`.
- `ResultView`, `ResultSearch` and `ResultTree` each get the field `rb: Option<Arc<Readbacks>>`. Each `call` makes its reply `String` as now. When it is `Ok`, the tool calls `rb.record(&stored, "<its tool name>", &reply)` and then returns `Ok(Value::String(reply))`. An `Err` returns before `record`.

### Existing tests and this change

| Test | Drives a successful result reply? | After this node |
|---|---|---|
| `crates/zygo-mcp/tests/mcp_server.rs` (all; `build_with` at `:298-397`, `:548-558`) | No: only fake tools and `|_| vec![]` | Passes unchanged |
| `crates/zygo-mcp/tests/work_verdict.rs` | No | Passes unchanged |
| unit tests in `crates/zygo-mcp/src/tools/work.rs` | No | Pass unchanged |
| `crates/zygo/tests/mcp_stdio.rs` (4 tests) | No: `tools/list` (`:63`) and `compact` (`:41`) only; `err == ""` (`:48`) is for a run with no result tools | Passes unchanged; no stderr line, no `usage.db` |
| `crates/zygo-results/tests/results.rs:839-854` `tools_exist_inside_a_session_only` | No call | Passes unchanged |
| `crates/zygo-results/tests/results.rs:857-875` `tools_acceptance_100_kib_…` | Yes, 4 replies, real `HOME` | Passes, but writes rows to a real `usage.db` when it exists. Step 9 moves it to `tools_with(sd.results(), None)` |
| `crates/zygo-results/tests/results.rs:878-899` `tools_bad_arguments_…` | No: every call is an error | Passes unchanged; no row, no open |
| `crates/zygo/tests/workspace_restore.rs:779-787` | Yes, 1 `result_view` reply, scratch `HOME` (`env_clear`, `crates/zygo/tests/common/workspace.rs:332`) | Passes: it asserts only the reply text. The row goes to the scratch database, or the stderr line is written, which no assertion reads |

No test needs a `usage.db` that is absent now: `open_bounded` creates no file (`store.rs:196-199`; `crates/zygo-usage/tests/store.rs:874-881`). Step 1, test 6 proves this for the MCP path.

### Steps

- [x] **Step 1. Add the test dependency.** In `crates/zygo-mcp/Cargo.toml` `[dev-dependencies]`, add `rusqlite.workspace = true`. Run `cargo build -p zygo-mcp --tests`. Pass: it builds, and `git diff Cargo.lock` shows only `"rusqlite"` added to the `zygo-mcp` entry.

- [x] **Step 2. Write the failing tests** in the new file `crates/zygo-mcp/tests/readbacks.rs`. Helpers (test code, no body is fixed beyond this):
  ```rust
  use std::path::PathBuf;
  use std::time::{Duration, SystemTime, UNIX_EPOCH};
  use rusqlite::Connection;
  use serde_json::json;
  use zygo_mcp::tools::results::{tools_with, Readbacks};
  use zygo_mcp::{Tool, ToolError};

  const READ_IN: &str = "S-read";
  const TRIM: &str = "S-trim";
  type Row = (i64, String, String, String, String, i64);   // ts, session, read_in, result_id, tool, bytes

  struct Env { _t: tempfile::TempDir, dir: PathBuf, db: PathBuf }
  /// `dir` = <tmp>/results; `db` = <tmp>/usage.db, created by `zygo_usage::Store::open` (the full open, as the launcher does).
  fn env() -> Env;
  fn tools(e: &Env, bound: Duration) -> Vec<Box<dyn Tool>> { tools_with(e.dir.clone(), Some(Readbacks::new(e.db.clone(), READ_IN.into(), bound))) }
  fn by_name<'a>(t: &'a [Box<dyn Tool>], n: &str) -> &'a dyn Tool;
  fn call(t: &dyn Tool, args: serde_json::Value) -> Result<String, ToolError>;   // reply as a String
  /// `SELECT ts, session, read_in, result_id, tool, bytes FROM readbacks ORDER BY rowid` on a new connection.
  fn rows(db: &std::path::Path) -> Vec<Row>;
  fn now() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 }
  /// 500 lines `row {i} payload\n`, i = 1..=500.
  fn text() -> String;
  ```
  Tests and their assertions. Tests 2, 3 and 4 start with `let e = env(); let t = tools(&e, zygo_usage::store::HOOK_DB_BOUND);`.
  1. `readback_view_writes_one_row_with_the_reply_bytes`
     ```rust
     let e = env();
     let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), text().as_bytes()).unwrap();
     let t = tools(&e, zygo_usage::store::HOOK_DB_BOUND);
     let before = now();
     let reply = call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap();
     let after = now();
     let first_400: Vec<String> = (1..=400).map(|i| format!("row {i} payload")).collect();
     assert_eq!(reply, first_400.join("\n"));
     let r = rows(&e.db);
     assert_eq!(r.len(), 1);
     assert!((before..=after).contains(&r[0].0), "{r:?}");
     assert_eq!((&r[0].1[..], &r[0].2[..], &r[0].3[..], &r[0].4[..], r[0].5), (TRIM, READ_IN, &s.id[..], "result_view", reply.len() as i64));
     ```
  2. `readback_without_meta_session_takes_the_reading_session`
     ```rust
     let s = zygo_results::Store::put(&e.dir, "Bash", None, text().as_bytes()).unwrap();
     let reply = call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "61-120"})).unwrap();
     let want: Vec<String> = (61..=120).map(|i| format!("row {i} payload")).collect();
     assert_eq!(reply, want.join("\n"));
     let r = rows(&e.db);
     assert_eq!(r.len(), 1);
     assert_eq!((&r[0].1[..], &r[0].2[..], &r[0].3[..], &r[0].4[..], r[0].5), (READ_IN, READ_IN, &s.id[..], "result_view", reply.len() as i64));
     ```
  3. `readback_search_and_tree_each_write_one_row`
     ```rust
     let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), text().as_bytes()).unwrap();
     let j = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), br#"{"b":{"a":[1,2]}}"#).unwrap();
     let found = call(by_name(&t, "result_search"), json!({"id": s.id, "pattern": "row 7 "})).unwrap();
     assert_eq!(found, "6- row 6 payload\n7: row 7 payload\n8- row 8 payload");
     let tree = call(by_name(&t, "result_tree"), json!({"id": j.id})).unwrap();
     assert_eq!(tree, "b: object\n  a: array[2]");
     let r: Vec<(String, String, String, String, i64)> = rows(&e.db).into_iter().map(|r| (r.1, r.2, r.3, r.4, r.5)).collect();
     assert_eq!(r, [
         (TRIM.to_string(), READ_IN.to_string(), s.id.clone(), "result_search".to_string(), found.len() as i64),
         (TRIM.to_string(), READ_IN.to_string(), j.id.clone(), "result_tree".to_string(), 23),
     ]);
     ```
  4. `readback_error_replies_write_no_row` (O§6.4 "an error reply writes no row"; review focus 23)
     ```rust
     let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), b"a\nb\n").unwrap();
     let other = tempfile::tempdir().unwrap();
     let unknown = zygo_results::Store::put(other.path(), "Bash", None, b"x\n").unwrap().id;   // a valid id, absent from e.dir
     let (v, se, tr) = (by_name(&t, "result_view"), by_name(&t, "result_search"), by_name(&t, "result_tree"));
     assert!(matches!(call(v, json!({"id": "../x"})), Err(ToolError::InvalidParams(_))));            // bad id
     assert!(matches!(call(v, json!({"id": unknown})), Err(ToolError::Failed(_))));                 // unknown id
     assert!(matches!(call(tr, json!({"id": unknown})), Err(ToolError::Failed(_))));
     assert!(matches!(call(se, json!({"id": unknown, "pattern": "a"})), Err(ToolError::Failed(_))));
     for lines in ["0-5", "1-401", "abc"] {                                                          // bad range
         assert!(matches!(call(v, json!({"id": s.id, "lines": lines})), Err(ToolError::InvalidParams(_))), "{lines}");
     }
     assert!(matches!(call(se, json!({"id": s.id, "pattern": "("})), Err(ToolError::InvalidParams(_))));   // bad pattern
     assert!(matches!(call(se, json!({"id": s.id})), Err(ToolError::InvalidParams(_))));                  // no pattern
     assert_eq!(rows(&e.db), Vec::<Row>::new());
     ```
  5. `readback_busy_store_keeps_the_reply_and_writes_no_row` (O§6.4, O§10 row, O§16 item 14; review focus 23)
     ```rust
     let e = env();
     let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), text().as_bytes()).unwrap();
     let t = tools(&e, Duration::ZERO);
     let raw = Connection::open(&e.db).unwrap();
     raw.execute_batch("BEGIN IMMEDIATE").unwrap();          // a second connection holds the write lock
     let locked = call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "61-120"})).unwrap();
     assert!(matches!(call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "0-5"})), Err(ToolError::InvalidParams(_))));
     assert_eq!(rows(&e.db).len(), 0);                       // WAL: a reader is not blocked by the held lock
     raw.execute_batch("ROLLBACK").unwrap();
     let free = call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "61-120"})).unwrap();
     assert_eq!(locked, free);                               // byte-equal to the reply with no lock
     let r = rows(&e.db);
     assert_eq!(r.len(), 1);                                 // only the reply after the lock ended
     assert_eq!(r[0].5, free.len() as i64);
     ```
  6. `readback_missing_store_creates_nothing_and_opens_again_at_the_next_readback` (O§6.4 "the next read-back tries to open again"; the existing tests that run with no `usage.db`)
     ```rust
     let t0 = tempfile::tempdir().unwrap();
     let dir = t0.path().join("results");
     let db = t0.path().join("data").join("usage.db");
     let s = zygo_results::Store::put(&dir, "Bash", Some(TRIM), b"a\nb\n").unwrap();
     let t = tools_with(dir, Some(Readbacks::new(db.clone(), READ_IN.into(), zygo_usage::store::HOOK_DB_BOUND)));
     assert_eq!(call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap(), "a\nb");
     assert!(!t0.path().join("data").exists());              // no directory, no file
     drop(zygo_usage::Store::open(&db).unwrap());
     assert_eq!(call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap(), "a\nb");
     let r = rows(&db);
     assert_eq!(r.len(), 1);
     assert_eq!(r[0].5, 3);
     ```
  7. `tools_takes_the_session_view_and_the_home_usage_db`: the parent runs this test binary again with `std::env::current_exe()`, args `["--exact", "tools_child", "--nocapture", "--test-threads=1"]`. It sets the env `ZYGO_READBACKS_CHILD=1`, and sets `HOME`, `LOCALAPPDATA` and `APPDATA` to a new temporary directory `h`, as `crates/zygo/tests/common/mod.rs:10-11` does. It asserts `status.code() == Some(0)` and that stdout contains `"1 passed"`, so the child really ran. The child `tools_child` returns at once when `ZYGO_READBACKS_CHILD` is unset (the pattern of `mcp_server.rs:455-456`). Else:
     ```rust
     let home = PathBuf::from(std::env::var_os("HOME").unwrap());
     let db = zygo_core::paths::Paths::from_env().usage_db;
     assert!(db.starts_with(&home), "{}", db.display());
     drop(zygo_usage::Store::open(&db).unwrap());
     let sd = zygo_core::session_dir::SessionDir::new(home.join("s"));
     sd.create().unwrap();
     zygo_core::session_view::SessionView { session: "S-view".into(), ..Default::default() }.save(&sd).unwrap();
     let s = zygo_results::Store::put(&sd.results(), "Bash", Some(TRIM), b"a\nb\n").unwrap();
     let ctx = zygo_mcp::McpCtx { agent: zygo_core::Agent::ClaudeCode, features: zygo_config::Features::default(), session_dir: Some(sd) };
     let t = zygo_mcp::tools::results::tools(&ctx);
     assert_eq!(call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap(), "a\nb");
     let r = rows(&db);
     assert_eq!(r.len(), 1);
     assert_eq!((&r[0].1[..], &r[0].2[..], &r[0].3[..], &r[0].4[..], r[0].5), (TRIM, "S-view", &s.id[..], "result_view", 3));
     ```

- [x] **Step 3. Run and see the tests fail.** `cargo test -p zygo-mcp --test readbacks`. Pass condition for this step: the build fails because `zygo_mcp::tools::results::tools_with` and `Readbacks` do not exist.

- [x] **Step 4. Add `Readbacks`** to `crates/zygo-mcp/src/tools/results.rs`, with the exact fields, `new` and `record` of **Produces** above. Imports: `std::sync::{Arc, Mutex}`, `std::time::{Duration, SystemTime, UNIX_EPOCH}`, `std::io::Write`, `zygo_usage::ReadbackRow`, `zygo_core::paths::Paths`.

- [x] **Step 5. Add `tools_with` and change `tools`** in `results.rs`, as **Produces** says. `tools` keeps `pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>>` and its early `return Vec::new()` without a session directory (`results.rs:9`).

- [x] **Step 6. Record each successful reply.** Give `ResultView`, `ResultSearch` and `ResultTree` the field `rb: Option<Arc<Readbacks>>`. In each `call`, keep the reply text and every error exactly as now (`results.rs:45-50`, `:67-69`, `:86-87`). After a successful reply, call `record` with the tool name `"result_view"`, `"result_search"` or `"result_tree"`. Only `record` touches the store. No error path reaches it.

- [x] **Step 7. Run and see the new tests pass.** `cargo test -p zygo-mcp --test readbacks`. Pass: 8 tests pass, 0 fail (tests 1-7, and the child `tools_child`, which returns at once when its env is unset).

- [x] **Step 8. Run the crate tests.** `cargo test -p zygo-mcp`. Pass: all pass, with `mcp_server` and `work_verdict` unchanged.

- [x] **Step 9. Keep the zygo-results test off the real usage store.** In `crates/zygo-results/tests/results.rs:866`, change `let tools = zygo_mcp::tools::results::tools(&ctx(Some(t.path())));` to `let tools = zygo_mcp::tools::results::tools_with(sd.results(), None);`. Leave `:841`, `:853` and `:882` as they are: the first two make no call, and the third gets only error replies, which open no store. Run `cargo test -p zygo-results --test results`. Pass: all pass.

- [x] **Step 10. Run the full acceptance check** from the header, alone: `cargo test -p zygo-mcp --test readbacks && cargo test -p zygo-mcp && cargo test -p zygo --test mcp_stdio`. Pass: exit 0, and every test passes.

- [x] **Step 11. Run the verification floor**, the CI command (`.github/workflows/ci.yml:45`): `cargo test --workspace --locked --no-fail-fast`. Pass: all pass, `workspace_restore` included. `--locked` also proves that the `Cargo.lock` change of Step 1 is complete.

- [x] **Step 12. Commit.** Stage only `crates/zygo-mcp/src/tools/results.rs`, `crates/zygo-mcp/tests/readbacks.rs`, `crates/zygo-mcp/Cargo.toml`, `Cargo.lock`, `crates/zygo-results/tests/results.rs` and this node file (with the checked boxes). Commit:
  ```
  git commit -F - <<'MSG'
  OS-5: read-back rows from the MCP result tools

  Zygo-Session: <session>
  Zygo-Work: phase3:OS-5
  MSG
  ```

**Deviations:** Step 6: the three `call`s reach `record` through one private helper `reply(rb, stored, tool, text) -> Value` in `results.rs`, not three copies of the `if let Some(rb)` block; the behavior is as **Produces** says. Fail-first: Step 3 failed at the build (no `tools_with`, no `Readbacks`); with `record` disabled, tests 1, 2, 3, 5, 6 and 7 failed, and test 4 (`readback_error_replies_write_no_row`) passed, because it asserts that no row is written.

### Author's checks

- **Spec coverage.**
  - O§6.4: one row for each successful reply of all three tools (tests 1, 3). Row key `session` = the meta's session (test 1). No meta session means the reading session (test 2). `read_in` = the reading session (tests 1-3, 7). An error reply writes no row (test 4). A bounded open at the first read-back, kept (Step 4; test 5 writes its row through the kept or the new store). A busy or broken store: no row, reply unchanged (tests 5, 6). The next read-back opens again (test 6).
  - O§6.5 item 2: the row has the six columns of `ReadbackRow` (`schema.rs:156`), with `tool` one of the three names (tests 1, 3).
  - O§7.11 last line: `open_bounded` with `HOOK_DB_BOUND` in `tools` (Step 5; test 7 checks the session and the path; the bound value is a constant passed by `tools` and is not visible from outside). The bound is a parameter, so test 5 passes `Duration::ZERO`.
  - O§10 row "Usage store busy or broken (MCP read-back)": the reply is returned, with no row (tests 5, 6), and one stderr line (Step 4). No test asserts the stderr line. An in-process test cannot read it, and the fixed **Produces** signature takes no writer. OS-4 has this limit too: only a binary test reads its line.
  - O§16 item 14: test 1 (bytes, id, trim session, reading session), test 4 (bad id), test 5 (lock with bound 0, reply byte-equal, no row).
- **Review focus 23** (`graph.md:136`): a busy store does not change the reply text (test 5) or an error reply (test 5, the `0-5` call under the lock). No row for an error reply (test 4).
- **Step scan.** Each step has one action and a checkable result. There are no placeholders. `<session>` in Step 12 is the implementer's session id, which the handoff gives.
- **Interface match.** `Readbacks::new(usage_db: PathBuf, read_in: String, bound: Duration) -> Readbacks` and `results::tools_with(dir: PathBuf, rb: Option<Readbacks>) -> Vec<Box<dyn Tool>>` match **Produces** character for character. `results::tools(ctx: &McpCtx)` keeps its signature, so `tools::build` (`mod.rs:11`) and `zygo-results` (`results.rs:841,853,882`) still compile. No node in `graph.md` consumes from OS-5.
- **Proportion.** About 40 lines of code in `results.rs`, 7 tests (and one child entry), a one-line dependency and a one-line test change. The plan gives bodies only for the test assertions.
- **Live code.** Every path, line and signature above was read at 882a7bdd. The plan reuses `zygo_work::hooks::session_id` and does not write a new session-view loader.
- **Not in scope.** The doc comment of `Store::open` (`crates/zygo-usage/src/store.rs:111-113`, "The hook and the MCP server's read-back write move to the bounded open (O§7.11); until then they call this open") is stale after OS-4 and this node. It is in `zygo-usage`, which is outside **Files**.
