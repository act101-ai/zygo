# RT-2 — Run-time start rule, `<form>` text, and the `runtime:` line of `zygo status`

**Spec:** docs/specs/2026-11-24-session-runtime-design.md §2.1, §2.2, §2.3, §2.5, §2.6, §3.3, §6 (rows `ids.rs`, `phase.rs`, `render.rs` format function, `status.rs`), §7 items 1, 2, 3 and 8; base-spec amendment §17 `zygo status` row (docs/specs/2026-11-18-zygo-design.md)
**Files:** `crates/zygo-core/src/ids.rs`, `crates/zygo-hooks/src/statusline/phase.rs`, `crates/zygo-hooks/tests/statusline_phase.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo/src/cmd/status.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** `zygo_core::ids::ulid_secs(s: &str) -> Option<u64>` (ULID `timestamp_ms ÷ 1000`, floor; `None` for a string that is not a ULID); `zygo_hooks::statusline::phase::MemberRead { pub phase: Option<String>, pub start: Option<u64> }`; `zygo_hooks::statusline::phase::member_read(paths: &Paths, view: &SessionView) -> MemberRead` (one `workspace.json` read gives the phase and the T§2.2 start; a start of 0 counts as none); `zygo_hooks::statusline::phase::start(paths: &Paths, view: &SessionView) -> Option<u64>`; `zygo_hooks::statusline::render::run_time_text(secs: u64) -> String` (the T§2.3 form); `zygo::cmd::status::run(ctx: &VerbCtx, repo: Option<&Path>, hide: bool, now: u64) -> anyhow::Result<String>`
**Acceptance:** `test "$(cargo test -q -p zygo-core -p zygo-hooks -p zygo -- ulid_secs --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-core -p zygo-hooks -p zygo -- run_time_format --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-core -p zygo-hooks -p zygo -- start_rule --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-core -p zygo-hooks -p zygo -- runtime_line --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test --locked -p zygo-core -p zygo-hooks -p zygo` → every count holds (at least 1 named `ulid_secs…` in the zygo-core, zygo-hooks, zygo tests; at least 1 named `run_time_format…` in the zygo-core, zygo-hooks, zygo tests; at least 1 named `start_rule…` in the zygo-core, zygo-hooks, zygo tests; at least 3 named `runtime_line…` in the zygo-core, zygo-hooks, zygo tests; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows CI, and the tests whose names contain `ulid_secs`, `run_time_format` (T§7 item 1: all 19 listed values and `u64::MAX` with no panic), `start_rule` (T§7 item 2: every listed case) and `runtime_line` (T§7 items 3 and 8: `runtime: 2d04h` right after `session:` with `now` = start + 187200, also after `session: unreachable (…)`; `runtime: 0m` with start > now; no line outside a session or with no start) run and pass, with the existing `inside_a_session`, `outside_a_session` and `statusline_phase` tests unchanged and passing

## Plan

Premise check (live, at `b10525fc`): ML-1 and WD-5 are DONE. RT-1 produced the field this node reads: `MemberRecord.first_started_at: Option<u64>` (`crates/zygo-workspace/src/state.rs:104`), with `zygo_session: Option<String>` (`:99`) and `pending: Option<Pending>` (`:101`), where `Pending { zygo_session: String, started_at: u64, … }` (`:136-147`). The ULID crate is `ulid = "3.0.0"` (`Cargo.toml:70`, `crates/zygo-core/Cargo.toml:13`): `Ulid::from_string(&str) -> Result<Ulid, DecodeError>` and `Ulid::timestamp_ms(&self) -> u64`. `from_string` checks only the length and the alphabet, so a 26-character string that starts with `8`-`Z` decodes with its top bits lost; `ulid_secs` refuses it (first character `0`-`7`). The reader in `phase.rs:57-77` is the H AU25 read; `data.rs:586` calls `phase::read` and stays as it is (RT-3 replaces that call, graph row 65). No helper in `crates/zygo/src/cmd/compact.rs:145-230` writes a `view.json`, so the existing `status.rs` tests load no view and get no `runtime:` line.

