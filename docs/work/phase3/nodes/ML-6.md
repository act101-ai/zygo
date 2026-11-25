# ML-6 — Wake of an idle member after a send

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§7.3, M§9 (last bullet), M§10 rows "tmux unreachable at wake" and "Pane reused by another program"; `docs/specs/2026-11-20-workspaces-design.md` AU26, AU63, AU74, AU84
**Files:** `crates/zygo-workspace/src/mail/mod.rs`, `crates/zygo-workspace/src/mail/wake.rs`, `crates/zygo-workspace/tests/mail_wake.rs`, `crates/zygo-workspace/src/state.rs`
**Consumes:** `send_keys_literal` (ML-5), `send_enter` (ML-5), `Target` (ML-4), `mail_dir` (ML-2), `MailConfig` (ML-1), `parse_duration` (ML-1)
**Produces:** `zygo_workspace::mail::wake::WAKE_TEXT: &str` (`zygo: you have mail; read it with mail_read`), `zygo_workspace::mail::wake::Outcome` (enum: `Woken`, `Busy`, `NotLive`, `Mismatch`, `Gap`, `NoPane`, `Failed(String)`), `zygo_workspace::mail::wake::Probe` (trait: `fn idle(&self, session_dir: &Path) -> bool; fn now(&self) -> SystemTime`), `zygo_workspace::mail::wake::RealProbe`, `zygo_workspace::mail::wake::wake(store: &Store, ws: &Workspace, mux: &dyn Multiplexer, key: &str, gap: Duration, probe: &dyn Probe) -> Outcome`, `zygo_workspace::mail::wake::after_send(paths: &Paths, ws: &Workspace, tmux: &Path, keys: &[String], cfg: &MailConfig)`
**Acceptance:** `cargo test -p zygo-workspace --test mail_wake -- --include-ignored` → all pass (tmux cases run on tmux 3.2 or later, else print the skip reason; `cfg(unix)`): with a fake multiplexer and an injected probe and clock, an idle live member gets exactly two calls (literal `WAKE_TEXT`, then Enter); a busy member, a member whose launcher is not live, a pane whose `@zygo-pane` differs from the key (M§13 #6) and a session whose `@zygo-ws` differs get no call; a second wake 29 s after the first is `Gap` and one at 30 s wakes (M§13 #5, gap part); `wake.json` is written under the mail lock; a tmux error gives one `errors.log` line with no message text and leaves the file in `new/`; no call writes `activity.json`; on a private tmux server the wake text reaches the pane of a stub program

## Plan

Unit: phase3:ML-6:plan

**Premise check** (live code at `064e23f2`). ML-1, ML-2, ML-4, ML-5 and ML-17 are in `phase-3`; PM2 is `verified` (`docs/premises.md:56`), so no fallback node is needed.

**Premise drift.**
- D1. `wake` takes `store: &Store`, and `Probe::idle` takes the member's session folder. `Store` keeps `sessions_dir` private (`crates/zygo-workspace/src/state.rs:444-449`) and has no getter. The live check of one member exists only as a private fn in `down.rs:822` (`live_session(paths, m)`). `Store::live_launcher` (`state.rs:634`) returns the first live member and not its folder. So `wake` cannot find the folder with the contract's signature. Fix (step 2): add one method, `Store::live_session`, to `crates/zygo-workspace/src/state.rs`. That file is not in Files. **The controller must add it to Files** (or change `wake` to take `&Paths`) before this node runs.
- D2. M§7.3 step 1 reads `@zygo-pane` "of the target pane", but zygo stores no tmux pane id (`%N`) for a member. `Pane` and `MemberRecord` (`state.rs:83-155`) and the session view have no pane id. Plan: find the pane by its `@zygo-pane` with `list_panes` (`crates/zygo-workspace/src/mux/mod.rs:389`, `PaneRow.key`, `:546`). Then read `@zygo-pane` again with `show_pane_option` (`:391`) just before `send-keys` (this is the M§7.3 step 1 read). `Mismatch` is the result when that read differs, or when two panes have the key. `NoPane` is the result when no pane has the key.
- D3 (no effect). `MailConfig` now also has `host`, `sync_gap` and `workspace` (`crates/zygo-config/src/schema.rs:692-701`). ML-6 reads only `wake_gap`.
- D4 (no effect). The contract consumes `Target` (ML-4), but no step needs it: `after_send` takes keys, and callers get them from `SendReport.sent[].key` (`mail/mod.rs:116-119`).

**Interfaces (live).**
- Consumed:
  - `Multiplexer::send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError>` (`mux/mod.rs:412`). It refuses a pane that is not `%<digits>`, and a text with a control character, before any spawn.
  - `Multiplexer::send_enter(&self, pane: &str) -> Result<(), MuxError>` (`:415`).
  - Also used: `show_session_option(&self, session: &str, name: &str) -> Result<Option<String>, MuxError>` (`:362`, gives `None` when the option is unset or the session is missing); `list_panes(&self, session: &str) -> Result<Vec<String>, MuxError>` (`:389`); `show_pane_option(&self, pane: &str, name: &str) -> Result<Option<String>, MuxError>` (`:391`); `parse_pane_line(line: &str) -> Result<(WindowRow, PaneRow), ParseError>` (`:700`).
  - `mail::names::Target { key: String, name: Option<String> }` (`mail/names.rs:10`): not used (D4).
  - `mail::mail_dir(paths: &Paths, ws_id: &str) -> PathBuf` (`mail/mod.rs:105`). It is `Store::open(paths, ws_id).dir().join("mail")`.
  - `zygo_config::MailConfig { keep: String, max_unread: u32, wake_gap: String, host, sync_gap, workspace }` (`schema.rs:692`, default `wake_gap = "30s"`).
  - `zygo_config::parse_duration(s: &str) -> Option<Duration>` (`schema.rs:718`, forms `<n>s|m|h|d`, `None` on overflow).
  - `state::Store` (`state.rs:444`): `open(paths: &Paths, ws_id: &str) -> Store` (`:453`), `dir(&self) -> &Path` (`:458`), `log_error(&self, line: &str)` (`:649`, adds `<unix secs> <line>`), and `pub(crate) lock_file(path: &Path, bound: Duration) -> io::Result<Option<File>>` (`:814`; with `Duration::ZERO` it tries once).
  - `state::Workspace { id, tmux_socket: Option<PathBuf>, tmux_session: String, members: BTreeMap<String, MemberRecord>, .. }` (`:26`). `MemberRecord { zygo_session: Option<String>, pending: Option<Pending { zygo_session: String, .. }>, launch: Launch { events_off: bool, .. }, .. }` (`:116`, `:159`, `:173`).
  - `mux::tmux::Tmux::new(program: PathBuf, socket: Option<PathBuf>) -> Tmux` (`mux/tmux.rs:205`).
  - `zygo_core::paths::Paths` (`crates/zygo-core/src/paths.rs:5`).
  - For `RealProbe`: `activity::read(dir: &SessionDir) -> anyhow::Result<Activity>` (`activity.rs:198`, takes no lock); `drain::idle(activity, status, now) -> Idle` (`drain.rs:111`); `drain::hooks_off(dir: &SessionDir) -> bool` (`:26`); `zygo_proxy::client::SessionClient::from_session_dir(dir)` with `.timeout(d)` and `.status()` (`crates/zygo-proxy/src/client.rs:153`, `:180`, `:201`).
  - Also: `zygo_core::session_dir::{live, write_atomic}` (`session_dir.rs:13`, `:141`; `write_atomic` writes mode `0600`); `state::open_nofollow` (`state.rs:711`); `zygo_core::esc`.
- Produced (all in `crates/zygo-workspace/src/mail/wake.rs`):
  - `pub const WAKE_TEXT: &str = "zygo: you have mail; read it with mail_read";`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum Outcome { Woken, Busy, NotLive, Mismatch, Gap, NoPane, Failed(String) }`
  - `pub trait Probe { fn idle(&self, session_dir: &Path) -> bool; fn now(&self) -> SystemTime; }`
  - `pub struct RealProbe;`
  - `pub fn wake(store: &Store, ws: &Workspace, mux: &dyn Multiplexer, key: &str, gap: Duration, probe: &dyn Probe) -> Outcome`
  - `pub fn after_send(paths: &Paths, ws: &Workspace, tmux: &Path, keys: &[String], cfg: &MailConfig)`
  - Addition (review fix R4): `pub fn after_send_with<P: Probe + Sync, M: Multiplexer + Sync>(paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, mux: &M, probe: &P, now: SystemTime)`. `probe` gives the idle rule and `now` the clock (the clock of `probe` is not used). `after_send` calls it with `Tmux`, `RealProbe` and `SystemTime::now()`. For MS-13, which tests `after_send` with the injected clock and probe (`docs/work/phase3/nodes/MS-13.md:5,7`).
  - Their consumers are ML-11, ML-12, ML-18 and MS-13 (`after_send`). Each uses it by name only, so this signature matches them.

**Existing code.** No code waits for, records or types a wake: `grep -rn -e wake.json -e WAKE_TEXT -e woken_at crates` finds nothing. Reused as they are: the AU26 rule (`drain::idle`, `hooks_off`), `Store::log_error`, `lock_file`, `write_atomic`, and the D14 live-folder rule (step 2 copies it into `Store`).

**Author's decisions** (the spec leaves them open; each adds no gate):
- A1. "The mail lock" is not defined in M§5. Plan: an fs4 lock on the file `mail/<key>/wake.lock`, taken with `lock_file(.., Duration::MAX)`, which waits until the lock is free (review fix R1). The waiter then reads `wake.json` and applies the `wake_gap` rule: a holder that woke the pane wrote `woken_at`, so the waiter gets `Gap` from that rule; a holder that failed before it wrote `woken_at` wrote nothing, so the waiter tries the wake. A held lock is not a skip rule of its own. The hold is bounded by the existing tmux call bounds; there is no new timeout. The lock covers the steps from the read of `wake.json`, through the tmux checks and the write of `wake.json`, to the two sends.
- A2. `wake.json` is `{"woken_at":"<RFC 3339, UTC>"}` (the form of M§5 `sent_at`), written as compact JSON (`serde_json::to_vec`) with `write_atomic`. A file that is missing, cannot be read, is a link, does not parse or is not RFC 3339 is read as "no earlier wake", and the next wake writes a new file.
- A3. "Within `wake_gap`" means the distance between `now` and `woken_at` in either direction is less than `gap`. A `woken_at` after `now` (a clock that went back) therefore blocks only for less than `gap`. The check only compares durations, so a very large `wake_gap` cannot overflow (ML-1 review note 6).
- A4. `woken_at` is written under the lock after the pane checks pass and before the `send-keys -l` call (review fix R6). A wake whose text or Enter send then fails still counts for `wake_gap`, so the text is never typed twice inside the gap; the failure still writes its `errors.log` line and gives `Failed`.
- A5. A member with `launch.events_off`, or whose session `settings.json` turns hooks off, is `Busy` (AU26 FIX-24). A key that is not in `ws.members` is `NotLive`.
- A6. Only `Failed` writes a line to `errors.log`: `mail wake <key>: <error>`. The error text comes from tmux or the OS. No mail text goes into a tmux argument (M-C5), so the line cannot hold mail text. If `wake.json` cannot be written after a wake, the result is still `Woken`, with one line `mail wake <key>: wake.json not written: <error>`.
- A7. `after_send` reads a `wake_gap` that does not parse as `30s` (the M§11 default) and writes one line: `mail wake: [mail] wake_gap "<esc(text)>" is not <n>s, <n>m, <n>h or <n>d; using 30s`. This follows the `drain_timeout` fallback (`down.rs:461-464`). The proxy status bound is 2 s, the drain's bound for the same AU26 read (`down.rs:89`).

**Steps**

- [x] **1. Write the failing tests.** Create `crates/zygo-workspace/tests/mail_wake.rs` with `#![cfg(unix)]` and the module doc `//! ML-6: the wake of an idle member (M§7.3, M§9, M§10, M§13 #5 #6).`. Fixture:
  ```rust
  const ID: &str = "0123456789abcdef"; const ELENA: &str = "kelena222"; const OTHER: &str = "kother222";
  const T0: u64 = 1_700_000_000; // 2023-11-14T22:13:20Z
  const GAP: Duration = Duration::from_secs(30);
  fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
  fn paths(p: &Path) -> Paths                          // copy tests/mail_store.rs:30-34
  fn hold(path: PathBuf) -> fs::File                   // copy tests/admin.rs:149-154
  fn live(p: &Paths, sid: &str) -> fs::File            // copy tests/admin.rs:191-196 (token file + held pid lock)
  fn until(f: impl Fn() -> bool) -> bool               // copy tests/mux.rs:1495-1504
  fn ws() -> Workspace // identity fields as tests/mail_names.rs:32-35; members[ELENA]: MemberRecord::new(Agent::ClaudeCode),
                       // name "elena", zygo_session Some("s1")
  // setup: TempDir, paths, Store::open(&p, ID), ws(), live(&p, "s1"), and one message to ELENA with subject
  // "subj-7f3a" and body "body-9c1e" delivered with mail::store::deliver(&mail_dir(&p, ID), ELENA, "elena", &m, 500)
  struct At { idle: bool, now: SystemTime }            // impl Probe: returns the fields
  fn row(id: &str, key: &str) -> String { format!("0|w|1|l|80|24|1|0|{id}|0|1|{key}|/srv/app") }
  struct Fake { ws_opt: Result<Option<String>, MuxError>, panes: Result<Vec<String>, MuxError>,
      pane_opt: Result<Option<String>, MuxError>, keys: Result<(), MuxError>, enter: Result<(), MuxError>,
      calls: RefCell<Vec<String>>, on_send: Option<Box<dyn Fn()>> }
  // Fake::good(): Ok(Some(ID)), Ok(vec![row("%3", ELENA), row("%4", OTHER)]), Ok(Some(ELENA)), Ok(()), Ok(()).
  // impl Multiplexer: the five methods that wake uses record "<method> <args joined by ' '>" and return their field
  // (send_keys_literal runs on_send first); every other method panics "not used by wake".
  fn errors(s: &Store) -> Vec<String>                 // lines of errors.log; empty when it is missing
  fn wake_json(s: &Store) -> Option<String>           // mail/ELENA/wake.json as text
  fn new_count(p: &Paths) -> usize                     // mail::store::unread(&mail_dir(p, ID), ELENA).unwrap()
  ```
  Tests (each asserts `errors(&store).is_empty()` unless it says otherwise):
  - `wake_text_is_fixed`: `assert_eq!(WAKE_TEXT, "zygo: you have mail; read it with mail_read")`.
  - `idle_live_member_gets_the_text_then_enter`: `wake(&s, &w, &f, ELENA, GAP, &At { idle: true, now: at(T0) }) == Outcome::Woken`. `f.calls` is exactly `["show_session_option zygo-app @zygo-ws", "list_panes zygo-app", "show_pane_option %3 @zygo-pane", "send_keys_literal %3 zygo: you have mail; read it with mail_read", "send_enter %3"]`. `wake_json == Some(r#"{"woken_at":"2023-11-14T22:13:20Z"}"#)`, its mode is `0o600`, and `new_count == 1`.
  - `busy_member_gets_no_call`: `At { idle: false, .. }` gives `Busy`. Separately, `launch.events_off = true` with `idle: true` gives `Busy`. In both cases `calls` is empty and `wake_json` is `None`.
  - `member_whose_launcher_is_not_live_gets_no_call`: drop the `live` guard, then `NotLive`. `zygo_session = None` gives `NotLive`. Key `"kzzzzzzzz"` gives `NotLive`. In each case `calls` is empty.
  - `pane_whose_zygo_pane_differs_is_refused` (M§13 #6): `pane_opt = Ok(Some(OTHER))` gives `Mismatch`, and `Ok(None)` gives `Mismatch`. `panes = [row("%3", ELENA), row("%5", ELENA)]` gives `Mismatch`. In each case no `calls` entry starts with `send_`, and `wake_json` is `None`.
  - `session_whose_zygo_ws_differs_is_refused`: `ws_opt = Ok(Some("fedcba9876543210".into()))` gives `Mismatch`, and `Ok(None)` gives `Mismatch`. In both cases `calls == ["show_session_option zygo-app @zygo-ws"]`.
  - `no_pane_with_the_key`: `panes = [row("%4", OTHER)]` gives `NoPane`, and no `send_` call.
  - `second_wake_inside_the_gap_is_skipped` (M§13 #5, gap part): one `Fake`. At `T0` the result is `Woken`. At `T0 + 29` it is `Gap`, and there are still 2 `send_` calls. At `T0 + 30` it is `Woken`, with 4 `send_` calls, and `wake_json` holds `"2023-11-14T22:13:50Z"`. Then write `wake.json` by hand: `woken_at` `T0 + 5` with `now = T0` gives `Gap`; `T0 + 86400` with `now = T0` gives `Woken`; the text `x` gives `Woken`. With `gap = parse_duration("213503982334601d").unwrap()`: `T0` gives `Woken`, then `T0 + 1` gives `Gap`, and nothing panics.
  - `wake_json_is_written_under_the_mail_lock` (review focus 5): `on_send` opens `mail/ELENA/wake.lock` and asserts that `fs4::FileExt::try_lock` gives `Err(TryLockError::WouldBlock)`, then sets a flag. The result is `Woken` and the flag is set. Next, the test holds `hold(mail/ELENA/wake.lock)` itself and calls at `T0 + 60`: the result is `Gap`, `calls` gains nothing, and `wake.json` is not changed.
  - `tmux_error_logs_one_line_and_keeps_the_message` (M§10): `ws_opt = Err(MuxError::Gone { message: "no server running on /tmp/zygo-x/s".into() })` gives `Failed("no server running on /tmp/zygo-x/s".into())`. `errors` has exactly 1 line, which ends with ` mail wake kelena222: no server running on /tmp/zygo-x/s`. No line holds `subj-7f3a` or `body-9c1e`. `new_count == 1` and `wake_json` is `None`. Separately, `enter = Err(Failed { message: "boom" })` gives `Failed("boom")`, adds 1 more line, and `wake_json` is still `None`.
  - `wake_writes_no_activity_json`: write `cache/sessions/s1/activity.json` = `{"version":1,"busy":false}`. Run a `Woken` wake with `At`, then `RealProbe.idle(&p.sessions_dir.join("s1"))`. `RealProbe` gives `false`, because there is no proxy url, so the proxy status is not read (AU26). The file bytes do not change, and `s1/activity.lock` does not exist.
  - `after_send_skips_busy_and_not_live_members`: use tmux path `/nonexistent/tmux`, plus a second member `kanon2222` with `zygo_session Some("s2")` that is not live. `after_send(&p, &w, Path::new("/nonexistent/tmux"), &[ELENA.into(), "kanon2222".into()], &MailConfig::default())` writes no `errors.log` and no `wake.json`: `RealProbe` finds ELENA busy, and `kanon2222` is not live, so neither causes a tmux call. With `wake_gap = "soon"`, `errors` is exactly 1 line, which ends with `mail wake: [mail] wake_gap "soon" is not <n>s, <n>m, <n>h or <n>d; using 30s`. With `keys = &[]`, nothing is logged.
  - `wake_text_reaches_a_stub_program_on_a_private_server`, with `#[ignore = "timing: real tmux under the fixed 2 s tmux bound"]`. Start with `let Some(_) = require_tmux() else { return };` (this prints `tmux tests skipped: <reason>`), then `PrivateServer::start()`. The stub is `sh -c "read -r l; printf '%s' \"$l\" > '<dir>/got'; sleep 600"`. Create it with `srv.tmux().new_session(&NewSession { name: "zygo-app", start_dir, width: 100, height: 30, env: &env, argv: &argv, owner: Some((ID, ELENA)) })`. Set `w.tmux_socket = Some(srv.socket().into())`. Then `wake(&s, &w, &srv.tmux(), ELENA, GAP, &At { idle: true, now: at(T0) }) == Woken`, `until(|| read_to_string(got) == WAKE_TEXT)`, and at `T0 + 29` the result is `Gap`.

  Check: `cargo test -p zygo-workspace --test mail_wake` fails to compile (no `mail::wake`, no `Store::live_session`).

  Deviation (step 1): in `second_wake_inside_the_gap_is_skipped` the test removes `wake.json` before the `213503982334601d` case (`crates/zygo-workspace/tests/mail_wake.rs:291`). The wake of the text `x` case just before writes `woken_at` = `T0`, so without the remove the first huge-gap wake at `T0` is `Gap`, not `Woken`. The intent (huge gap: wake, then `Gap`, no panic) is kept.

  Deviation (step 1, found in step 4): the tests of `mail_wake.rs` run one at a time. `setup()` holds the test-only mutex `FORKS` (`crates/zygo-workspace/tests/mail_wake.rs:74`), as `serial()` in `tests/admin.rs` and `forks()` in `tests/mux.rs` do. Cause: the tmux test forks, and a child that has not yet run `exec` keeps an inherited `flock` held, so a parallel wake saw `WouldBlock` on `wake.lock` and gave `Gap` (`second_wake_inside_the_gap_is_skipped`, 1 failure in 78 runs; 0 in 300 runs without the tmux test; 0 in 300 runs with the mutex). No product code changed for this.

  Note (review fix R1-R9): the fake multiplexer is `Sync` (`Mutex` for `calls`, a `Send + Sync` `on_send`), it records each `send_enter` with its time, and `pane_keys` makes `show_pane_option` give the key of the listed pane. The held-lock half of `wake_json_is_written_under_the_mail_lock` moved to `held_wake_lock_waits_then_follows_wake_json` (R1), and the Enter-error half of `tmux_error_logs_one_line_and_keeps_the_message` moved to `each_tmux_error_fails_with_one_line` (R6), because both now assert the opposite result. New tests: `held_wake_lock_waits_then_follows_wake_json` (R1), `pane_line_that_does_not_parse_fails` (R2), `slow_member_does_not_delay_another` (R3, `#[ignore = "timing: …"]`), `after_send_with_wakes_the_target_once` (R4), `invalid_key_builds_no_path` (R5), `each_tmux_error_fails_with_one_line` (R6), `gap_keeps_the_fraction_of_a_second` (R7), `live_session_prefers_pending_and_refuses_bad_ids` (R8), `missing_maildir_gives_not_live` (R9).

- [x] **2. `Store::live_session`** in `crates/zygo-workspace/src/state.rs` (D1), after `live_launcher`: `pub fn live_session(&self, m: &MemberRecord) -> Option<PathBuf>`. Doc: `The live session folder of a member (D14): sessions/<pending.zygo_session> when live, else sessions/<zygo_session> when live; an id that fails valid_session_id is not looked up.` It is the rule of `down.rs:822`, with `self.sessions_dir` and `valid_session_id`. `down.rs` does not change.

  Note (review fix R8): `live_session_prefers_pending_and_refuses_bad_ids` tests it: `pending` wins over `zygo_session` when both are live (as `down.rs:822`), and a live folder at `sessions/../x` is not found for the id `../x`. The code did not change.

- [x] **3. `wake.rs`.** Add `pub mod wake;` to `crates/zygo-workspace/src/mail/mod.rs`, and change its doc owner to `nodes ML-2, ML-4, ML-6`. Create `crates/zygo-workspace/src/mail/wake.rs` (doc: `//! The wake of an idle member after a send (M§7.3). Owner: node ML-6.`) with the Produced items and private consts `PANE_OPTION = "@zygo-pane"`, `WS_OPTION = "@zygo-ws"`, `STATUS_BOUND = 2 s`, `WAKE_FILE = "wake.json"`, `LOCK = "wake.lock"`.
  - `wake` runs these checks in this order. It returns at the first result:
    1. `ws.members.get(key)` is `None`, or `store.live_session(m)` is `None`: `NotLive`.
    2. `m.launch.events_off`, or `!probe.idle(&dir)`: `Busy`.
    3. `md = store.dir().join("mail").join(key)`. `lock_file(&md.join(LOCK), Duration::ZERO)`: `Ok(None)` gives `Gap`, and `Err(e)` gives `fail(e)`.
    4. `now = probe.now()`. If `wake.json` gives a `woken_at` within `gap` (A2, A3): `Gap`.
    5. `show_session_option(&ws.tmux_session, WS_OPTION)`: `Ok(Some(v)) if v == ws.id` continues; any other `Ok` gives `Mismatch`.
    6. `list_panes(&ws.tmux_session)`. Lines that `parse_pane_line` refuses are skipped. Rows with `key == key`: 0 rows give `NoPane`, 2 or more give `Mismatch`, and 1 row gives `id`.
    7. `show_pane_option(&id, PANE_OPTION)`: `Ok(Some(v)) if v == key` continues; any other `Ok` gives `Mismatch`.
    8. `send_keys_literal(&id, WAKE_TEXT)`, then `send_enter(&id)`.
    9. Write `wake.json` with `now` (A6 on error). Then `Woken`.

    Every tmux `Err(e)` in 5-8 gives `fail(e)`. `fail` writes `store.log_error(&format!("mail wake {key}: {e}"))` and returns `Failed(e.to_string())`. The lock is released when `wake` returns.
  - Body of the gap check (A3): `match now.duration_since(woken) { Ok(d) => d < gap, Err(e) => e.duration() < gap }`.
  - `impl Probe for RealProbe`:
    - `now` = `SystemTime::now()`.
    - `idle(dir)`: `false` when `drain::hooks_off(&SessionDir::new(dir))` is true. Else read `activity::read` and the status from `SessionClient::from_session_dir(dir)` with `.timeout(STATUS_BOUND)` and `.status()` (each error becomes its text). Then `matches!(drain::idle(..), Idle::Idle | Idle::IdleRuleSkipped)`.
  - `after_send`: return at once when `keys` is empty. Get `gap` from `parse_duration(&cfg.wake_gap)`, else 30 s with the A7 line. Set `store = Store::open(paths, &ws.id)` and `mux = Tmux::new(tmux.to_path_buf(), ws.tmux_socket.clone())`. Call `wake(&store, ws, &mux, key, gap, &RealProbe)` for each key, in order. The outcomes are dropped: `wake` already logs `Failed`.

  Notes (review fixes, changes to the order above):
  - Review fix R5 (item 1): the key must pass `valid_pane_key` (`identity.rs:58`, as `mail/store.rs:31`) before any path is built; else `NotLive`.
  - Review fix R9 (between items 1 and 2): when `mail/<key>/` is not a folder (for example after `forget-pane`), the result is `NotLive`, and no folder or file is created. `remove_pane`, `sweep` and `write_atomic` are not changed.
  - Review fix R1 (item 3): the lock is taken with `lock_file(.., Duration::MAX)`, which waits (A1). A held lock no longer gives `Gap`.
  - Review fix R2 (item 6): a line that `parse_pane_line` refuses gives `Failed("pane list line not read: <parse error>")` with one `errors.log` line, as a tmux error does.
  - Review fix R6 (items 8 and 9 change places): `wake.json` is written before `send_keys_literal` (A4); a write error keeps the A6 line and the wake goes on.
  - Review fix R7 (item 9): `woken_at` keeps the full precision of `now` in RFC 3339 (no `replace_nanosecond(0)`); a whole second still prints as `…:20Z`.
  - Review fix R3 (`after_send`): each key's wake runs on its own thread in `std::thread::scope`, and all are joined. A forked child that holds another key's inherited lock until its `exec` now makes a wake wait (R1), not skip.
  - Review fix R4 (`after_send`): the body moved to `after_send_with` (see Interfaces); `after_send` builds `Tmux` and calls it with `RealProbe` and `SystemTime::now()`. `after_send_with` reads the clock once, before the threads start.

- [x] **4. Run the tests.** Run `cargo test -p zygo-workspace --test mail_wake -- --include-ignored`. Pass: all tests pass. The tmux test runs on tmux 3.2 or later, or prints `tmux tests skipped: …`. Then run `cargo test -p zygo-workspace --locked --no-fail-fast` and `cargo clippy -p zygo-workspace --all-targets --locked -- -D warnings`. Pass: both are green, and `down`, `state`, `mail_store` and `mail_names` do not change.

  Deviation (step 4): `cargo clippy -p zygo-workspace --all-targets --locked -- -D warnings` fails on code outside Files that this node does not touch: `clippy::collapsible_if` in `crates/zygo-work/src/gate/transition.rs:158` (a dependency), and, with `--no-deps`, unused variables and a dead struct in `crates/zygo-workspace/tests/down.rs:419`, `:1674`, `:1688`, `:1703`. `cargo clippy -p zygo-workspace --lib --test mail_wake --locked --no-deps -- -D warnings` is clean. `cargo test -p zygo-workspace --locked --no-fail-fast` is green.

- [x] **5. Commit** `ML-6: wake of an idle member after a send`, with the trailers `Zygo-Session:` and `Zygo-Work: phase3:ML-6`.

**Spec coverage.**
- M§7.3, the idle rule and the live launcher: step 3 items 1-2 and `RealProbe`; tests `busy…`, `…not_live…`, `wake_writes_no_activity_json`.
- M§7.3 step 1: items 5-7; tests `pane_whose…`, `session_whose…`, `no_pane…`.
- M§7.3 step 2: item 8; tests `idle_live…` and the private-server test.
- M§7.3 step 3: items 3, 4 and 9; tests `second_wake…` and `…under_the_mail_lock`.
- M§9 last bullet (a wake is not a `UserPromptSubmit`): nothing writes `activity.json`; test `wake_writes_no_activity_json`.
- M§10 "tmux unreachable": `fail`; test `tmux_error…`.
- M§10 "Pane reused": item 7.
- M-C2 and M-C5: the error lines hold no mail text, and only `WAKE_TEXT` reaches `send-keys`.
- AU63: item 5. AU74: item 1. AU84: keys come from `ws.members`, which the store checks at read, and every `@zygo-*` value is compared for equality with a valid value.
- Review focus 5: the `pane_whose…`, `session_whose…`, `idle_live…` (exact `calls`), `second_wake…` and `…under_the_mail_lock` tests.
