# OS-1 — Result meta records the session that stored it

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.5 item 1, O§6.4 "Key"
**Files:** `crates/zygo-results/src/store.rs`, `crates/zygo-results/tests/results.rs`, `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/hooks_basic.rs`
**Consumes:** —
**Produces:** `Store::put(dir: &Path, tool: &str, session: Option<&str>, bytes: &[u8]) -> anyhow::Result<Stored>`, `Stored.session: Option<String>` (from `Meta.session`, `#[serde(default)]`)
**Acceptance:** `test "$(cargo test -q -p zygo-results --test results -- session --list 2>/dev/null | grep -c ': test$')" -ge 2 && test "$(cargo test -q -p zygo-hooks --test hooks_basic -- trim_session --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-results --test results && cargo test -p zygo-hooks --test hooks_basic` → every count holds (at least 2 named `session…` in zygo-results `results`; at least 1 named `trim_session…` in zygo-hooks `hooks_basic`; a count of 0 fails before any run), then all pass, including new tests: a put with `Some("S1")` gives `Store::get(..).session == Some("S1")`; a meta file without `session` (the `97d8767` form) reads as `None`; the trim hook stores its session in the meta

## Plan

**Premises checked against the live code (branch `phase3/OS-1` at `f30d9d3`):**

- `Meta` is `crates/zygo-results/src/store.rs:11-18` with five fields (`tool`, `bytes`, `lines`, `kind`, `created_at`), equal to the `97d8767` form (`git show 97d8767:crates/zygo-results/src/store.rs`). No `deny_unknown_fields`, so an older binary already ignores a new `session` key.
- `Stored` is `store.rs:43-52` (`#[derive(Debug, Clone, PartialEq)]`); `Store::put` is `store.rs:57-81` and builds `Stored` at `:80`; `Store::get` is `store.rs:83-91` and builds `Stored` at `:90`.
- Every `Store::put` caller in the workspace (all 13 members, tests included; no other crate, bench or doc test calls it):
  - `crates/zygo-hooks/src/trim.rs:115` (the trim hook; `sv.session: String` from `SessionView`, `crates/zygo-core/src/session_view.rs:8`, is moved into `TrimRow` only at `:120`).
  - `crates/zygo-results/tests/results.rs:9` (helper `put`), `:59`, `:86`, `:87`, `:90`, `:92`, `:99`, `:114`, `:189`, `:689`, `:701`, `:742`, `:753`, `:761` — 14 call lines (the helper `put` at `:9` plus 13 others; `:7` is the helper's `fn put` definition, not a call).
- Every other site that the new `Stored` field breaks: the struct literal `crates/zygo-hooks/src/trim.rs:109-110` (`preview`). `crates/zygo-mcp/src/tools/results.rs` only reads `Stored` (`:4`, `:18`); no change.
- `results.rs:68` asserts the meta object has exactly 5 keys after a put; `results.rs:324` defines a non-test helper `fn session(..)`, which the new test names do not clash with.
- `crates/zygo-hooks/tests/hooks_basic.rs:17` gives the trim env the session `"sess-1"`; helpers `run_trim` (`:98`), `bash` (`:100`), `lines` (`:96`), `result_ids` (`:107-110`) exist.
- Baseline of the Acceptance counts today: the `session` filter already lists 14 tests in `results` (substring match: `au69_…_session…`, `sweep_…_session…`, `tools_exist_inside_a_session_only`); `trim_session` lists 0. So the first count holds before this node; Step 2 proves the new tests by exact name.

**Decisions:**

- `Meta.session` carries `#[serde(default, skip_serializing_if = "Option::is_none")]`: `default` is the contract (old meta reads as `None`); `skip_serializing_if` keeps a `None` put byte-identical in shape to the `97d8767` meta, so `results.rs:68` (5 keys) stays true unchanged and the old form is the one form for "no session".
- Existing test callers pass `None` (their behavior is unchanged); the hook passes `Some(&sv.session)` (O§6.4 "Key": the session whose hook stored it).
- `preview` in `trim.rs:109` takes `session: None`: the marker text does not use the session (`view.rs`), and the preview is never stored.

**Review focus:** no graph review-focus item names OS-1. Related items whose tests live in later nodes but rest on this one: #20 (`Store::put` error must write no row — OS-28; this node keeps `trim.rs:115`'s fixed-text error and its `?` before the row), #22 ("a read-back keyed by the reading session" — OS-6/OS-7; this node supplies the stored session they key on, and the `None` fallback that O§6.4 assigns to the reading session).

### Steps

- [x] **Step 1 — failing tests.** Append to `crates/zygo-results/tests/results.rs` (after `store_put_creates_the_results_directory_with_mode_0700`, before the `use zygo_config::Features;` block):

  ```rust
  #[test]
  fn session_put_with_some_is_in_the_meta_and_read_back_by_get() {     // O§6.5 item 1, O§6.4 Key
      let t = tempfile::tempdir().unwrap();
      let dir = t.path().join("results");
      let s = Store::put(&dir, "Bash", Some("S1"), b"x").unwrap();
      assert_eq!(s.session.as_deref(), Some("S1"));
      let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", s.id))).unwrap()).unwrap();
      assert_eq!(meta["session"], "S1");
      assert_eq!(meta.as_object().unwrap().len(), 6);
      assert_eq!(Store::get(&dir, &s.id).unwrap().session.as_deref(), Some("S1"));
      assert_eq!(Store::get(&dir, &s.id).unwrap(), s);
  }

  #[test]
  fn session_absent_from_an_old_meta_file_reads_as_none() {           // the 97d8767 meta form
      let t = tempfile::tempdir().unwrap();
      let dir = t.path().join("results");
      std::fs::create_dir_all(&dir).unwrap();
      let id = ulid::Ulid::generate().to_string();
      std::fs::write(dir.join(&id), b"x").unwrap();
      std::fs::write(dir.join(format!("{id}.json")), br#"{"tool":"Bash","bytes":1,"lines":1,"kind":"text","created_at":1700000000}"#).unwrap();
      let s = Store::get(&dir, &id).unwrap();
      assert_eq!(s.session, None);
      assert_eq!((s.tool.as_str(), s.bytes, s.lines, s.kind, s.created_at, s.data.as_slice()), ("Bash", 1, 1, Kind::Text, 1700000000, &b"x"[..]));
  }

  #[test]
  fn session_none_writes_no_session_key() {                           // a None put keeps the 97d8767 meta shape
      let t = tempfile::tempdir().unwrap();
      let dir = t.path().join("results");
      let s = Store::put(&dir, "Bash", None, b"x").unwrap();
      assert_eq!(s.session, None);
      let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", s.id))).unwrap()).unwrap();
      assert!(meta.get("session").is_none(), "{meta}");
      assert_eq!(meta.as_object().unwrap().len(), 5);
      assert_eq!(Store::get(&dir, &s.id).unwrap().session, None);
  }
  ```

  Append to `crates/zygo-hooks/tests/hooks_basic.rs` after `trim_records_bytes_trimmed` (`:134-138`):

  ```rust
  #[test] fn trim_session_is_stored_in_the_result_meta() {             // O§6.5 item 1: the session whose hook stored it
      let e = env(true);
      updated(&out(run_trim(&bash(&lines(5000)), &e)));
      let ids = result_ids(&e); assert_eq!(ids.len(), 1);
      assert_eq!(zygo_results::Store::get(&e.dir.results(), &ids[0]).unwrap().session.as_deref(), Some("sess-1"));
      let meta: Value = serde_json::from_slice(&std::fs::read(e.dir.results().join(format!("{}.json", ids[0]))).unwrap()).unwrap();
      assert_eq!(meta["session"], "sess-1");
  }
  ```

- [x] **Step 2 — see them fail.** `cargo test -p zygo-results --test results -- session_ 2>&1 | tail -20` → compile error E0061 (`put` takes 3 arguments, 4 supplied) and E0609 (no field `session` on `Stored`). `cargo test -p zygo-hooks --test hooks_basic -- trim_session 2>&1 | tail -20` → compile error E0609 (no field `session` on `Stored`).

- [x] **Step 3 — implement the store** in `crates/zygo-results/src/store.rs`:
  - `Meta` (`:11-18`) gains, after `created_at`:
    ```rust
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
    ```
  - `Stored` (`:43-52`) gains `pub session: Option<String>,` after `created_at`.
  - Signature `:57`: `pub fn put(dir: &Path, tool: &str, session: Option<&str>, bytes: &[u8]) -> anyhow::Result<Stored>`; the `Meta` literal sets `session: session.map(str::to_owned)`; the `Stored` literal at `:80` adds `session: meta.session` (after `created_at: meta.created_at`).
  - `get`'s `Stored` literal at `:90` adds `session: meta.session`.

- [x] **Step 4 — update every caller.**
  - `crates/zygo-results/tests/results.rs`: insert `None, ` before the bytes argument at each of `:9`, `:59`, `:86`, `:87`, `:90`, `:92`, `:99`, `:114`, `:189`, `:689`, `:701`, `:742`, `:753`, `:761` (line numbers before Step 1's append, which lies below all but `:742`, `:753`, `:761`). Check: `grep -n 'Store::put(' crates/zygo-results/tests/results.rs | grep -vc 'None, \|Some("S1")'` → `0`.
  - `crates/zygo-hooks/src/trim.rs:109-110`: the `preview` literal adds `session: None` after `created_at: 0`. Then run `cargo test -p zygo-hooks --test hooks_basic -- trim_session` with `:115` passing `None` → it compiles and fails on the `Some("sess-1")` assertion (the hook does not yet store its session).
  - `crates/zygo-hooks/src/trim.rs:115`: `Store::put(&dir.results(), &tool, Some(&sv.session), text.as_bytes())`; the `map_err` fixed text stays.
  - Check: `grep -rn 'Store::put(' crates --include='*.rs'` lists only `store.rs`'s definition and calls with 4 arguments.

- [x] **Step 5 — see them pass.**
  - `cargo test -p zygo-results --test results -- --exact session_put_with_some_is_in_the_meta_and_read_back_by_get session_absent_from_an_old_meta_file_reads_as_none session_none_writes_no_session_key` → 3 passed.
  - `cargo test -p zygo-hooks --test hooks_basic -- --exact trim_session_is_stored_in_the_result_meta` → 1 passed.
  - The Acceptance command, verbatim → every count holds (`session` ≥ 2, `trim_session` ≥ 1), then both test binaries pass.
  - `cargo build --workspace --locked` → builds (no other crate constructs `Stored` or calls `put`); `cargo test -p zygo-mcp -p zygo --locked --no-run` → compiles.

- [x] **Step 6 — commit** in `/Users/kagemartin/Work/zygo/.worktrees/phase3/OS-1`: `git add crates/zygo-results/src/store.rs crates/zygo-results/tests/results.rs crates/zygo-hooks/src/trim.rs crates/zygo-hooks/tests/hooks_basic.rs`, message `OS-1: result meta records the session that stored it`, trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:OS-1`, no attribution. Check: `git show --stat HEAD` lists exactly those four files.