Consumes (live):
- `zygo_core::session_view::SessionView { pub session: String, pub workspace: Option<ViewWorkspace>, … }` (`crates/zygo-core/src/session_view.rs:7-48`), `ViewWorkspace { pub member: Member, pub resume_note: Option<ResumeNote>, pub extra }` (`:164-169`), `Member { pub ws_id: String, pub pane: String, pub extra }` (`:135-140`); `SessionView::load(dir: &SessionDir) -> anyhow::Result<Self>` (`:177`).
- `zygo_core::paths::Paths::workspaces_dir(&self) -> PathBuf` (`crates/zygo-core/src/paths.rs:73`).
- `zygo_hooks::statusline::phase::valid_ws_id(s: &str) -> bool` (`phase.rs:32`), `open(path: &Path) -> Option<File>` (`:39`, `:47`), `STATE_FILE`, `VERSION`, `MAX_BYTES` (`:18-22`).
- `zygo::cmd::verbs::VerbCtx { pub slash, pub save, pub session_dir: Option<SessionDir>, pub paths: Paths }` (`crates/zygo/src/cmd/verbs.rs:27-32`).
- Test helpers `crate::cmd::compact::tests::{ctx, paths, canned_session, dead_session}` (`compact.rs:145`, `:158`, `:188`, `:201`).

Produces (exact, after this node):
- `pub fn ulid_secs(s: &str) -> Option<u64>` in `crates/zygo-core/src/ids.rs`.
- `#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct MemberRead { pub phase: Option<String>, pub start: Option<u64> }` in `crates/zygo-hooks/src/statusline/phase.rs`.
- `pub fn member_read(paths: &Paths, view: &SessionView) -> MemberRead` and `pub fn start(paths: &Paths, view: &SessionView) -> Option<u64>` in `phase.rs`; `pub fn read(paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>) -> Option<String>` keeps its signature and its results.
- `pub fn run_time_text(secs: u64) -> String` in `crates/zygo-hooks/src/statusline/render.rs`.
- `pub fn run(ctx: &VerbCtx, repo: Option<&Path>, hide: bool, now: u64) -> anyhow::Result<String>` in `crates/zygo/src/cmd/status.rs`; `verb` passes the system clock.

Steps:

- [ ] 1. Add a test module at the end of `crates/zygo-core/src/ids.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test] // T§2.1: the start is `timestamp_ms ÷ 1000`, floor
      fn ulid_secs_is_the_ulid_time_in_whole_seconds() {
          assert_eq!(ulid_secs(&ulid::Ulid::from_parts(1_800_000_000_000, 0).to_string()), Some(1_800_000_000));
          assert_eq!(ulid_secs(&ulid::Ulid::from_parts(1_800_000_000_999, 1).to_string()), Some(1_800_000_000));
          // The literal that the zygo-hooks and zygo tests use as `view.session`.
          assert_eq!(ulid_secs("01MCC5RM000000000000000000"), Some(1_800_000_000));
          assert_eq!(ulid_secs("00000000000000000000000000"), Some(0));
          assert_eq!(ulid_secs("7ZZZZZZZZZZZZZZZZZZZZZZZZZ"), Some(281_474_976_710));
      }

      #[test] // T§2.2 rule 1: a `session` that is not a ULID gives no start
      fn ulid_secs_of_a_string_that_is_not_a_ulid_is_none() {
          for bad in ["", "S1", "01MCC5RM00000000000000000", "01MCC5RM0000000000000000000", "01MCC5RM00000000000000000U", "80000000000000000000000000", "ZZZZZZZZZZZZZZZZZZZZZZZZZZ"] {
              assert_eq!(ulid_secs(bad), None, "{bad:?}");
          }
      }
  }
  ```
