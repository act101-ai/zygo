# ML-7 — Member name in the session view marker, hand start `--name`

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§6 (hand-started member, `--name <n>`), M§9a (Member name row: `name: Option<String>` on `Member`, `#[serde(default)]`); `docs/specs/2026-11-20-workspaces-design.md` AU9, AU58, AU102
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/claude.rs`, `crates/zygo/src/cmd/opencode.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/src/cmd/ws.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo/tests/workspace_member.rs`
**Consumes:** `MemberRecord.name` (ML-3), `MemberRecord::valid_name` (ML-3), `valid_member_name` (ML-1)
**Produces:** `zygo_core::session_view::Member.name: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `cmd::member::MemberCtx.name: Option<String>`, `cmd::Launch.name: Option<String>` and `cmd::LaunchOpenCode.name: Option<String>` (`--name <n>`)
**Acceptance:** `test "$(cargo test -q -p zygo-core --test core_basics -- view_member_name --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo --test workspace_member -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-core --test core_basics && cargo test -p zygo --lib && cargo test -p zygo --test workspace_member -- --include-ignored` → every count holds (at least 1 named `view_member_name…` in zygo-core `core_basics`; at least 4 named `member_name…` in zygo `workspace_member`; a count of 0 fails before any run), then all pass (tmux cases run or print the skip reason; `cfg(unix)`): a view without `name` loads; `zygo ws run` of a named record writes `workspace.member.name` into `view.json`; a hand start with `--name elena` writes the name to the record and the view; a `--name` that fails the grammar, `all`, `cli`, or a name another member of the workspace holds exits 1 with one line and starts nothing; `--name` outside a workspace pane exits 1 with one line

## Plan

Premise check (live code at origin/phase-3 6c4abfdb):
- `zygo_core::session_view::Member` is at `crates/zygo-core/src/session_view.rs:133-140` (`ws_id`, `pane`, flattened `extra: UnknownFields`; derives `Default`). Every literal in the workspace ends with `..Default::default()` (for example `crates/zygo/src/cmd/member.rs:64`), so a new field breaks no caller. The launcher copies `opts.member` into `view.json` unchanged (`crates/zygo-launch/src/launcher.rs:585`, `:609`): a name on `LaunchOpts.member` reaches the view with no launcher change.
- `MemberCtx` (`crates/zygo/src/cmd/member.rs:44-50`) has no name; `run_wrapped` builds the marker at `member.rs:64`. Its two literals: `member.rs:938` (hand start) and `crates/zygo/src/cmd/ws.rs:256` (`ws run`).
- `cmd::Launch` (`crates/zygo/src/cmd/mod.rs:209-231`) and `cmd::LaunchOpenCode` (`mod.rs:234-245`) have no `--name`. Their other literals: `ws::launch_of` (`ws.rs:420-431`), `ws::launch_open_code_of` (`ws.rs:434-443`), the test helpers `claude.rs:93-103` and `opencode.rs:64-72`.
- The hand start: `launch_hand` (`member.rs:945-962`) calls `hand_start` (`member.rs:855-939`), which assigns the pane key (`:924`), runs `conflict` (`:932-934`) and returns `HandStart::Refused` (exit 1, nothing started). The locked write is `Callback::hand_write` (`member.rs:281-303`) over `join` (`member.rs:797-833`); a refusal there is `NotMember` (v1 launch). `StopLaunch` (`crates/zygo-launch/src/launcher.rs:106`, `:453-455`) makes the launcher exit 1 with no agent; a hand start whose callback wrote nothing prints no exit lines (`member.rs:150`).
- `join` keeps the record only for the accepted resume (`member.rs:814-815`); otherwise it removes it and makes `MemberRecord::new` (name none).
- No function finds a member name held by another key; `Pane::valid_name` (`crates/zygo-workspace/src/state.rs:109`) and `MemberRecord::valid_name` (`state.rs:344`) are the per-entry AU84 reads.
- Carried item (graph Log, DR-4 row): the top-level help line `Decision records: ls, show, deps, check` (`mod.rs:361`) omits `new`; ML-7 is the first of ML-7/ML-12/MS-17/WD-14 and edits `mod.rs`, so it fixes it here.
- The node has no Review focus line; no review-focus test is owed.

