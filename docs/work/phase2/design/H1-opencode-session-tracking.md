# H1 re-design: OpenCode session record, start commit, resume note, turn state

Author's call, 2026-11-22. Scope: the four OpenCode failures of the H1 attempt-1 diff review (rounds 1-3). Operator rules apply: an open turn stays busy until Stop/StopFailure, agent exit, drain timeout or `down --now`; no invented requirements.

## TLDR

1. **Commit is separate from the id.** The plugin sends `zygo hook workspace --event session` with no `sessionID` when it loads (the plugin factory runs only in a spawned agent, so this proves the spawn). That call commits `pending` (AU92). The id is recorded by later `session` calls that carry a `sessionID`.
2. **The clear compares, it does not depend on order.** `down` W§9.3 step 8 writes `member.saved_for = <ULID of the live zygo session>` with the values it saves. The commit of zygo session S always clears `note_pending`. It clears `closed_units`, `interrupted`, `clean_stop` and `saved_for` only when `saved_for` is not S. One rule for Claude Code and OpenCode.
3. **A session is classified only from its `info`.** A `session.*` event with `properties.info` tells if the session has a `parentID`. `parentID` is set at creation and does not change, so the classification is final. A session with no `info` seen is "unknown". Only a known top session is sent in a `session` call. A child is never sent, whatever the event order.
4. **The main session is the known top session that was activated last.** Activation = `session.created` or `session.status` `busy`, numbered by one counter. An `info` event classifies, it does not activate. So a late `info` (for example from `session.updated` or `session.deleted`) of an older session does not take the record.
5. **The note belongs to one agent session.** The view's note gets `agent_session` = the resume id (none for a fresh start). A `session` call with id X: note bound to none → bind to X, keep; bound to X → keep; bound to another id → remove. The commit never removes the note.
6. **Turn set = sessions in a turn that are not known children.** `busy` adds, idle removes, classification as a child removes. `prompt` when the set becomes non-empty, `stop` when it becomes empty. The `session` call does not change `activity.json`.

## 1. Evidence used (and not used)

- `fixtures/opencode/workspace/pw4.txt` 4.1: for `opencode run`, the first hook with the id is `session.created`. 4.2: `run -s <id>` keeps the same id. 4.3: the idle event is `session.idle` / `session.status`. 4.4: `tool.execute.before` sees `task` before the subagent's first request. 4.6: the TUI plugin sees a `sessionID` and an idle event.
- `fixtures/opencode/workspace/plugin-event-types.txt`: `session.created`, `session.updated`, `session.status`, `session.idle`, `session.diff`, `message.*`. No `session.deleted`. No session-switch event.
- `fixtures/opencode/workspace/tool.execute.before-task.json`: `sessionID` is the lead's id.
- Not used, because no fixture shows it: the order of a child's `busy` and its `session.created`; a `sessionID` in the `experimental.chat.system.transform` input; the plugin `client` SDK (`client.session.get`); `session.deleted`; a `session.updated` in the TUI-switch case.

The design is correct for every order of the events above. It does not need the unproven facts.

## 2. Design

### 2.1 Hook side (node H1a)

`zygo hook workspace --agent opencode --event session`, stdin `{"cwd": <dir>}` or `{"sessionID": <id>, "cwd": <dir>}`. Two parts, each idempotent, in this order, in one write under `lock`:

1. **Commit** (always tried). When `member.pending.zygo_session` is this zygo session (from the session view marker): move `pending` into `zygo_session` (and `agent_session` only when `pending.agent_session` is some), `state = running`, then apply the clear rule (2.2). Else: nothing.
2. **Record** (only with a non-empty string `sessionID`). Write `member.agent_session` when it differs; start the W§7.4 snapshot when it changed. Then apply the note rule (2.3) to the session view.

`prompt` / `stop` (stdin `{"sessionID", "cwd"}`): `busy = true` / `false` and `tools_running = 0`, as now. The hook does no main-session check. The `session` event does not change `activity.json` (attempt round 1 F2 stays).