- [ ] 2. Run `cargo test --locked -p zygo-core --lib ulid_secs`. It fails to compile: `ulid_secs` is not defined.
- [ ] 3. Add to `crates/zygo-core/src/ids.rs`, after `ulid()`:
  ```rust
  /// The creation time of ULID `s` in whole seconds (`timestamp_ms ÷ 1000`, floor; session run time design T§2.1);
  /// `None` for a string that is not a ULID. The first character is `0`-`7`: a larger one is 130 bits.
  pub fn ulid_secs(s: &str) -> Option<u64> {
      if !matches!(s.as_bytes().first(), Some(b'0'..=b'7')) {
          return None;
      }
      ulid::Ulid::from_string(s).ok().map(|u| u.timestamp_ms() / 1000)
  }
  ```
- [ ] 4. Run `cargo test --locked -p zygo-core --lib ulid_secs`. 2 tests pass.
- [ ] 5. Append to `crates/zygo-hooks/tests/statusline_render.rs` a new import line `use zygo_hooks::statusline::render::run_time_text;` (below the existing `use` lines, which do not change) and this test:
  ```rust
  // ---- run time (session run time design T§2.3, T§7 items 1 and 3) ----

  #[test]
  fn run_time_format_t7_item_1() {
      for (secs, want) in [
          (0, "0m"), (59, "0m"), (60, "1m"), (599, "9m"), (600, "10m"), (2820, "47m"), (3599, "59m"), (3600, "1h00m"),
          (18720, "5h12m"), (35999, "9h59m"), (36000, "10h00m"), (86399, "23h59m"), (86400, "1d00h"), (187200, "2d04h"),
          (863999, "9d23h"), (864000, "10d00h"), (1062000, "12d07h"), (8639999, "99d23h"), (8640000, "100d00h"),
      ] {
          assert_eq!(run_time_text(secs), want, "{secs}");
      }
      // Every `u64` has a form; no cap, no panic.
      assert_eq!(run_time_text(u64::MAX), "213503982334601d07h");
  }
  ```
- [ ] 6. Run `cargo test --locked -p zygo-hooks --test statusline_render run_time_format`. It fails to compile: `run_time_text` is not defined.
- [ ] 7. Add to `crates/zygo-hooks/src/statusline/render.rs`, after `spend_text` (`:253-265`), under a `// ---- run time (T§2.3) ----` line:
  ```rust
  /// The run-time form of the HUD and of `zygo status` (session run time design T§2.3), `secs` = now − start (the
  /// caller gives 0 when start > now): `<m>m` below 1 hour, `<h>h<mm>m` below 1 day, else `<D>d<hh>h`. Floor at each
  /// unit; the second unit has two digits; no cap.
  pub fn run_time_text(secs: u64) -> String {
      match secs {
          0..3600 => format!("{}m", secs / 60),
          3600..86400 => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
          _ => format!("{}d{:02}h", secs / 86400, secs % 86400 / 3600),
      }
  }
  ```