Author's calls (the spec does not decide them):
- Check order of a hand start with `--name <n>`: grammar (`valid_member_name`) first, before any tmux call or write; then the W§8.4 checks; then `conflict`; then the name holder. A hand start that ends as a v1 launch (any `HandStart::V1`, also after a not-saved line) with `--name` exits 1 and starts nothing.
- The name holder is another key's `MemberRecord::valid_name` or `Pane::valid_name` equal to `n` (AU84 at each read). The callback checks it again under `lock`; there a holder is `StopLaunch` (exit 1, no agent), not `NotMember`, because `--name` asks for a named member or nothing.
- `join` is unchanged in signature; `hand_write` sets `rec.name` after `join` only when `--name` is given. So a replaced record gets the given name or none (M§6 "else it has no name"), and the accepted resume keeps its saved name unless `--name` replaces it, as it keeps its saved launch. The view's name is the same value: `--name`, else the kept record's `valid_name`.
- `ws run` writes `rec.valid_name()` into the marker; a stored name that fails the grammar gives no name.
- Lines (exit 1, one line each): `zygo: bad --name value `<n>`: use ^[a-z][a-z0-9-]{0,31}$, not all, cli, zygo or a pane key` (`<n>` through `zygo_core::esc`); `zygo: --name needs a pane of a workspace; nothing started`; `zygo: name <n> belongs to pane <key>; nothing started`.

Interfaces

Consumes (exact, as in the code now):
- `zygo_workspace::state::MemberRecord.name: Option<String>` (`state.rs:128-130`, `#[serde(default, deserialize_with = "lenient_name", skip_serializing_if = "Option::is_none")]`) (ML-3)
- `zygo_workspace::state::MemberRecord::valid_name(&self) -> Option<&str>` (`state.rs:344`) (ML-3)
- `zygo_core::ids::valid_member_name(s: &str) -> bool` (`crates/zygo-core/src/ids.rs:29`) (ML-1)
- Existing code used: `zygo_workspace::state::Pane::valid_name(&self) -> Option<&str>` (`state.rs:109`); `zygo_launch::launcher::StopLaunch(pub String)` (`launcher.rs:106`); `zygo_core::esc`.

Produces (exact):
- `zygo_core::session_view::Member.name: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`, placed before `extra`
- `cmd::member::MemberCtx.name: Option<String>` (the marker name)
- `cmd::Launch.name: Option<String>` and `cmd::LaunchOpenCode.name: Option<String>` (`#[arg(long)]`, `--name <n>`)
- Support (no node consumes it): `cmd::member::HandJoin.name: Option<String>`; `pub(crate) fn name_holder(ws: &Workspace, key: &str, name: &str) -> Option<String>`; `pub(crate) fn kept<'a>(ws: &'a Workspace, key: &str, kind: Agent, own: Option<&str>) -> Option<&'a MemberRecord>`; `enum HandErr { NotSaved(String), Stop(String) }`; `launch_hand(.., launch, name: Option<String>) -> i32`; `hand_start(launch, name: Option<String>, env, paths, tmux) -> HandStart`

Steps:

- [x] 1. In `crates/zygo-core/tests/core_basics.rs`, after `session_view_workspace_round_trips_and_is_none_for_an_old_file`, add:
  ```rust
  #[test] // M§9a: `name` on the member marker; a marker without it still loads
  fn session_view_member_name_is_optional_and_round_trips() {
      use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
      let t = tempfile::tempdir().unwrap();
      let d = SessionDir::new(t.path().to_path_buf());
      let old: Member = serde_json::from_value(serde_json::json!({"ws_id": "w1", "pane": "p1"})).unwrap();
      assert_eq!(old.name, None);
      assert_eq!(serde_json::to_value(&old).unwrap(), serde_json::json!({"ws_id": "w1", "pane": "p1"}));
      let named = Member { name: Some("elena".into()), ..old };
      let v = SessionView { workspace: Some(ViewWorkspace { member: named.clone(), ..Default::default() }), ..Default::default() };
      v.save(&d).unwrap();
      assert_eq!(SessionView::load(&d).unwrap().workspace.unwrap().member, named);
      let json: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
      assert_eq!(json["workspace"]["member"], serde_json::json!({"ws_id": "w1", "pane": "p1", "name": "elena"}));
  }
  ```
