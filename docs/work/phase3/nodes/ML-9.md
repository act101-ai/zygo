# ML-9 — Session text item 8 (workspace mail) at main-loop SessionStart

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§8a items 1 and 2; `docs/specs/2026-11-18-zygo-design.md` §8.6
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-hooks/src/lib.rs`, `crates/zygo-hooks/src/mail.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/session_text.rs`
**Consumes:** `Features.mail` (ML-1), `members` (ML-4), `Member.name` (ML-7)
**Produces:** `zygo_core::session_view::SessionView.mail: bool` (`#[serde(default)]`; the launcher sets it from `features.mail`), `zygo_hooks::mail::session_item(ws: &zygo_workspace::state::Workspace, own_key: &str) -> String`
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test session_text -- mail_item --list 2>/dev/null | grep -c ': test$')" -ge 5 && cargo test -p zygo-hooks --test session_text` → every count holds (at least 5 named `mail_item…` in zygo-hooks `session_text`; a count of 0 fails before any run), then all pass: a named member with `mail` on gets item 8 after the mechanics text and before the resume note, with its name, the workspace name and every member name read from `workspace.json` at this start (a member added while it was down is listed: M§13 #12, second half); a member with no name gets the no-name clause with its pane key; with `mail` off, or outside a workspace, there is no item 8; `mechanics` (the subagent text) has no item 8; each name passes `esc`; the item holds no subject or body text

## Plan

### Contract as the code has it now (origin/phase-3 c44b6b6a)

**Consumes:**
- `zygo_config::Features.mail: bool` (default `true`) — `crates/zygo-config/src/features.rs:16` (ML-1)
- `zygo_workspace::mail::names::members(ws: &Workspace) -> Vec<Target>` (key order; `Target { key: String, name: Option<String> }`, `name` = `MemberRecord::valid_name`) — `crates/zygo-workspace/src/mail/names.rs:16` (ML-4)
- `zygo_workspace::state::MemberRecord::valid_name(&self) -> Option<&str>` — `crates/zygo-workspace/src/state.rs:344` (ML-3)
- `zygo_core::session_view::Member.name: Option<String>` — `crates/zygo-core/src/session_view.rs:141` (ML-7). Not read by item 8: the Acceptance takes every name from `workspace.json` at this start, so the own name is `members[own_key].valid_name()`; the view marker gives only `pane` (the `own_key`).
- `zygo_workspace::state::{Workspace, Store}`; `Store::read(&self) -> Result<Option<Workspace>, StateError>` — `state.rs:466`; `zygo_core::esc(s: &str) -> String` — `crates/zygo-core/src/text.rs:4`
- `zygo_hooks::workspace::session_start(payload: &Value, dir: &SessionDir, view: &SessionView, paths: &Paths, zygo: &Path, out: &mut dyn Write, text: &str) -> anyhow::Result<Option<String>>` — `crates/zygo-hooks/src/workspace.rs:621` (signature unchanged)

**Produces:**
- `zygo_core::session_view::SessionView.mail: bool` — `#[serde(default)]`; `SessionView::default()` has `false`; the launcher sets it to `c.features.mail`
- `zygo_hooks::mail::session_item(ws: &zygo_workspace::state::Workspace, own_key: &str) -> String` — new module `crates/zygo-hooks/src/mail.rs` (ML-10 and WD-18 add to it)

### Fixed text (M§8a item 1)

```rust
const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";
// item = "Workspace mail: {first}. Members: `{names}`. {MAIL_TAIL}"
// first, named:   "you are `{esc(name)}` in workspace `{esc(ws.name)}`"
// first, no name: "you have no mail name; others reach you by pane {esc(own_key)}"
// names: the valid names of `names::members(ws)` in key order (own name included, members with no valid name left out), each through `esc`, joined by ", "
```

### Steps