Claude Code `SessionStart` (first main-loop start): prints the note, then calls the same commit part (2.2 rule) and the record part. Its note handling stays AU60 (print, then remove the inputs). A later `SessionStart` (no `pending` of its own) commits nothing and clears nothing, as now.

### 2.2 Clear rule (AU60, both agents)

- `down` W§9.3 step 8 writes, with `closed_units`, `interrupted`, `clean_stop = true`, the field `member.saved_for = <name of the live session directory>` (the ULID of the zygo session that `down` stops; `down.rs` already has that `dir` per live member).
- The commit of zygo session S clears `note_pending`. It clears `closed_units`, `interrupted`, `clean_stop` and `saved_for` only when `saved_for` is absent or is not S.
- The AU75 repair that sets `clean_stop = false` and clears the lists also removes `saved_for`.
- The wrapper commit at exit (W§8.2 step 7) keeps "no clear" (AU60 known limit, unchanged).

Result: a `down --now` during the first turn writes values for S. A commit of S that runs after it keeps them. The next start S2 shows them in its note and clears them. The order of the two writes does not matter.

### 2.3 Note rule (W§9.4, OpenCode)

- `ResumeNote` (session view, `crates/zygo-core/src/session_view.rs:116`) gets `agent_session: Option<String>`: `ws run` sets it to the resume id when it resumes, none for a fresh start (AU72 path: `LaunchOpts.resume_note`).
- The record part of a `session` call with id X: no note → nothing. Note with `agent_session` none → set it to X, keep the note. Note bound to X → keep. Note bound to another id → remove the note inputs.
- The commit part never touches the note. `session-start` (per model request) prints the note while the inputs are in the view, as now.

### 2.4 Plugin (node H1b)

State: `kind: Map<id, "top"|"child">`, `act: Map<id, n>`, counter `n`, `main`, `mainAct`, `sent` (last id sent in `session`), `inTurn: Set<id>`.

At load (factory body, member gate on): queue `session` with `{}` (commit only).

For each event, before any wait (D11 stays), with `id` = the event's session id (as now):

1. **Classify.** If `e.type` starts with `session.` and `properties.info` is an object with `id`: `kind[info.id] = info.parentID ? "child" : "top"` (first value wins; a later value does not change it).
2. **Activate.** `session.created` (for `info.id`) or `session.status` `busy` (for `sessionID`): `act[id] = ++n`.
3. **Turn set.** `busy` of a session that is not a known child and is not in `inTurn`: add; if the set was empty, queue `prompt` with that id. Idle (`session.idle` or `session.status` `idle`) of a session in `inTurn`: remove; if the set is now empty, queue `stop` with that id. When step 1 classified a session in `inTurn` as a child: remove it; if the set is now empty, queue `stop` with its id. `retry` and repeated signals send nothing.
4. **Main.** For the event's session (and for `info.id` after step 1): if `kind == "top"` and `act` is defined and (`main` is null or `act > mainAct`): `main = id`, `mainAct = act`; if `id != sent`: queue `session` with `{sessionID: id}`, `sent = id`.

`tool.execute.before` on `task`: unchanged (`pre-tool`, wait, deny throws).

### 2.5 How each failure is removed

| # | Failure | Handling |
|---|---|---|
| 1 | Child or deleted id recorded as `member.agent_session` | Only a session whose own `info` shows no `parentID` is sent. `parentID` does not change, so a child is never sent in any order (busy before created, idle before created, two late children). An `info` event does not activate, so a `session.deleted`/`session.updated` of an old session does not take the record. |
| 2 | Commit early vs. deferred | Commit at plugin load, before any event, with no id: it cannot record a child. The clear compares `saved_for`, so a commit after `down --now` keeps what `down` saved. The id is recorded at the first known-top activation (`session.created` for `opencode run`, the first event, 4.1), so an exit after the first event keeps the id; a resume already holds its id. |
| 3 | Resume note lost or kept on the wrong session | The commit never removes the note. The note is bound to the resume id (or the first recorded id) and is removed only when a known top session with another id becomes main. Every model request of the bound session gets it. |
| 4 | Turn state | Per-session turn set; a late child leaves at its classification or its idle, so no child stays. `prompt`/`stop` only on empty ↔ non-empty. The `session` call does not end an open turn. |