- [x] 2. Run `cargo test -p zygo-core --test core_basics -- view_member_name`. Check: E0609/E0560, no field `name` on `Member`.
- [x] 3. In `session_view.rs:135-140`, add to `Member` before `extra`: `/// The member name (M§6, M§9a), written by the launcher; none for an unnamed member and in an older view.` `#[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,`
- [x] 4. Run `cargo test -p zygo-core --test core_basics`. Check: all pass, `session_view_member_name_is_optional_and_round_trips` among them.
- [x] 5. Commit `crates/zygo-core/src/session_view.rs` and `crates/zygo-core/tests/core_basics.rs`, message `ML-7: member name in the session view marker` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:ML-7:implement`, each on its own line. Check: `git show --stat HEAD` lists those two files.
- [x] 6. In the `tests` module of `crates/zygo/src/cmd/mod.rs` (`:604`), add (import `super::Cmd` and `clap::CommandFactory`):
  ```rust
  #[test] // M§6: `--name <n>` on both launch verbs; DR-4 carried item: the help names `new`
  fn launch_name_parses_for_both_agents_and_help_lists_decision_new() {
      let Cmd::Claude(l) = Cli::try_parse_from(["zygo", "claude", "--name", "elena", "--", "-p"]).unwrap().command else { panic!() };
      assert_eq!((l.name.as_deref(), l.rest.clone()), (Some("elena"), vec!["-p".to_string()]));
      let Cmd::Opencode(a) = Cli::try_parse_from(["zygo", "opencode", "--name", "elena"]).unwrap().command else { panic!() };
      assert_eq!(a.name.as_deref(), Some("elena"));
      let Cmd::Claude(l) = Cli::try_parse_from(["zygo", "claude"]).unwrap().command else { panic!() };
      assert_eq!(l.name, None);
      assert!(Cli::command().render_help().to_string().contains("Decision records: new, ls, show, deps, check"));
  }
  ```
- [x] 7. Run `cargo test -p zygo --lib launch_name_parses`. Check: E0609, no field `name` on `Launch`.
- [x] 8. In `mod.rs`, add to `Launch` after `config_dir`: `/// Member name in a workspace pane (M§6)` `#[arg(long)] pub(crate) name: Option<String>,`; to `LaunchOpenCode` after `yolo`: `#[arg(long)] pub(crate) name: Option<String>,`; change `mod.rs:361` to `/// Decision records: new, ls, show, deps, check`. Add `name: None` to the literals at `ws.rs:422` and `ws.rs:436` (`ws run` takes the name from the record, step 13), `claude.rs:94` and `opencode.rs:65`. Check: `cargo build -p zygo` compiles.
- [x] 9. Run `cargo test -p zygo --lib`. Check: all pass.
- [x] 10. In the `tests` module of `crates/zygo/src/cmd/member.rs` (import `zygo_workspace::state::Pane`), add:
  ```rust
  #[test] // M§6, AU84: a name held by another key's record or pane entry; its own key and a bad stored name hold none
  fn name_holder_reads_other_keys_by_au84() {
      let mut a = rec(MemberState::Ended, None);
      a.name = Some("elena".into());
      let mut b = rec(MemberState::Ended, None);
      b.name = Some("x_y".into());
      let mut w = ws_with(&[("kaaaaaaaa", a), ("kbbbbbbbb", b)]);
      w.panes.push(Pane { key: "kcccccccc".into(), name: Some("ops".into()), ..Default::default() });
      assert_eq!(name_holder(&w, "kbbbbbbbb", "elena"), Some("kaaaaaaaa".to_string()));
      assert_eq!(name_holder(&w, "kaaaaaaaa", "elena"), None);
      assert_eq!(name_holder(&w, "kaaaaaaaa", "x_y"), None);
      assert_eq!(name_holder(&w, "kaaaaaaaa", "ops"), Some("kcccccccc".to_string()));
      assert_eq!(name_holder(&w, "kcccccccc", "ops"), None);
  }

  #[test] // the record that `join` keeps: the accepted resume only (review round 2, finding 6(b))
  fn kept_is_the_record_of_its_own_resume_only() {
      let w = ws_with(&[("kaaaaaaaa", rec(MemberState::Resumable, Some("abc")))]);
      assert!(kept(&w, "kaaaaaaaa", Agent::ClaudeCode, Some("abc")).is_some());
      assert!(kept(&w, "kaaaaaaaa", Agent::ClaudeCode, None).is_none());
      assert!(kept(&w, "kaaaaaaaa", Agent::ClaudeCode, Some("xyz")).is_none());
      assert!(kept(&w, "kaaaaaaaa", Agent::OpenCode, Some("abc")).is_none());
      assert!(kept(&w, "kbbbbbbbb", Agent::ClaudeCode, Some("abc")).is_none());
  }
  ```