- [x] 1. Test helpers in `crates/zygo-hooks/tests/session_text.rs`. Add `use zygo_hooks::mail;`, `use zygo_workspace::state::{MemberRecord, Workspace};` and, under `#[cfg(unix)]`, `use std::time::{Duration, SystemTime}; use zygo_core::paths::Paths; use zygo_workspace::state::{Phase, Store};`. Add:
  ```rust
  const MAIL_TAIL: &str = /* the text above */;
  const WS_ID: &str = "0123456789abcdef";
  const OWN: &str = "kaaaaaaaa";
  const PEER: &str = "kbbbbbbbb";
  const LATE: &str = "kcccccccc";
  const U1: &str = "Units u-1 were closed when zygo stopped this session; call work_begin to open them again if work continues.";
  fn item8(first: &str, names: &str) -> String { format!("Workspace mail: {first}. Members: `{names}`. {MAIL_TAIL}") }
  fn rec(name: Option<&str>) -> MemberRecord // MemberRecord::new(Agent::ClaudeCode) with `name`
  fn ws_of(name: &str, members: &[(&str, Option<&str>)]) -> Workspace // Workspace::new(WS_ID), `name`, `members` from the pairs
  #[cfg(unix)] struct Mx { t: tempfile::TempDir, paths: Paths, dir: SessionDir }
  #[cfg(unix)] fn mx(agent: Agent, mail: bool, member: bool, note: Option<ResumeNote>, members: &[(&str, Option<&str>)]) -> Mx
  // Paths as `paths()` in tests/workspace.rs:45 under the temp dir; dir = sessions_dir/"S1", created; saves
  // SessionView { agent, mail, workspace: member.then(|| ViewWorkspace { member: Member { ws_id: WS_ID, pane: OWN, .. }, resume_note: note, .. }), ..view() };
  // then set_members(&x, members)
  #[cfg(unix)] fn set_members(x: &Mx, members: &[(&str, Option<&str>)])
  // Store::open(&x.paths, WS_ID).update(Duration::from_secs(5), ..): name "app", tmux_session "zygo-app", common_dir t/app/.git, main_root t/app, loadout "solo", phase Phase::Up, members from the pairs
  #[cfg(unix)] fn member_text(x: &Mx, input: &HookInput) -> String
  // session_start::run_member(input, &x.dir, &x.paths, &x.t.path().join("missing"), &mut out): None -> additionalContext of `out`; Json(v) -> its additionalContext; Text(t) -> t
  #[cfg(unix)] fn start(agent: Agent) -> HookInput // ClaudeCode: hook("SessionStart-main"); OpenCode: HookInput::parse(Agent::OpenCode, b"{}")
  ```
  Check: the helpers compile once steps 2-9 exist (step 3).

