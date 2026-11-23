# FIX-26 — zygo prints a --settings value only when it names an entry, reads an --agents file once, and skips §8.4 settings files that are not regular files

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §8.5 (Flags: how a user `--settings` value is read; the line for a value that cannot be read; Subagents: how a user `--agents` file is read) and §8.4 (Preflight: how the user, project, local and managed settings files that `zygo` finds by itself are read). Defects: (a) a `--settings` value that is not a JSON object and names no file is printed whole, so broken inline settings (a BOM prefix, an array, a JSON string, broken JSON) put their secrets on stderr; (b) the line for a value that cannot be read gives no reason; (c) the §8.4 preflight (`ClaudeCode::preflight`, `crates/zygo-launch/src/claude_code.rs:512-520`) and the passthrough refusal (`ClaudeCode::refusal`, `:541-545`) read the settings files that `zygo` finds by itself with `read_object` (`:226-232`, `std::fs::read`), so a FIFO planted at `<cwd>/.claude/settings.json` (for example by a cloned repository) stops the launch for ever and a device is read; (d) the launcher calls `refusal` and then `build` (`crates/zygo-launch/src/launcher.rs:583`, `:587`), and each calls `user_agents` (`claude_code.rs:522`, `:482`), so a pipe given as `--print --agents <(gen)` is read twice: the second read is empty and `build` drops the user's agents with no line (`unwrap_or_default`, `:482`). (a) and (b) were found in the FIX-23 attempt (3 review rounds, 2026-11-23; the evidence this node needs is quoted under D7; the full diff was kept at `/tmp/claude-1000/-home-kagemartin-Work-zygo--claude-worktrees-zygo-phase-2/58560339-0d35-4cf4-b289-b88946e38748/scratchpad/fix23-attempt1.diff`). Split from FIX-23 by controller decision (re-plan of the BLOCKED FIX-23, 2026-11-23); (c) added by controller decision of the same day; (d) found in the first plan review of the same day. Second plan review, controller decisions 2026-11-23: a value the user gives keeps today's read semantics (a FIFO with no writer waits; that is the user's own input, not a `zygo` defect); only the files that `zygo` finds by itself use the safe rule.
**Files:** `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`, `crates/zygo-launch/tests/stub_launch.rs`, `crates/zygo-launch/tests/stub_agent/harness.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** `esc` (FIX-23), `user_settings` (FIX-23), `unreadable_line` (FIX-23), `bad_hooks` (FIX-23), `MERGE_HOOKS_OFF` (FIX-23), `RAW` (FIX-23)
**Produces:** `ClaudeCode::new(program: impl Into<String>, managed_settings: PathBuf, zygo: PathBuf) -> ClaudeCode` (the struct gains a private field, so code outside `zygo-launch` builds it with `new` or `Default`)
**Acceptance:** `cargo test -p zygo-launch --locked --test claude_code --test stub_launch` → all pass, including `settings_value_text_is_printed_only_for_a_file`, `settings_from_a_pipe_with_a_late_writer_are_used`, `settings_file_parse_errors_name_only_category_and_position`, `print_agents_from_a_pipe_are_used`, `print_agents_from_a_pipe_with_a_late_writer_are_used`, `preflight_skips_a_found_fifo_with_no_hang` and `preflight_does_not_open_a_found_fifo_link_or_device`. A test that prints `note: <test name> skipped: <reason>` did not check its case, and that skip is not a pass of the case. Skip conditions: `settings_under_unsearchable_folder_are_not_shown` and `settings_file_with_mode_0_names_the_path_and_error` skip when the tests run as root; `preflight_reads_a_leased_settings_file_after_a_blocking_open` skips when `F_SETLEASE` fails (`/proc/sys/fs/leases-enable` is `0`, the file system has no lease support, or the test user does not own the file); `preflight_does_not_open_a_found_fifo_link_or_device` skips when `posix_openpt`, `grantpt`, `unlockpt` or `inotify_add_watch` fails. Tests under `cfg(target_os = "linux")` do not run on macOS or Windows, and tests under `cfg(unix)` do not run on Windows; that is not a skip.

## Plan

Premise check (2026-11-23, on `b913a1d`, before FIX-23 lands).

- `user_settings` (`claude_code.rs:159-178`) tries `serde_json::from_str::<Value>` on each value; any value that is not a JSON object goes to `read_object(&ctx.cwd.join(&v))`, and a `None` pushes the value, as typed, to `UserSettings.unreadable`. `read_object` reads with `std::fs::read`: one blocking open that follows links, then the whole content. `unreadable_line(value)` (`:180-184`) prints every such value as a JSON string, so a value that is broken inline JSON is printed whole.
- `read_object` is also the reader of the §8.4 preflight (`:514`, over the files of `settings_files`, `:424-436`: the user file, `.claude/settings.json` and `.claude/settings.local.json` in the working directory and at the repository root, the managed file) and of the passthrough refusal (`:542`, the managed file). Both use only `Some` or `None`: `None` skips the file with no line. No other launch code reads these files (`rg -n "settings\.json|settings\.local\.json" crates --glob '!**/tests/**'`, 2026-11-23: only `claude_code.rs`, `session_dir.rs:62` for zygo's own file, and the `zygo doctor` config folder in `crates/zygo/src/cmd/doctor.rs:300-301`, which reads no settings file).
- `user_agents` (`:79-92`) merges the user's `--agents` values; with `--print`, a value that does not parse as JSON is read as a file at `ctx.cwd.join(&v)` with `std::fs::read` (`:85`), and a file that cannot be read, is not JSON or holds JSON that is not an object gives `None`. `refusal` (`:522`) turns `None` into `BAD_AGENTS` (`:20`), which the launcher prints as `zygo: <text>` with exit 2 (`launcher.rs:583-586`). `build` calls `user_agents` again (`:482`) and uses `unwrap_or_default()`. The launcher runs `refusal` (`:583`), then `build` (`:587`), then `preflight` (`:594`), all on one adapter. §8.5 Subagents (`docs/specs/2026-11-18-zygo-design.md:370`) says only that a user `--agents` value is merged.
- `ClaudeCode` (`claude_code.rs:32-36`) has three public fields and no derive. Struct literals: `Default` (`:38-42`), `crates/zygo-launch/tests/claude_code.rs:191` (`cc`), `:350` and `:361` (struct update from `cc(&fx)`), `:846`, and `crates/zygo-launch/tests/stub_agent/harness.rs:69-71` (`claude`, used by the test binaries `stub_launch`, `stub_member` and `premise`). Production code uses one `ClaudeCode::default()` per `zygo claude` process (`crates/zygo/src/cmd/claude.rs:21`); tests also use `ClaudeCode::default()` (`tests/launcher.rs:710`, `tests/claude_code.rs:255`, `:1043`).
- Rust 1.98 (`rust-toolchain.toml`) on Linux: `symlink_metadata` gives `ErrorKind::InvalidFilename` for `ENAMETOOLONG`, `NotADirectory` for `ENOTDIR`, `FilesystemLoop` for `ELOOP` and `NotFound` for `ENOENT`; for a link loop at the last part `symlink_metadata` succeeds and `metadata` gives `ELOOP` (probe run 2026-11-23). On Windows, std maps `ERROR_INVALID_NAME` (123) and `ERROR_BAD_PATHNAME` (161) to `InvalidFilename` (`library/std/src/sys/io/error/windows.rs:30`) and `ERROR_FILENAME_EXCED_RANGE` (206) too (`:65`), and gives `InvalidFilename` with "Filename too long" in `library/std/src/sys/fs/windows/dir.rs:141` (rust-src of the installed stable 1.99.0; the pinned 1.98 has no rust-src on this machine).
- `serde_json` 1.0.154 with the workspace features (`preserve_order`, `arbitrary_precision`, `raw_value`): `not json` gives `Category::Syntax` at line 1 column 2; `{"$serde_json::private::Number": 1234567890123}` gives `Category::Data` at line 1 column 46, and its text holds `1234567890123`; `{"a": 1` gives `Category::Eof` at line 1 column 7 (probe run 2026-11-23).
- Pipes and devices on Linux 7.2.8, uid 1000 (probe run 2026-11-23): the path `/dev/fd/<n>` of an anonymous pipe (the form of bash `<(…)`) has `FileType::is_fifo` through `metadata`, and `std::fs::read` of it gives the whole data once the write end is closed; a link to `/dev/fd/<n>` gives the same. A second `std::fs::read` of the same `/dev/fd/<n>` gives `""`, and bash `f() { cat "$1"; cat "$1"; }; f <(printf '{"a":1}')` prints the text once and then nothing. A FIFO with no writer has `is_fifo`; an `O_NONBLOCK` open of it succeeds at once. A pseudo-terminal slave (`posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r`) has `is_char_device`; an inotify watch with `IN_OPEN` on it reports no event after `metadata` and one event after an open.
- A Linux write lease (`F_SETLEASE`, `F_WRLCK`) can be taken by the file owner without root: a later `O_RDONLY | O_NONBLOCK` open fails with `EAGAIN` and starts the lease break (`F_GETLEASE` then gives `F_RDLCK`), and a blocking open waits until the holder releases the lease (probe run 2026-11-23).
- The crate's unix open form is `OpenOptionsExt::custom_flags(libc::O_NONBLOCK)` (`crates/zygo-core/src/session_dir.rs:94-103`), and the `fcntl` form that clears `O_NONBLOCK` is `crates/zygo-core/src/session_view.rs:182-199`; `libc` is already a dependency of `zygo-launch`. No function in the workspace reads a path with the rules of this node.

The implementer starts after FIX-23 merges and confirms, before the first step, that `user_settings`, `unreadable_line`, the test names `bad_hooks(kind: &str)`, `MERGE_HOOKS_OFF` and `RAW`, and the tests `merge_line_is_true_when_disable_all_hooks_is_set`, `settings_lines_escape_every_control_character` and `settings_lines_on_stderr_are_safe_and_in_order` exist as `docs/work/phase2/nodes/FIX-23.md` describes them.

Interfaces, as FIX-23 leaves them:

- Consumed: `pub fn esc(s: &str) -> String` (`crates/zygo-core/src/text.rs`, re-exported as `zygo_core::esc`; escapes the controls, the 12 Bidi_Control characters, U+2028 and U+2029). `fn user_settings(ctx: &LaunchCtx) -> UserSettings` (private, `claude_code.rs`). `fn unreadable_line(value: &str) -> String` (private, `claude_code.rs`). In `crates/zygo-launch/tests/claude_code.rs`: `fn bad_hooks(kind: &str) -> String`, `const MERGE_HOOKS_OFF: &str`, `const RAW: [char; 5]`.
- Changed, public: `pub struct ClaudeCode` keeps `pub program: String`, `pub managed_settings: PathBuf`, `pub zygo: PathBuf` and gains the private field `agents_files: std::sync::Mutex<std::collections::HashMap<PathBuf, Option<Map<String, Value>>>>` with the doc comment `/// FIX-26 D12, §8.5 Subagents: the \`--print\` \`--agents\` files read in this launch, by path, so a pipe such as \`<(gen)\` is read once: \`refusal\` reads and stores, \`build\` uses the stored value. It holds values for one launch: one adapter serves one launch (\`crates/zygo/src/cmd/claude.rs:21\` makes one \`ClaudeCode::default()\` per \`zygo claude\` process).` New `pub fn new(program: impl Into<String>, managed_settings: PathBuf, zygo: PathBuf) -> ClaudeCode` with the doc comment `/// A Claude Code adapter for one launch, with an empty \`--agents\` file memo (FIX-26 D12). Make a new one for each launch, as \`crates/zygo/src/cmd/claude.rs:21\` does.`; `Default` calls it.
- Changed, private: `fn unreadable_line(value: &str, error: &FileError) -> String`. `UserSettings.unreadable: Vec<String>` keeps its type and now holds the finished lines, in value order (doc: `One line per value that is not a readable JSON object, in value order; zygo does not use those values.`); `build` extends `lines` with `u.unreadable.iter().cloned()`. `fn user_settings(ctx: &LaunchCtx) -> UserSettings` keeps its signature; for a value that does not parse as a JSON object it calls `read_settings_file(&ctx.cwd.join(&v))` in place of `read_object`, and on `Err(e)` pushes `unreadable_line(&v, &e)`.
- Changed, private: `fn user_agents(ctx: &LaunchCtx) -> Option<Map<String, Value>>` becomes the method `fn user_agents(&self, ctx: &LaunchCtx) -> Option<Map<String, Value>>` in `impl ClaudeCode` (`:321`); its callers become `self.user_agents(ctx)` (`refusal`, `build`). The `--print` file arm reads `ctx.cwd.join(&v)` through the memo: the first call for a path stores `read_settings_file(&path).ok()`, a later call clones the stored value (D12). Its doc comment ends `(with \`--print\`, a value that is not JSON is read as a file that holds one, as a \`--settings\` file is read, once per launch: FIX-26 D10, D12).`
- Changed, private: `fn read_object(p: &Path) -> Option<Map<String, Value>>` keeps its signature and its callers (`preflight`, `refusal`); its body becomes the safe rule of D9, with the doc comment `/// §8.4 (FIX-26 D9): a settings file that zygo found by itself, as a JSON object; \`None\` (the caller skips it with no line) when it is absent, is not a regular file (never opened then), cannot be read, is not JSON or is not an object.`
- New, private to `claude_code.rs`:
  - `const NO_FILE: &str = "zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used";` with the doc comment `/// §8.5 Flags (FIX-26 D2): the line for a user \`--settings\` value that is not a JSON object and has no entry at its path (no file found, or on Windows a name the system refuses); it does not hold the value, which can be inline settings with secrets.`
  - `const NOT_OBJECT: &str = "the file holds JSON that is not an object";` with the doc comment `/// §8.5 Flags (FIX-26 D7): the error for a file that holds JSON that is not an object.`
  - `enum FileError { NoFile, NotOpened(String), File(String) }` with the doc comment `/// §8.5 Flags (FIX-26 D2, D6): why a user value that is not a JSON object could not be used as a file.` and one doc line per variant: `NoFile`: `No entry at the path, and no file was found.`; `NotOpened`: `No entry at the path for another reason: the system error text, which holds no path.`; `File`: `An entry exists at the path and could not be used: the error.`
  - `fn read_settings_file(p: &Path) -> Result<Map<String, Value>, FileError>` with the doc comment `/// §8.5 Flags (FIX-26 D2, D3, D6): a file the user gave (\`--settings\`, \`--print --agents\`) as a JSON object, read as before FIX-26 (one blocking open that follows links), with the entry rule for the error.`
  - `fn parse_error(e: &serde_json::Error) -> String` with the doc comment `/// §8.5 Flags (FIX-26 D4): a \`serde_json\` error of user input as its category and position only; its text can hold literals of the input.`, giving `format!("{category} error at line {} column {}", e.line(), e.column())`, where `category` is `io`, `syntax`, `data` or `eof` for `serde_json::error::Category::{Io, Syntax, Data, Eof}` (`e.classify()`).
  - The three lines of `unreadable_line`: for `File(error)`, `format!("zygo: your --settings value {} could not be read as a JSON object and was not used: {}", zygo_core::esc(&Value::from(value).to_string()), zygo_core::esc(error))`; for `NotOpened(error)`, `format!("zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: {}; it was not used", zygo_core::esc(error))`; for `NoFile`, `NO_FILE`.
- FIX-24 consumes nothing from this node.

Decisions:

- D1 (controller decision, 2026-11-23). A value that parses as a JSON object is inline settings. Any other value is a file path, relative to the working directory (§8.5, observed on Claude Code 2.1.330). There is no first-character rule: a value that starts with `{` and does not parse is a path.
- D2 (controller decisions, 2026-11-23). The value text is printed only when an entry exists at the path (`std::fs::symlink_metadata` succeeds): the line holds the escaped value and the error (`FileError::File`). With no entry, the error of `symlink_metadata` decides: `ErrorKind::NotFound`, or on Windows `ErrorKind::InvalidFilename`, gives `NO_FILE` on every platform (`FileError::NoFile`). On Windows `InvalidFilename` covers `ERROR_INVALID_NAME` (123), `ERROR_BAD_PATHNAME` (161) and `ERROR_FILENAME_EXCED_RANGE` (206) (premise check), so on Windows a name that is too long also gives `NO_FILE`. Any other error (`EACCES` on a parent folder, `ELOOP` in a folder part, `ENOTDIR`, `ENAMETOOLONG` on unix) gives the line with the io error text and no value (`FileError::NotOpened`). On unix `ENAMETOOLONG` is `ErrorKind::InvalidFilename` too (premise check), so the rule is `cfg!(windows) && e.kind() == ErrorKind::InvalidFilename`.
- D3 (controller decision 1 of the second plan review, 2026-11-23). A value the user gives (`--settings`, `--print --agents`) keeps today's read semantics: one blocking open that follows links, as `std::fs::read` opens it, then the whole content, with no size limit and no time limit. Any type the system can read is read: regular files, FIFOs, pipes (`<(…)`, `/dev/fd/<n>`) and devices. A FIFO with no writer waits until a writer opens it and closes it, as it does today: that is the user's own input, not a `zygo` defect. A pipe whose writer writes after the launch starts is read in full.
- D4 (controller decision, 2026-11-23). A file that does not parse gives only `parse_error(&e)`. The text of a `serde_json` error of user input (`e.to_string()`) is never printed: a data error can hold literals of the file through the `$serde_json::private::Number` key.
- D5 (controller decision, 2026-11-23). §8.5 states the `serde_json` limit: `arbitrary_precision` and `raw_value` are on for the whole workspace, so the private keys are read by `serde_json`'s rule. §8.4, §8.5 Flags and §8.5 Subagents are rewritten in place; one change-log row names the controller decisions and lists every change.
- D6 (author's call). `read_settings_file` takes these steps in this order, each failure ending it: (1) `symlink_metadata(p)`: an error gives `NoFile` or `NotOpened` by D2. (2) `std::fs::read(p)` (D3): an error gives `File(e.to_string())` (a dangling link, a link loop at the last part, a directory, a file the user cannot read). (3) `serde_json::from_slice::<Value>`: an error gives `File(parse_error(&e))`; an object is `Ok`; any other JSON gives `File(NOT_OBJECT)`.
- D7 (author's call). The texts of `NO_FILE`, `NOT_OBJECT` and the open-error line are the ones the FIX-23 attempt used, so the evidence of its review rounds still applies. The attempt diff holds, in `crates/zygo-launch/src/claude_code.rs`:
  > `+const NO_FILE: &str = "zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used";`
  > `+        _ => Err(FileError::File("the file holds JSON that is not an object".into())),`
  > `+            "zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: {}; it was not used",`

  and its round notes record the evidence for these texts:
  > `R1 (unreadable line). ... for \`None\` it gives \`NO_FILE\` ... Red: \`settings_value_text_is_printed_only_for_a_file\` failed on all ten rows; for example \`BOM prefix\` gave \`... value "\u{feff}{\"env\": {\"TOKEN\": \"s3cret\"}}" could not be read ...\`, \`array\` gave \`... value "[\"s3cret\"]" ...\` ... Green: all ten rows pass.`
  > `long_broken_inline_settings_are_not_shown\`, 400 characters with \`s3cret\` (red: \`left: [NO_FILE]\`, \`right: [... could not be opened as a file: File name too long (os error 36); it was not used]\`)`
- D8 (controller decision, 2026-11-23). AU59 of `docs/specs/2026-11-20-workspaces-design.md` (`:200`) quotes the old unreadable line. FIX-24 owns that text; this node does not edit the workspaces spec. The change FIX-24 makes is under "AU59 text for FIX-24" below.
- D9 (controller decision 2 of the second plan review, 2026-11-23). The §8.4 settings files that `zygo` finds by itself (`read_object`, used by `preflight` at `:514` and `refusal` at `:542`; a cloned repository can plant them) use the safe rule, in this order, each failure giving `None` (skip with no line): (1) `std::fs::metadata(p)` (links followed): an error, or a target that is not a regular file (a FIFO, a socket, a device, a directory), gives `None`, and the target is not opened. (2) On unix, open with `OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK)`; when it fails with `ErrorKind::WouldBlock` (a lease), one second open `std::fs::File::open(p)` with no `O_NONBLOCK`; there is no third open and no loop. On other platforms one plain open. (3) The handle's metadata (`fstat`): an error or a type that is not a regular file gives `None`. (4) On unix, when the handle was opened with `O_NONBLOCK`, the `fcntl` pair of `session_view.rs:182-199` clears it; a `-1` gives `None`. (5) `read_to_end`, with no size limit and no time limit; an error gives `None`. (6) `serde_json::from_slice::<Value>`: an object is `Some`; anything else gives `None`. Printed text: no change; these reads print nothing for a file they cannot use, today and after this node. Behavior change: a FIFO, a socket, a device or a directory at one of these paths, also through a link, is skipped with no line and not opened, in place of a launch that waits for ever (FIFO) or a read of the device. Because no FIFO is ever read by this path, no platform needs evidence of how an empty FIFO reads (the macOS question of the review is gone).
- D10 (controller decisions, 2026-11-23). The `--print` `--agents` file read of `user_agents` uses `read_settings_file` (D3, D6) through the memo (D12). Printed text: no change. A file that cannot be used still gives `None`, so `refusal` still prints `zygo: --agents: not a JSON object (or, with --print, a file that holds one)` with exit 2, as it does today. A pipe that holds a JSON object (`--print --agents <(gen)`) is used, also when its writer writes after the launch starts. A FIFO with no writer waits, as today (D3).
- D11 (controller decisions M2 and 2 of the second plan review, 2026-11-23). Residual risks of the D9 rule, stated in §8.4 and not closed by this node; there is no retry loop and no time limit: (a) a FIFO swapped in for a regular file between the `metadata` read and the open is opened, but with `O_NONBLOCK`, so the open does not wait, and the handle check (D9 step 3) skips it, so the launch does not wait. (b) After a write lease makes the first open fail with `EAGAIN`, the second open has no `O_NONBLOCK`, so a FIFO swapped in at that moment with no writer makes the launch wait until a writer opens it; this needs a lease holder and the right to rename in that folder. Linux `O_PATH` (open the path once, check the type on that handle, then reopen through `/proc/self/fd`) could close it; it is not used because macOS has no `O_PATH`. (c) On Windows, `std::fs::metadata` itself opens the target with access 0 (`CreateFileW` with no access rights). (d) A FUSE mount that does not answer makes the `metadata` read wait.
- D12 (author's call, from defect (d); controller decisions M1 and m-3, 2026-11-23). A pipe can be read once (premise check), and the launcher calls `refusal` and then `build` on one adapter, each reading the `--agents` file. So `ClaudeCode` keeps a memo of the `--print` `--agents` files it read, by path: `refusal` reads and stores, `build` uses the stored value. The memo holds values for one launch, and one adapter serves one launch (`crates/zygo/src/cmd/claude.rs:21`); the doc comments of the field and of `new` say so, and no code guard is added. The memo is a private field, so `ClaudeCode` gets `new` (Produces), and the five struct-literal sites change (premise check). The lock is taken with `.lock().unwrap_or_else(std::sync::PoisonError::into_inner)`. Not in the memo: the managed file, which `refusal` (passthrough) and `preflight` both read; under D9 a managed path that is a pipe is not a regular file and is skipped by both.

Steps:

- [ ] Refactor with no behavior change, in `crates/zygo-launch/src/claude_code.rs`: add the field `agents_files` and `ClaudeCode::new` with the doc comments under Interfaces; `Default` returns `ClaudeCode::new("claude", MANAGED.into(), std::env::current_exe().unwrap_or_default())`. Replace the struct literals: `crates/zygo-launch/tests/claude_code.rs:191` (`cc`) with `ClaudeCode::new("claude", fx.managed.clone(), fx.root.join("bin").join("zygo"))`; `:846` with `ClaudeCode::new("claude", fx.managed.clone(), fx.bin.join("zygo"))`; `:350` with `let mut a = cc(&fx); a.zygo = PathBuf::new(); let e = a.build(&c).err().unwrap();`; `:361` with `let mut a = cc(&fx); a.zygo = zygo; let e = a.build(&c).err().unwrap();`; `crates/zygo-launch/tests/stub_agent/harness.rs:70` with `ClaudeCode::new("claude", rig.root.join("managed-settings.json"), rig.zygo.clone())`. Run `cargo test -p zygo-launch --locked --no-fail-fast 2>&1` → all pass, as before the step.
- [ ] In `crates/zygo-launch/tests/claude_code.rs`, replace the helper `unreadable(value: &str) -> String` (after `defeating_flags_are_logged_and_settings_warns`) with:
  ```rust
  /// §8.5 Flags (FIX-26 D2): the line for a user `--settings` value that names an entry zygo cannot read as a JSON object;
  /// `why` is the error.
  fn unreadable(value: &str, why: &str) -> String {
      format!("zygo: your --settings value {} could not be read as a JSON object and was not used: {why}", json!(value))
  }

  /// §8.5 Flags (FIX-26 D2): the line for a user `--settings` value that is not a JSON object and has no entry at its
  /// path (no file found, or on Windows a name the system refuses).
  const NO_FILE: &str = "zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used";

  /// §8.5 Flags (FIX-26 D2): the line for a value with no entry at its path for another reason than a missing file.
  #[cfg(unix)]
  fn not_opened(why: &str) -> String {
      format!("zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: {why}; it was not used")
  }

  /// The error in `unreadable` for a file that holds `not json` (FIX-26 D4).
  const NOT_JSON: &str = "syntax error at line 1 column 2";
  /// The error in `unreadable` for a file that holds JSON that is not an object (FIX-26 D7).
  const NOT_OBJECT: &str = "the file holds JSON that is not an object";

  /// The system error text for `code`.
  #[cfg(unix)]
  fn os_error(code: i32) -> String {
      std::io::Error::from_raw_os_error(code).to_string()
  }

  /// The lines of a build with the user arguments `args`.
  fn lines_of(fx: &Fx, args: &[&str]) -> Vec<String> {
      let mut c = ctx(fx, "");
      c.args = strings(args);
      cc(fx).build(&c).unwrap().lines
  }

  /// `text` in an anonymous pipe whose write end is closed, and the `/dev/fd/<n>` path of its read end: the form of
  /// bash `<(printf ...)`. The data is in the pipe before the build starts.
  #[cfg(unix)]
  fn pipe_with(text: &str) -> (std::io::PipeReader, String) {
      use std::io::Write;
      use std::os::fd::AsRawFd;
      let (r, mut w) = std::io::pipe().unwrap();
      w.write_all(text.as_bytes()).unwrap();
      drop(w);
      let path = format!("/dev/fd/{}", r.as_raw_fd());
      (r, path)
  }

  /// Runs `f` on its own thread and gives its result, or fails the test: a timeout means a hang in the code under test,
  /// a disconnect means the thread panicked in its setup. The 20 s bound is a guard of the test only; zygo has no time
  /// limit.
  #[cfg(unix)]
  fn no_hang<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
      use std::sync::mpsc::{channel, RecvTimeoutError};
      let (tx, rx) = channel();
      std::thread::spawn(move || {
          let _ = tx.send(f());
      });
      match rx.recv_timeout(std::time::Duration::from_secs(20)) {
          Ok(v) => v,
          Err(RecvTimeoutError::Timeout) => panic!("{what}: did not return within 20 s: it waits"),
          Err(RecvTimeoutError::Disconnected) => panic!("{what}: test setup failed: the thread ended with no result (its panic is above)"),
      }
  }

  /// Runs `f` with the `/dev/fd/<n>` path of an anonymous pipe whose write end the test keeps open, checks that `f` has
  /// not returned after 1 s (it waits for the writer), then writes `text`, closes the write end and gives the result of
  /// `f`: a writer that writes after the build starts (FIX-26 D3). The bounds are guards of the test only.
  #[cfg(unix)]
  fn late_pipe<T: Send + std::fmt::Debug + 'static>(text: &str, f: impl FnOnce(String) -> T + Send + 'static) -> T {
      use std::io::Write;
      use std::os::fd::AsRawFd;
      use std::sync::mpsc::{channel, RecvTimeoutError};
      use std::time::Duration;
      let (r, mut w) = std::io::pipe().unwrap();
      let path = format!("/dev/fd/{}", r.as_raw_fd());
      let (tx, rx) = channel();
      std::thread::spawn(move || {
          let _ = tx.send(f(path));
      });
      match rx.recv_timeout(Duration::from_secs(1)) {
          Err(RecvTimeoutError::Timeout) => {}
          Err(RecvTimeoutError::Disconnected) => panic!("test setup failed: the thread ended with no result (its panic is above)"),
          Ok(v) => panic!("returned before the writer wrote, so a late writer is dropped: {v:?}"),
      }
      w.write_all(text.as_bytes()).unwrap();
      drop(w);
      let v = match rx.recv_timeout(Duration::from_secs(20)) {
          Ok(v) => v,
          Err(RecvTimeoutError::Timeout) => panic!("did not return within 20 s after the writer closed the pipe"),
          Err(RecvTimeoutError::Disconnected) => panic!("test setup failed: the thread ended with no result (its panic is above)"),
      };
      drop(r);
      v
  }

  /// Makes a FIFO at `p` (test setup).
  #[cfg(unix)]
  fn mkfifo(p: &Path) {
      let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
      // SAFETY: `c` is a NUL-terminated path that lives for the call.
      assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "test setup: mkfifo {}", p.display());
  }
  ```
- [ ] In the same file, change the rows that expect the old texts. `defeating_flags_are_logged_and_settings_warns`: `assert_eq!(spec.lines, [unreadable("x.json")]);` becomes `assert_eq!(spec.lines, [NO_FILE]);`. `unreadable_user_settings_keep_only_zygos_file`: the `cases` become
  ```rust
  (vec!["--settings", "missing.json", "-p", "hi"], vec![NO_FILE.to_string()]),
  (vec!["--settings", "bad.json", "-p", "hi"], vec![unreadable("bad.json", NOT_JSON)]),
  (vec!["--settings", "list.json", "-p", "hi"], vec![unreadable("list.json", NOT_OBJECT)]),
  (vec!["--settings", "{bad", "-p", "hi"], vec![NO_FILE.to_string()]),
  (vec!["--settings=[1]", "-p", "hi"], vec![NO_FILE.to_string()]),
  (vec!["--settings", "\"two\nlines\"", "-p", "hi"], vec![NO_FILE.to_string()]),
  (vec!["--settings", "good.json", "--settings", "missing.json", "-p", "hi"], vec![MERGE_WARN.to_string(), NO_FILE.to_string()]),
  ```
  and its comment gains `; FIX-26: kills: a value with no entry printed`. `merge_line_is_true_when_disable_all_hooks_is_set`: the `lines` of row `true and unreadable` become `vec![MERGE_HOOKS_OFF.to_string(), NO_FILE.to_string()]`, of row `number and unreadable` `vec![MERGE_WARN.to_string(), bad_hooks("a number"), NO_FILE.to_string()]`. `settings_lines_escape_every_control_character`: the third expected line becomes `r#"zygo: your --settings value "m\u{9b}i\u{85}s\u{7f}s\u{2028}x\u{61c}.json" could not be read as a JSON object and was not used: syntax error at line 1 column 2"#.to_string()`. `settings_lines_on_stderr_are_safe_and_in_order`: `let path_line = at(r#"zygo: your --settings value "m\u{9b}i\u{85}s\u{7f}s\u{2028}x\u{61c}.json" could not be read as a JSON object and was not used: syntax error at line 1 column 2"#);` and `let missing = at(NO_FILE);`.
- [ ] In the same file, after `user_disable_all_hooks_wins_and_attribution_keys_stay`, add `settings_value_text_is_printed_only_for_a_file`, with no `cfg`, comment `// FIX-26 (D1, D2), §8.5 Flags: kills: a value with no entry at its path printed (a BOM or U+200B prefix, an array, a broken array, a JSON string, broken inline JSON on a member launch or in the = form), so a secret reaches stderr; a first-character rule that takes the file "{x" as inline; on Windows an invalid name given the open-error line in place of NO_FILE; a printed path with a raw bidi control`. Body:
  ```rust
  let fx = fx();
  const BAD_INLINE: &str = r#"{"env": {"TOKEN":"s3cret"}, "disableAllHooks": tru}"#;
  let rlo = "a\u{202e}b.json";
  std::fs::write(fx.cwd.join("{x"), "not json").unwrap();
  std::fs::write(fx.cwd.join(rlo), "not json").unwrap();
  let bom = "\u{feff}{\"env\": {\"TOKEN\": \"s3cret\"}}";
  let zwsp = "\u{200b}{\"env\": {\"TOKEN\": \"s3cret\"}}";
  let eq_form = format!("--settings={BAD_INLINE}");
  type Row<'a> = (&'a str, bool, Vec<&'a str>, Vec<String>);
  let rows: [Row; 10] = [
      ("BOM prefix", false, vec!["--settings", bom], vec![NO_FILE.to_string()]),
      ("U+200B prefix", false, vec!["--settings", zwsp], vec![NO_FILE.to_string()]),
      ("array", false, vec!["--settings", r#"["s3cret"]"#], vec![NO_FILE.to_string()]),
      ("broken array", false, vec!["--settings", r#"["s3cret""#], vec![NO_FILE.to_string()]),
      ("JSON string", false, vec!["--settings", r#""s3cret""#], vec![NO_FILE.to_string()]),
      ("missing path", false, vec!["--settings", "nofile.json"], vec![NO_FILE.to_string()]),
      ("broken inline, member", true, vec!["--settings", BAD_INLINE], vec![NO_FILE.to_string()]),
      ("broken inline, = form", false, vec![&eq_form], vec![NO_FILE.to_string()]),
      ("file named {x", false, vec!["--settings", "{x"], vec![unreadable("{x", NOT_JSON)]),
      ("U+202E path", false, vec!["--settings", rlo], vec![r#"zygo: your --settings value "a\u{202e}b.json" could not be read as a JSON object and was not used: syntax error at line 1 column 2"#.to_string()]),
  ];
  let mut failed = vec![];
  for (name, member, args, lines) in rows {
      let mut c = if member { member_ctx(&fx, true) } else { ctx(&fx, "") };
      c.args = strings(&args);
      let spec = cc(&fx).build(&c).unwrap();
      let ok = spec.lines == lines && !spec.lines.iter().any(|l| l.contains("s3cret") || l.contains('\u{202e}'));
      if !ok {
          failed.push(format!("{name}: {:?}", spec.lines));
      }
  }
  assert!(failed.is_empty(), "{failed:#?}");
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `settings_path_errors_show_the_value_only_for_an_entry`, comment `// FIX-26 (D2, D6), §8.5 Flags: kills: an error with no entry at the path (ENOTDIR, ELOOP in a folder part) given NO_FILE or printed with the value; an entry at the path (a link loop at the last part, a dangling link, a directory) given NO_FILE or the open-error line`. Body:
  ```rust
  let fx = fx();
  std::fs::write(fx.cwd.join("file.json"), "{}").unwrap();
  std::os::unix::fs::symlink("loop.json", fx.cwd.join("loop.json")).unwrap();
  std::os::unix::fs::symlink("nowhere.json", fx.cwd.join("dangle.json")).unwrap();
  std::fs::create_dir(fx.cwd.join("dir.json")).unwrap();
  let rows: [(&str, Vec<String>, &str); 5] = [
      ("file.json/x", vec![not_opened(&os_error(libc::ENOTDIR))], "file.json"),
      ("loop.json/x", vec![not_opened(&os_error(libc::ELOOP))], "loop.json"),
      ("loop.json", vec![unreadable("loop.json", &os_error(libc::ELOOP))], ""),
      ("dangle.json", vec![unreadable("dangle.json", &os_error(libc::ENOENT))], ""),
      ("dir.json", vec![unreadable("dir.json", &os_error(libc::EISDIR))], ""),
  ];
  for (value, lines, hidden) in rows {
      let got = lines_of(&fx, &["--settings", value]);
      assert_eq!(got, lines, "{value}");
      assert!(hidden.is_empty() || !got.iter().any(|l| l.contains(hidden)), "{value}: {got:?}");
  }
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `settings_link_to_a_file_is_followed`, comment `// FIX-26 (D3), §8.5 Flags: kills: a user --settings link not followed (O_NOFOLLOW, or a type check on the link itself)`. Body:
  ```rust
  let fx = fx();
  std::fs::write(fx.cwd.join("good.json"), r#"{"outputStyle": "x"}"#).unwrap();
  std::os::unix::fs::symlink("good.json", fx.cwd.join("link.json")).unwrap();
  assert_eq!(lines_of(&fx, &["--settings", "link.json"]), [MERGE_WARN]);
  assert_eq!(settings(&fx)["outputStyle"], "x");
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `settings_from_a_pipe_are_used`, comment `// FIX-26 (D3), §8.5 Flags: kills: a pipe (the <(…) form /dev/fd/<n>) refused as not a regular file, or a link to one not followed, so process substitution no longer works`. Body:
  ```rust
  let fx = fx();
  let (_r1, direct) = pipe_with(r#"{"outputStyle": "x"}"#);
  let (_r2, target) = pipe_with(r#"{"theme": "dark"}"#);
  std::os::unix::fs::symlink(&target, fx.cwd.join("pipelink.json")).unwrap();
  assert_eq!(lines_of(&fx, &["--settings", &direct, "--settings", "pipelink.json"]), [MERGE_WARN]);
  let s = settings(&fx);
  assert_eq!((s["outputStyle"].as_str(), s["theme"].as_str()), (Some("x"), Some("dark")));
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `settings_from_a_pipe_with_a_late_writer_are_used`, comment `// FIX-26 (D3), §8.5 Flags: kills: a user --settings pipe read with O_NONBLOCK or with a type check that ends the read at once, so a writer that writes after the build starts is dropped (an eof error line in place of the merge)`. Body:
  ```rust
  let fx = Arc::new(fx());
  let f = Arc::clone(&fx);
  let lines = late_pipe(r#"{"outputStyle": "x"}"#, move |value| lines_of(&f, &["--settings", &value]));
  assert_eq!(lines, [MERGE_WARN]);
  assert_eq!(settings(&fx)["outputStyle"], "x");
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `settings_under_unsearchable_folder_are_not_shown`, comment `// FIX-26 (D2), §8.5 Flags: kills: EACCES on a parent folder (no entry found) given NO_FILE, or printed with the value`. Body:
  ```rust
  use std::os::unix::fs::PermissionsExt;
  /// Gives the folder its search right back, also when an assertion fails, so the test folder can be removed.
  struct Restore(PathBuf);
  impl Drop for Restore {
      fn drop(&mut self) {
          let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
      }
  }
  let fx = fx();
  let locked = fx.cwd.join("locked");
  std::fs::create_dir(&locked).unwrap();
  std::fs::write(locked.join("x.json"), "{}").unwrap();
  let _restore = Restore(locked.clone());
  std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
  if std::fs::symlink_metadata(locked.join("x.json")).is_ok() {
      eprintln!("note: settings_under_unsearchable_folder_are_not_shown skipped: the test runs as root, so the folder can be searched");
      return;
  }
  let lines = lines_of(&fx, &["--settings", "locked/x.json"]);
  assert_eq!(lines, [not_opened(&os_error(libc::EACCES))]);
  assert!(!lines.iter().any(|l| l.contains("locked") || l.contains("x.json")), "{lines:?}");
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `settings_file_with_mode_0_names_the_path_and_error`, comment `// FIX-26 (D2, D6), §8.5 Flags: kills: a file that exists but cannot be opened given NO_FILE or the open-error line with no path`. Body:
  ```rust
  use std::os::unix::fs::PermissionsExt;
  let fx = fx();
  let p = fx.cwd.join("noperm.json");
  std::fs::write(&p, "{}").unwrap();
  std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
  if std::fs::File::open(&p).is_ok() {
      eprintln!("note: settings_file_with_mode_0_names_the_path_and_error skipped: the test runs as root, so mode 0 does not stop an open");
      return;
  }
  assert_eq!(lines_of(&fx, &["--settings", "noperm.json"]), [unreadable("noperm.json", &os_error(libc::EACCES))]);
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `long_broken_inline_settings_are_not_shown`, comment `// FIX-26 (D2), §8.5 Flags: kills: ENAMETOOLONG (ErrorKind::InvalidFilename on unix) given NO_FILE, that is the Windows invalid-name rule applied on unix; broken inline JSON longer than NAME_MAX printed`. Body:
  ```rust
  let fx = fx();
  let head = r#"{"env": {"TOKEN": "s3cret"}, "pad": ""#;
  let tail = r#"", tru}"#;
  let value = format!("{head}{}{tail}", "x".repeat(400 - head.len() - tail.len()));
  assert_eq!(value.len(), 400);
  let lines = lines_of(&fx, &["--settings", &value]);
  assert_eq!(lines, [not_opened(&os_error(libc::ENAMETOOLONG))]);
  assert!(!lines.iter().any(|l| l.contains("s3cret") || l.contains("TOKEN")), "{lines:?}");
  ```
- [ ] In the same file, after that test, add `settings_file_parse_errors_name_only_category_and_position`, with no `cfg`, comment `// FIX-26 (D4), §8.5 Flags: kills: serde_json's error text printed (for a data error it holds the literal 1234567890123 through the $serde_json::private::Number key); a category written as the wrong word; the position lost`. Body:
  ```rust
  let fx = fx();
  let cases = [
      ("syntax.json", "not json", "syntax error at line 1 column 2"),
      ("data.json", r#"{"$serde_json::private::Number": 1234567890123}"#, "data error at line 1 column 46"),
      ("eof.json", r#"{"a": 1"#, "eof error at line 1 column 7"),
  ];
  for (file, text, error) in cases {
      std::fs::write(fx.cwd.join(file), text).unwrap();
      let lines = lines_of(&fx, &["--settings", file]);
      assert_eq!(lines, [unreadable(file, error)], "{file}");
      assert!(!lines.iter().any(|l| l.contains("1234567890123") || l.contains("expected") || l.contains("EOF while")), "{lines:?}");
  }
  ```
- [ ] In the same file, after `bad_user_agents_are_refused`, add `#[cfg(unix)]` `print_agents_from_a_pipe_are_used`, comment `// FIX-26 (D10, D12), §8.5 Subagents: kills: the --agents file read again in build after refusal read it, so a pipe is empty there and the user's agents are dropped with no line; an --agents pipe refused`. One adapter serves both calls, as in the launcher (`launcher.rs:583`, `:587`). Body:
  ```rust
  let fx = fx();
  let (_r, value) = pipe_with(r#"{"helper": {"description": "h", "prompt": "y"}}"#);
  let mut c = ctx(&fx, "");
  c.args = strings(&["--print", "--agents", &value]);
  let a = cc(&fx);
  assert_eq!(a.refusal(&c), None);
  let spec = a.build(&c).unwrap();
  assert_eq!(agents_of(&spec.args)["helper"]["prompt"], "y");
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `print_agents_from_a_pipe_with_a_late_writer_are_used`, comment `// FIX-26 (D3, D10, D12), §8.5 Subagents: kills: the --print --agents pipe read with O_NONBLOCK or ended at once, so a writer that writes after the launch starts is dropped and the launch is refused; the file read twice`. Body:
  ```rust
  let fx = Arc::new(fx());
  let f = Arc::clone(&fx);
  let (refusal, agents) = late_pipe(r#"{"helper": {"description": "h", "prompt": "y"}}"#, move |value| {
      let mut c = ctx(&f, "");
      c.args = strings(&["--print", "--agents", &value]);
      let a = cc(&f);
      let refusal = a.refusal(&c);
      let spec = a.build(&c).unwrap();
      (refusal, agents_of(&spec.args))
  });
  assert_eq!(refusal, None);
  assert_eq!(agents["helper"]["prompt"], "y");
  ```
- [ ] In the same file, after `repo_root_settings_stop_the_launch`, add `#[cfg(unix)]` `preflight_skips_a_found_fifo_with_no_hang`, comment `// FIX-26 (D9), §8.4 Preflight, §12.3: kills: a found settings file or the managed file opened with a blocking open, so a FIFO with no writer at <cwd>/.claude/settings.json or at the managed path makes the launch, the preflight or the passthrough refusal wait for ever`. Body:
  ```rust
  let fx = Arc::new(fx());
  script(&fx, "claude", "[ \"$1\" = --version ] && exit 0\nexit 0");
  std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
  mkfifo(&fx.cwd.join(".claude/settings.json"));
  mkfifo(&fx.managed);
  let f = Arc::clone(&fx);
  let (code, err) = no_hang("launch", move || {
      let o = launch_opts(&f, &[], false);
      let err = stderr_of(&o);
      (Launcher::run(&cc(&f), o).code, text_of(&err))
  });
  assert_eq!(code, 0, "{err}");
  let f = Arc::clone(&fx);
  let (preflight, refusal) = no_hang("preflight and passthrough refusal", move || {
      let mut c = ctx(&f, "");
      c.passthrough = true;
      let a = cc(&f);
      (a.preflight(&c), a.refusal(&c))
  });
  assert_eq!((preflight, refusal), (Ok(()), None));
  ```
- [ ] In the same file, after that test, add `#[cfg(target_os = "linux")]` `preflight_does_not_open_a_found_fifo_link_or_device`, comment `// FIX-26 (D9), §8.4 Preflight, §12.3: kills: a found FIFO, a found link to a FIFO or a found device (here the managed path is a pseudo-terminal slave) opened before the type check, with O_NONBLOCK too: an IN_OPEN event on a path that only this test can open`. Body:
  ```rust
  use std::os::fd::{FromRawFd, OwnedFd};
  let fx = Arc::new(fx());
  std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
  let fifo = fx.cwd.join(".claude/settings.json");
  let other = fx.cwd.join("other-fifo");
  mkfifo(&fifo);
  mkfifo(&other);
  std::os::unix::fs::symlink(&other, fx.cwd.join(".claude/settings.local.json")).unwrap();
  // SAFETY: plain libc calls; every descriptor is owned below and closed on drop.
  let m = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
  if m < 0 {
      eprintln!("note: preflight_does_not_open_a_found_fifo_link_or_device skipped: posix_openpt failed ({})", std::io::Error::last_os_error());
      return;
  }
  // SAFETY: `m` is a new descriptor that nothing else owns.
  let master = unsafe { OwnedFd::from_raw_fd(m) };
  // SAFETY: `m` is open for the calls.
  if unsafe { libc::grantpt(m) } != 0 || unsafe { libc::unlockpt(m) } != 0 {
      eprintln!("note: preflight_does_not_open_a_found_fifo_link_or_device skipped: grantpt or unlockpt failed ({})", std::io::Error::last_os_error());
      return;
  }
  let mut name = [0 as libc::c_char; 128];
  // SAFETY: `name` is a buffer of the given length.
  assert_eq!(unsafe { libc::ptsname_r(m, name.as_mut_ptr(), name.len()) }, 0, "test setup: ptsname_r");
  // SAFETY: `ptsname_r` wrote a NUL-terminated name.
  let slave = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_str().unwrap().to_string();
  // SAFETY: plain libc call.
  let i = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
  assert!(i >= 0, "test setup: inotify_init1");
  // SAFETY: `i` is a new descriptor that nothing else owns.
  let ino = unsafe { OwnedFd::from_raw_fd(i) };
  for p in [fifo.clone(), other.clone(), PathBuf::from(&slave)] {
      let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
      // SAFETY: `c` is a NUL-terminated path that lives for the call.
      if unsafe { libc::inotify_add_watch(i, c.as_ptr(), libc::IN_OPEN) } < 0 {
          eprintln!("note: preflight_does_not_open_a_found_fifo_link_or_device skipped: no inotify watch on {} ({})", p.display(), std::io::Error::last_os_error());
          return;
      }
  }
  let (f, s) = (Arc::clone(&fx), slave.clone());
  let (preflight, refusal) = no_hang("preflight and passthrough refusal", move || {
      let a = ClaudeCode::new("claude", PathBuf::from(s), f.root.join("bin").join("zygo"));
      let mut c = ctx(&f, "");
      let preflight = a.preflight(&c);
      c.passthrough = true;
      (preflight, a.refusal(&c))
  });
  assert_eq!((preflight, refusal), (Ok(()), None));
  let mut ev = [0u8; 256];
  // SAFETY: `ev` is a buffer of the given length; `i` is open (owned by `ino`).
  let n = unsafe { libc::read(i, ev.as_mut_ptr().cast(), ev.len()) };
  assert_eq!(n, -1, "a found FIFO, a link to a FIFO or the device {slave} was opened: inotify reported IN_OPEN");
  drop((ino, master));
  ```
- [ ] In the same file, after that test, add `#[cfg(unix)]` `preflight_follows_a_link_to_a_settings_file`, comment `// FIX-26 (D9), §8.4 Preflight: kills: a found settings file that is a link not followed (O_NOFOLLOW, or a type check on the link itself), so its credential key does not stop the launch`. Body:
  ```rust
  let fx = fx();
  put(&fx.cwd.join("real.json"), r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#);
  std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
  let link = fx.cwd.join(".claude/settings.json");
  std::os::unix::fs::symlink(fx.cwd.join("real.json"), &link).unwrap();
  assert_eq!(cc(&fx).preflight(&ctx(&fx, "")), pf(link, "env.CLAUDE_CODE_USE_VERTEX"));
  ```
- [ ] In the same file, after that test, add `#[cfg(target_os = "linux")]` `preflight_reads_a_leased_settings_file_after_a_blocking_open`, comment `// FIX-26 (D9), §8.4 Preflight: kills: a found regular file skipped when the O_NONBLOCK open fails with EWOULDBLOCK (a lease) and is not opened again without O_NONBLOCK, so its credential key does not stop the launch`. A Linux write lease is taken without root by the file owner (premise check); macOS has no `F_SETLEASE` in `libc`. Body:
  ```rust
  use std::os::unix::io::AsRawFd;
  use std::time::{Duration, Instant};
  let fx = fx();
  std::fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
  let p = fx.cwd.join(".claude/settings.json");
  std::fs::write(&p, r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#).unwrap();
  let holder = std::fs::File::open(&p).unwrap();
  let fd = holder.as_raw_fd();
  // The kernel sends SIGIO to a lease holder when another open waits; the test ignores it and restores it at the end.
  // SAFETY: a plain signal disposition change; no handler runs.
  let old = unsafe { libc::signal(libc::SIGIO, libc::SIG_IGN) };
  // SAFETY: `fd` is the open descriptor of `holder`, which lives for the test.
  if unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_WRLCK) } != 0 {
      let e = std::io::Error::last_os_error();
      // SAFETY: restores the disposition read above.
      unsafe { libc::signal(libc::SIGIO, old) };
      eprintln!("note: preflight_reads_a_leased_settings_file_after_a_blocking_open skipped: no write lease on the test folder ({e})");
      return;
  }
  let result = std::thread::scope(|s| {
      let pre = s.spawn(|| cc(&fx).preflight(&ctx(&fx, "")));
      // The O_NONBLOCK open fails with EWOULDBLOCK and starts the lease break, so the lease then reads F_RDLCK.
      let start = Instant::now();
      // SAFETY: as above.
      while unsafe { libc::fcntl(fd, libc::F_GETLEASE) } != libc::F_RDLCK && start.elapsed() < Duration::from_secs(20) {
          std::thread::sleep(Duration::from_millis(10));
      }
      // SAFETY: as above.
      assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_UNLCK) }, 0, "release the lease");
      pre.join().unwrap()
  });
  // SAFETY: restores the disposition read above.
  unsafe { libc::signal(libc::SIGIO, old) };
  assert_eq!(result, pf(p, "env.CLAUDE_CODE_USE_VERTEX"));
  ```
- [ ] In `crates/zygo-launch/tests/stub_launch.rs`, replace `unreadable(value: &str) -> String` (`:137-140`) with the two-argument `unreadable(value: &str, why: &str) -> String` and the `NO_FILE` constant, with the same texts and doc comments as in `tests/claude_code.rs`. In `unreadable_user_settings_launch_with_only_zygos_file`, the loop becomes
  ```rust
  for (value, line) in [(missing.as_str(), NO_FILE.to_string()), (bad.as_str(), unreadable(&bad, "syntax error at line 1 column 2")), ("[1]", NO_FILE.to_string())] {
      let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.args = vec!["--settings".into(), value.into()]);
      assert_eq!(r.code, 0, "{value}: {}", r.stderr);
      assert!(r.stderr.lines().any(|l| l == line), "{value}: {}", r.stderr);
      assert!(!r.stderr.contains(MERGE_WARN), "{value}: {}", r.stderr);
      assert_only_zygos_settings(&r, &rig);
  }
  ```
  and its comment gains `; FIX-26: kills: a value with no entry printed, or a file line with no reason`. In `member_with_unreadable_user_settings_stays_a_member`, `assert!(r.stderr.lines().any(|l| l == unreadable("missing.json")), "{}", r.stderr);` becomes `assert!(r.stderr.lines().any(|l| l == NO_FILE), "{}", r.stderr);`.
- [ ] Run and see red: `cargo test -p zygo-launch --locked --test claude_code --test stub_launch 2>&1`. Expected failures, with `left` holding the old line `zygo: your --settings value "<value>" could not be read as a JSON object and was not used` (no reason) or the old value text, or a guard panic: `defeating_flags_are_logged_and_settings_warns`, `unreadable_user_settings_keep_only_zygos_file`, `merge_line_is_true_when_disable_all_hooks_is_set` (row `true and unreadable`), `settings_lines_escape_every_control_character`, `settings_lines_on_stderr_are_safe_and_in_order` (the path line not found), `settings_value_text_is_printed_only_for_a_file` (every row; `BOM prefix` holds `s3cret`), `settings_path_errors_show_the_value_only_for_an_entry`, `settings_under_unsearchable_folder_are_not_shown` (or its skip note as root), `settings_file_with_mode_0_names_the_path_and_error` (or its skip note as root), `long_broken_inline_settings_are_not_shown`, `settings_file_parse_errors_name_only_category_and_position`, `print_agents_from_a_pipe_are_used` and `print_agents_from_a_pipe_with_a_late_writer_are_used` (no `helper` in the `--agents` argument: the second read was empty), `preflight_skips_a_found_fifo_with_no_hang` (`launch: did not return within 20 s`), `preflight_does_not_open_a_found_fifo_link_or_device` (`preflight and passthrough refusal: did not return within 20 s`: the old blocking open of the FIFO waits), and in `stub_launch` `unreadable_user_settings_launch_with_only_zygos_file` and `member_with_unreadable_user_settings_stays_a_member`. Five new tests pass on the old code, because the old reader is the blocking `std::fs::read`, which follows links and reads pipes and leased files: `settings_link_to_a_file_is_followed`, `settings_from_a_pipe_are_used`, `settings_from_a_pipe_with_a_late_writer_are_used`, `preflight_follows_a_link_to_a_settings_file` and `preflight_reads_a_leased_settings_file_after_a_blocking_open`; they keep D3 and D9 against a later `O_NONBLOCK` read of user values, `O_NOFOLLOW`, or a skip on `WouldBlock`.
- [ ] Implement in `crates/zygo-launch/src/claude_code.rs`, as under Interfaces, D6 and D9: the constants `NO_FILE`, `NOT_OBJECT` and `enum FileError` with the doc comments under Interfaces; `read_settings_file` (D6); `parse_error`; the new `unreadable_line`; `user_settings` calling `read_settings_file` and pushing finished lines; `build` extending `lines` with `u.unreadable.iter().cloned()` after the `bad_hooks` lines; `read_object` with the steps of D9; `user_agents` as a method with the memo (D10, D12), and its two callers. The unix parts of `read_object` use `std::os::unix::fs::OpenOptionsExt::custom_flags(libc::O_NONBLOCK)` and `std::os::unix::io::AsRawFd`, behind `#[cfg(unix)]`, with a `// SAFETY:` comment on each `fcntl` call as in `session_view.rs:182-199`. `preflight`, `refusal` (except the method call) and `BAD_AGENTS` do not change.
- [ ] Run and see green: `cargo test -p zygo-launch --locked --test claude_code --test stub_launch 2>&1` → all pass. A test that prints a skip note did not check its case (see Acceptance).
- [ ] Apply "Spec text" below to `docs/specs/2026-11-18-zygo-design.md`. Checks: `rg -n -F "names no file; it was not used" docs/specs/2026-11-18-zygo-design.md crates/zygo-launch` → hits in §8.5 Flags, the change-log row, `src/claude_code.rs`, `tests/claude_code.rs` and `tests/stub_launch.rs`; `rg -n -F "could not be read as a JSON object and was not used\`" docs/specs/2026-11-18-zygo-design.md` → no hit (every quote of that line ends with `: <error>`); `rg -n -F "is not a regular file or pipe" docs/specs/2026-11-18-zygo-design.md crates/zygo-launch` → no hit; `rg -n -F "O_PATH" docs/specs/2026-11-18-zygo-design.md` → one hit, in §8.4 Preflight; `rg -n -F "zygo: --agents: not a JSON object (or, with --print, a file that holds one)" docs/specs/2026-11-18-zygo-design.md` → hits in §8.5 Subagents and the change-log row.
- [ ] Package and lint checks, scoped to `zygo-launch`: `cargo test -p zygo-launch --locked --no-fail-fast 2>&1` → all pass (`stub_member` and `premise` use the changed `harness::claude`); `cargo clippy -p zygo-launch --locked --tests -- -D warnings 2>&1` → clean. The Windows rows (the `settings_value_text_is_printed_only_for_a_file` rows, `settings_file_parse_errors_name_only_category_and_position`, the `stub_launch` tests) are checked by the CI job `test` on windows-latest, and the unix rows on macOS by the job on macos-latest (including the `/dev/fd/<n>` pipe tests); a local `cargo check --target x86_64-pc-windows-gnu` stops at `ring`'s C build here (no `x86_64-w64-mingw32-gcc`, run 2026-11-23).
- [ ] Tick these steps in this file. Write the message below to `"$(git rev-parse --git-path FIX-26.msg)"`. Stage exactly `git add crates/zygo-launch/src/claude_code.rs crates/zygo-launch/tests/claude_code.rs crates/zygo-launch/tests/stub_launch.rs crates/zygo-launch/tests/stub_agent/harness.rs docs/specs/2026-11-18-zygo-design.md docs/work/phase2/nodes/FIX-26.md`, then commit with `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com git commit -F "$(git rev-parse --git-path FIX-26.msg)"`. Message (no other trailer, no attribution of any kind):
  ```
  fix(launch): FIX-26 safe settings lines, one --agents read, safe found files

  A --settings value that is not a JSON object was printed whole when zygo
  could not read it, so broken inline settings put their secrets on
  stderr. zygo now prints the value only when an entry exists at the path,
  with the reason; with no entry it prints a fixed line, or the system
  error with no value. A file that does not parse is named by the
  category and position of the error only. A value the user gives is read
  as before, so pipes, FIFOs and late writers work as they did. An --agents
  file is read once per launch, so <(...) agents are no longer dropped.
  The §8.4 settings files that zygo finds by itself are read by a safe
  rule: a target that is not a regular file is skipped and not opened, so
  a FIFO planted in a repository no longer stops the launch. Spec §8.4
  Preflight, §8.5 Subagents, §8.5 Flags and the change log are updated.

  Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
  Zygo-Work: FIX-26
  ```

## Spec text

One change in §8.4 Preflight of `docs/specs/2026-11-18-zygo-design.md` (`:362`), one in §8.5 Subagents (`:370`), two in §8.5 Flags (`:373`, as FIX-23 leaves it) and one change-log row. Each quoted text is written without the leading `> ` in the spec file.

In §8.4 Preflight, after the sentence that ends `(\`~/.claude/settings.json\`, \`.claude/settings.json\`, \`.claude/settings.local.json\`) and the managed settings file.`, insert

> `zygo` finds these files by itself, and a cloned repository can put anything at their paths, so each is read by a safe rule: links are followed; a target that is not a regular file (a FIFO, a socket, a device, a directory) is skipped with no line and is not opened; a regular file is opened, on unix with `O_NONBLOCK` and, when that open fails because it would block (a file lease), once more without it, with no loop; the open handle must again be a regular file, else the file is skipped; `O_NONBLOCK` is then cleared and the file is read whole, with no size limit and no time limit. A file that is absent, is skipped, cannot be read, is not JSON or holds JSON that is not an object is skipped with no line. The passthrough check of the managed file (§12.3) reads it the same way. Residual risks, with no retry loop and no time limit: a FIFO swapped in between the metadata read and the open is opened, but with `O_NONBLOCK`, and the check on the handle skips it, so the launch does not wait; after a lease makes the first open fail, the second open has no `O_NONBLOCK`, so a FIFO swapped in at that moment with no writer makes the launch wait until a writer opens it (this needs a lease holder and the right to rename in that folder; Linux `O_PATH` could close it, but it is not used because macOS has no `O_PATH`); on Windows the metadata read itself opens the target, with no access rights; a FUSE mount that does not answer makes the metadata read wait.

In §8.5 Subagents, after the sentence `A user \`--agents\` value is merged; a user key with the same name wins.`, insert

> Each user `--agents` value must be a JSON object; with `--print`, a value that does not parse as JSON is read as a file path, relative to the working directory, the same way as a `--settings` file (§8.5 Flags), so a pipe such as `<(…)` can hold it, and the file must hold a JSON object. `zygo` reads each such file once per launch and uses that text for both the check and the merge, because a pipe can be read only once. A value that is neither stops the launch with exit 2 and `zygo: --agents: not a JSON object (or, with --print, a file that holds one)`.

In §8.5 Flags, after the sentence that ends `any other value is a file path, relative to the working directory (observed on 2.1.330).`, insert

> A value is never judged by its first character. Known limit: the `serde_json` features `arbitrary_precision` and `raw_value` are on for the whole workspace, so in user settings, inline or in a file, a JSON object whose first key is `$serde_json::private::Number` or `$serde_json::private::RawValue` is read by `serde_json`'s rule for that key (as a number, or as the JSON text it holds), not as an object with a plain key; `zygo` does not change this.

In §8.5 Flags, replace the text from `A value that is not a readable JSON object (a missing or unreadable file` up to and including `and \`<value>\` is not always valid JSON.` (the unreadable-value sentence as FIX-23 leaves it) with

> A value that is not a readable JSON object (no file at the path, a file that cannot be read, a file that is not JSON, JSON that is not an object) is not used and not passed on: the agent gets only `zygo`'s file, with the readable values merged, and for each such value `zygo` prints one line. The value text is in the line only when an entry exists at the path (`symlink_metadata` finds one), because a value with no entry (with a BOM or other prefix, an array, a JSON string, broken JSON of any length) can be inline settings that hold secrets. With no entry, when no file is found (`NotFound`, or on Windows a name the system refuses: `ERROR_INVALID_NAME`, `ERROR_BAD_PATHNAME`, or `ERROR_FILENAME_EXCED_RANGE` for a name that is too long), the line is `zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used` on every platform; for any other error (for example a folder on the path that cannot be searched, a part of the path that is not a folder, a link loop in a folder part of the path, or on unix a name longer than the system limit) the line is `zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: <io error>; it was not used`, where `<io error>` is the system error text, which holds no path. With an entry (a link loop at the last part of the path and a dangling link are entries), `zygo` reads the path as it did before this rule: one blocking open that follows links, then the whole content, with no size limit and no time limit. So any type that the system can read is read, regular files, pipes and FIFOs (a `<(…)` path such as `/dev/fd/63`) and devices included; a writer that writes after the launch starts is read in full, and a FIFO with no writer makes the launch wait until a writer opens and closes it, because the value is the user's own input. When that read fails, the line is `zygo: your --settings value <value> could not be read as a JSON object and was not used: <error>`, where `<value>` is the value as typed, written as a compact JSON string in which every character that `zygo_core::esc` escapes is then written in `esc`'s form, and `<error>` is the system error text of the open or the read (for example for a directory or a file the user cannot read); `<category> error at line L column C` for a file that does not parse, where `<category>` is `syntax`, `data`, `eof` or `io` (`serde_json::Error::classify`); or `the file holds JSON that is not an object`. `<error>` is escaped the same way, so the line is always one line. `zygo` never prints the text of a `serde_json` error of user input: a data error can hold literals of the file, for example through the `$serde_json::private::Number` key.

Append to the change-log table:

> | 2026-11-23 | §8.4 Preflight (reading the settings files `zygo` finds), §8.5 Subagents (reading an `--agents` file), §8.5 Flags (the unreadable-value lines, `serde_json` limit) | controller decisions 2026-11-23 (re-plan of the BLOCKED FIX-23: the value-or-file rule with no first-character rule, value text only for an entry, the Windows invalid-name rule with its three errors, parse errors as category and position, the `serde_json` limit sentence; second plan review: a value the user gives keeps today's read semantics, the §8.4 files `zygo` finds skip anything that is not a regular file, the residual risks stated); author's call (FIX-26: the order of the checks, the line texts, one read of an `--agents` file per launch) | A `--settings` value that is not a JSON object was printed whole when `zygo` could not read it, so broken inline settings put secrets on stderr; the line gave no reason; the §8.4 settings files that `zygo` finds by itself were read with a blocking open, so a FIFO planted at one of their paths stopped the launch and a device was read; a `<(…)` pipe given as `--print --agents` was read twice, so its agents were dropped with no line. Changes: a value that parses as a JSON object is inline and any other value is a path, never judged by its first character; the value text is printed only when an entry exists at the path, with the reason; with no entry, `NotFound` (and on Windows `ERROR_INVALID_NAME`, `ERROR_BAD_PATHNAME`, `ERROR_FILENAME_EXCED_RANGE`) gives `zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used` and any other error gives `zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: <io error>; it was not used`; a file that does not parse gives `<syntax|data|eof|io> error at line L column C`, never `serde_json`'s text; known limit: a `$serde_json::private::Number` or `$serde_json::private::RawValue` first key is read by `serde_json`'s rule. A `--settings` or `--print --agents` value the user gives is read as before (one blocking open that follows links; pipes, FIFOs and devices read; a FIFO with no writer waits). The `--print` `--agents` file is read once per launch; its refusal line and exit 2 are unchanged. The §8.4 settings files that `zygo` finds by itself, and the §12.3 passthrough check of the managed file, skip with no line and do not open a target that is not a regular file; a regular file is opened with `O_NONBLOCK` on unix and once more without it when that open would block, checked again on the handle and read whole; they still skip a file they cannot use with no line; their residual risks are stated in §8.4. The workspaces-spec text AU59 is FIX-24's. |

## AU59 text for FIX-24

FIX-24 owns this change to AU59 of `docs/specs/2026-11-20-workspaces-design.md` (`:200`); FIX-26 does not edit that file. Replace

> and `zygo: your --settings value <value> could not be read as a JSON object and was not used` for each value it cannot read.

with

> and, for each value it cannot read, one of the main spec §8.5 Flags lines for such a value: `zygo: your --settings value <value> could not be read as a JSON object and was not used: <error>` when an entry exists at the path, `zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used` when no file is found, and `zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: <io error>; it was not used` for any other error.

## Self-review (§3.3)

1. Spec coverage. §8.5 Flags: inline only for a JSON object, no first-character rule (row `file named {x`; `{bad` in `unreadable_user_settings_keep_only_zygos_file`); value text only with an entry (every row of `settings_value_text_is_printed_only_for_a_file`, `settings_path_errors_show_the_value_only_for_an_entry`, `settings_under_unsearchable_folder_are_not_shown`, `long_broken_inline_settings_are_not_shown`); `NO_FILE` for `NotFound` on every platform and for `InvalidFilename` only on Windows (the all-platform rows on windows-latest; the 400-character row on unix); the open-error line for `ENOTDIR`, `ELOOP` in a folder part, `EACCES` on a parent, `ENAMETOOLONG`; a link loop at the last part, a dangling link and a directory printed with their error; today's read semantics for user values: links followed (`settings_link_to_a_file_is_followed`, `pipelink.json`), pipes read (`settings_from_a_pipe_are_used`), a late writer read in full (`settings_from_a_pipe_with_a_late_writer_are_used`); parse errors as category and position, each category in its own case, with no literal (`settings_file_parse_errors_name_only_category_and_position`); a file with mode 0 printed with its error; escaping of the printed value (row `U+202E path`, `settings_lines_escape_every_control_character`); the member launch and the `=` form; the order of the lines (`settings_lines_on_stderr_are_safe_and_in_order`). §8.5 Subagents (D10, D12): a pipe is read once and its agents reach `build`, written before or after the launch starts (`print_agents_from_a_pipe_are_used`, `print_agents_from_a_pipe_with_a_late_writer_are_used`); an object file is used and a `[1]` file refused (the existing `bad_user_agents_are_refused`, unchanged). §8.4 (D9): a found FIFO with no writer at the cwd path or the managed path makes neither the launch, the preflight nor the passthrough refusal wait (`preflight_skips_a_found_fifo_with_no_hang`); a found FIFO, a found link to a FIFO and a found device are not opened (`preflight_does_not_open_a_found_fifo_link_or_device`, inotify `IN_OPEN`); a found link to a file is followed (`preflight_follows_a_link_to_a_settings_file`); a found leased file is read after the second open (`preflight_reads_a_leased_settings_file_after_a_blocking_open`); a found file that cannot be used is still skipped with no line (the existing `preflight_passes_clean_and_unrelated_files`, unchanged). The limit sentence (D5) is spec text with no behavior change. Parts with no test that can fail, and why: the lease test cannot tell the D9 open from a plain blocking first open, and it cannot tell one second open from a retry loop: each of the three waits for the lease break and then reads the file (controller decision m-1: stated, no seam added). The check on the handle and the risks (a) and (b) of D11 need a file swapped between two system calls; risk (c) needs Windows internals; risk (d) needs a FUSE mount that does not answer; the clearing of `O_NONBLOCK` in D9 is not visible for a regular file on a local file system. These are stated in §8.4 as residual risks, with no retry loop and no time limit. Since no FIFO is ever read by the D9 path, the empty-FIFO read on macOS needs no evidence; the user-value pipe tests run on macOS in the CI job on macos-latest.
2. Step scan. Each step is one action with a checkable result. The first step is a refactor with no behavior change, so the red run of the new tests fails on behavior, not on compilation. D6 and D9 give the order of the checks, which the signatures do not decide; they are prose, not bodies. Every test assertion is given as code (controller decision m-6). The tests with a writer (`late_pipe`) check that the build has not returned after 1 s before they write, so the order "build waits, then writer writes" is checked, not assumed; the other pipe tests write and close before the build.
3. Interface match. Consumes: `esc`, `user_settings`, `unreadable_line`, `bad_hooks`, `MERGE_HOOKS_OFF` and `RAW` are in FIX-23's Produces with the signatures above; FIX-26 depends on FIX-23. Produces: `ClaudeCode::new`; its callers in this node are the five former literal sites; `crates/zygo` uses `ClaudeCode::default()`, which stays. `read_object`, `preflight` and `refusal` keep their signatures; `user_agents` becomes a private method with the same parameters and result.
4. Review focus. The graph's Review focus assigns no line to FIX-26.
5. Proportion. The code is two constants, one enum, `read_settings_file` (three steps), `parse_error`, a new `unreadable_line`, two changed lines in `user_settings` and `build`, the six steps of `read_object`, a memo lookup in `user_agents`, one field and one constructor; the plan gives signatures, texts, the check order and the tests, and no function body. The test code is given in full, as the controller asked (m-6).
