# FIX-23 — zygo's settings merge line is true when the user sets disableAllHooks

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §8.5 (Flags: the merge of user `--settings` keys; the merge line; a user `disableAllHooks: true` wins and stops every hook, `zygo`'s included, while `attribution` and `includeCoAuthoredBy` always stay `zygo`'s; the line for a value that cannot be read). Defect: with a user `--settings` value that holds `disableAllHooks: true`, `ClaudeCode::build` (`crates/zygo-launch/src/claude_code.rs:458-511`; the line is pushed at `:494-497`) still prints `MERGE_WARN` (`claude_code.rs:22`), `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`, but the merged file then stops every hook, so the line is false. Found in the FIX-21 merge review (2026-11-22). Plan review 2026-11-23, controller decision F1: a user `disableAllHooks` that is not `true` or `false` is not used (schema type boolean, `fixtures/claude-code/settings-schema.json:2398-2400`). Re-planned 2026-11-23 after BLOCKED (3 rounds), controller decision: the rules for reading a `--settings` value moved to FIX-26; this node does not change how a value is read or which values are printed. Plan review of the re-plan, controller decisions 2026-11-23: M4 (the `disableAllHooks` line names only the JSON type of the value), m5 (the merge-rule sentence states the non-boolean exception).
**Files:** `crates/zygo-core/src/text.rs`, `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** `ClaudeCode::settings` (FIX-21)
**Produces:** `zygo_core::esc(s: &str) -> String` (also escapes U+061C, so all 12 Bidi_Control characters); `user_settings(ctx: &LaunchCtx) -> UserSettings` (private to `claude_code.rs`; `UserSettings` gains `bad_hooks: Vec<Value>`); `unreadable_line(value: &str) -> String` (private to `claude_code.rs`; the value written through `zygo_core::esc`); `bad_hooks_line(value: &Value) -> String` (private to `claude_code.rs`; names only the JSON type); `MERGE_HOOKS_OFF: &str` (private to `claude_code.rs`, and the test constant of the same name and text in `crates/zygo-launch/tests/claude_code.rs`); test helpers in `crates/zygo-launch/tests/claude_code.rs` that FIX-26 uses: `bad_hooks(kind: &str) -> String`, `RAW: [char; 5]`
**Acceptance:** `cargo test -p zygo-core -p zygo-launch --locked --lib --test claude_code` → all pass, including `every_bidi_control_character_is_escaped` and a case where a user `--settings` value with `disableAllHooks: true` gives a stderr line that does not say the hooks stay in force

## Plan

Premise check (2026-11-23, on `b913a1d`). The defect is live, and the code is as the first plan found it on `7557a60`: `git hash-object` of `crates/zygo-core/src/text.rs` (`f24ce32`), `crates/zygo-launch/src/claude_code.rs` (`921082e`), `crates/zygo-launch/tests/claude_code.rs` (`cedee10`), `crates/zygo-launch/tests/stub_launch.rs` (`c89ce4a`) and `docs/specs/2026-11-18-zygo-design.md` (`0963952`) equals the base blob of each file in the discarded attempt diff. `ClaudeCode::settings` (`claude_code.rs:387-422`) writes `"disableAllHooks": false`. `user_settings` (`:159-178`) merges each value that parses as a JSON object or that `read_object` (`:226-232`) reads, and pushes the other values, as typed, to `UserSettings.unreadable` (`:149-157`). `unreadable_line` (`:180-184`) writes the value as a JSON string with `Value::from(value)`, which leaves DEL, the C1 controls, the bidi controls and U+2028/U+2029 raw. `merge_settings` (`:186-224`) keeps a user `disableAllHooks` as an ordinary key that wins (`:218-220`), whatever its JSON type. `build` (`:458-511`) merges (`:467-470`), writes the file (`:471`) and at `:494-499` pushes `MERGE_WARN` when `u.read > 0`, then the unreadable lines. The launcher prints every `ChildSpec.lines` entry to stderr as is (`crates/zygo-launch/src/launcher.rs:600-602`). `zygo_core::esc` (`crates/zygo-core/src/text.rs:4-18`, re-exported at `crates/zygo-core/src/lib.rs:18`) escapes the controls, U+200E, U+200F, U+2028, U+2029, U+202A-U+202E and U+2066-U+2069, but not U+061C ARABIC LETTER MARK, the twelfth Bidi_Control character. The §8.5 Flags body (`docs/specs/2026-11-18-zygo-design.md:373`) has no `*(2026-11-23, FIX-23: ...)*` note, so there is none to remove. `read_object` is also used by the preflight (`claude_code.rs:514`, `:542`); FIX-26 changes it, this node does not.

Interfaces, as they exist now:

- Consumed: `ClaudeCode::settings(&self, ctx: &LaunchCtx, env: &[(String, String)]) -> Value` (private, `claude_code.rs:387`); its `"disableAllHooks"` is `false`.
- Changed, public: `pub fn esc(s: &str) -> String` (`crates/zygo-core/src/text.rs:4`), signature unchanged; it also escapes U+061C as `\u{61c}`.
- Kept as is: `const MERGE_WARN: &str` (`claude_code.rs:22`), text unchanged (only its doc comment changes); `fn merge_settings(zygo: Value, user: Map<String, Value>) -> Value`, code unchanged (only its doc comment changes); `fn read_object(p: &Path) -> Option<Map<String, Value>>`, unchanged.
- Changed, private: `struct UserSettings` gains `bad_hooks: Vec<Value>` (doc: `The user \`disableAllHooks\` values that are not \`true\` or \`false\`, in value order; zygo does not use them.`). `fn user_settings(ctx: &LaunchCtx) -> UserSettings` keeps its signature. `fn unreadable_line(value: &str) -> String` keeps its signature and its text; the JSON string of the value goes through `zygo_core::esc`.
- New, private to `claude_code.rs`:
  - `const MERGE_HOOKS_OFF: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included";`
  - `fn bad_hooks_line(value: &Value) -> String`, giving `format!("zygo: your --settings disableAllHooks value is {kind}, not true or false; it was removed")`, where `kind` is `a string` for `Value::String`, `a number` for `Value::Number`, `an object` for `Value::Object`, `an array` for `Value::Array` and `null` for `Value::Null`. `Value::Bool` never reaches it (D4 removes only non-booleans); it maps to `a boolean` so the match is total.