### 2.6 Known limits (to state in AU27 / AU104)

1. A session that becomes active with no `info` event in this process (a TUI switch to an existing session, if OpenCode sends no `session.updated` for it) is not recorded. `member.agent_session` keeps the earlier top session, so `up` resumes that one. Its turn signals still work. E2 records whether `session.updated` fires on a TUI switch.
2. No fixture shows `session.deleted`. A main session deleted in the TUI stays recorded until another top session is activated; the resume then fails, and AU12 applies (shell, printed command, every later `up` tries again).
3. A top session whose turn gets no idle event keeps the member busy until agent exit, drain timeout or `down --now` (operator rule; AU26 open-turn rule).
4. A child whose `busy` comes before its `info` is in the turn set until its `info` or its idle. Its parent's turn is open in that time, so the member state does not change.
5. A load-time `session` call that does not get `lock` in 1 s commits nothing; the next `session` call with an id commits (and clears by 2.2); else the wrapper commits at exit with no clear (AU60 limit, unchanged).
6. `opencode run` can exit before its queued `stop` runs; the turn ends at agent exit (AU26). `--attach` and `--pure`: no member events (unchanged).

## 3. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| Attempt 1: commit and record in one `session` call, deferred to "confirmation" (idle of a not-known-child), pending main, history of mains | Idle is not proof of top: a `parentID` after the idle reclassifies a recorded child. Deferral let the commit erase `down --now` values and lost the id at early exit. Three review rounds added state and still failed. |
| Commit at the first event of any kind | Correct with `saved_for`, but later than load and needs an event to come. Load is certain and earlier. |
| Clear by call order (AU60 as written) | Correct only if the commit always comes before any `down`. Not true for OpenCode, and not proven for Claude Code. |
| Clear by timestamp (`down` writes a time; the commit compares with `started_at`) | Clock compares, same-second ties. The ULID equality is exact. |
| Count child sessions in the turn set too | Needs a child idle event; no fixture shows one. Removal at classification does not need it. |
| `client.session.get(id)` to read `parentID` at once | No fixture shows the plugin `client`. Would remove limit 1. |
| Read `sessionID` in `experimental.chat.system.transform` to bind the note per request | No fixture shows the field. |
| `session.deleted` handler | No fixture shows the event (AU27 rule: use only what the evidence shows). |

## 4. What would change this decision

- A fixture with `client.session.get` returning `parentID` → classify status-only sessions at once; limit 1 goes.
- A fixture with `session.deleted` (with `info`) → on it, drop the session from `kind`/`inTurn`; if it is `main`, fall back to the known top session with the highest `act`; limit 2 goes.
- A fixture showing that `session.created` of a child always comes before its `busy` → no change needed (the design already covers it); limit 4 goes.
- A fixture where `sessionID` is in the system-transform input → bind the note per request instead of per id record.
- Evidence that OpenCode loads the plugin before the agent can serve a session in some mode where the launch fails → move the commit to the first event.

## 5. Spec rewrites (`docs/specs/2026-11-20-workspaces-design.md`)