- [ ] 8. Run `cargo test --locked -p zygo-hooks --test statusline_render run_time_format`. 1 test passes.
- [ ] 9. Append to `crates/zygo-hooks/tests/statusline_phase.rs` a new import line `use zygo_hooks::statusline::phase::{member_read, start, MemberRead};` (the existing `use` lines and tests do not change) and these items:
  ```rust
  // ---- the start-time rule (session run time design T§2.2, T§7 item 2) ----

  /// A ULID whose time is 1_800_000_000_000 ms: the session's own start is 1_800_000_000.
  const SID: &str = "01MCC5RM000000000000000000";
  const OWN: u64 = 1_800_000_000;
  /// Another zygo session.
  const OTHER: &str = "01MCC5RMZ70000000000000001";
  const PANE: &str = "kabcd2345";

  /// A member view of `ws_id`, pane `PANE`, zygo session `session` (in memory; `member_read` reads no `view.json`).
  fn member(session: &str, ws_id: &str) -> SessionView {
      let member = Member { ws_id: ws_id.into(), pane: PANE.into(), ..Default::default() };
      SessionView { session: session.into(), workspace: Some(ViewWorkspace { member, resume_note: None, ..Default::default() }), ..Default::default() }
  }

  /// A version-1 `workspace.json` in phase `up` whose `members` is `{PANE: record}`.
  fn with_record(record: serde_json::Value) -> Vec<u8> {
      serde_json::json!({ "version": 1, "phase": "up", "members": { PANE: record } }).to_string().into_bytes()
  }

  /// The start for `record` in a member view of session `SID`.
  fn start_of(record: serde_json::Value) -> Option<u64> {
      let fx = Fx::new();
      fx.state(ID, &with_record(record));
      start(&fx.paths(), &member(SID, ID))
  }

  #[test] // rule 4: `first_started_at` when the record names this session, before `pending`
  fn start_rule_first_started_at_when_the_record_names_this_session() {
      let fx = Fx::new();
      fx.state(ID, &with_record(serde_json::json!({ "zygo_session": SID, "first_started_at": 1_700_000_000u64 })));
      assert_eq!(member_read(&fx.paths(), &member(SID, ID)), MemberRead { phase: Some("up".into()), start: Some(1_700_000_000) });
      let p = serde_json::json!({ "zygo_session": SID, "started_at": 1_750_000_000u64 });
      assert_eq!(start_of(serde_json::json!({ "zygo_session": SID, "pending": p, "first_started_at": 1_700_000_000u64 })), Some(1_700_000_000));
      // The record belongs through `pending` only (`record_of`, `crates/zygo-hooks/src/workspace.rs:480-482`).
      assert_eq!(start_of(serde_json::json!({ "zygo_session": OTHER, "pending": p, "first_started_at": 1_700_000_000u64 })), Some(1_700_000_000));
  }

  #[test] // rule 5: `pending.started_at` when the field is absent and `pending` names this session
  fn start_rule_pending_when_the_field_is_absent() {
      let p = serde_json::json!({ "zygo_session": SID, "started_at": 1_750_000_000u64 });
      assert_eq!(start_of(serde_json::json!({ "pending": p })), Some(1_750_000_000));
      assert_eq!(start_of(serde_json::json!({ "zygo_session": OTHER, "pending": p })), Some(1_750_000_000));
  }

  #[test] // rules 3 and 6: the session's own start
  fn start_rule_own_start_for_another_session_no_record_and_an_old_record() {
      // A record of another session (graph row 61): neither `zygo_session` nor `pending.zygo_session` is `SID`.
      let p = serde_json::json!({ "zygo_session": OTHER, "started_at": 1_750_000_000u64 });
      assert_eq!(start_of(serde_json::json!({ "zygo_session": OTHER, "pending": p, "first_started_at": 1_700_000_000u64 })), Some(OWN));
      // No record: `members` without the pane, and no `members` key.
      let fx = Fx::new();
      fx.state(ID, br#"{"version":1,"phase":"up","members":{}}"#);
      assert_eq!(start(&fx.paths(), &member(SID, ID)), Some(OWN));
      fx.state(ID, br#"{"version":1,"phase":"up"}"#);
      assert_eq!(start(&fx.paths(), &member(SID, ID)), Some(OWN));
      // A committed old record: no field, no `pending` (T§4).
      assert_eq!(start_of(serde_json::json!({ "zygo_session": SID })), Some(OWN));
  }

  #[test] // T§2.2: a value of 0 counts as unknown
  fn start_rule_zero_counts_as_absent() {
      let p = serde_json::json!({ "zygo_session": SID, "started_at": 1_750_000_000u64 });
      assert_eq!(start_of(serde_json::json!({ "zygo_session": SID, "pending": p, "first_started_at": 0 })), Some(1_750_000_000));
      assert_eq!(start_of(serde_json::json!({ "zygo_session": SID, "first_started_at": 0 })), Some(OWN));
      let p0 = serde_json::json!({ "zygo_session": SID, "started_at": 0 });
      assert_eq!(start_of(serde_json::json!({ "pending": p0 })), Some(OWN));
      // An own start of 0 is none.
      let fx = Fx::new();
      assert_eq!(start(&fx.paths(), &SessionView { session: "00000000000000000000000000".into(), ..Default::default() }), None);
  }

  #[test] // rule 2 (graph row 62): any read failure gives no start, and no ULID fallback
  fn start_rule_no_start_when_workspace_json_fails_a_read_check() {
      let rec = serde_json::json!({ "zygo_session": SID, "first_started_at": 1_700_000_000u64 });
      let fx = Fx::new();
      // A bad `ws_id`: no path is built, so no file is read.
      fx.state(ID, &with_record(rec.clone()));
      assert_eq!(member_read(&fx.paths(), &member(SID, "../x")), MemberRead::default());
      // A missing file.
      let fx = Fx::new();
      assert_eq!(member_read(&fx.paths(), &member(SID, ID)), MemberRead::default());
      // A file above 64 KiB, a parse error, `version` 2.
      for body in [padded(64 * 1024 + 1), b"not json".to_vec(), serde_json::json!({ "version": 2, "phase": "up", "members": { PANE: rec } }).to_string().into_bytes()] {
          fx.state(ID, &body);
          assert_eq!(member_read(&fx.paths(), &member(SID, ID)), MemberRead::default());
      }
      // The pane's record does not decode: no start; the phase still shows.
      for bad in [serde_json::json!({ "zygo_session": SID, "first_started_at": "x" }), serde_json::json!(7)] {
          fx.state(ID, &with_record(bad));
          assert_eq!(member_read(&fx.paths(), &member(SID, ID)), MemberRead { phase: Some("up".into()), start: None });
      }
      // `members` is not an object: no start; the phase still shows.
      fx.state(ID, br#"{"version":1,"phase":"up","members":[]}"#);
      assert_eq!(member_read(&fx.paths(), &member(SID, ID)), MemberRead { phase: Some("up".into()), start: None });
  }

  #[test] // rule 1: outside a workspace, the ULID time of `view.session`; no file read
  fn start_rule_outside_a_workspace() {
      let fx = Fx::new();
      let own = |s: &str| member_read(&fx.paths(), &SessionView { session: s.into(), ..Default::default() });
      assert_eq!(own(SID), MemberRead { phase: None, start: Some(OWN) });
      for bad in ["S1", "", "80000000000000000000000000"] {
          assert_eq!(own(bad), MemberRead::default(), "{bad:?}");
      }
      assert!(!fx.paths().workspaces_dir().exists());
  }
  ```
