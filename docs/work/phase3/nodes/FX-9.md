# FX-9 — OpenCode session text item 8 fixed at SessionStart, not rebuilt per request

**Spec:** — (found in the ML-9 review, F5). `docs/specs/2026-11-22-workspace-mail-design.md` M§8a item 1 says item 8 is "built at each main-loop SessionStart". The OpenCode plugin's `experimental.chat.system.transform` runs `session-start` on each model request (`crates/zygo-launch/assets/opencode-plugin.js:186`, `:190`); `run_member` reaches `workspace::session_start` (`crates/zygo-hooks/src/session_start.rs:146`), which for OpenCode "runs per model request" (`crates/zygo-hooks/src/workspace.rs:624`) and reads `workspace.json` and builds item 8 there each time (`crates/zygo-hooks/src/workspace.rs:631`). So OpenCode rebuilds item 8 on every request: a member that joins mid-session changes the system text between two requests and breaks the provider prompt cache.
**Files:** `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/session_text.rs`
**Consumes:** `zygo_hooks::mail::session_item(ws: &zygo_workspace::state::Workspace, own_key: &str) -> Option<String>` (ML-9), `zygo_hooks::mail::session_item_with(ws: &Workspace, own_key: &str, remote: &[String]) -> Option<String>` (MS-20), `remote_addresses(paths: &Paths, ws_id: &str, ws_name: &str) -> Vec<String>` (MS-20)
**Produces:** OpenCode item 8 built once per main-loop SessionStart and reused for later requests of that session; the reused item 8 includes the S§10.4 "Other machines" clause
**Acceptance:** `cargo test -p zygo-hooks --test session_text` → all pass, including a test that OpenCode's text for a second request equals its first after a member joins between them.

## Plan

Unit: phase3:FX-9:plan

### Premises (checked on `phase3/FX-9` at `c8792dd4`)

- The premise holds; the line numbers moved. `crates/zygo-launch/assets/opencode-plugin.js:186-192`: `experimental.chat.system.transform` runs `run("session-start", { input, output }, extra)` on each `zygo/` model request; `extra` is `[]` for the main role, `["--event", "subagent"]` otherwise (SX-10). The payload is `{input, output, cwd}`; `normalize` leaves it as it is (no `input.tool`, `crates/zygo-core/src/hook_opencode.rs:31-32`).
- `crates/zygo-hooks/src/session_start.rs:146`: `run_member` calls `workspace::session_start` for a member view.
- `crates/zygo-hooks/src/workspace.rs:748`: "The OpenCode call runs per model request"; `:755`: `item` = `c.store.read()` of `workspace.json`, then `session_item_with(&ws, &c.pane, &remote_addresses(..))`, on every call; `:786` OpenCode returns `Some(output)` and writes nothing. No cache of item 8 exists.
- The transform input holds `sessionID`: `fixtures/opencode/live/st-transform.jsonl` lines 1-8, keys `model,sessionID` on each `transform` line (OpenCode 1.18.16, premise ST3 in `docs/premises.md:74`). After `/compact` (line 6, `compacting`, session `ses_placeholder0001`) the next main-model requests (lines 7-8) keep the same session id.
- Session text design T§9.3: the transform text is in `output.system`, not in the message history, so a compaction cannot remove it; T§10.4 (cost table): a text that is the same on each request is part of the cached prefix.
- Claude Code's per-session state lives in the session folder (`SessionDir`, `ZYGO_SESSION_DIR`): `view.json` (resume note, `clear_note`), `activity.json` under `activity.lock`, and the per-id flags `reinject/<agent_id>` (T§9.2, `pre_compact::flag_path`, `create_flag`). The folder is per zygo launch and is removed at exit (`SessionDir::log` doc, §8.2 step 6). Claude Code needs no stored copy of item 8: its `SessionStart` runs once per start and the agent keeps `additionalContext` in its history.

### Decisions (author)