- [x] 11. Run `cargo test -p zygo --lib -- name_holder kept_is`. Check: E0425, `name_holder` and `kept` not found.
- [x] 12. In `member.rs` before `join`, add `name_holder` (the first `ws.members` entry with key ≠ `key` and `valid_name() == Some(name)`, else the first `ws.panes` entry with key ≠ `key` and `valid_name() == Some(name)`; its key) and `kept` (`own?`, then `ws.members.get(key)` filtered by `agent_session.as_deref() == Some(own) && launch.kind == kind`). In `join` replace `member.rs:814-815` by `let keep = kept(ws, key, launch.kind, own).is_some();`. Run `cargo test -p zygo --lib`. Check: all pass, the existing `join_*` tests unchanged.
- [x] 13. Commit `crates/zygo/src/cmd/{mod,member,ws,claude,opencode}.rs`, message `ML-7: --name flag, name_holder, kept`, same trailers. Check: `git show --stat HEAD` lists those five files.
- [x] 14. In `crates/zygo/tests/workspace_member.rs`, add after `errors_log` the constants and tests below. Common shape: `let (out, env) = h.stub(Agent::ClaudeCode, "a");`, launch by `hand(&h, &pane, args, &pairs(&env))` or `h.zygo(..)` under `Guarded::spawn(..).output(..)`; tmux cases begin `let Some(h) = Harness::new() else { return };` and carry `#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]`; no-tmux launches carry `#[ignore = "timing: the agent launch has 2 s launcher probes"]`.
  ```rust
  const NAME_NEEDS_PANE: &str = "zygo: --name needs a pane of a workspace; nothing started\n";
  fn taken(n: &str, key: &str) -> String { format!("zygo: name {n} belongs to pane {key}; nothing started\n") }
  fn named(kind: Agent, state: MemberState, agent: Option<&str>, zs: Option<&str>, n: &str) -> MemberRecord {
      let mut r = record(kind, state, agent, zs);
      r.name = Some(n.into());
      r
  }
  ```
  - `ws_run_writes_the_record_member_name_into_the_view` (`Harness::no_tmux()`): `up_state(&h, &[(KEY, named(.., Ended, None, None, "elena")), ("kcccccccc", named(.., Ended, None, None, "Elena"))], &mut |_| {})`; `ws run --ws-id WS_ID --pane KEY` with stub `a`: success, `view(&out)["workspace"]["member"] == json!({"ws_id": WS_ID, "pane": KEY, "name": "elena"})`; `ws run --pane kcccccccc` with stub `b`: success, `view(&out_b)["workspace"]["member"] == json!({"ws_id": WS_ID, "pane": "kcccccccc"})` (AU84: a bad stored name gives none).
  - `a_hand_start_with_name_writes_the_member_name_to_the_record_and_the_view`: `pane = split_in_workspace(&h, &mut |_| {})`; `hand(.., &["claude", "--name", "elena"], ..)`: exit 0; `key = pane_key(&h, &pane)`; `view(&out)["workspace"]["member"] == json!({"ws_id": WS_ID, "pane": key, "name": "elena"})`; `member(&h, &key).unwrap().name == Some("elena".into())`; then a second split with `hand(.., &["opencode", "--name", "ops"], ..)` (stub OpenCode `o`): exit 0, view name `"ops"`, record name `Some("ops")`.
  - `a_bad_or_reserved_member_name_exits_1_and_starts_nothing`: `pane = split_in_workspace(..)`, `before = state_bytes(&h)`; for `n` in `["Elena", "1x", "a_b", "-a", &"a".repeat(33), "all", "cli", "zygo", "kabcdefgh"]` (each with its own stub): `hand(.., &["claude", &format!("--name={n}")], ..)`: `status.code() == Some(1)`; `text(&o.stderr) == format!("zygo: bad --name value `{n}`: use ^[a-z][a-z0-9-]{{0,31}}$, not all, cli, zygo or a pane key\n")`; `!out.join("argv.json").exists()`; `pane_key(&h, &pane) == ""`; `state_bytes(&h) == before`. After the loop, `"a".repeat(32)` is accepted (exit 0, record name of 32 `a`).
  - `a_member_name_another_member_holds_exits_1_and_starts_nothing`: `workspace_session(&h, Some(KEY2), Some(WS_ID))`; `up_state(&h, &[(KEY, named(ClaudeCode, Ended, None, None, "elena"))], &mut |_| {})`; `before = raw_member(&h, KEY)`; `pane = split(&h, "=zygo-app:", "cat")`; `hand(.., &["claude", "--name", "elena"], ..)`: code 1, `text(&o.stderr) == taken("elena", KEY)`, no `argv.json`, `raw_member(&h, KEY) == before`, `member(&h, &pane_key(&h, &pane)).is_none()`.
  - `the_callback_refuses_a_member_name_taken_since_the_check`: `refused_in_callback(&h, &["claude", "--name", "elena"], &mut |h, _| { insert KEY = named(ClaudeCode, Ended, None, None, "elena") with update_existing })`: code 1, `text(&o.stderr).matches(&taken("elena", KEY)).count() == 1`, `!text(&o.stderr).contains(NOT_SAVED)`, no `argv.json`, `member(&h, &key).is_none()`.
  - `member_name_outside_a_workspace_pane_exits_1_and_starts_nothing` (`Harness::no_tmux()`): for `kind` in `[ClaudeCode, OpenCode]`, `h.zygo(&[verb, "--name", "elena"], &[pairs(&env).., no_check()])` with no `TMUX`: code 1, `text(&o.stderr) == NAME_NEEDS_PANE`, no `argv.json`, `!h.store(WS_ID).dir().exists()`.
  - `an_accepted_hand_resume_keeps_the_member_name_without_name`: `first = workspace_session(&h, Some(KEY), Some(WS_ID))`; `up_state(&h, &[(KEY, named(ClaudeCode, Resumable, Some("abc"), Some(OLD_ZS), "elena"))], ..)`; `hand(&h, &first, &["claude", "--", "--resume", "abc"], ..)`: exit 0, `view(&out)["workspace"]["member"]["name"] == "elena"`, `member(&h, KEY).unwrap().name == Some("elena".into())`.