- [ ] 10. Run `cargo test --locked -p zygo-hooks --test statusline_phase start_rule`. It fails to compile: `member_read`, `start` and `MemberRead` are not defined.
- [ ] 11. Change `crates/zygo-hooks/src/statusline/phase.rs`:
  - Module doc: add one line that the reader also gives the run-time start of session run time design T§2.2 from the same read (T§2.6).
  - `Marker` gets `#[serde(default)] members: serde_json::Value` (absent reads as `Null`); `version` and `phase` stay as they are, so a file that `read` refuses today is still refused.
  - Add the record struct (unknown fields ignored; still no `zygo-workspace` dependency, H§7.6 as amended by T§3.1):
    ```rust
    #[derive(Deserialize)]
    struct Record {
        #[serde(default)]
        zygo_session: Option<String>,
        #[serde(default)]
        pending: Option<PendingStart>,
        #[serde(default)]
        first_started_at: Option<u64>,
    }

    #[derive(Deserialize)]
    struct PendingStart {
        zygo_session: String,
        started_at: u64,
    }
    ```
  - Move lines `:63-77` (path, open, handle check, bounded read, parse, version check) into `fn read_marker(paths: &Paths, ws_id: &str) -> Option<Marker>` with no change of logic (the caller has checked `valid_ws_id`).
  - Add `pub struct MemberRead` (Produces) and:
    ```rust
    /// The phase (H§7.6, AU25) and the run-time start (session run time design T§2.2) of the session of `view`, from one
    /// `workspace.json` read. A start of 0 counts as none.
    pub fn member_read(paths: &Paths, view: &SessionView) -> MemberRead
    /// The T§2.2 start of the session of `view`: `member_read(paths, view).start`.
    pub fn start(paths: &Paths, view: &SessionView) -> Option<u64>
    ```
    Rules of `member_read`, in order: `own = zygo_core::ids::ulid_secs(&view.session).filter(|&s| s != 0)`. No `view.workspace`: `MemberRead { phase: None, start: own }`, and no file is read. `ws_id` fails `valid_ws_id`, or `read_marker` gives `None`: `MemberRead::default()`. Else `phase = Some(marker.phase)`, and `start` from `marker.members`: `Null` → `own`; an object without the key `view.workspace.member.pane` → `own`; an object whose value for the pane does not decode into `Record` → `None`; any other `members` value → `None`. For a decoded record: when neither `pending.zygo_session` nor `zygo_session` equals `view.session` → `own`; else `first_started_at` when not 0; else `pending.started_at` when `pending.zygo_session == view.session` and it is not 0; else `own`.
  - `read` becomes: the `ZYGO_SESSION_DIR` line (`:58`), then `member_read(paths, &SessionView::load(&SessionDir::new(session_dir)).ok()?).phase`. Its doc keeps its text.