- [x] 2. Write the 8 tests in `crates/zygo-hooks/tests/session_text.rs`:
  ```rust
  #[test] // M§8a item 1
  fn mail_item_named_member_lists_every_member_name() {
      let ws = ws_of("app", &[(OWN, Some("elena")), (PEER, Some("kai")), (LATE, None)]);
      assert_eq!(mail::session_item(&ws, OWN), item8("you are `elena` in workspace `app`", "elena, kai"));
  }
  #[test] // M§8a item 1, no-name clause
  fn mail_item_member_with_no_name_gets_the_pane_clause() {
      let ws = ws_of("app", &[(OWN, None), (PEER, Some("kai"))]);
      assert_eq!(mail::session_item(&ws, OWN), item8("you have no mail name; others reach you by pane kaaaaaaaa", "kai"));
      assert_eq!(mail::session_item(&ws, LATE), item8("you have no mail name; others reach you by pane kcccccccc", "kai"));
  }
  #[test] // AU84, esc
  fn mail_item_escapes_every_name_and_skips_invalid_names() {
      let ws = ws_of("a\u{1b}[2J\nb", &[(OWN, Some("elena")), (PEER, Some("Kai\u{202e}x"))]);
      assert_eq!(mail::session_item(&ws, OWN), item8("you are `elena` in workspace `a\\u{1b}[2J\\nb`", "elena"));
      let ws = ws_of("app", &[(OWN, Some("bad\nname")), (PEER, Some("kai"))]);
      assert_eq!(mail::session_item(&ws, OWN), item8("you have no mail name; others reach you by pane kaaaaaaaa", "kai"));
      assert_eq!(mail::session_item(&ws, "k\u{7}"), item8("you have no mail name; others reach you by pane k\\u{07}", "kai"));
  }
  #[cfg(unix)] #[test] // M§8a item 1 order, §8.6 items 7 and 8; M§13 #12 second half
  fn mail_item_at_session_start_after_mechanics_before_resume_note() {
      for agent in [Agent::ClaudeCode, Agent::OpenCode] {
          let note = ResumeNote { clean_stop: true, closed_units: vec!["u-1".into()], ..Default::default() };
          let x = mx(agent, true, true, Some(note), &[(OWN, Some("elena")), (PEER, Some("kai"))]);
          let base = build(&SessionView::load(&x.dir).unwrap());
          let first = "you are `elena` in workspace `app`";
          assert_eq!(member_text(&x, &start(agent)), format!("{base}\n\n{}\n\n{U1}", item8(first, "elena, kai")), "{agent:?}");
          set_members(&x, &[(OWN, Some("elena")), (PEER, Some("kai")), (LATE, Some("lena"))]);
          let tail = if agent == Agent::ClaudeCode { String::new() } else { format!("\n\n{U1}") };
          assert_eq!(member_text(&x, &start(agent)), format!("{base}\n\n{}{tail}", item8(first, "elena, kai, lena")), "{agent:?}");
      }
  }
  #[cfg(unix)] #[test] // graph review focus 58: item 8 only with `features.mail` on, only in a workspace
  fn mail_item_absent_when_mail_off_or_outside_a_workspace() {
      let x = mx(Agent::ClaudeCode, false, true, None, &[(OWN, Some("elena"))]);
      let got = member_text(&x, &start(Agent::ClaudeCode));
      assert_eq!(got, build(&SessionView::load(&x.dir).unwrap()));
      assert!(!got.contains("Workspace mail:"));
      let x = mx(Agent::ClaudeCode, true, false, None, &[(OWN, Some("elena"))]);
      assert_eq!(member_text(&x, &start(Agent::ClaudeCode)), build(&SessionView::load(&x.dir).unwrap()));
  }
  #[cfg(unix)] #[test] // M§8a item 1: not appended to subagent prompts
  fn mail_item_not_in_mechanics_or_subagent_text() {
      assert_eq!(mechanics(&SessionView { mail: true, ..view() }), mechanics(&view()));
      let x = mx(Agent::ClaudeCode, true, true, None, &[(OWN, Some("elena"))]);
      let m = mechanics(&SessionView::load(&x.dir).unwrap());
      assert_eq!(member_text(&x, &hook("SubagentStart-sub")), m);
      let sub = HookInput { cli_event: Some("subagent".into()), ..HookInput::parse(Agent::OpenCode, b"{}").unwrap() };
      assert_eq!(member_text(&x, &sub), m);
  }
  #[cfg(unix)] #[test] // M§8a "names and tool names only", M-C2
  fn mail_item_holds_no_subject_or_body_text() {
      let x = mx(Agent::ClaudeCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
      let ws = Store::open(&x.paths, WS_ID).read().unwrap().unwrap();
      let r = zygo_workspace::mail::send(&x.paths, &ws, Some(PEER), "elena", "subj-7f3a", "body-9c1d", &zygo_config::MailConfig::default(), SystemTime::now()).unwrap();
      assert_eq!(r.sent.len(), 1);
      let got = member_text(&x, &start(Agent::ClaudeCode));
      assert!(got.contains(&item8("you are `elena` in workspace `app`", "elena, kai")), "{got}");
      assert!(!got.contains("subj-7f3a") && !got.contains("body-9c1d"), "{got}");
  }
  #[test] // Produces: `#[serde(default)]`
  fn mail_item_view_field_defaults_to_off() {
      let (_t, d) = sd();
      let mut json = serde_json::to_value(view()).unwrap();
      json.as_object_mut().unwrap().remove("mail");
      std::fs::write(d.view_json(), serde_json::to_vec(&json).unwrap()).unwrap();
      assert!(!SessionView::load(&d).unwrap().mail);
      assert!(!SessionView::default().mail);
      SessionView { mail: true, ..view() }.save(&d).unwrap();
      let raw: Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
      assert_eq!(raw["mail"], true);
  }
  ```
  Check: the file holds 8 functions whose names start with `mail_item`.

- [x] 3. Run `cargo test -p zygo-hooks --test session_text`. Check: it fails to compile: no field `mail` on `SessionView`, unresolved `zygo_hooks::mail`.

- [x] 4. In `crates/zygo-core/src/session_view.rs`, add to `SessionView` after `repo_root`, before `extra`: `#[serde(default)] pub mail: bool,` with a doc line (`[features] mail` of the launch, M§8, M§8a; `false` for a `view.json` without the field), and `mail: false,` in `impl Default for SessionView`. Check: `cargo test -p zygo-core --test core_basics` passes.

- [x] 5. In `crates/zygo-launch/src/launcher.rs`, in the `SessionView` literal (now lines 589-612), add `mail: c.features.mail,` after `compact_tool: c.features.compact_tool,`. Check: `cargo build -p zygo-launch --tests` succeeds.

- [x] 6. Create `crates/zygo-hooks/src/mail.rs` with a module doc (M§8a; owner ML-9) and `pub fn session_item(ws: &zygo_workspace::state::Workspace, own_key: &str) -> String`, the text of "Fixed text" above, with `MAIL_TAIL` a private `const`; the own name is `ws.members.get(own_key).and_then(MemberRecord::valid_name)`; the list is `zygo_workspace::mail::names::members(ws)` filtered to `Some` names. Check: `cargo build -p zygo-hooks` succeeds after step 7.

- [x] 7. In `crates/zygo-hooks/src/lib.rs`, add `pub mod mail;` on the line before `pub mod pre_compact;`. Check: `cargo build -p zygo-hooks` succeeds.

- [x] 8. In `crates/zygo-hooks/src/workspace.rs` `session_start`, replace the `let output = match inputs.and_then(note) { … };` block (lines 628-632) with:
  ```rust
  // M§8a item 1: item 8 after the mechanics text, before the resume note, from `workspace.json` read at this start.
  // A read error or no state gives no item and no line of its own (record_session's read writes the one line, C6).
  let item = if view.mail { c.store.read().ok().flatten().map(|ws| crate::mail::session_item(&ws, &c.pane)) } else { None };
  let output = [Some(text.to_string()), item, inputs.and_then(note)]
      .into_iter()
      .flatten()
      .filter(|s| !s.is_empty())
      .collect::<Vec<_>>()
      .join("\n\n");
  ```
  Update the doc comment above `session_start`: "the session text, an empty line, item 8 when `view.mail` is on and the state reads, then the note; empty parts are left out". Check: `cargo test -p zygo-hooks --test workspace` passes (no view in it sets `mail`, so its outputs do not change).

- [x] 9. Run `cargo test -p zygo-hooks --test session_text`. Check: all pass, the 8 `mail_item_*` among them.

- [x] 10. Run the Acceptance: `test "$(cargo test -q -p zygo-hooks --test session_text -- mail_item --list 2>/dev/null | grep -c ': test$')" -ge 5 && cargo test -p zygo-hooks --test session_text`. Check: exit 0 with a count of 8.

- [x] 11. Run `cargo test --workspace --locked --no-fail-fast`. Check: exit 0.

- [x] 12. Commit the six files with `-m` only: subject `ML-9: session text item 8 (workspace mail)`, a blank line, then the `Zygo-Session:` and `Zygo-Work:` trailers that the implementation brief names, each on its own line. Check: `git show --stat HEAD` lists only the six files of **Files**.

### Requirement map

| Requirement | Step / test |
|---|---|
| `SessionView.mail`, `#[serde(default)]` | 4; `mail_item_view_field_defaults_to_off` |
| Launcher sets it from `features.mail` | 5 (build only; see Notes) |
| Item 8 text, own name, workspace name, member names | 6; `mail_item_named_member_lists_every_member_name` |
| No-name clause with pane key | 6; `mail_item_member_with_no_name_gets_the_pane_clause` |
| After mechanics, before resume note; names read at this start; member added while down (M§13 #12) | 8; `mail_item_at_session_start_after_mechanics_before_resume_note` |
| `mail` off or outside a workspace: no item (review focus 58) | 8; `mail_item_absent_when_mail_off_or_outside_a_workspace` |
| Not in `mechanics` / subagent text | 8; `mail_item_not_in_mechanics_or_subagent_text` |
| Each name passes `esc`; names checked at read (AU84) | 6; `mail_item_escapes_every_name_and_skips_invalid_names` |
| No subject or body text (M-C2) | `mail_item_holds_no_subject_or_body_text` |

### Notes

- `crates/zygo-hooks/src/mail.rs` does not exist at c44b6b6a; this node creates it.
- `Member.name` (ML-7) is listed in Consumes but item 8 does not read it: the Acceptance requires the names from `workspace.json` at this start, and `session_item` takes no view.
- The launcher assignment has no test here: the launcher view tests in `crates/zygo-launch/tests/launcher.rs` (for example line 672) are `#[ignore = "timing: …"]` and that file is not in **Files**.
- Spec gap, not decided here: M§8a gives no text for an empty member list (own member unnamed and no other named member); the plan prints ``Members: ``.`` with an empty pair of backticks.
- The four state-backed tests are `#[cfg(unix)]` (Global constraints: tests that use workspace state); the Acceptance count of 5 holds on unix (8) but not on Windows (4).

## Round 2

- F1: `mail::session_item` returns `Option<String>`; `None` when `own_key` has no member record (no one can reach that pane by mail: `names::resolve`), so item 8 is left out. The `workspace.rs` caller uses `and_then`. Tests: `mail_item_absent_for_a_pane_with_no_member_record`, `mail_item_absent_at_session_start_for_a_pane_with_no_member_record`.
- F2: test `mail_item_absent_when_workspace_state_is_unreadable`: a malformed `workspace.json` gives no item 8, the same text and resume note as with `mail` off, and one `errors.log` line for Claude Code (none for OpenCode).
- F3: not added. The view is built inside `Session::run` (`crates/zygo-launch/src/launcher.rs`), after plugin sync and the session client and before the agent spawn; it has no seam a unit test reaches, and the view tests of `crates/zygo-launch/tests/launcher.rs` are `#[ignore = "timing: …"]`.
- F4: the member list holds each name once, in key order of its first member. Test: `mail_item_lists_each_name_once`.