- [x] 15. Run `cargo test -p zygo --test workspace_member -- member_name --include-ignored`. Check: it compiles (the CLI flag exists) and the 7 tests fail: `--name` reaches no marker or check yet (an exit 0 v1 or member launch where 1 is expected, a view without `name`).
- [x] 16. In `member.rs`: add `pub name: Option<String>` to `MemberCtx` and `HandJoin`; at `:64` set `name: m.name.clone()` in the `Member`; add the three line constants of the author's calls. `hand_start` gets `name: Option<String>` after `launch`; after the `conflict` check at `:932-934` it returns `HandStart::Refused(<taken line>)` when `name_holder(&ws, &key, n)` is `Some`; the `MemberCtx` at `:938` gets `name: name.clone().or_else(|| kept(&ws, &key, launch.kind, own.as_deref()).and_then(|r| r.valid_name().map(String::from)))` and `HandJoin { own_session: own, name }`. `launch_hand` gets `name: Option<String>`: first, a `Some(n)` with `!valid_member_name(n)` prints the bad line and returns 1; then `HandStart::V1` with `name.is_some()` prints `NAME_NEEDS_PANE` and returns 1. `hand_write` returns `Result<Workspace, HandErr>`: in its closure, before `join`, a holder sets `stop = Some(<taken line>)` and bails; after `join` succeeds and `h.name` is `Some(n)`, `ws.members.get_mut(&self.pane)` gets `name = Some(n.clone())`. In `Callback::run`, `Err(HandErr::Stop(line))` prints `line` and returns `Err(anyhow::Error::new(StopLaunch(line)))`; `Err(HandErr::NotSaved(reason))` keeps the current not-saved path. In `ws.rs:256` add `name: rec.valid_name().map(String::from)`. In `claude.rs:21` and `opencode.rs:21` pass `l.name.clone()` / `a.name.clone()` to `launch_hand`. Check: `cargo build -p zygo` compiles.
- [x] 17. Run `cargo test -p zygo --test workspace_member -- member_name --include-ignored`. Check: 7 passed (tmux cases print the skip reason when tmux 3.2 or later is missing).
- [x] 18. Run the Acceptance, then `cargo fmt --check` and `cargo clippy -p zygo -p zygo-core --all-targets -- -D warnings`. Check: counts 1 and 7; `core_basics`, `zygo --lib` and `workspace_member` with `--include-ignored` all pass; fmt and clippy report nothing.
- [x] 19. Commit `crates/zygo/src/cmd/{member,ws,claude,opencode}.rs` and `crates/zygo/tests/workspace_member.rs`, message `ML-7: hand start --name and the member name in the view`, same trailers. Check: `git show --stat HEAD` lists those five files.