1. "Main-loop SessionStart" for OpenCode = the first `session-start` call (main role, no `--event`) of an OpenCode session id (`input.sessionID` of the payload). At that call item 8 is built (`session_item_with`, with the S§10.4 remote addresses, as today) and stored; later calls with the same id reuse the stored text byte for byte. A new id builds it again.
2. A compaction is not a new session: M§8a item 1 says "built at each main-loop `SessionStart` (so a resume gets the current names)"; T§9.3 says the transform text survives a compaction; the session id stays the same over `/compact` (`st-transform.jsonl` lines 6-8). A resume (`opencode --session <id>` in a new zygo launch) has a new session folder, so it builds item 8 again with the current names, as M§8a asks.
3. Store: the session folder, one file per OpenCode session id, in the pattern of `reinject/<agent_id>`: `<session folder>/session-item/<id>` (folder `0700`, file `0600`). The id must pass the same rule as `pre_compact::flag_path` (1 to 64 ASCII letters, digits, `_`, `-`; the work-log agent-name rule cited by T§9.2 item 1), which keeps the path inside the folder. Not `view.json`: the plugin's `session` call (`bind_note`) and `clear_note` rewrite `view.json` without a lock, so a write there from the transform could lose their change or ours. An id that fails the rule, or a payload with no id, keeps today's behavior (built at each call, nothing stored); nothing is refused.
4. Concurrency: the file is written to a unique temp file next to it (`0600`, synced), then hard-linked to its name, which fails when the name exists (the no-clobber form of `write_atomic`'s temp-then-rename); the temp file is removed. The call that loses the race reads the winner's file and returns that text, so two calls for a new id give one text.
5. Only a built item is stored. `None` (no member record for the pane, `workspace.json` that does not read) stores nothing, so the next call builds again; a stored file is never replaced. With `view.mail` off nothing is read or stored. A stored entry that is a link, not a regular file or not readable: the call builds item 8 and stores nothing, with one session-log line; a store that fails: the built text, one session-log line.
6. Hook point: `workspace::session_start_with_sync` (where item 8 is built), OpenCode branch only. The plugin is not changed: it already sends `input.sessionID`. The `--event subagent` branch returns before the member part (`session_start.rs:107-109`) and is not touched. Claude Code is not changed.
7. Out of scope (reported): the resume note and the M§7.2 delivery line in the OpenCode output still follow the state at each call.

### Consumes

- `zygo_hooks::mail::session_item(ws: &Workspace, own_key: &str) -> Option<String>`
- `zygo_hooks::mail::session_item_with(ws: &Workspace, own_key: &str, remote: &[String]) -> Option<String>`
- `zygo_hooks::mail::remote_addresses(paths: &Paths, ws_id: &str, ws_name: &str) -> Vec<String>` (`pub(crate)`)
- `zygo_hooks::pre_compact::flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf>`

### Produces

- `zygo_hooks::session_start::item_path(dir: &SessionDir, agent_session: &str) -> Option<PathBuf>`: `<dir>/session-item/<agent_session>` when the id passes the `flag_path` rule.
- `zygo_hooks::session_start::fixed_item(dir: &SessionDir, agent_session: &str, build: &mut dyn FnMut() -> Option<String>, log: &dyn Fn(&str)) -> Option<String>`: the stored item 8 of the OpenCode session, else `build()` stored first-writer-wins (Decisions 3-5).
- `workspace::session_start_with_sync` (signature unchanged): an OpenCode member with `view.mail` on and a payload `input.sessionID` gets item 8 from `fixed_item`.

### Steps

- [ ] 1. Failing tests in `crates/zygo-hooks/tests/session_text.rs` through `session_start::run_member` with an OpenCode payload `{"input": {"sessionID": .., "model": {"providerID": "zygo", "id": "lead"}}, "output": {"system": []}}`:
  - `opencode_item_8_is_fixed_for_the_session_after_a_member_joins` (Acceptance): the second request's text equals the first after `lena` joins;
  - `opencode_new_session_after_a_join_gets_the_new_member`: a new id gets `lena`; the first id keeps its text;
  - `opencode_fixed_item_8_holds_the_other_machines_clause`: sync on and `bob@b` in the member cache: the first text holds the S§10.4 clause; after the cache gains `eve@b` the session keeps `bob@b` only; the stored file is item 8;
  - `opencode_stored_item_8_is_used_as_it_is`: a file already stored for the id (another call won) is the item 8 of the output;
  - `claude_code_item_8_is_built_at_each_session_start`: Claude Code gets `lena` at its next `SessionStart` and no `session-item` folder is made (unchanged behavior; passes before and after);
  - `opencode_session_id_that_fails_the_rule_is_not_stored`: id `../x` and no id: built at each call, no `session-item` folder (unchanged behavior).
  Run `cargo test -p zygo-hooks --test session_text`; see the first four fail. Commit `FX-9: failing tests for a fixed OpenCode item 8`.
- [ ] 2. Failing tests for the store: `fixed_item_loser_of_a_race_returns_the_winner` (the `build` closure stores another text first, so the link finds the name taken) and `fixed_item_from_threads_gives_one_text` (8 threads, a `Barrier`, each with its own text). See them fail (they do not compile: `fixed_item` does not exist). Commit `FX-9: failing tests for the item 8 store`.
- [ ] 3. Implement `item_path`, `fixed_item` and the temp-then-link write in `session_start.rs`; call it from `session_start_with_sync` for `Agent::OpenCode` with `view.mail` and `input.sessionID`; update the doc comments of `session_start`, `session_start_with_sync` and `run_member`. See all of `--test session_text` pass. Commit `FX-9: OpenCode item 8 fixed at the session's first request`.
- [ ] 4. Mutation: make the OpenCode path build item 8 at each call again; see the Acceptance test fail; revert, touch the file, see it pass.
- [ ] 5. Run `cargo test -p zygo-hooks --locked --no-fail-fast`, `cargo test -p zygo-launch --locked --test opencode`, the `crates/zygo/tests` files that cover session text, mail session items and OpenCode, and `cargo clippy -p zygo-hooks --locked --all-targets` against the baseline taken at `c8792dd4` (5 warnings, all in `tests/shaping_latency.rs`, `tests/shaping.rs`, `tests/shaping_hook.rs`). Tick the steps; commit; push.
