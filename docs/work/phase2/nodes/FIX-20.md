# FIX-20 — Remove invented expiries: no idle guess ends an open turn; no 30-day results deletion

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` AU26, AU27, AU69, AU92, W§7.2 (`member.last_run_at` row), W§14 (drain idle rule row), W§17 PW7 (row and note), W§18 #61, #81, #82, #84, as amended by the operator rule of 2026-11-22: no invented requirement may end user work. Two invented rules go. (a) The AU26 interrupted-turn rule: a member whose turn is open (`busy` `true`, no `Stop`) counted as idle after 60 s with no model request and no running tool, so `down` stopped it (`INTERRUPTED_TURN_AFTER`, `crates/zygo-workspace/src/down.rs:84`; `Track.open_since` and the turn line, `down.rs:1156-1174`). After this node an open turn is busy at every poll; if it never ends, `down` reaches its drain timeout, sets `phase = up` again, stops nothing and exits 1; `down --now` is the operator's explicit override. (b) The AU69 (b) case that deletes `results/<pane key>/` of a member that has not run for 30 days (`member.last_run_at`); not built yet (M1 would build it). No code reads `member.last_run_at` (`crates/zygo-workspace/src/state.rs:111` defines it; only tests set it), so the field goes too. The W§9 sections cite no 60 s rule: W§9.3 step 6 already gives `no Stop since its last prompt` as the timeout reason of an open turn and stays as it is. The final spec text is in "Spec text" below; this node applies it verbatim
**Files:** `crates/zygo-workspace/src/drain.rs`, `crates/zygo-workspace/src/down.rs`, `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/drain.rs`, `crates/zygo-workspace/tests/down.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-workspace/tests/up.rs`, `docs/specs/2026-11-20-workspaces-design.md`, `docs/work/phase2/nodes/M1.md`, `docs/work/phase2/nodes/E2.md`
**Consumes:** `drain::idle` (W9)
**Produces:** no idle guess ends an open turn; no 30-day results deletion
**Acceptance:** `cargo test -p zygo-workspace --locked --test drain --test down` → all pass, including `open_turn_is_busy_until_the_drain_timeout` (a member with `busy` `true`, an empty running set and a quiet proxy is never counted idle: at the drain timeout `down` exits 1, `phase` is `up`, the member is listed with `busy: no Stop since its last prompt`, its launcher got no SIGTERM, and no interrupted-turn line is printed; a `Stop` written during the drain lets `down` go on and stop the workspace) and `an_open_turn_is_a_turn_open_at_every_poll` (`drain::idle` gives `TurnOpen` at 0 s, 61 s, 600 s and 86 400 s)

## Plan

The fix is in `down.rs`: the drain loop treats `Idle::TurnOpen` as busy at every poll; `INTERRUPTED_TURN_AFTER`, `Track.open_since`, `Track.said_turn` and the interrupted-turn line go. `drain::idle` keeps its result type: `TurnOpen { rule_skipped }` still tells `down` to print the AU26 rule-skipped line and gives the timeout reason `no Stop since its last prompt` (`time_out`, `down.rs:1212`); only its doc comments lose the 60 s. `member.last_run_at` goes from `MemberRecord`. A `workspace.json` written before this node that holds `last_run_at` still parses: `MemberRecord.extra` (`#[serde(flatten)]`, `state.rs:127-128`) keeps an unknown key and writes it back.

Builds on: (signatures in the code now)
- `pub fn idle(activity: Result<&Activity, &str>, status: Result<&SessionStatus, &str>, now: SystemTime) -> Idle` and `pub enum Idle { Idle, Busy { reason: String, rule_skipped: bool }, IdleRuleSkipped, TurnOpen { rule_skipped: bool } }` (`crates/zygo-workspace/src/drain.rs:15-38`, `:83`).
- `pub const INTERRUPTED_TURN_AFTER: Duration` (`down.rs:84`); `struct Track { open_since, said_skip, said_turn, said_idle, last }` (`down.rs:756-763`); `drain_tick` (`down.rs:1136-1202`); `time_out` (`down.rs:1205-1229`).
- Test helpers in `crates/zygo-workspace/tests/down.rs`: `one_member`, `set_activity`, `hooks`, `quiet`, `in_flight`, `release_on_sigterm`, `run`, `count`, constants `TURN` (`:55`), `SKIP`; pattern of `rule_skipped_waits_until_the_timeout` (`:1000`).
- `MemberRecord.last_run_at: Option<u64>` (`state.rs:111`), set in `MemberRecord::new` (`state.rs:293`); test sites `tests/state.rs:60`, `:86`, `tests/up.rs:1852`.

