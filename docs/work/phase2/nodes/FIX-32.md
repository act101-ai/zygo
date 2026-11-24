# FIX-32 — Whole-branch review fixes: OpenCode short clusters, inline --settings, ws run live launcher, attribution writer, README drain line

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W-C7, W-C9, AU30; `docs/specs/2026-11-18-zygo-design.md` §13.4 (attribution guard row and its known limits). Created 2026-11-23 from the phase 2 whole-branch review (controller decisions: blocking findings B1-B4 and one README line R1; minors deferred, see "Deferred findings").
**Files:** `crates/zygo-workspace/src/args.rs`, `crates/zygo-workspace/tests/args.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/tests/workspace_run.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-work/src/guards.rs`, `crates/zygo-work/tests/guards.rs`, `docs/specs/2026-11-20-workspaces-design.md`, `docs/specs/2026-11-18-zygo-design.md`, `README.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-workspace -p zygo -p zygo-work --locked` → all pass, including the new tests of B1-B4 below
**Capability:** implementation

## Plan

Work test first: for each item, write the tests, run them red, fix, run them green.

**B1. OpenCode short clusters use the yargs-parser rule** (`crates/zygo-workspace/src/args.rs`, cluster walk).
- [x] Confirm the yargs-parser short-flag loop in `/home/kagemartin/.opencode/bin/opencode` (read with `python3 -I`); record offset and quote below.
- [x] Tests in `crates/zygo-workspace/tests/args.rs`: `-mc` (no value for `-m`, `-c` is `--continue`, `has_session_arg` true, restore adds no `--session`); `-ms ses_1` (`-s` takes `ses_1`, `has_session_arg` true); the `-sses_1` case re-derived from the confirmed rule; a Claude Code case that keeps today's rule.
- [x] Test in `crates/zygo/tests/workspace_run.rs`: a saved OpenCode launch `-mc` with a resumable session is restored with no `--session`.
- [x] Run → FAIL.
- [x] Make the attach rule of a cluster letter depend on the agent: Claude Code keeps Commander's rule; OpenCode uses the confirmed yargs rule.
- [x] Run → PASS.

**B2. An inline `--settings` JSON value is never saved** (W-C7, AU30).
- [x] Tests: `--settings '{"env":{"K":"v"}}'` is removed and the label names `--settings`; `--settings=/abs/s.json` is kept; `--settings ./s.json` is kept as typed.
- [x] Run → FAIL.
- [x] Add the smallest mechanism for a class that depends on the value; rewrite the AU30 `--settings` sentence; change-log row.
- [x] Run → PASS.

**B3. `ws run` refuses to resume while that conversation's launcher is live** (W-C9; `crates/zygo/src/cmd/member.rs`, the `None` branch of the session callback).
- [x] Test in `crates/zygo/tests/workspace_run.rs`: a record whose launcher is live, then `ws run` for that pane: the W-C9 line, no agent start, an unchanged record.
- [x] Run → FAIL.
- [x] In the `update_existing` closure, before `record_start`, apply the live-launcher check of the hand-start path (`live_session`, `Conflict::Live`); the callback fails and the agent is not spawned.
- [x] Run → PASS.

**B4. The attribution guard reads what the same command writes** (`crates/zygo-work/src/guards.rs`, `find_attribution`).
- [x] Tests in `crates/zygo-work/tests/guards.rs`: heredoc writer with a trailer then `git commit -F msg.txt`: denied, heredoc named; clean heredoc writer while the file on disk holds a trailer: allowed; a pre-existing file with a trailer and no writer: denied.
- [x] Run → FAIL.
- [x] (a) a file named by `-F`, `--body-file` or `--notes-file` that an earlier segment writes (`>`, `>>`, `tee`) is not read from disk; that segment's heredocs and command text are searched; (b) the heredoc bodies of every earlier segment are searched. Update the §13.4 guard text; change-log row.
- [x] Run → PASS.

**R1. README.** One sentence in the `zygo down` part: a drain that times out puts the workspace back up and stops nothing (`crates/zygo-workspace/src/down.rs`, `time_out`).
- [x] Add the sentence.

- [x] Measure: `cargo test -p zygo-workspace -p zygo -p zygo-work --locked --no-fail-fast`; `cargo clippy -p zygo-workspace -p zygo -p zygo-work --all-targets --locked -- -D warnings`; `cargo test --workspace --locked --no-run`; `sh scripts/check-premises.sh`.
- [x] Commit.

## Results

### B1 evidence: the yargs-parser short-flag loop of OpenCode 1.19.6

Read from `/home/kagemartin/.opencode/bin/opencode` with `python3 -I`: a byte search for `slice(1,-1).split("")` found one hit. Offset 125729461 is the start of `slice(1,-1).split("")`. Just before it, the branch opens with `else if(v.match(/^-[^-]+/)&&!v.match(_)){`. The quote runs from `L=` to the end of the last-letter branch:

```js
L=v.slice(1,-1).split(""),P=!1;for(let Ve=0;Ve<L.length;Ve++){if(q=v.slice(Ve+2),L[Ve+1]&&L[Ve+1]==="="){if(je=v.slice(Ve+3),E=L[Ve],Y(E,y.arrays))F=ue(F,E,o,je);else if(Y(E,y.nargs)!==!1)F=se(F,E,o,je);else W(E,je);P=!0;break}if(q==="-"){W(L[Ve],q);continue}if(/[A-Za-z]/.test(L[Ve])&&/^-?\d+(\.\d*)?(e-?\d+)?$/.test(q)&&Y(q,y.bools)===!1){W(L[Ve],q),P=!0;break}if(L[Ve+1]&&L[Ve+1].match(/\W/)){W(L[Ve],q),P=!0;break}else W(L[Ve],ie(L[Ve]))}if(E=v.slice(-1)[0],!P&&E!=="-")if(Y(E,y.arrays))F=ue(F,E,o);else if(Y(E,y.nargs)!==!1)F=se(F,E,o);else if(q=o[F+1],q!==void 0&&(!/^(-|--)[^-]/.test(q)||q.match(_))&&!Y(E,y.bools)&&!Y(E,y.counts))W(E,q),F++;else if(/^(true|false)$/.test(q))W(E,q),F++;else W(E,ie(E))
```

The confirmed rule:
- `v` is the token, `L` is its letters without the last character, and `q` is the rest of the token after letter `L[j]`.
- For each letter `j` of `L` (every letter except the last character), in this order:
  1. `L[j+1]` is `=`: the letter takes the text after `=`. The walk ends.
  2. `q` is exactly `-`: the letter takes `-`. The walk continues, but no letter is left, and a last character `-` takes nothing.
  3. The letter is `[A-Za-z]` and `q` matches `^-?\d+(\.\d*)?(e-?\d+)?$`, and `q` is not a boolean key: the letter takes `q`. The walk ends.
  4. `L[j+1]` matches `\W`: the letter takes `q`, **and `q` includes that character**. The walk ends.
  5. In all other cases, the letter is set with its type default and no value. The walk continues.
- When the walk did not end and the last character is not `-`, the last character takes the next token by its type: an array or nargs option takes by its count; any other option takes the next token unless that token looks like a flag; a boolean takes only `true` or `false`.

Two points differ from the rule that the brief expected:
- The `\W` branch gives the letter `q`, and `q` includes the non-word character (`W(L[Ve],q)` with `q=v.slice(Ve+2)`). Only the `=` branch drops the character. So `-m/x` gives `-m` the value `/x`.
- Steps 1 and 4 read `L[j+1]`, and `L` does not hold the last character. So the loop never sees the character after the second-to-last letter as a "next character": `-m/` is `-m` with no value, and `/` is the last letter. Step 2 (`q==="-"`) covers the one case `-m-`.

The rule is in `attached_value` in `crates/zygo-workspace/src/args.rs` (the OpenCode branch), and `yargs_number` holds the pattern of step 3. The Claude Code rule (Commander) does not change.

`-sses_1` with the confirmed rule:
- The letters `s`, `s`, `e`, `s` and `_` each come before a word character, so each one is set with no value (step 5).
- The last character `1` takes the next token by its type.
- So the cluster holds `-s` three times, each with no value.

The cluster is still removed whole, because it holds a session letter. So the "gone" case in `opencode_rules` keeps its result, and the comment there now cites the offset. The old expectation in `occurrences_give_flag_values_before_double_dash` (`--session` with the value `ses_1`) came from Commander's rule. It is corrected to three `--session` with no value, and a new `-s=ses_1` case gives the value.

### B2

- The mechanism is `Class::PathOrInline` in `args.rs`. It is the smallest one: a single class whose `saved_value` depends on the value.
- A value whose first non-space character is `{`:
  - is removed with the flag;
  - adds the flag name to `dropped_flags`, so `up` prints `zygo: pane <key>: --settings had an inline value; not restored`, as it does for a content flag;
  - makes `removed_labels` name `--settings`.
- Any other value is kept as typed.
- `--settings` with no value stays kept, as before.

Note: base spec §8.5 Flags says "A value is never judged by its first character". That sentence is about the launch-time reader of `--settings`, which parses the value as JSON. It does not apply to the save filter. The save filter uses the first-character rule, as the controller decided. Both texts stay, and AU30 now says which rule the save filter uses.

### B3

- In the `None` branch (`ws run`), the callback's locked write now calls `live_session` on the record before `record_start`. `hand_write` and `conflict` use this same function.
- When it finds a live launcher, the closure fails with `Conflict::Live { .. }.line(key)`, which is the W-C9 line of the hand start. The record does not change.
- **Not as the brief expected: the launcher did not stop on a callback error.** `launcher.rs` says "an error never stops the launch (C6)" (AU34). So a refused callback alone still spawns the agent.
  - I added `zygo_launch::launcher::StopLaunch`. (The name `Refused` is already taken in `launcher.rs` by `serve_client::Refused`.)
  - On this error, the launcher logs it, spawns no agent and returns `signals::or_stopped(1)`. This is the same return as the team-config failure just after it.
  - AU34 and W§8.2 step 4 now say this, with a change-log row.
