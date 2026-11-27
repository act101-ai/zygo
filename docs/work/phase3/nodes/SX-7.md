# SX-7 — Base-spec amendments for session text (§8.5, §8.6 item 7, §8.6 last paragraph, §8.7 N33, §10.6)

**Spec:** docs/specs/2026-11-24-session-text-design.md §13 (all bullets but the last), §8.5
**Files:** `docs/specs/2026-11-18-zygo-design.md`, `crates/zygo-hooks/tests/reinject.rs` (one doc comment, line 22; author's call at planning, see Premise check)
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-config --test builtin && sh scripts/check-premises.sh` → all pass, exit 0 (the base-spec sections that tests and the premise script read stay intact). Review of `git diff -- docs/specs/2026-11-18-zygo-design.md` against T§13: each of the five bullets (§8.5 hooks list, §8.6 item 7 parenthesis, §8.6 new last paragraph, §8.7 N33, §10.6 flag sentence) is present with the meaning T§13 gives, and no other base-spec text changes.

## Plan

### Premise check (live code, branch `phase3/SX-7` at `c8ed1282`, `origin/phase-3` after WD-16)

T§13 was read against the code that SX-3, SX-4, SX-5 and SX-6 merged. Where they disagree, the merged code wins, and T§13 was rewritten as final text in the planning commit, with a line in the session text design's `## Change log` (2026-11-26, author's call).

| T§13 bullet | Code | Result |
|---|---|---|
| §8.5: `SubagentStart` → `zygo hook session-start`, every launch, before a member's workspace entry | `crates/zygo-launch/src/claude_code.rs:521-526` (one `session-start` entry, no matcher; `workspace --event subagent-start` pushed second for a member); tests `crates/zygo-launch/tests/claude_code.rs:1757` (plain launch, passthrough included), `:1773` (member order) | Agrees. Bullet unchanged. |
| §8.6 item 7: Claude Code `zygo-<role>` `--agents` prompt | `crates/zygo-launch/src/claude_code.rs:397`, `:406` (`format!("{prompt} {mech}")`) | Agrees. |
| §8.6 item 7: OpenCode | The transform is not the only OpenCode channel: each OpenCode `zygo-<role>` prompt still holds the text (`crates/zygo-launch/src/opencode.rs:139-143`) until the ST4 row (T§7.2 item 3, SX-10). | Disagrees. T§13 rewritten: the item points to §8.7 for the OpenCode `zygo-<role>` prompt, so SX-10 changes only §8.7. |
| §8.6 item 7: "again after that subagent's compaction" | The copy after a subagent's compaction comes from `zygo hook trim` (`crates/zygo-hooks/src/trim.rs:227-262`, `take_flag` at `:243`); `session-start` prints nothing for a subagent `SessionStart` (`crates/zygo-hooks/src/session_start.rs:40-48`). | The T§13 sentence put the copy under `session-start`. Rewritten: "again by `zygo hook trim`". |
| §8.6 item 7: OpenCode receivers "a `zygo/` role other than the main role" | `crates/zygo-launch/assets/opencode-plugin.js:186-192`: `role && input.model.id === role ? [] : ["--event","subagent"]`, so an empty or unset `ZYGO_MAIN_ROLE` sends every `zygo/` request `--event subagent`; test `crates/zygo-launch/tests/opencode.rs:884` | Missing case. Rewritten: "or on any `zygo/` model when no main role is set". |
| §8.6 last paragraph: "the main loop gets the text again from `SessionStart` with `source: compact`" | `SessionStart` has no matcher (`crates/zygo-launch/src/claude_code.rs:509-512`); `run_in` does not read `source` (`crates/zygo-hooks/src/session_start.rs:24-34`); test `crates/zygo-hooks/tests/subagent_text.rs:214`. `SessionStart` is a Claude Code event; the OpenCode main agent gets the text per request from the transform. | Rewritten: "the Claude Code main loop". |
| §8.7 N33: subagents and the title agent get the mechanics text only | `crates/zygo-launch/assets/opencode-plugin.js:186-192`; `small_model` is `zygo/read` or `zygo/<main_role>` (`crates/zygo-launch/src/opencode.rs:157`) | Agrees; the no-main-role case is added, as above. |
| §10.6: "For a subagent (an input with `agent_id`) the hook also sets the flag" | `crates/zygo-hooks/src/pre_compact.rs:33-44`: the flag is created only when `agent_type` does not start with `zygo-` and `agent_id` passes `flag_path` (`:24-28`: 1 to 64 of `[A-Za-z0-9_-]`); the printed text does not change; tests `crates/zygo-hooks/tests/reinject.rs:54`, `:84`, `:91` | Too wide. Rewritten with the `zygo-` exclusion and the name rule. |
| WD-16 | Changed only §6.1 of the base spec (commit `175e2041`; change-log row "2026-11-26 · §6.1 · … node WD-16") | No overlap with §8.5 to §10.6. |

Readers of the base spec that the edits can break: `crates/zygo-config/tests/builtin.rs:136-150` reads §6.5 to the `## 7.` heading; `scripts/check-premises.sh:54-56` reads §19, §19a and §10.5 up to the `### 10.6 ` heading. No step changes those headings. `crates/zygo-hooks/tests/reinject.rs:22` cites `docs/specs/2026-11-18-zygo-design.md:520` in a doc comment; step 3 moves that text to line 522, so step 6 changes the comment to a section citation (author's call: a stale line number is a defect, and a section citation does not go stale again; one comment line, no behavior). Other line citations of the base spec are in closed node files (`SX-3.md`, `OS-44.md`, phase 2) and stay as written history.

`zygo decision check` checks the records in `docs/decisions/`, which this node does not change; it is not part of the verification. The base spec has no other docs test.

### Interfaces

Consumes: —. Produces: —.

### Steps

Each edit is an exact string replacement in `docs/specs/2026-11-18-zygo-design.md`. Each old string occurs once in the file now (checked at planning). Change only the text named.

- [ ] **1. §8.5, hooks list (line 386).** Replace
  `` `SessionStart` → `zygo hook session-start`, `PreToolUse` `Edit|Write|NotebookEdit` ``
  with
  `` `SessionStart` → `zygo hook session-start`, `SubagentStart` → `zygo hook session-start` (every launch; for a member, before `workspace --event subagent-start`), `PreToolUse` `Edit|Write|NotebookEdit` ``.
  Check: `grep -cF '`SubagentStart` → `zygo hook session-start` (every launch; for a member, before `workspace --event subagent-start`)' docs/specs/2026-11-18-zygo-design.md` → `1`.

- [ ] **2. §8.6 item 7 (line 402).** Replace
  `7. The mechanics text (also appended to each subagent prompt): `
  with
  `` 7. The mechanics text (also given to each subagent: in the prompt of each `zygo-<role>` agent, built at launch, on Claude Code (§8.5 Subagents) and on OpenCode as §8.7 states; on Claude Code by `zygo hook session-start` on `SubagentStart` to every other subagent whose type does not start with `zygo-`, and again by `zygo hook trim` after that subagent's compaction (session text design T§9.2); on OpenCode by the system-prompt transform on each request whose model is a `zygo/` role other than the main role, or on any `zygo/` model when no main role is set (session text design T§7)):  ``
  (the item's sentences after the colon do not change).
  Check: `grep -cF 'and again by `zygo hook trim` after that subagent' docs/specs/2026-11-18-zygo-design.md` → `1`; `grep -cF 'also appended to each subagent prompt' docs/specs/2026-11-18-zygo-design.md` → `0`.

- [ ] **3. §8.6, new last paragraph (after line 405).** After the line `Nothing in the text names a context window, a size or a price (C7).` insert one empty line and the line
  `` After a compaction, the Claude Code main loop gets the text again from `SessionStart` with `source: compact`; see `docs/specs/2026-11-24-session-text-design.md` T§9. ``
  so §8.6 ends with this paragraph, one empty line, then `### 8.7 OpenCode`.
  Check: `grep -nF 'After a compaction, the Claude Code main loop gets the text again' docs/specs/2026-11-18-zygo-design.md` → one line, number 407; the next non-empty line is `### 8.7 OpenCode`.

- [ ] **4. §8.7, N33 note.** Replace
  `so subagents and the title agent do not get it;`
  with
  `` so subagents and the title agent get the mechanics text only, and with no main role (a direct model id) every request on a `zygo/` model gets the mechanics text only (session text design T§7.2); ``.
  Check: `grep -cF 'so subagents and the title agent get the mechanics text only, and with no main role' docs/specs/2026-11-18-zygo-design.md` → `1`; `grep -cF 'the title agent do not get it' docs/specs/2026-11-18-zygo-design.md` → `0`.

- [ ] **5. §10.6.** Replace
  `Never names a window or size.`
  (end of the §10.6 paragraph) with
  `` Never names a window or size. For a Claude Code subagent whose `agent_type` does not start with `zygo-`, and whose `agent_id` passes the agent-name rule of the work log (1 to 64 ASCII letters, digits, `_` and `-`), the hook also creates the empty flag `$ZYGO_SESSION_DIR/reinject/<agent_id>` of session text design T§9.2; the text it prints does not change. ``
  Check: `grep -cF 'the hook also creates the empty flag `$ZYGO_SESSION_DIR/reinject/<agent_id>`' docs/specs/2026-11-18-zygo-design.md` → `1`; `grep -n '^### 10.6 Compaction instructions$' docs/specs/2026-11-18-zygo-design.md` → one line.

- [ ] **6. Stale line citation.** In `crates/zygo-hooks/tests/reinject.rs:22` replace `` (`docs/specs/2026-11-18-zygo-design.md:520`) `` with `` (`docs/specs/2026-11-18-zygo-design.md` §10.6) ``. Nothing else in the file changes.
  Check: `grep -cF 'zygo-design.md:520' crates/zygo-hooks/tests/reinject.rs` → `0`.

- [ ] **7. Base-spec change log.** Append five rows to the table under `## Change log` at the end of `docs/specs/2026-11-18-zygo-design.md`, after the WD-16 row, in its four columns. `<D>` is the UTC date of the commit (`date -u +%F`).
  ```
  | <D> | §8.5 (settings file hooks) | session text design T§13 (approved 2026-11-24); author's call (node SX-7) for the text | Every launch registers `SubagentStart` → `zygo hook session-start`, before a member's workspace entry (`crates/zygo-launch/src/claude_code.rs:521-526`). |
  | <D> | §8.6 item 7 | session text design T§8.5, T§13 (approved 2026-11-24; T§13 rewritten 2026-11-26, author's call); author's call (node SX-7) for the text | The item names the channel for each receiver: the `zygo-<role>` prompt, `SubagentStart`, `zygo hook trim` after a subagent's compaction, the OpenCode transform (`crates/zygo-hooks/src/session_start.rs:40-48`, `crates/zygo-hooks/src/trim.rs:227-262`, `crates/zygo-launch/assets/opencode-plugin.js:186-192`). |
  | <D> | §8.6 (last paragraph) | session text design T§9.1, T§13; author's call (node SX-7) for the text | After a compaction the Claude Code main loop gets the text from `SessionStart` with `source: compact`; the entry has no matcher (`crates/zygo-launch/src/claude_code.rs:509-512`). |
  | <D> | §8.7 (N33 note) | session text design T§7.2, T§13; author's call (node SX-7) for the text | Requests on another `zygo/` role, and every `zygo/` request when no main role is set, get the mechanics text (`crates/zygo-launch/assets/opencode-plugin.js:186-192`). |
  | <D> | §10.6 | session text design T§9.2, T§13; author's call (node SX-7) for the text | `zygo hook pre-compact` also creates the T§9.2 flag for a subagent that is not `zygo-` and whose id passes the agent-name rule (`crates/zygo-hooks/src/pre_compact.rs:24-44`). |
  ```
  The change-log rows are required by the execute-work procedure (§5.4) and are not "other base-spec text" in the Acceptance review.
  Check: `grep -c '(node SX-7) for the text' docs/specs/2026-11-18-zygo-design.md` → `5`.

- [ ] **8. Cited lines hold.** Each code citation in steps 1 to 7 still points at the code named: `sed -n '521,526p' crates/zygo-launch/src/claude_code.rs` shows the `subagent_start` vector and its insert; `sed -n '509,512p'` of the same file shows the `SessionStart` insert; `sed -n '40p;48p' crates/zygo-hooks/src/session_start.rs` shows `fn subagent(` and its closing `}`; `sed -n '227p;262p' crates/zygo-hooks/src/trim.rs` shows the T§9.2 doc line and the closing `}` of `take_flag`; `sed -n '186p;192p' crates/zygo-launch/assets/opencode-plugin.js` shows the transform key and its closing `},`; `sed -n '24p;44p' crates/zygo-hooks/src/pre_compact.rs` shows `pub fn flag_path(` and the closing `}` of `run_with`. A citation that moved is corrected in the row before the commit.

- [ ] **9. Diff scope.** `git -C <checkout> diff --stat` → only `docs/specs/2026-11-18-zygo-design.md` and `crates/zygo-hooks/tests/reinject.rs`. `git -C <checkout> diff -- docs/specs/2026-11-18-zygo-design.md` shows only the five edits of steps 1 to 5 and the five rows of step 7.

- [ ] **10. Acceptance.** `cargo test -p zygo-config --test builtin && sh scripts/check-premises.sh` → `test result: ok. 13 passed; 0 failed` and `check-premises: ok (…)`, exit 0 (baseline at planning: 13 passed; script ok). Then `cargo test -p zygo-hooks --test reinject` → 0 failed (the package step 6 touched).

- [ ] **11. Commit.** Message `SX-7: base-spec amendments for session text (T§13)`, then the `Zygo-Session` and `Zygo-Work: SX-7` trailers that the implementer's brief names, each on its own line. No Claude attribution.

### Author's checks (§3.3), run at planning

1. Spec coverage: T§13 bullets 1-5 map to steps 1-5; the T§13 change-log line is in the planning commit; base-spec change log is step 7. Bullet 6 (after the ST4 row) belongs to SX-10 and is not in scope.
2. Step scan: each step gives the exact old and new text; no step leaves a choice.
3. Interface match: no Consumes or Produces. Dependents OS-39 (base spec, after SX-7) and SX-10 (§8.7 ST4 sentence) edit other text; step 2 points the OpenCode prompt case to §8.7, so SX-10 needs no edit of item 7.
4. Review focus: no review-focus line names SX-7.
5. Proportion: the plan is the text to write; no code.
6. Live code: every path and line above was opened at `c8ed1282`; the anchors of steps 1 to 5 each occur once (`grep -noE`, planning); the baseline of step 10 passed.