Decisions (author's call, from the brief and the evidence):
- D1. `Idle::TurnOpen { rule_skipped }` stays: removing it would merge it into `Busy` and change the reason text path of `time_out` and the skip line of `drain_tick`, which this node does not need. The `TURN` constant in `tests/down.rs` stays as a guard: every test that counts it expects 0.
- D2. AU69 (b) loses only the 30-day case. The case "the pane key has no member record" stays: it follows a user's `ws forget-pane` or `ws forget`, it is not a time rule. W§18 #61 keeps its first clause (a workspace whose `workspace.json` does not parse keeps all results). M1's `sweep::WorkspaceInfo.members` becomes `Vec<String>`, the pane keys that have a member record; `sweep::sweep` keeps `now` for the base 24 h sweep of ended sessions (§10.1).
- D3. `member.last_run_at` is removed from the spec (W§7.2 row, AU92) and from `MemberRecord`: no code reads it and its only spec reader was AU69 (b).
- D4. The known-limit text names what ends an open turn as the code does: `Stop` or `StopFailure` (W§9.1 table, `busy = false`), the agent's exit (a member whose launcher is not live counts as idle, W§9.3 step 5), or the drain timeout. `SessionEnd` sets only `tools_running = 0` (W§9.1), not `busy`, so it is not named as a turn end.
- D5. The W§17 PW7 note of P2 is history and stays; a new dated note after it records the change.
- D6. E2's contract text for #82 and for a failed PW4 4.7 loses the interrupted-turn rule (E2.md is in Files).

Steps:

- [x] Write the failing tests in `crates/zygo-workspace/tests/down.rs`: in `constants_and_mount_namespace` delete the `down::INTERRUPTED_TURN_AFTER` assertion; replace `interrupted_turn_counts_after_60s` with `open_turn_is_busy_until_the_drain_timeout`, two cases on `one_member`, `set_activity(&o, SID, true, &[])` and `quiet()`: (a) `o.timeout = Some(Duration::from_secs(120))`, no `release_on_sigterm`: exit 1, `count(&err, TURN) == 0`, `count(&err, "zygo: app pane kaaaaaaaa: busy: no Stop since its last prompt") == 1`, `count(&err, SKIP) == 0`, `phase` `Up`, the launcher child still running (no SIGTERM: `try_wait` gives `None`), then kill it; (b) timeout 600 s, `release_on_sigterm`, `on_poll` writes `set_activity(&o2, SID, false, &[])` at 90 s (a `Stop`): exit 0, out `zygo: app is down (1 agents saved)\n`, `count(&err, TURN) == 0`, last poll time at least 90 s, `phase` `Down`.
- [x] In `crates/zygo-workspace/tests/drain.rs`: add `an_open_turn_is_a_turn_open_at_every_poll` (`tools(true, 0)`, `quiet()`, poll times `now()` plus 0, 61, 600 and 86 400 s → `Idle::TurnOpen { rule_skipped: false }`); change the comment of `an_interrupted_turn_with_no_running_tool_is_a_turn_open` to `FIX-20: an Esc-interrupted turn with no running tool is a turn open; down keeps it busy until a Stop or the drain timeout`.
- [x] Run and see red: `cargo test -p zygo-workspace --locked --test drain --test down`. Expected: `open_turn_is_busy_until_the_drain_timeout` fails, case (a) with `left: 0`, `right: 1` on the exit code (the old code prints the turn line at 60 s and stops the member); `an_open_turn_is_a_turn_open_at_every_poll` passes (`drain::idle` holds no clock state).
- [x] Implement in `down.rs`: delete `INTERRUPTED_TURN_AFTER` and its doc lines, `Track.open_since`, `Track.said_turn`, the `open_since` updates, `turn_ended` and the turn line; the idle test is `matches!(r, Idle::Idle | Idle::IdleRuleSkipped)`. In `drain.rs`: the `TurnOpen` doc says it is busy until a `Stop`, the agent's exit or the drain timeout, and that `rule_skipped` only drives the AU26 skip line; the `idle` doc loses the 60 s sentence.
- [x] Remove `last_run_at` from `MemberRecord` and `MemberRecord::new` (`state.rs`), from `tests/state.rs:60` and the JSON fixture at `:86`, and from `tests/up.rs:1852`.
- [x] Apply "Spec text" below to `docs/specs/2026-11-20-workspaces-design.md`, and "Contract text" below to `M1.md` and `E2.md`.
- [x] Run the Acceptance → all pass. Then `cargo test -p zygo-workspace --locked --no-fail-fast 2>&1`, `cargo check --workspace --locked --tests 2>&1`, `cargo clippy -p zygo-workspace --locked --tests -- -D warnings` → all clean; `rg -n "60 s|interrupted turn|last_run_at|30 days" docs/specs/2026-11-20-workspaces-design.md` shows only the change-log rows and the dated notes.
- [x] Tick the steps; commit the files of Files and this file: "fix(workspace): FIX-20 an open turn stays busy; no 30-day results deletion".

## Spec text

Final text for `docs/specs/2026-11-20-workspaces-design.md`, applied verbatim by this node.

1. AU26: replace the sentences from `Interrupted turn (author's call, 2026-11-22):` through `waits until the drain timeout.` with:

   ```text
   Open turn (FIX-20, operator 2026-11-22: no invented requirement may end user work): a member whose `busy` is `true` and whose main loop runs no tool is busy at every poll, whatever time passes; no quiet time counts it as idle. Known limit: a turn ended with Esc gives no `Stop` or `StopFailure` (PW7 7.2), so `busy` stays `true`. The member then stays busy until a `Stop` or `StopFailure` comes (at the latest at the end of the turn of its next prompt), until its agent exits (a member whose launcher is not live counts as idle, W§9.3 step 5), or until the drain timeout, where `down` writes `phase = up` again with no work lost, stops nothing, lists the member with `no Stop since its last prompt` and exits 1; `down --now` is the operator's explicit override.
   ```

2. AU27: replace `All three rules of AU26 apply to OpenCode members, and so do the AU26 interrupted-turn rule (60 s) and its known limits;` with `All three rules of AU26 apply to OpenCode members, and so do its open-turn rule and its known limits;`.

3. AU69: replace `it deletes `results/<pane key>/` when the pane key has no member record, or when the member has not run for 30 days (`member.last_run_at`, set at each start) and no live session links to it.` with `it deletes `results/<pane key>/` when the pane key has no member record. No results directory is deleted by its age (FIX-20).`.

4. AU92: replace `(moved into `zygo_session`, `agent_session`, `last_run_at`, with `state = running`)` with `(moved into `zygo_session` and `agent_session`, with `state = running`)`.

5. W§7.2: delete the row `| `member.last_run_at` | Unix time of the member's last committed start (AU69). |`.

6. W§14, row "Drain idle rule from notifications and a quiet time": replace `a tool call sends no model request, so without the count a tool that ran more than 60 s looked like an interrupted turn and `down` cut it.` with `a tool call sends no model request, so without the count a tool that runs while `busy` is `false` looked idle.`.

7. W§17 PW7, last column: replace `AU26, author's call 2026-11-22: when `busy` is `true` but rules 2 and 3 have held for 60 s, `down` counts the member as idle and prints `zygo: <member>: no Stop since its last prompt (interrupted turn); treated as idle` (7.2);` with `AU26, FIX-20: a turn with no `Stop` stays busy until a `Stop` or `StopFailure`, the agent's exit or the drain timeout, and `down --now` is the override (7.2);`.

8. W§17, after the note `*(Note 2026-11-22, P2, PW7: …)*`, add:

   ```text
   *(Note 2026-11-22, FIX-20: the operator rule that no invented requirement may end user work removes the 60 s fallback of the note above. A turn with no Stop stays busy until a Stop or StopFailure, the agent's exit or the drain timeout; down --now is the explicit override.)*
   ```

9. W§18 #61: `61. The results sweep keeps all results of a workspace whose `workspace.json` does not parse (AU69).`

10. W§18 #81: `81. A member whose `activity.json` has `busy = true` and an empty running set, and whose proxy reports no model request in flight and none that ended in the last 10 s: `down` never counts it as idle and prints no interrupted-turn line; at the drain timeout it writes `phase = up`, stops nothing, lists the member with `no Stop since its last prompt` and exits 1. A `Stop` during the drain lets `down` go on and stop the workspace. A member with `busy = true` and a request in flight is busy too (AU26, PW7 7.2).`

11. W§18 #82: `82. A stub member sends `UserPromptSubmit` and then no `Stop` and no `StopFailure` (a turn ended by Esc, PW7 7.2), then a `SubagentStop` with an empty `agent_type` and no matching `SubagentStart` (PW7 7.5): the running set in `activity.json` stays empty; `down` does not stop the member: at its drain timeout it exits 1 with `phase = up` and the stub still running; `down --now` then stops the workspace with `phase = down` (AU26).`

12. W§18 #84: replace `prints `zygo: <member>: proxy request state not usable; idle rule 3 skipped` once, never prints the interrupted-turn line, and at the drain timeout` with `prints `zygo: <member>: proxy request state not usable; idle rule 3 skipped` once, and at the drain timeout`.

13. Change log, new row: `| 2026-11-22 | AU26, AU27, AU69, AU92, W§7.2, W§14, W§17 PW7, W§18 #61, #81, #82, #84 | operator | operator: no invented requirement may end user work |`.

## Contract text

- `M1.md` Produces: `sweep::WorkspaceInfo { cache_dir: PathBuf, readable: bool, members: Vec<(String, u64)> }` becomes `sweep::WorkspaceInfo { cache_dir: PathBuf, readable: bool, members: Vec<String> }` (the pane keys that have a member record; AU69 as amended by FIX-20).
- `E2.md` Spec: `checks that the real `down` waits at least 60 s, prints the interrupted-turn line once and reaches `phase = down`` becomes `checks that the real `down` does not stop the member: at its drain timeout it exits 1 with `phase = up`, and `down --now` then reaches `phase = down``; `(rules 2 and 3 of AU26 and the interrupted-turn rule only)` becomes `(rules 2 and 3 of AU26 only)`.