- [ ] 12. Run `cargo test --locked -p zygo-hooks --test statusline_phase`. The 6 `start_rule_*` tests and every existing test in the file (`ws_id_rule_equals_the_w1_rule` to `h14_36_phase_colors_and_hidden_cases`) pass; the two `#[ignore = "timing: …"]` tests stay ignored.
- [ ] 13. In `crates/zygo/src/cmd/status.rs`, append to `mod tests` (after the existing `use` lines, add `use zygo_core::session_dir::SessionDir;` and `use zygo_core::session_view::{Member, SessionView, ViewWorkspace};`):
  ```rust
  // ---- the `runtime:` line (session run time design T§2.5, T§7 items 3 and 8) ----

  /// A ULID whose time is 1_800_000_000 s.
  const SID: &str = "01MCC5RM000000000000000000";
  const START: u64 = 1_800_000_000;

  fn save_view(dir: &Path, view: SessionView) {
      view.save(&SessionDir::new(dir.to_path_buf())).unwrap();
  }

  /// The line right after the first line that starts with `prefix`.
  fn after<'a>(out: &'a str, prefix: &str) -> Option<&'a str> {
      let mut it = out.lines().skip_while(|l| !l.starts_with(prefix));
      it.next()?;
      it.next()
  }

  #[test] // T§7 item 8: right after `session:`, with and without C7
  fn runtime_line_follows_the_session_line() {
      let d = TempDir::new().unwrap();
      let dir = canned_session(d.path(), SID, 200, reply(SID, serde_json::json!({})));
      save_view(&dir, SessionView { session: SID.into(), ..Default::default() });
      for hide in [false, true] {
          let out = run(&ctx(d.path(), Some(&dir)), None, hide, START + 187_200).unwrap();
          assert_eq!(after(&out, "session: "), Some("runtime: 2d04h"), "{out}");
          assert_eq!(out.lines().filter(|l| l.starts_with("runtime:")).count(), 1, "{out}");
      }
  }

  #[test] // T§2.5: the start is local, so the line also follows `session: unreachable (…)`
  fn runtime_line_follows_an_unreachable_session() {
      let d = TempDir::new().unwrap();
      let dir = dead_session(d.path(), SID);
      save_view(&dir, SessionView { session: SID.into(), ..Default::default() });
      let out = run(&ctx(d.path(), Some(&dir)), None, false, START + 187_200).unwrap();
      assert!(out.lines().any(|l| l.starts_with("session: unreachable (")), "{out}");
      assert_eq!(after(&out, "session: unreachable ("), Some("runtime: 2d04h"), "{out}");
  }

  #[test] // T§7 item 3: start > now gives `0m`
  fn runtime_line_clock_behind_start_is_0m() {
      let d = TempDir::new().unwrap();
      let dir = dead_session(d.path(), SID);
      save_view(&dir, SessionView { session: SID.into(), ..Default::default() });
      let out = run(&ctx(d.path(), Some(&dir)), None, false, START - 1).unwrap();
      assert_eq!(after(&out, "session: unreachable ("), Some("runtime: 0m"), "{out}");
  }

  #[test] // T§2.5: no line outside a session, and none with no start (T§2.2)
  fn runtime_line_absent_outside_a_session_or_with_no_start() {
      let d = TempDir::new().unwrap();
      let out = run(&ctx(d.path(), None), None, false, START + 187_200).unwrap();
      assert!(!out.contains("runtime:"), "{out}");
      // No `view.json`; then a `session` that is not a ULID; then a member whose `workspace.json` is missing.
      let dir = dead_session(d.path(), "S1");
      let out = run(&ctx(d.path(), Some(&dir)), None, false, START + 187_200).unwrap();
      assert!(!out.contains("runtime:"), "{out}");
      save_view(&dir, SessionView { session: "S1".into(), ..Default::default() });
      let out = run(&ctx(d.path(), Some(&dir)), None, false, START + 187_200).unwrap();
      assert!(!out.contains("runtime:"), "{out}");
      let member = Member { ws_id: "0123456789abcdef".into(), pane: "kabcd2345".into(), ..Default::default() };
      let workspace = Some(ViewWorkspace { member, resume_note: None, ..Default::default() });
      save_view(&dir, SessionView { session: SID.into(), workspace, ..Default::default() });
      let out = run(&ctx(d.path(), Some(&dir)), None, false, START + 187_200).unwrap();
      assert!(!out.contains("runtime:"), "{out}");
  }

  #[test] // T§2.1: a member's line counts from `first_started_at`, through the one T§2.2 function
  fn runtime_line_of_a_member_uses_first_started_at() {
      let d = TempDir::new().unwrap();
      let dir = dead_session(d.path(), SID);
      let member = Member { ws_id: "0123456789abcdef".into(), pane: "kabcd2345".into(), ..Default::default() };
      let workspace = Some(ViewWorkspace { member, resume_note: None, ..Default::default() });
      save_view(&dir, SessionView { session: SID.into(), workspace, ..Default::default() });
      let ws = crate::cmd::compact::tests::paths(d.path()).workspaces_dir().join("0123456789abcdef");
      std::fs::create_dir_all(&ws).unwrap();
      let body = serde_json::json!({ "version": 1, "phase": "up", "members": { "kabcd2345": { "zygo_session": SID, "first_started_at": START - 86_400 } } });
      std::fs::write(ws.join("workspace.json"), body.to_string()).unwrap();
      let out = run(&ctx(d.path(), Some(&dir)), None, false, START + 187_200).unwrap();
      assert_eq!(after(&out, "session: unreachable ("), Some("runtime: 3d04h"), "{out}");
  }
  ```
  In the existing tests of the module, each call `run(a, b, c)` gets the argument `0` as `now` (`run(a, b, c, 0)`); no assertion and no other line changes. Their session dirs hold no `view.json`, so they get no `runtime:` line.
