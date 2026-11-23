# FIX-23 — zygo's settings merge line is true when the user sets disableAllHooks

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §8.5 (Flags: the merge line; a user `disableAllHooks: true` wins and stops every hook, `zygo`'s included, while `attribution` and `includeCoAuthoredBy` always stay `zygo`'s). Defect: with a user `--settings` value that holds `disableAllHooks: true`, `ClaudeCode::build` (`crates/zygo-launch/src/claude_code.rs:458-511`; the line is pushed at `:494-497`) still prints `MERGE_WARN` (`claude_code.rs:22`), `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`, but the merged file then stops every hook, so the line is false. Found in the FIX-21 merge review (2026-11-22). Line numbers checked on `7557a60` (H1 merged). Plan review 2026-11-23, controller decision F1: a user `disableAllHooks` that is not `true` or `false` is not used (schema type boolean, `fixtures/claude-code/settings-schema.json:2398-2400`).
**Files:** `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** `ClaudeCode::settings` (FIX-21)
**Produces:** a §8.5 Flags merge line that `build` prints and that is true for the merged settings: it does not say the hooks stay in force when the merged user settings set `disableAllHooks: true`
**Acceptance:** `cargo test -p zygo-launch --locked --test claude_code` → all pass, including a case where a user `--settings` value with `disableAllHooks: true` gives a stderr line that does not say the hooks stay in force

## Plan

Premise check (2026-11-23, on `7557a60`): the defect is live. `ClaudeCode::settings` (`claude_code.rs:387-422`) writes `"disableAllHooks": false`; `user_settings` (`:162-178`) extends `UserSettings.merged` with each readable value; `merge_settings` (`:191-224`) keeps a user `disableAllHooks` as an ordinary key that wins (`:218-220`), whatever its JSON type; `build` (`:458-511`) merges, writes the file (`:471`), and at `:494-497` pushes `MERGE_WARN` (`:22`) when `u.read > 0`, whatever the merged file holds. The launcher prints every `ChildSpec.lines` entry to stderr as is (`crates/zygo-launch/src/launcher.rs:600-602`, `out.line(l)`). H1 changed `claude_code.rs` only in `hooks` (member entries); `MERGE_WARN`, `user_settings`, `merge_settings` and the merge branch of `build` are as FIX-21 left them. The schema types `disableAllHooks` as a boolean and describes it as "Disable all hooks and statusLine execution" (`fixtures/claude-code/settings-schema.json:2398-2400`). The contract's old line reference (`:474-478`) is corrected above.

Interfaces, as they exist now:

- Consumed: `ClaudeCode::settings(&self, ctx: &LaunchCtx, env: &[(String, String)]) -> Value` (private, `claude_code.rs:387`), the zygo file that `build` merges into; its `"disableAllHooks"` is `false`.
- Kept as is: `const MERGE_WARN: &str` (`claude_code.rs:22`), text unchanged; `fn merge_settings(zygo: Value, user: Map<String, Value>) -> Value`, code unchanged (only its doc comment changes); `fn unreadable_line(value: &str) -> String`, unchanged.
- Changed, private: `struct UserSettings` gains `bad_hooks: Vec<Value>` (doc: the user `disableAllHooks` values that are not `true` or `false`, in order; zygo does not use them). `fn user_settings(ctx: &LaunchCtx) -> UserSettings` keeps its signature.
- New, private to `claude_code.rs`:
  - `const MERGE_HOOKS_OFF: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included";`
  - `fn bad_hooks_line(value: &Value) -> String`, giving `format!("zygo: your --settings disableAllHooks value {value} is not true or false and was not used")`. `Value`'s `Display` is compact JSON and escapes `"`, `\` and control characters inside strings, the same rendering `unreadable_line` uses (`Value::from(value)` in `format!`); `unreadable_line` takes a `&str` and renders it as a JSON string, so it cannot render a number, `null` or an object and is not reused.
- No public name is added. FIX-24 consumes nothing from this node; its signal is the same merged file (`<session folder>/settings.json`, top-level `disableAllHooks` equal to the JSON value `true`), which after this node is always a boolean.

Decisions:

- D1 (author's call). The merge line reads the merged file, not the user values: `build` prints `MERGE_HOOKS_OFF` in place of `MERGE_WARN` exactly when the merged settings value holds the top-level key `disableAllHooks` equal to `Value::Bool(true)`. So a later user value `disableAllHooks: false` that wins over an earlier `true` gives `MERGE_WARN`, and the line always describes the file the agent runs with. The test `Value::Bool(true)` is the one the §8.4 preflight uses (`conflict`, `claude_code.rs:249`) and the one FIX-24's `drain::hooks_off` uses.
- D2 (author's call; status line by controller decision F7, 2026-11-23). The new line keeps the `MERGE_WARN` prefix up to the first `;`, then states only what is true for that file: the attribution keys (settings keys, not hooks) stay zygo's; the user's `disableAllHooks` stops every hook and the status line, zygo's included (the schema: "Disable all hooks and statusLine execution"). It is one line, no newline, and adds no list of what the hooks do (§8.5 already says it).
- D3 (author's call). One merge line per launch: `MERGE_HOOKS_OFF` or `MERGE_WARN`, never both. Line order in `ChildSpec.lines`: the merge line, then one `bad_hooks_line` per entry of `bad_hooks` in value order, then the unreadable-value lines as today.
- D4 (controller decision F1, 2026-11-23). zygo never passes a non-boolean `disableAllHooks` from a user `--settings`. In `user_settings`, for each readable value, a top-level `disableAllHooks` that is not a JSON boolean is removed from that value before it is merged and pushed to `bad_hooks`; the value still counts in `read`. So a boolean `disableAllHooks` of another user value still counts by the merge order, and with none zygo's `false` stays. The merge line is then decided on the merged boolean (D1). Reason: the schema types it boolean, and §8.5 records that settings Claude Code could not load made it run with neither file; its handling of an off-schema `disableAllHooks` was not observed.
- D5 (controller decision F4, 2026-11-23). After this node three texts outside Files are false for a merged `disableAllHooks: true`: AU59 of `docs/specs/2026-11-20-workspaces-design.md` (`:200`, quotes only `MERGE_WARN` and says the agent always has the member hooks), its W§16 row for `crates/zygo-launch/src/claude_code.rs` (`:670`, "a member with a user `--settings` keeps them") and `docs/work/phase2/nodes/M2.md:3` ("the member hooks stay in force"). FIX-24 owns AU59 and the W§16 row (the controller adds them to its contract scope); the controller fixes the `M2.md` sentence at commit. FIX-23 edits none of them.

Steps:

- [ ] In `crates/zygo-launch/tests/claude_code.rs`, after `const MERGE_WARN` (`:446`), add:
  ```rust
  /// §8.5 Flags (FIX-23): the merge line when the merged settings set `disableAllHooks: true`.
  const MERGE_HOOKS_OFF: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included";

  /// §8.5 Flags (FIX-23): the line for a user `disableAllHooks` value that is not `true` or `false`.
  fn bad_hooks(value: Value) -> String {
      format!("zygo: your --settings disableAllHooks value {value} is not true or false and was not used")
  }
  ```
- [ ] In the same file, change `user_disable_all_hooks_wins_and_attribution_keys_stay` (`:673-683`): keep the `ChildSpec` from `build` (`let spec = cc(&fx).build(&c).unwrap();`) and add, after the three existing assertions:
  ```rust
  assert_eq!(spec.lines, [MERGE_HOOKS_OFF]);
  assert!(!spec.lines.iter().any(|l| l.contains("hooks stay in force")), "{:?}", spec.lines);
  ```
  Change its comment to `// FIX-21 round 3, FIX-23, §8.5 Flags: a user disableAllHooks wins; the attribution keys stay; the merge line says the hooks stop`.
- [ ] In the same file, after that test, add `merge_line_is_true_when_disable_all_hooks_is_set` with the comment `// FIX-23, §8.5 Flags: kills: the "hooks stay in force" line for merged settings that stop every hook; the new line for a user value that a later false overrides; the line for other user settings changed; a non-boolean disableAllHooks passed on or not named; a lost unreadable-value line`. Body: `let fx = fx();`, write `fx.cwd.join("off.json")` with `{"disableAllHooks": true}`; then a table `[(&str, bool, &[&str], Vec<String>, bool)]` of `(name, member, args, lines, off)`, where `off` is the expected merged `disableAllHooks`:

  | name | member | args | lines | off |
  |---|---|---|---|---|
  | `inline true` | no | `&[r#"--settings={"disableAllHooks": true}"#]` | `[MERGE_HOOKS_OFF]` | `true` |
  | `file true` | no | `&["--settings", "off.json", "-p", "hi"]` | `[MERGE_HOOKS_OFF]` | `true` |
  | `inline false` | no | `&["--settings", r#"{"disableAllHooks": false}"#]` | `[MERGE_WARN]` | `false` |
  | `true then false` | no | `&["--settings", r#"{"disableAllHooks": true}"#, "--settings", r#"{"disableAllHooks": false}"#]` | `[MERGE_WARN]` | `false` |
  | `true then other` | no | `&["--settings", "off.json", "--settings", r#"{"outputStyle": "x"}"#]` | `[MERGE_HOOKS_OFF]` | `true` |
  | `other only` | no | `&["--settings", r#"{"outputStyle": "x"}"#]` | `[MERGE_WARN]` | `false` |
  | `true and unreadable` | no | `&["--settings", "off.json", "--settings", "missing.json"]` | `[MERGE_HOOKS_OFF, unreadable("missing.json")]` | `true` |
  | `member true` | yes | `&["--settings", "off.json"]` | `[MERGE_HOOKS_OFF]` | `true` |
  | `string true` | no | `&["--settings", r#"{"disableAllHooks": "true"}"#]` | `[MERGE_WARN, bad_hooks(json!("true"))]` | `false` |
  | `number` | no | `&["--settings", r#"{"disableAllHooks": 1}"#]` | `[MERGE_WARN, bad_hooks(json!(1))]` | `false` |
  | `null` | no | `&["--settings", r#"{"disableAllHooks": null}"#]` | `[MERGE_WARN, bad_hooks(Value::Null)]` | `false` |
  | `object` | no | `&["--settings", "{\"disableAllHooks\": {\"a\": \"x\\ny\"}}"]` | `[MERGE_WARN, bad_hooks(json!({"a": "x\ny"}))]` | `false` |
  | `true then string` | no | `&["--settings", "off.json", "--settings", r#"{"disableAllHooks": "x"}"#]` | `[MERGE_HOOKS_OFF, bad_hooks(json!("x"))]` | `true` |
  | `number and unreadable` | no | `&["--settings", r#"{"disableAllHooks": 1}"#, "--settings", "missing.json"]` | `[MERGE_WARN, bad_hooks(json!(1)), unreadable("missing.json")]` | `false` |

  `lines` entries are `String`s (`MERGE_HOOKS_OFF.to_string()`, `MERGE_WARN.to_string()`, `bad_hooks(..)`, `unreadable(..)`). For each row build the context as the existing tests do (`tests/claude_code.rs:456-464`): `let mut c = if member { member_ctx(&fx, true) } else { ctx(&fx, "") };`, then `c.args = strings(args);`, `let spec = cc(&fx).build(&c).unwrap(); let s = settings(&fx);` and assert, each with `"{name}"`:
  ```rust
  assert_eq!(spec.lines, lines, "{name}");
  assert!(spec.lines.iter().all(|l| !l.contains('\n')), "{name}");
  assert_eq!(s["disableAllHooks"], json!(off), "{name}");
  assert_eq!(spec.lines.iter().any(|l| l.contains("hooks stay in force")), !off, "{name}");
  assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{name}");
  assert_eq!(s["includeCoAuthoredBy"], json!(false), "{name}");
  assert_eq!(spec.log, ["degraded: --settings"], "{name}");
  ```
- [ ] In the same file, after `passthrough_alias_turns_the_role_off`, add the launcher test, `#[cfg(unix)]`, `disable_all_hooks_merge_line_reaches_stderr`, comment `// FIX-23, §8.5 Flags: the lines the user sees on stderr`. For each `(user, want, not)` in `[(r#"{"disableAllHooks": true}"#, MERGE_HOOKS_OFF.to_string(), "hooks stay in force"), (r#"{"outputStyle": "x"}"#, MERGE_WARN.to_string(), "disableAllHooks"), (r#"{"disableAllHooks": 1}"#, bad_hooks(json!(1)), "stops every hook")]`: `let fx = fx();`, `script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");`, `let o = launch_opts(&fx, &["--settings", user], false); let err = stderr_of(&o);`, then
  ```rust
  assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
  assert_eq!(text_of(&err).lines().filter(|l| *l == want).count(), 1, "{}", text_of(&err));
  assert!(!text_of(&err).contains(not), "{}", text_of(&err));
  ```
- [ ] Run and see red: `cargo test -p zygo-launch --locked --test claude_code 2>&1`. Expected failures: `user_disable_all_hooks_wins_and_attribution_keys_stay` (`left` holds the `... attribution keys and hooks stay in force` line), `merge_line_is_true_when_disable_all_hooks_is_set` (first row `inline true`, same `left`) and `disable_all_hooks_merge_line_reaches_stderr` (first case, count `0`); every other test passes, `settings_force_no_claude_attribution` and `unreadable_user_settings_keep_only_zygos_file` included.
- [ ] Implement in `crates/zygo-launch/src/claude_code.rs`:
  - After `MERGE_WARN` (`:22`) add `const MERGE_HOOKS_OFF: &str` with the text above and the doc comment `/// §8.5 Flags (FIX-23): the merge line when the merged settings set \`disableAllHooks: true\`, which stops every hook and the status line, zygo's included.`; change the doc comment of `MERGE_WARN` to `/// §8.5 Flags (FIX-21 round 2 F1): the user's \`--settings\` values are merged into zygo's file; the line when the merged settings do not set \`disableAllHooks: true\`.`
  - `UserSettings`: add `bad_hooks: Vec<Value>` with the doc above; `user_settings` initializes it empty and, for each readable map `m`, before `out.merged.extend(m)`, removes a top-level `disableAllHooks` that is not a JSON boolean (`!v.is_boolean()`) and pushes it to `out.bad_hooks` (D4).
  - After `unreadable_line` add `fn bad_hooks_line(value: &Value) -> String` with the doc `/// §8.5 Flags (FIX-23): the line for a user \`disableAllHooks\` value that is not \`true\` or \`false\`, as compact JSON, so the line is one line.` and the format given under Interfaces.
  - `merge_settings` doc comment: replace `(a user \`disableAllHooks\` is an ordinary key and wins)` with `(a user boolean \`disableAllHooks\` is an ordinary key and wins; \`user_settings\` has already removed a non-boolean one)`. Its code does not change.
  - `build`: after the merge (`:468-470`) bind `let hooks_off = file.get("disableAllHooks") == Some(&Value::Bool(true));`. At `:494-498`, when `u.read > 0` push `if hooks_off { MERGE_HOOKS_OFF } else { MERGE_WARN }` (as `String`), then `u.bad_hooks.iter().map(bad_hooks_line)`, then the unreadable lines as today (D3).
- [ ] Run and see green: `cargo test -p zygo-launch --locked --test claude_code 2>&1` → all pass, the three tests above included.
- [ ] Apply "Spec text" below to `docs/specs/2026-11-18-zygo-design.md` (§8.5 Flags sentences and one change-log row at the end of the table). Check: `rg -n -F "your disableAllHooks stops every hook and the status line, zygo's included" docs/specs/2026-11-18-zygo-design.md crates/zygo-launch` → exactly three hits: §8.5 Flags, `src/claude_code.rs` and `tests/claude_code.rs`; `rg -n -F "disableAllHooks value" docs/specs/2026-11-18-zygo-design.md crates/zygo-launch` → exactly three hits: §8.5 Flags, `src/claude_code.rs` (`bad_hooks_line`) and `tests/claude_code.rs` (`bad_hooks`).
- [ ] Package and lint checks, scoped to `zygo-launch`: `cargo test -p zygo-launch --locked --no-fail-fast 2>&1` → all pass (`stub_launch` keeps its `MERGE_WARN` checks unchanged and passes); `cargo clippy -p zygo-launch --locked --tests -- -D warnings 2>&1` → clean.
- [ ] Tick these steps in this file. Write the message below to `"$(git rev-parse --git-path FIX-23.msg)"`. Stage exactly `git add crates/zygo-launch/src/claude_code.rs crates/zygo-launch/tests/claude_code.rs docs/specs/2026-11-18-zygo-design.md docs/work/phase2/nodes/FIX-23.md`, then commit with `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com git commit -F "$(git rev-parse --git-path FIX-23.msg)"`. Message (no other trailer, no attribution of any kind):
  ```
  fix(launch): FIX-23 the settings merge line says when disableAllHooks stops the hooks

  With a user --settings value that sets disableAllHooks: true, the merged
  settings file stops every hook and the status line, zygo's included, but
  zygo printed that its attribution keys and hooks stay in force. For that
  merged file zygo now prints, in place of that line, that its attribution
  keys stay in force and that the user's disableAllHooks stops every hook
  and the status line. A user disableAllHooks that is not true or false is
  not used, and zygo prints a line that names it. The line for other user
  settings is unchanged. Spec §8.5 Flags and the change log are updated.

  Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
  Zygo-Work: FIX-23
  ```

## Spec text

In §8.5 Flags of `docs/specs/2026-11-18-zygo-design.md`, replace the sentence

> When at least one value is merged, `zygo` prints `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`.

with

> A user `disableAllHooks` that is not `true` or `false` is not used: the settings schema types it as a boolean, settings that Claude Code could not load made it run with neither file (see below), and its handling of such a value was not observed. `zygo` removes it from that value before the merge, so a boolean `disableAllHooks` of another user value still counts by the merge order and, with none, `zygo`'s `false` stays; for each such value `zygo` prints `zygo: your --settings disableAllHooks value <value> is not true or false and was not used`, where `<value>` is the value as compact JSON (control characters escaped), so the line is always one line. When at least one value is merged, `zygo` prints one merge line for the merged settings: `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`, or, in its place, when the merged `disableAllHooks` is `true` (a later user value `disableAllHooks: false` wins over an earlier `true`), `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included` (the schema: "Disable all hooks and statusLine execution"). The merge line comes first, then the `disableAllHooks` lines, then the lines for values that could not be read. *(2026-11-23, FIX-23: the first merge line was printed also for settings that stop every hook, so it was false.)*

Append to the change-log table:

> | 2026-11-23 | §8.5 Flags (merge line, non-boolean `disableAllHooks`) | author's call (FIX-23, from the FIX-21 merge review, 2026-11-22); controller decisions on the FIX-23 plan review (2026-11-23: non-boolean value, status line) | With a user `--settings` value that sets `disableAllHooks: true`, the merged file stops every hook and the status line, `zygo`'s included, but `zygo` printed that its attribution keys and hooks stay in force. When the merged `disableAllHooks` is `true`, `zygo` now prints `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included` in place of that line; the line for other settings is unchanged. A user `disableAllHooks` that is not `true` or `false` is not used, with one line that names it. The workspaces-spec texts AU59 and the W§16 row for `crates/zygo-launch/src/claude_code.rs` are FIX-24's. |

(Without the leading `> ` in the spec file.)

## Self-review (§3.3)

1. Spec coverage: §8.5 Flags requires a merge line when at least one value is merged (kept: rows `inline false`, `other only`; `settings_force_no_claude_attribution`), a user `disableAllHooks: true` that wins (row `inline true`, `s["disableAllHooks"]`), attribution keys that always stay (every row), unreadable values with their own line (rows `true and unreadable`, `number and unreadable`), the same merge for members (row `member true`). The new requirements: the merge line in place of `MERGE_WARN` for merged `true` (the `contains("hooks stay in force") == !off` assertion, the stderr test); a non-boolean value not used and named (rows `string true`, `number`, `null`, `object`, `true then string`; `s["disableAllHooks"]` always a boolean; the third stderr case); one line per line (`!l.contains('\n')`, row `object` with a newline inside); the line order (row `number and unreadable`). Each maps to a step.
2. Step scan: each step is one action. The only rules stated in prose are D1 (the merged file decides) and D4 (removal per value, before the merge); rows `true then false` and `true then string` test both, so the prose and the tests agree.
3. Interface match: Consumes `ClaudeCode::settings` (FIX-21) exists with the signature above and is unchanged. Produces names no code symbol; FIX-24 consumes nothing from FIX-23 and reads the same merged file with the same `Value::Bool(true)` test (D1). `MERGE_WARN`'s text is unchanged, so `stub_launch.rs` stays valid; AU59, the W§16 row and `M2.md:3` are assigned in D5.
4. Review focus: the graph's Review focus assigns no line to FIX-23.
5. Proportion: the code change is one constant, one field, one removal in `user_settings`, one line function, one binding and one changed push; the plan gives their texts and the tests, no other body.