| Section | Gist |
|---|---|
| AU27 | Replace the main-session text with 2.4: classification from `info` only, final; activation by `session.created` / `busy` with one counter; main = known top with the highest activation; `session` call on a change of main; turn set rules (add on `busy` of a not-known-child, remove on idle or child classification, `prompt`/`stop` on empty ↔ non-empty); `session` changes no activity; limits 1-4 and 6 of 2.6. Remove "a `session` call sets `busy = false`". |
| AU60 | Commit of zygo session S clears `note_pending` always, and the three saved values and `saved_for` only when `saved_for` is not S; remove "keeps what `down` saved meanwhile" order wording; OpenCode: the plugin's load-time `session` call commits; keep the wrapper limit. |
| AU92 | Commit sources: Claude Code first main-loop `SessionStart`; OpenCode any `session` call while `pending` names this zygo session (first: the load-time call with no id); wrapper at `spawned = true`. The commit does not write `agent_session` from the payload. |
| AU104 | Load-time `session` (no id) commits; `session` with id records a known top session (AU27); note rule 2.3; limits 2 and 5. Remove "first `session` call keeps/clears" text. |
| AU75 (repair) | The repair that clears the saved values also removes `saved_for`. |
| W§7.2 | New row `member.saved_for` (ULID of the zygo session that `down` saved; written by W§9.3 step 8; used by AU60). `clean_stop` / `note_pending` rows: cleared per AU60 rule. `agent_session` row: OpenCode records only a known top session (AU27). |
| W§8.2 step 4 / AU72 | `LaunchOpts.resume_note` carries the resume id as `agent_session` (none for a fresh start). |
| W§9.1 | OpenCode paragraph = 2.4 in prose; `busy` row: OpenCode `prompt`/`stop` per the turn set, `session` does not write; `tools_running` row: OpenCode `prompt`/`stop` set 0, `session` does not write (FIX-19 changes the tool part). |
| W§9.3 step 8 | Also write `saved_for = <live zygo session>`. |
| W§9.4 | OpenCode: note bound to one agent session (2.3); commit never removes it. Claude Code unchanged. |
| W§18 | Amend #86 (`session` leaves `busy`); add items for the 2.2 clear rule, the 2.3 note rule and the plugin orders of section 6. Change-log row. |

## 6. Tests

### 6.1 Plugin orders (H1b, `crates/zygo-launch/tests/opencode.rs`, node harness with the stub zygo that records calls)

Notation: `L` = plugin load; `created(X)`, `created(X<P)` = with `parentID` P; `updated(X)`; `deleted(X)` = an `info` event of a type the plugin does not know; `busy(X)`, `idle(X)`. Calls: `session{}`, `session X`, `prompt X`, `stop X`.

| Test | Events | Expected calls |
|---|---|---|
| P1 run fresh | L, created(A), busy(A), idle(A) | session{}, session A, prompt A, stop A |
| P2 resume, no info | L, busy(A), idle(A) | session{}, prompt A, stop A |
| P3 resume, info in turn | L, busy(A), updated(A), idle(A) | session{}, prompt A, session A, stop A |
| P4 child, normal order | L, created(A), busy(A), created(K<A), busy(K), idle(K), idle(A) | session{}, session A, prompt A, stop A |
| P5 late child | L, created(A), busy(A), busy(K), created(K<A), idle(K), idle(A) | session{}, session A, prompt A, stop A |
| P6 two late children | L, created(A), busy(A), busy(K1), busy(K2), created(K1<A), created(K2<A), idle(A) | session{}, session A, prompt A, stop A |
| P7 child after its idle | L, created(A), busy(A), busy(K), idle(K), created(K<A), idle(A) | session{}, session A, prompt A, stop A |
| P8 late child alone | L, busy(K), created(K<P) | session{}, prompt K, stop K |
| P9 TUI switch with info | P1 events, then busy(B), updated(B), idle(B) | P1 calls, prompt B, session B, stop B |
| P10 TUI switch, no info | P1 events, then busy(B), idle(B) | P1 calls, prompt B, stop B (no `session B`; limit 1) |
| P11 overlapping turns | L, created(A), busy(A), created(B), busy(B), idle(A), idle(B) | session{}, session A, prompt A, session B, stop B |
| P12 late info of older session | L, busy(Y), created(X), busy(X), updated(Y) | session{}, prompt Y, session X (no `session Y`) |
| P13 info event does not activate | P1 events, created(B), deleted(A) | P1 calls, session B (no second `session A`) |
| P14 not a member | `ZYGO_WORKSPACE` unset, P1 events | none |
| P15 decides before waits | P1 events in one tick, stub zygo slow | calls in P1 order |

### 6.2 Hook side (H1a, `crates/zygo-hooks/tests/workspace.rs` and `crates/zygo/tests/cli_surface.rs`)