- [ ] 14. Run `cargo test --locked -p zygo --lib runtime_line` (`cmd` is in the lib target, `crates/zygo/src/lib.rs:1`). It fails to compile: `run` takes 3 arguments.
- [ ] 15. Change `crates/zygo/src/cmd/status.rs`:
  - Imports: `use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};`, `use zygo_core::session_view::SessionView;`, `use zygo_hooks::statusline::phase;`, and `run_time_text` added to the existing `use zygo_hooks::statusline::render::spend_text;` line.
  - Signature: `pub fn run(ctx: &VerbCtx, repo: Option<&Path>, hide: bool, now: u64) -> anyhow::Result<String>`.
  - Before the `match session_status(ctx)`: 
    ```rust
    // T§2.5: inside a session only; the T§2.2 start is local, so it needs no proxy; no start gives no line.
    let runtime = ctx
        .session_dir
        .as_ref()
        .and_then(|d| SessionView::load(d).ok())
        .and_then(|v| phase::start(&ctx.paths, &v))
        .map(|start| format!("runtime: {}", run_time_text(now.saturating_sub(start))));
    ```
  - In the `Some(Ok(s))` arm, `session.extend(runtime);` right after the `session: …` push and before the `if hide` block (C7 does not change the line). In the `outside` arm, `session.extend(runtime);` right after the `session: unreachable (…)` push, inside the `if let Some(Err(e))` block.
  - `verb`: `run(ctx, repo_top().as_deref(), hide_window(ctx), SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))`.