- **Not as the brief expected: `ws run` does not exit non-zero.** W§8.2 step 8 says every path of `ws run` after the id checks ends in the pane's shell (`exec_shell`), and this path does too.
  - The launch outcome is code 1, with no spawn.
  - After the W-C9 line, the wrapper prints `zygo: the claude launch failed before the agent started (exit 1)`.
- The test checks:
  - the W-C9 line and the exit-1 line;
  - that no agent started (`argv.json` is absent);
  - that the record and all of `workspace.json` are byte-equal to before;
  - that the shell opens in the pane folder.

  It does this for a committed `zygo_session` and for an uncommitted `pending.zygo_session` of the live launcher.

### B4

- (b) `find_attribution` searches the heredoc bodies of the message command and of every segment before it in the line.
- (a) For each file argument, the guard looks for the last earlier segment that writes the same path, and uses it in place of the file on disk.
  - `written_files` finds these writes:
    - `>`, `>>`, `>|`, `&>` and `&>>` redirections, with an optional descriptor number, with the target in the same word or the next one;
    - `tee` operands.

    A `>&` duplication is not a write.
  - The guard searches the words of the writing segment and of the segments piped into it. The place is ``the command that writes `<path>` ``. Step (b) already searched the writer's heredoc bodies.
  - The writer's path is resolved from that segment's own directory (`segment_dirs`), and the two paths are compared with `.` parts left out.
- One existing test expectation changed with (b): `cat <<'EOF' > notes.txt` (with a trailer), then `git commit -m "docs: notes"`. It was in the "allowed" list of `attribution_searches_only_the_message_command`. It is now in that test's "denied" list, with the place "a heredoc". This follows the controller's decision for (b).
- The base spec §13.4 (the attribution row and its known limits) is rewritten, with a change-log row. A new known limit: when the line writes a message file in another way (`cp`, `mv`, a shell `-c` string), the guard reads the file from disk as it was before the line ran.

### R1

The README `zygo down` bullet now says: "A drain that times out stops nothing: the workspace is up again, and `zygo down` names each busy pane and exits 1." The evidence is `time_out` in `crates/zygo-workspace/src/down.rs`:
- `revert` with "drain timed out after <n> s; phase is up again";
- one `busy:` line per pane;
- `fail(1)`.

### Red, then green

- Red:
  - `args`: 2 failed. In `opencode_clusters_follow_the_yargs_rule`, `-mc` gave `[("--model", ["c"])]`. In `settings_keeps_a_path_and_removes_inline_json`, the inline value was kept.
  - `guards` attribution: 2 failed. The heredoc writer was allowed, and the earlier heredoc was allowed.
  - `workspace_run`: 2 failed. The argv was `["--session=ses_1", "-mc"]`, and there was no W-C9 line.
- Green in the checkout: see Measure.

### Measure (checkout `.worktrees/phase2/FIX-32`)

- `cargo test -p zygo-workspace -p zygo -p zygo-work -p zygo-launch --locked --no-fail-fast`: exit 0.
  - 56 suites: 1610 passed, 0 failed, 13 ignored.
  - Changed suites: `args` 35, `guards` 76, `workspace_run` 54, `workspace_member` 44.
  - zygo-launch is in the run because `launcher.rs` changed.
- `cargo clippy -p zygo-workspace -p zygo -p zygo-work -p zygo-launch --all-targets --locked -- -D warnings`: exit 0.
- `cargo test --workspace --locked --no-run`: exit 0.
- `sh scripts/check-premises.sh`: ok (P1 P2 P3 P4 P5 P6a P6b Q1-Q7 PW6 PW11 PW15 PW1-PW5 PW7 PW9 PW10 PW16). Note: N2_FORBIDDEN_NAMES was not set, so the name scan was skipped.

## Deferred findings

The controller's decisions, word for word:
- m1: `down` step 8 save failure exits 0: deferred (needs a write failure such as a full disk; the state stays consistent).
- m8: `-h`/`-v` kept: rejected (a launch with them never saves a session).
- m18: `serve` sessions have no idle expiry: rejected (by design; the operator's no-TTL rule).
- m20: the update download is not signed: deferred (not new on this branch).
- m2-m7, m9-m17, m19: deferred (text, hints, hardening, or edge cases with no user-reported path); see the review in the PR.

## Merge

Merged `worktree-zygo-phase-2--FIX-32` (444a952) into `worktree-zygo-phase-2` with `--no-ff`, with no conflict. Checks on the merged tree:
- `cargo test -p zygo-workspace -p zygo -p zygo-work -p zygo-launch --locked --no-fail-fast`: exit 0.
  - 56 suites: 1610 passed, 0 failed, 13 ignored.
  - Changed suites: `args` 35, `guards` 76, `workspace_run` 54, `workspace_member` 44.
- `cargo clippy -p zygo-workspace -p zygo -p zygo-work -p zygo-launch --all-targets --locked -- -D warnings`: exit 0.
- `cargo test --workspace --locked --no-run`: exit 0.
- `sh scripts/check-premises.sh`: ok. The name scan was skipped because N2_FORBIDDEN_NAMES was not set.
- `sh scripts/test-install.sh`: all passed.