## Review round 1

- F1 (high): with `--name`, a refusal of the callback's locked write (`HandErr::NotSaved`) printed the not-saved line and went on as a v1 launch. Now it prints that line and returns `StopLaunch` (exit 1, no agent). Test: `a_callback_refusal_with_member_name_exits_1_and_starts_nothing` (`crates/zygo/tests/workspace_member.rs`).
- F2 (medium): `name_holder` also read `ws.panes[].name`, which mail does not resolve and `join` does not update. Now it reads `ws.members` only. Test: `name_holder_reads_other_keys_by_au84` (`crates/zygo/src/cmd/member.rs`; a pane entry with no record now holds no name).
- F3 (low): with `--name`, a not-saved line (or the AU98 line) of `hand_start` was followed by the needs-a-pane line. `hand_start` now gives `HandStart::V1Said` after a printed line; with `--name` that exits 1 with the one line. Test: `member_name_after_a_not_saved_line_prints_only_that_line` (`crates/zygo/tests/workspace_member.rs`).
- F4 (low): the marker name was chosen before the lock and the record's name after `join`, so a race could make them differ. The locked write is now `hand_join`: it checks the marker name (`HandJoin.marker`) with `name_holder` under the lock (a holder gives `HandErr::Stop`), runs `join`, then sets the record's name to the marker name (none when the marker has none). Test: `hand_join_writes_the_marker_member_name_to_a_replaced_record` (`crates/zygo/src/cmd/member.rs`).
- F5 (low): a non-string `workspace.member.name` made `SessionView::load` fail. `Member.name` now has a lenient deserializer (a string is kept, any other value reads as none). Test: `session_view_member_name_that_is_not_a_string_reads_as_none` (`crates/zygo-core/tests/core_basics.rs`).