- [ ] 16. Run `cargo test --locked -p zygo --lib status::tests`. The 5 `runtime_line_*` tests and the existing `outside_a_session`, `inside_a_session`, `h14_18_trimmed_in_tokens_and_the_spend_line`, `spend_line_forms`, `unreachable_session` and `server_text_cannot_forge_a_status_line` pass.
- [ ] 17. In `docs/specs/2026-11-18-zygo-design.md`, rewrite the §17 `zygo status` row (`:821`) as: ``| `zygo status` | Mode, server, profile, work item, repo policy, session status (inside a session: context use, override, savings in tokens, the `spend:` line of §10.7, and the `runtime:` line (session run time; P3-13); a status reply with no `spend_usd`, from an older server, gives no `spend:` line). *(2026-11-22, HUD design)* *(2026-11-24, session run time design T§3.3)* |``, and append to the change table after the last row (`:1119`): ``| 2026-11-24 | §17 (`zygo status` row) | operator (session run time design T§2.5, T§3.3, approved 2026-11-24); node RT-2 | Inside a session, `zygo status` prints the `runtime:` line right after `session:`. |``. Check: `grep -c 'the \`runtime:\` line (session run time; P3-13)' docs/specs/2026-11-18-zygo-design.md` prints `1`.
- [ ] 18. Run the acceptance check of this node (the four `--list` counts, then `cargo test --locked -p zygo-core -p zygo-hooks -p zygo`). Counts: `ulid_secs` 2, `run_time_format` 1, `start_rule` 6, `runtime_line` 5; every test passes. Then `cargo build --workspace --locked`.
- [ ] 19. Commit `crates/zygo-core/src/ids.rs`, `crates/zygo-hooks/src/statusline/phase.rs`, `crates/zygo-hooks/tests/statusline_phase.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo/src/cmd/status.rs` and `docs/specs/2026-11-18-zygo-design.md` with the message `RT-2: start rule, run-time form and the runtime: line` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:RT-2`, each on its own line; no attribution line.

Review focus, each with its test: graph row 61 (record of another session → own ULID start) — `start_rule_own_start_for_another_session_no_record_and_an_old_record`; graph row 62 (read-check failure → no start, no ULID fallback) — `start_rule_no_start_when_workspace_json_fails_a_read_check`; graph row 63 (`u64::MAX`, unit floors, two-digit second unit) — `run_time_format_t7_item_1`; graph row 64 (placement after `session:` in the `Ok`, `unreachable` and `hide` forms) — `runtime_line_follows_the_session_line`, `runtime_line_follows_an_unreachable_session`.