- No new public name. FIX-24 consumes nothing from this node; its signal is the merged file (`<session folder>/settings.json`, top-level `disableAllHooks` equal to the JSON value `true`), which after this node is always a boolean. FIX-26 consumes `esc`, `user_settings`, `unreadable_line` and the test names `bad_hooks`, `MERGE_HOOKS_OFF` and `RAW`.

Decisions:

- D1 (author's call). The merge line reads the merged file, not the user values: `build` prints `MERGE_HOOKS_OFF` in place of `MERGE_WARN` exactly when the merged settings value holds the top-level key `disableAllHooks` equal to `Value::Bool(true)`. A later user value `disableAllHooks: false` that wins over an earlier `true` gives `MERGE_WARN`. The test `Value::Bool(true)` is the one the §8.4 preflight uses (`conflict`, `claude_code.rs:246-256`).
- D2 (author's call; status line by controller decision F7, 2026-11-23). `MERGE_HOOKS_OFF` keeps the `MERGE_WARN` prefix up to the first `;`, then states only what is true for that file: the attribution keys stay zygo's; the user's `disableAllHooks` stops every hook and the status line, zygo's included (the schema: "Disable all hooks and statusLine execution").
- D3 (author's call). One merge line per launch: `MERGE_HOOKS_OFF` or `MERGE_WARN`, never both. Line order in `ChildSpec.lines`: the merge line, then one `bad_hooks_line` per entry of `bad_hooks` in value order, then one `unreadable_line` per unreadable value in value order.
- D4 (controller decisions F1 and M4, 2026-11-23). In `user_settings`, for each readable value, a top-level `disableAllHooks` that is not a JSON boolean is removed from that value before it is merged and pushed to `bad_hooks`; the value still counts in `read`. A boolean `disableAllHooks` of another user value still counts by the merge order; with none, zygo's `false` stays. The line for a removed value names only its JSON type, never the value: the value can hold any text, for example a token. The wording of the type uses its article (`a string`, `an object`, `null`), so the line is correct English; the controller's form `is a <type>` is kept for the three types that take `a`.
- D5 (controller decision F4, 2026-11-23). AU59 of `docs/specs/2026-11-20-workspaces-design.md` (`:200`) and its W§16 row for `crates/zygo-launch/src/claude_code.rs` belong to FIX-24; `docs/work/phase2/nodes/M2.md:3` is fixed by the controller. FIX-23 edits none of them.
- D6 (controller decisions, re-plan and M4, 2026-11-23). Only `unreadable_line` carries user text. It passes the value through `zygo_core::esc` after `serde_json` writes it as a compact JSON string. So DEL, the C1 controls, the 12 Bidi_Control characters and U+2028/U+2029 are written as `\u{..}`, the line is always one line, and the text in it is not always valid JSON. `bad_hooks_line` carries no user text (D4), so it needs no escaping.
- D7 (controller decision, re-plan 2026-11-23). This node does not change how a `--settings` value is read: `user_settings` keeps its `serde_json::from_str` then `read_object` branch, and `unreadable_line` keeps today's text except the escaping. The value-reading rules are FIX-26's.
- D8 (controller decision m5, 2026-11-23). The §8.5 Flags sentence of the merge rule ("every other user key is kept and wins ...; `disableAllHooks` is one of these other keys") is rewritten in place, so it states the non-boolean exception.

Steps:

- [x] In `crates/zygo-core/src/text.rs`, in `mod tests`, after `line_and_paragraph_separators_are_escaped`, add:
  ```rust
  #[test] // FIX-23 (D6): kills: a Bidi_Control character printed raw; the list is the 12 characters with the Unicode property Bidi_Control
  fn every_bidi_control_character_is_escaped() {
      let bidi = ['\u{61c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'];
      let raw: Vec<String> = bidi.into_iter().filter(|c| esc(&format!("a{c}b")) != format!("a\\u{{{:x}}}b", *c as u32)).map(|c| format!("U+{:04X}", c as u32)).collect();
      assert!(raw.is_empty(), "not escaped: {raw:?}");
  }
  ```
- [x] Run and see red: `cargo test -p zygo-core --locked --lib text 2>&1` → `every_bidi_control_character_is_escaped` fails with `not escaped: ["U+061C"]`; `line_and_paragraph_separators_are_escaped` passes.
- [x] In `crates/zygo-core/src/text.rs`, `esc`: add `'\u{61c}'` as the first alternative of the `matches!` list. Run `cargo test -p zygo-core --locked --lib text 2>&1` → both tests pass.
- [x] In `crates/zygo-launch/tests/claude_code.rs`, after `const MERGE_WARN` (`:446`), add:
  ```rust
  /// §8.5 Flags (FIX-23): the merge line when the merged settings set `disableAllHooks: true`.
  const MERGE_HOOKS_OFF: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included";

  /// §8.5 Flags (FIX-23 D4): the line for a user `disableAllHooks` value that is not `true` or `false`; `kind` is its JSON
  /// type with its article (`a string`, `a number`, `an object`, `an array`, `null`).
  fn bad_hooks(kind: &str) -> String {
      format!("zygo: your --settings disableAllHooks value is {kind}, not true or false; it was removed")
  }

  /// Characters that `serde_json` leaves raw in a JSON string and that `zygo_core::esc` escapes (FIX-23 D6): the C1
  /// controls CSI and NEL, DEL, the line separator and the Arabic letter mark.
  const RAW: [char; 5] = ['\u{9b}', '\u{85}', '\u{7f}', '\u{2028}', '\u{61c}'];
  ```
- [x] In the same file, change `user_disable_all_hooks_wins_and_attribution_keys_stay` (`:673-683`): bind `let spec = cc(&fx).build(&c).unwrap();` and add, after the three existing assertions:
  ```rust
  assert_eq!(spec.lines, [MERGE_HOOKS_OFF]);
  assert!(!spec.lines.iter().any(|l| l.contains("hooks stay in force")), "{:?}", spec.lines);
  ```
  Change its comment to `// FIX-21 round 3, FIX-23, §8.5 Flags: kills: a user disableAllHooks that does not win; an attribution key lost; the "hooks stay in force" line for settings that stop every hook`.
- [x] In the same file, after that test, add `merge_line_is_true_when_disable_all_hooks_is_set` with the comment `// FIX-23 (D1, D3, D4), §8.5 Flags: kills: the "hooks stay in force" line for merged settings that stop every hook; the new line for a user value that a later false overrides; the line for other user settings changed; a non-boolean disableAllHooks passed on, not named by its type, or printed with its value; a lost unreadable-value line`. Body: `let fx = fx();`, write `fx.cwd.join("off.json")` with `{"disableAllHooks": true}`; a local `type Row<'a> = (&'a str, bool, &'a [&'a str], Vec<String>, bool, &'a str);` (clippy `type_complexity` refuses the tuple written out) and `let rows: [Row; 14]` of `(name, member, args, lines, off, hidden)`, where `off` is the expected merged `disableAllHooks` and `hidden` is value text that no line may hold (empty: no check):

  | name | member | args | lines | off | hidden |
  |---|---|---|---|---|---|
  | `inline true` | no | `&[r#"--settings={"disableAllHooks": true}"#]` | `[MERGE_HOOKS_OFF]` | `true` | `""` |
  | `file true` | no | `&["--settings", "off.json", "-p", "hi"]` | `[MERGE_HOOKS_OFF]` | `true` | `""` |
  | `inline false` | no | `&["--settings", r#"{"disableAllHooks": false}"#]` | `[MERGE_WARN]` | `false` | `""` |
  | `true then false` | no | `&["--settings", r#"{"disableAllHooks": true}"#, "--settings", r#"{"disableAllHooks": false}"#]` | `[MERGE_WARN]` | `false` | `""` |
  | `true then other` | no | `&["--settings", "off.json", "--settings", r#"{"outputStyle": "x"}"#]` | `[MERGE_HOOKS_OFF]` | `true` | `""` |
  | `other only` | no | `&["--settings", r#"{"outputStyle": "x"}"#]` | `[MERGE_WARN]` | `false` | `""` |
  | `true and unreadable` | no | `&["--settings", "off.json", "--settings", "missing.json"]` | `[MERGE_HOOKS_OFF, unreadable("missing.json")]` | `true` | `""` |
  | `member true` | yes | `&["--settings", "off.json"]` | `[MERGE_HOOKS_OFF]` | `true` | `""` |
  | `string true` | no | `&["--settings", r#"{"disableAllHooks": "true"}"#]` | `[MERGE_WARN, bad_hooks("a string")]` | `false` | `r#""true""#` |
  | `number` | no | `&["--settings", r#"{"disableAllHooks": 17}"#]` | `[MERGE_WARN, bad_hooks("a number")]` | `false` | `"17"` |
  | `null` | no | `&["--settings", r#"{"disableAllHooks": null}"#]` | `[MERGE_WARN, bad_hooks("null")]` | `false` | `""` |
  | `object` | no | `&["--settings", "{\"disableAllHooks\": {\"token\": \"s3cret\\ny\"}}"]` | `[MERGE_WARN, bad_hooks("an object")]` | `false` | `"s3cret"` |
  | `true then string` | no | `&["--settings", "off.json", "--settings", r#"{"disableAllHooks": "s3cret"}"#]` | `[MERGE_HOOKS_OFF, bad_hooks("a string")]` | `true` | `"s3cret"` |
  | `number and unreadable` | no | `&["--settings", r#"{"disableAllHooks": 17}"#, "--settings", "missing.json"]` | `[MERGE_WARN, bad_hooks("a number"), unreadable("missing.json")]` | `false` | `"17"` |

  `lines` entries are `String`s. For each row: `let mut c = if member { member_ctx(&fx, true) } else { ctx(&fx, "") };`, `c.args = strings(args);`, `let spec = cc(&fx).build(&c).unwrap(); let s = settings(&fx);`, then:
  ```rust
  assert_eq!(spec.lines, lines, "{name}");
  assert!(spec.lines.iter().all(|l| !l.contains('\n')), "{name}");
  assert!(hidden.is_empty() || !spec.lines.iter().any(|l| l.contains(hidden)), "{name}: {:?}", spec.lines);
  assert_eq!(s["disableAllHooks"], json!(off), "{name}");
  assert_eq!(spec.lines.iter().any(|l| l.contains("hooks stay in force")), !off, "{name}");
  assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{name}");
  assert_eq!(s["includeCoAuthoredBy"], json!(false), "{name}");
  assert_eq!(spec.log, ["degraded: --settings"], "{name}");
  ```
- [x] In the same file, after that test, add `non_boolean_disable_all_hooks_edges` with the comment `// FIX-23 (D3, D4), §8.5 Flags: kills: a non-boolean value from a file, an array or a long string passed on, not named by its type, or printed with its value; a non-boolean value that hides a later boolean; two values out of order or merged into one line; a member launch that passes a non-boolean value on`. Body: `let fx = fx();`, write `off.json` with `{"disableAllHooks": true}` and `yes.json` with `{"disableAllHooks": "s3cret-yes"}` in `fx.cwd`; `let big = "x".repeat(1 << 20); let big_inline = format!("{{\"disableAllHooks\": \"{big}\"}}");`; a local `type Row<'a> = (&'a str, bool, Vec<&'a str>, Vec<String>, bool, &'a str);` and `let rows: [Row; 8]` of `(name, member, args, lines, off, hidden)`:

  | name | member | args | lines | off | hidden |
  |---|---|---|---|---|---|
  | `file string` | no | `["--settings", "yes.json"]` | `[MERGE_WARN, bad_hooks("a string")]` | `false` | `"s3cret"` |
  | `array` | no | `["--settings", r#"{"disableAllHooks": [true, "s3cret"]}"#]` | `[MERGE_WARN, bad_hooks("an array")]` | `false` | `"s3cret"` |
  | `1 MB string` | no | `["--settings", &big_inline]` | `[MERGE_WARN, bad_hooks("a string")]` | `false` | `"xxxx"` |
  | `number then true` | no | `["--settings", r#"{"disableAllHooks": 17}"#, "--settings", "off.json"]` | `[MERGE_HOOKS_OFF, bad_hooks("a number")]` | `true` | `"17"` |
  | `number then false` | no | `["--settings", r#"{"disableAllHooks": 17}"#, "--settings", r#"{"disableAllHooks": false}"#]` | `[MERGE_WARN, bad_hooks("a number")]` | `false` | `"17"` |
  | `true, number, false` | no | `["--settings", "off.json", "--settings", r#"{"disableAllHooks": 17}"#, "--settings", r#"{"disableAllHooks": false}"#]` | `[MERGE_WARN, bad_hooks("a number")]` | `false` | `"17"` |
  | `two non-boolean` | no | `["--settings", r#"{"disableAllHooks": 17}"#, "--settings", r#"{"disableAllHooks": "s3cret-two"}"#]` | `[MERGE_WARN, bad_hooks("a number"), bad_hooks("a string")]` | `false` | `"s3cret"` |
  | `member number` | yes | `["--settings", r#"{"disableAllHooks": 17}"#]` | `[MERGE_WARN, bad_hooks("a number")]` | `false` | `"17"` |

  For each row, build as in the step above, then (the first assertion does not print the lines, in case a line holds 1 MB):
  ```rust
  assert!(spec.lines == lines, "{name}: {} lines", spec.lines.len());
  assert!(spec.lines.iter().all(|l| !l.contains('\n')), "{name}");
  assert!(!spec.lines.iter().any(|l| l.contains(hidden)), "{name}: a line holds the value");
  assert_eq!(spec.lines.iter().filter(|l| l.contains("disableAllHooks value")).count(), lines.len() - 1, "{name}");
  assert_eq!(s["disableAllHooks"], json!(off), "{name}");
  assert_eq!(spec.lines[0] == MERGE_HOOKS_OFF, off, "{name}: the merge line and the merged file disagree");
  assert_eq!(s["attribution"], json!({"commit": "", "pr": "", "sessionUrl": false}), "{name}");
  ```
- [x] In the same file, after that test, add `settings_lines_escape_every_control_character` with the comment `// FIX-23 (D4, D6), §8.5 Flags: kills: an unreadable value printed with a raw DEL, C1 control, line separator or bidi control, so one line can look like two or move the cursor; a non-boolean disableAllHooks printed with its value`. Body:
  ```rust
  let fx = fx();
  let hooks = format!("{{\"disableAllHooks\": \"a{}b{}c{}d{}e{}f\"}}", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
  let path = format!("m{}i{}s{}s{}x{}.json", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
  std::fs::write(fx.cwd.join(&path), "not json").unwrap();
  let mut c = ctx(&fx, "");
  c.args = strings(&["--settings", &hooks, "--settings", &path]);
  let spec = cc(&fx).build(&c).unwrap();
  assert_eq!(
      spec.lines,
      [
          MERGE_WARN.to_string(),
          bad_hooks("a string"),
          r#"zygo: your --settings value "m\u{9b}i\u{85}s\u{7f}s\u{2028}x\u{61c}.json" could not be read as a JSON object and was not used"#.to_string(),
      ]
  );
  for l in &spec.lines {
      assert!(!l.contains('\n') && !l.contains(RAW), "{l:?}");
  }
  ```
- [x] In the same file, after `passthrough_alias_turns_the_role_off` (`:852-873`), add `#[cfg(unix)]` `disable_all_hooks_merge_line_reaches_stderr` with the comment `// FIX-23 (D1, D2, D4), §8.5 Flags: kills: the merge line or the disableAllHooks line lost or changed between build and stderr`. For each `(user, want, not)` in `[(r#"{"disableAllHooks": true}"#, MERGE_HOOKS_OFF.to_string(), "hooks stay in force"), (r#"{"outputStyle": "x"}"#, MERGE_WARN.to_string(), "disableAllHooks"), (r#"{"disableAllHooks": "s3cret"}"#, bad_hooks("a string"), "s3cret")]`: `let fx = fx();`, `script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");`, `let o = launch_opts(&fx, &["--settings", user], false); let err = stderr_of(&o);`, then:
  ```rust
  assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
  assert_eq!(text_of(&err).lines().filter(|l| *l == want).count(), 1, "{}", text_of(&err));
  assert!(!text_of(&err).contains(not), "{}", text_of(&err));
  ```
- [x] In the same file, after that test, add `#[cfg(unix)]` `settings_lines_on_stderr_are_safe_and_in_order` with the comment `// FIX-23 (D3, D6), §8.5 Flags: kills: settings lines on stderr in value order and not grouped as merge line, disableAllHooks lines, unreadable lines; a raw control or bidi character on stderr`. Body:
  ```rust
  let fx = fx();
  script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");
  let hooks = format!("{{\"disableAllHooks\": \"a{}b{}c{}d{}e{}f\"}}", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
  let path = format!("m{}i{}s{}s{}x{}.json", RAW[0], RAW[1], RAW[2], RAW[3], RAW[4]);
  std::fs::write(fx.cwd.join(&path), "not json").unwrap();
  let args = ["--settings", &path, "--settings", &hooks, "--settings", "missing.json", "--settings", r#"{"disableAllHooks": 2}"#, "--settings", r#"{"outputStyle": "x"}"#];
  let o = launch_opts(&fx, &args, false);
  let err = stderr_of(&o);
  assert_eq!(Launcher::run(&cc(&fx), o).code, 0, "{}", text_of(&err));
  let text = text_of(&err);
  assert!(!text.contains(RAW), "{text:?}");
  let lines: Vec<&str> = text.lines().collect();
  let at = |want: &str| lines.iter().position(|l| *l == want).unwrap_or_else(|| panic!("{want:?} not in {text:?}"));
  let merge = at(MERGE_WARN);
  let bad1 = at(&bad_hooks("a string"));
  let bad2 = at(&bad_hooks("a number"));
  let path_line = at(r#"zygo: your --settings value "m\u{9b}i\u{85}s\u{7f}s\u{2028}x\u{61c}.json" could not be read as a JSON object and was not used"#);
  let missing = at(&unreadable("missing.json"));
  assert_eq!([merge + 1, merge + 2, merge + 3, merge + 4], [bad1, bad2, path_line, missing], "{text:?}");
  ```
- [x] Run and see red: `cargo test -p zygo-launch --locked --test claude_code 2>&1`. Expected failures, each with `left` holding the old text: `user_disable_all_hooks_wins_and_attribution_keys_stay` and `merge_line_is_true_when_disable_all_hooks_is_set` (row `inline true`: `... attribution keys and hooks stay in force`), `non_boolean_disable_all_hooks_edges` (row `file string`: `1 lines`), `settings_lines_escape_every_control_character` (no `disableAllHooks` line, and the raw characters in the path line), `disable_all_hooks_merge_line_reaches_stderr` (first case, count `0`) and `settings_lines_on_stderr_are_safe_and_in_order` (raw characters on stderr). Every other test passes.
- [x] Implement in `crates/zygo-launch/src/claude_code.rs`:
  - After `MERGE_WARN` (`:22`) add `const MERGE_HOOKS_OFF: &str` with the text under Interfaces and the doc comment `/// §8.5 Flags (FIX-23): the merge line when the merged settings set \`disableAllHooks: true\`, which stops every hook and the status line, zygo's included.`; change the doc comment of `MERGE_WARN` to `/// §8.5 Flags (FIX-21 round 2 F1): the user's \`--settings\` values are merged into zygo's file; the line when the merged settings do not set \`disableAllHooks: true\`.`
  - `UserSettings`: add `bad_hooks: Vec<Value>` with the doc under Interfaces. `user_settings` initializes it empty and, for each readable map `m`, before `out.merged.extend(m)`, removes a top-level `disableAllHooks` that is not a JSON boolean and pushes it to `out.bad_hooks` (D4).
  - `unreadable_line`: the value becomes `zygo_core::esc(&Value::from(value).to_string())`; its doc comment becomes `/// §8.5 Flags (FIX-21 round 3, FIX-23 D6): the line for a user \`--settings\` value that zygo cannot read as a JSON object. The value is written as a JSON string, then through \`zygo_core::esc\`, so the line is one line.`
  - After `unreadable_line` add `fn bad_hooks_line(value: &Value) -> String` with the format and the type words under Interfaces and the doc `/// §8.5 Flags (FIX-23 D4): the line for a user \`disableAllHooks\` value that is not \`true\` or \`false\`. It names only the JSON type: the value can hold any text, for example a token.`
  - `merge_settings` doc comment: replace `(a user \`disableAllHooks\` is an ordinary key and wins)` with `(a user boolean \`disableAllHooks\` is an ordinary key and wins; \`user_settings\` has already removed a non-boolean one)`. Its code does not change.
  - `build`: after the merge (`:467-470`) bind `let hooks_off = file.get("disableAllHooks") == Some(&Value::Bool(true));`. At `:494-499`, when `u.read > 0` push `if hooks_off { MERGE_HOOKS_OFF } else { MERGE_WARN }` as a `String`, then extend with `u.bad_hooks.iter().map(bad_hooks_line)`, then the unreadable lines as today (D3).
- [x] Run and see green: `cargo test -p zygo-core -p zygo-launch --locked --lib --test claude_code 2>&1` → all pass, the tests above included.
- [x] Apply "Spec text" below to `docs/specs/2026-11-18-zygo-design.md`. Check: `rg -n -F "your disableAllHooks stops every hook and the status line, zygo's included" docs/specs/2026-11-18-zygo-design.md crates/zygo-launch` → four hits: §8.5 Flags, the change-log row, `src/claude_code.rs` and `tests/claude_code.rs`; `rg -n -F "not true or false; it was removed" docs/specs/2026-11-18-zygo-design.md crates/zygo-launch` → hits in §8.5 Flags, `src/claude_code.rs` (`bad_hooks_line`) and `tests/claude_code.rs` (`bad_hooks`); `rg -n -F "except a user \`disableAllHooks\` that is not \`true\` or \`false\`" docs/specs/2026-11-18-zygo-design.md` → one hit, in §8.5 Flags; `rg -n -F "(2026-11-23, FIX-23" docs/specs/2026-11-18-zygo-design.md` → no hit.
- [x] Package and lint checks, scoped to the changed packages: `cargo test -p zygo-core -p zygo-launch --locked --no-fail-fast 2>&1` → all pass (`stub_launch` keeps its `MERGE_WARN` and `unreadable` checks unchanged and passes: its values hold no character that `esc` changes); `cargo clippy -p zygo-core -p zygo-launch --locked --tests -- -D warnings 2>&1` → clean.
- [x] `esc` has other callers. Direct imports of `zygo_core::esc` (`rg -l "zygo_core::esc|use zygo_core::text" crates` and the multi-line `use zygo_core::{..., esc}` form, 2026-11-23): `crates/zygo-config/src/display.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/typed.rs`, `crates/zygo-hooks/src/prompt.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-launch/src/opencode.rs`, `crates/zygo-plugins/src/check.rs`, `crates/zygo-plugins/src/select.rs`, `crates/zygo-plugins/src/snapshot.rs`, `crates/zygo-plugins/src/sync.rs`, `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/src/down.rs`, `crates/zygo-workspace/src/loadout.rs`, `crates/zygo-workspace/src/up.rs`, `crates/zygo/src/cmd/config.rs`, `crates/zygo/src/cmd/plugins.rs`, `crates/zygo/src/cmd/profile.rs`. Through the re-export `zygo_plugins::check::esc` (`crates/zygo-plugins/src/check.rs:133`, `pub use zygo_core::esc;`): `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/src/opencode.rs`, `crates/zygo/src/cmd/compact.rs`, `crates/zygo/src/cmd/doctor.rs`, `crates/zygo/src/cmd/help.rs`, `crates/zygo/src/cmd/marketplace.rs`, `crates/zygo/src/cmd/status.rs`, `crates/zygo/src/cmd/usage.rs`, `crates/zygo/src/cmd/verbs.rs`. `crates/zygo-config/src/profile.rs:54` has its own private `esc` (`escape_debug`) and does not change. Check that no test or fixture expects a raw U+061C: `rg -n -i "u\{61c\}|u\+061c|\x{61c}" crates fixtures` → hits only in `crates/zygo-core/src/text.rs` and `crates/zygo-launch/tests/claude_code.rs` (on `b913a1d` the same command gives no hit).
- [x] Tick these steps in this file. Write the message below to `"$(git rev-parse --git-path FIX-23.msg)"`. Stage exactly `git add crates/zygo-core/src/text.rs crates/zygo-launch/src/claude_code.rs crates/zygo-launch/tests/claude_code.rs docs/specs/2026-11-18-zygo-design.md docs/work/phase2/nodes/FIX-23.md`, then commit with `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com git commit -F "$(git rev-parse --git-path FIX-23.msg)"`. Message (no other trailer, no attribution of any kind):
  ```
  fix(launch): FIX-23 the settings merge line says when disableAllHooks stops the hooks

  With a user --settings value that sets disableAllHooks: true, the merged
  settings file stops every hook and the status line, zygo's included, but
  zygo printed that its attribution keys and hooks stay in force. For that
  merged file zygo now prints, in place of that line, that its attribution
  keys stay in force and that the user's disableAllHooks stops every hook
  and the status line. A user disableAllHooks that is not true or false is
  removed, and zygo prints a line that names only its JSON type, never the
  value. The unreadable-value line writes the value through
  zygo_core::esc, which now also escapes U+061C. Spec §8.5 Flags and the
  change log are updated.

  Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
  Zygo-Work: FIX-23
  ```

## Spec text

Three replacements in §8.5 Flags of `docs/specs/2026-11-18-zygo-design.md` (`:373`) and one change-log row. Each quoted text is written without the leading `> ` in the spec file.

Replace the text (D8)

> every other user key is kept and wins over `zygo`'s key of the same name, as a user `--agents` key does. `disableAllHooks` is one of these other keys: a user `disableAllHooks: true` wins

with

> every other user key is kept and wins over `zygo`'s key of the same name, as a user `--agents` key does, except a user `disableAllHooks` that is not `true` or `false`, which `zygo` removes (see below). A user `disableAllHooks` of `true` or `false` is one of these other keys: a user `disableAllHooks: true` wins

Replace the sentence

> When at least one value is merged, `zygo` prints `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`.

with

> A user `disableAllHooks` that is not `true` or `false` is not used: the settings schema types it as a boolean, settings that Claude Code could not load made it run with neither file (see below), and its handling of such a value was not observed. `zygo` removes it from that value before the merge, so a boolean `disableAllHooks` of another user value still counts by the merge order and, with none, `zygo`'s `false` stays; for each such value, in value order, `zygo` prints `zygo: your --settings disableAllHooks value is <type>, not true or false; it was removed`, where `<type>` is `a string`, `a number`, `an object`, `an array` or `null`. The value itself is never printed, because it can hold any text, for example a token. When at least one value is merged, `zygo` prints one merge line for the merged settings: `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`, or, in its place, when the merged `disableAllHooks` is `true` (a later user value `disableAllHooks: false` wins over an earlier `true`), `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included` (the schema: "Disable all hooks and statusLine execution"). The merge line comes first, then the `disableAllHooks` lines, then the lines for values that could not be read, each group in value order.

Replace the text

> , where `<value>` is the value as typed, written as a JSON string (in double quotes, with `"`, `\` and control characters escaped), so the line is always one line.

with

> , where `<value>` is the value as typed, written as a compact JSON string (in double quotes, with `"`, `\` and the characters below U+0020 escaped) in which every character that `zygo_core::esc` escapes (the control characters, the twelve Unicode Bidi_Control characters, U+2028 and U+2029) is then written in `esc`'s form (for example `\u{9b}`); so the line is always one line, and `<value>` is not always valid JSON.

Append to the change-log table:

> | 2026-11-23 | §8.5 Flags (merge rule, merge line, non-boolean `disableAllHooks`, escaping of the unreadable-value line) | author's call (FIX-23, from the FIX-21 merge review, 2026-11-22); controller decisions 2026-11-23 (non-boolean value, status line, escaping through `esc`, split of the value-reading rules to FIX-26, the type-only `disableAllHooks` line, the merge-rule exception) | With a user `--settings` value that sets `disableAllHooks: true`, the merged file stops every hook and the status line, `zygo`'s included, but `zygo` printed that its attribution keys and hooks stay in force. When the merged `disableAllHooks` is `true`, `zygo` now prints `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included` in place of that line; the line for other settings is unchanged. A user `disableAllHooks` that is not `true` or `false` is removed before the merge and not used, with one line that names only its JSON type; the merge-rule sentence states this exception. The lines come merge line, `disableAllHooks` lines, unreadable-value lines. The unreadable-value line writes the value through `zygo_core::esc`, which now also escapes U+061C, so all twelve Bidi_Control characters are escaped. The workspaces-spec texts AU59 and the W§16 row for `crates/zygo-launch/src/claude_code.rs` are FIX-24's. |

## Self-review (§3.3)

1. Spec coverage. §8.5 Flags, as rewritten: the merge rule with the non-boolean exception (rows `string true` to `number and unreadable`: `s["disableAllHooks"]` is always a boolean); the merge line for merged settings that do not set `disableAllHooks: true` (rows `inline false`, `other only`, `true then false`; the second stderr case); `MERGE_HOOKS_OFF` for merged `true` (rows `inline true`, `file true`, `true then other`, `member true`, `number then true`; `user_disable_all_hooks_wins_and_attribution_keys_stay`; the first stderr case); a non-boolean value removed before the merge and named by its type only, in value order (every non-boolean row asserts the type word through `bad_hooks(..)` and, with `hidden`, that no value text appears: a token inside the `object` row, `s3cret` in string and array rows, `17` in number rows, `xxxx` in the 1 MB row; the third stderr case); one line each (`!l.contains('\n')`); the escaping of the unreadable line through `esc` (`settings_lines_escape_every_control_character`, `settings_lines_on_stderr_are_safe_and_in_order`); U+061C in `esc` (`every_bidi_control_character_is_escaped`); the order merge line, `disableAllHooks` lines, unreadable lines (row `number and unreadable`, `settings_lines_on_stderr_are_safe_and_in_order` by index); attribution keys always zygo's (every row); the same rules on a member launch (rows `member true`, `member number`). The unreadable line keeps its text (rows `true and unreadable`, `number and unreadable`; `unreadable_user_settings_keep_only_zygos_file` unchanged). Each maps to a step.
2. Step scan. Each step is one action. Prose rules: D1 (the merged file decides) and D4 (removal per value, before the merge; type only); rows `true then false`, `true then string` and `number then true` test the order rules. No step gives a body that the signature and the tests decide; the type words are given as data.
3. Interface match. Consumes `ClaudeCode::settings` (FIX-21) exists with the signature above. Produces matches what FIX-26 consumes: `esc`, `user_settings`, `unreadable_line` keep their signatures. `bad_hooks_line` keeps the signature `fn bad_hooks_line(value: &Value) -> String`; FIX-26 does not consume it. FIX-26 consumes the test names `bad_hooks(kind: &str) -> String`, `MERGE_HOOKS_OFF` and `RAW: [char; 5]`, which are in Produces with these signatures. `MERGE_WARN` and the `unreadable` text are unchanged, so `crates/zygo-launch/tests/stub_launch.rs` stays valid and is not in Files.
4. Review focus. The graph's Review focus assigns no line to FIX-23.
5. Proportion. The code change is one character in `esc`, one constant, one field, one removal in `user_settings`, one `esc` call in `unreadable_line`, one five-arm type match, one binding and one changed push; the plan gives their texts and the tests, and no other body.