| Test | Start state | Call(s) | Expected member / view / activity |
|---|---|---|---|
| H1 commit, old values | `pending` = S; `saved_for` = S0; values set; `note_pending` | `session {}` from S | `zygo_session` = S, `pending` none, `running`; values, `saved_for`, `note_pending` cleared; `agent_session` unchanged; view note kept |
| H2 commit after `down --now` | `pending` = S; values with `saved_for` = S; `note_pending` | `session {}` from S | committed; `closed_units`, `interrupted`, `clean_stop`, `saved_for` kept; `note_pending` cleared |
| H3 Claude Code same rule | as H2 | first main-loop `SessionStart` | note printed; values saved for S kept; inputs removed from view |
| H4 record after commit | committed S; values `saved_for` = S | `session {sessionID: A}` | `agent_session` = A; values kept; snapshot child started |
| H5 commit + record in one call | `pending` = S, `saved_for` = S0 | `session {sessionID: A}` | committed, cleared, `agent_session` = A |
| H6 not my pending | `pending` = S2 | `session {}` from S | nothing changes |
| H7 no id keeps id | `agent_session` = X | `session {}` / `session {sessionID: ""}` / non-string | `agent_session` = X |
| H8 note bind fresh | note `agent_session` none | `session A`, `session-start`, `session B`, `session-start` | note bound to A and printed; removed at B; second output has no note |
| H9 note bind resume | note bound to X | `session X`, `session Y` | kept at X, removed at Y |
| H10 activity | `busy = true`, `tools_running = 2` | `session A` | `activity.json` unchanged |
| H11 prompt / stop | — | `prompt A`, `stop A` | `busy` true then false, `tools_running` 0 |
| H12 down writes `saved_for` | live member, launcher session dir S (pending or committed) | `down --now` step 8 | `saved_for` = S with the saved values (`crates/zygo-workspace` down tests) |
| H13 repair | `stopping`, live members | AU75 repair | values cleared and `saved_for` removed |
| H14 early exit keeps id | P1 prefix L, created(A), then agent exit (binary end-to-end with the stub) | — | `agent_session` = A; wrapper does not erase it |

## 7. Work split

- **H1a — hook side and Claude Code.** Everything of the attempt-1 node except the plugin's session/turn tracking, plus: `member.saved_for` (`zygo-workspace` state), `down` step 8 write, AU75 repair, the 2.2 clear rule in one commit helper used by `SessionStart` and the `session` event, the 2.3 note rule and `ResumeNote.agent_session` (`zygo-core`), the `session`/`prompt`/`stop` semantics of 2.1. Spec: AU60, AU92, AU104 (hook part), AU75, W§7.2, W§8.2 step 4, AU72, W§9.3 step 8, W§9.4, W§18 hook items. The plugin as it is today keeps working with H1a (its first `session` call with an id commits, and the clear is now safe), so H1a merges alone. Tests 6.2.
- **H1b — OpenCode plugin session and turn tracking.** `crates/zygo-launch/assets/opencode-plugin.js` (2.4) and its node tests (6.1). Spec: AU27, W§9.1 OpenCode paragraph and the OpenCode parts of the `busy`/`tools_running` rows, AU104 limits 1-2.
- **Interface H1b consumes from H1a** (`workspace_hook_cli`):
  - `zygo hook workspace --agent opencode --event session`, stdin `{"cwd"}`: commit the pending start of this zygo session and clear by the `saved_for` rule; idempotent; no id change; no `activity.json` change.
  - same, stdin `{"sessionID": <id>, "cwd"}`: the above, then record a non-empty id, start a snapshot when it changed, apply the note rule; no `activity.json` change.
  - `--event prompt` / `--event stop`, stdin `{"sessionID", "cwd"}`: `busy = true` / `false`, `tools_running = 0`; no session check.
  - `--event pre-tool`, stdin `{"sessionID", "tool_name": "Agent", "cwd"}`: deny JSON on stdout during drain/stopping (unchanged).
  - Exit 0 always; output only for `pre-tool`.
- **Edges:** H1a → H1b. H1b → FIX-19 (same plugin file). H1b → E2 (E2 records limit 1: `session.updated` on a TUI switch). M1, E1, FIX-22, FIX-23 depend on H1a only.
