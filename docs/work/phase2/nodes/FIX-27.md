# FIX-27 — loadout show, config show and config check print no secret and name what they remove

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§12 (row `zygo loadout list` / `zygo loadout show <n>`, `:560`), W§11 (`zygo config check` bullet, `:545`), W§16 (`check.rs` row, `:678`), AU30, AU59, AU98, AU103; W§18 #63. Created 2026-11-23 by the split of M1 (controller decision 2026-11-23, after M1 was `BLOCKED` for 3 rounds; round 3 review: `config check` read content files against the process cwd, not the pane cwd; short-flag and later `--mcp-config` values printed secrets; the hiding hid model ids). Rules from the controller decisions of 2026-11-23 (the split, and the plan review F2-F20 of the same day), stated in full in D1-D7 below: one function per argument token serves `loadout show` and `config show`, so their outputs are equal; user info by a scheme anywhere in the token or by the `user:pass@host` form; JSON by its first non-blank character; values of secret-named flags; content values shown only when they name a regular file relative to the pane's folder, which `up`'s own `find_checkout` and `pane_cwd` rule give; `config check` labels from the same AU30 walk, every positional labelled; the W§11, W§12 and W§16 rows rewritten in place; one change-log row. Every test value of this node is written below as code; nothing depends on the attempt diff. Edges (controller, review F1): M1 → FIX-27 (consumes the `loadout` verb and the config helper; shares `crates/zygo/src/cmd/loadout.rs`), FIX-19 → FIX-27 (both add a change-log row to the workspaces spec), FIX-27 → FIX-24 (FIX-24 shares `crates/zygo-workspace/src/args.rs`, `crates/zygo-workspace/tests/args.rs`, `crates/zygo-workspace/src/up.rs` and the workspaces spec). The CLI tests are in a new file, `crates/zygo/tests/loadout_secrets.rs`, so this node does not share `crates/zygo/tests/cli_surface.rs` with M4.
**Files:** `crates/zygo/src/cmd/loadout.rs`, `crates/zygo/src/cmd/config.rs`, `crates/zygo/tests/loadout_secrets.rs`, `crates/zygo-workspace/src/args.rs`, `crates/zygo-workspace/tests/args.rs`, `crates/zygo-workspace/src/loadout.rs`, `crates/zygo-workspace/tests/loadout.rs`, `crates/zygo-workspace/src/up.rs`, `docs/specs/2026-11-20-workspaces-design.md`
**Consumes:** `cmd::loadout::LoadoutArgs` (M1), `cmd::loadout::LoadoutCmd` (M1), `cmd::loadout::run` (M1), `cmd::user_config_or_exit` (M1), `args::occurrences` (L2), `args::has_user_info` (W4), `args::removable` (W4), `loadout::resolve` (W5), `loadout::builtin_solo` (W5), `loadout::LoadoutError` (W5), `identity::Worktree` (W1)
**Produces:** `args::removed_labels(agent: Agent, args: &[String], base: &Path) -> Vec<String>`, `loadout::pane_cwd(cwd: Option<&str>, main_root: &Path, worktrees: &[Worktree]) -> (PathBuf, Option<String>)`, `up::find_checkout(cwd: &Path, bound: Duration) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit>` (made public), `up::GIT_BOUND: Duration` (made public), `cmd::loadout::PaneBase { main_root: PathBuf, worktrees: Vec<Worktree> }`, `cmd::loadout::PaneBase::from_folder(here: &Path) -> (PaneBase, Option<String>)`, `cmd::loadout::hide_secrets(l: &mut Loadout, base: &PaneBase)`
**Acceptance:** `cargo test -p zygo-workspace --test args --test loadout && cargo test -p zygo --test loadout_secrets && cargo test -p zygo --lib cmd::loadout && cargo test --workspace --locked --no-run` → all pass, including D1-D7 with each mutation test named in the plan, W§18 #63, and every carried round 1-3 test

## Plan

Written 2026-11-23 against `b4482df`; revised the same day after the plan review (F2-F20). The steps run after M1 and FIX-19 are merged. Work in the node checkout. Tests are scoped to the changed crates `zygo` and `zygo-workspace`. The base tests of step 9 run `git`; `git` is on every CI runner.

### Interfaces (as in the code now, or as M1 produces them)

Consumed:

- From M1 (contract): `cmd::loadout::LoadoutArgs { sub: LoadoutCmd }`, `cmd::loadout::LoadoutCmd { List }`, `cmd::loadout::run(a: &LoadoutArgs) -> i32` (OP5 check, then `user_config_or_exit(&Paths::from_env(), "loadout")`), `cmd::user_config_or_exit(paths: &Paths, verb: &str) -> Result<zygo_config::Config, i32>`, `cmd::print_out`, `cmd::print_err`
- `args::occurrences(agent: Agent, args: &[String]) -> Vec<Occurrence>` (`crates/zygo-workspace/src/args.rs:375`), `Occurrence { name: &'static str /* long name */, values: Vec<String>, tokens: Range<usize> }` (`:149`; a short-flag cluster gives one `Occurrence` per letter, all with the same `tokens`; an attached value gives `tokens = i..i+1` and `values = [value]`), `args::removable(agent: Agent, args: &[String]) -> Vec<String>` (`:353`), `args::filter_from(agent: Agent, args: &[String], base: &Path) -> Filtered` (`:346`), `args::has_user_info(url: &str) -> bool` (`:418`; true for `me@example.com`, so it is not the show rule); the AU30 content check is `path.is_file()` (`:171-177`)
- `up::find_checkout(cwd: &Path, bound: Duration) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit>` (`crates/zygo-workspace/src/up.rs:302-321`, `pub(crate)` today; the bare-repository case at `:308-316`; errors `NOT_A_REPO` = `zygo up needs a git repository`, `BARE` = `zygo up needs a worktree; this repository is bare`, `:52-53`), `up::GIT_BOUND: Duration = 5 s` (`:36`, `pub(crate)` today), `up::Exit { pub code: i32, pub message: String }` (`:80`)
- `loadout::resolve(cfg: &Config, name: Option<&str>, main_root: &Path, worktrees: &[Worktree]) -> Result<Plan, LoadoutError>` (`crates/zygo-workspace/src/loadout.rs:118`; the pane folder rule is `:147-167`), `loadout::builtin_solo() -> Loadout` (`:98`), `loadout::LoadoutError::Unknown { name }` (`:69`; `Display`: ``unknown loadout `<name escaped>` ``), `identity::Worktree { path: PathBuf, branch: Option<String>, bare: bool }` (`identity.rs:93`)
- `zygo_config::{check, Config, Loadout, LoadoutPane, LoadoutWindow, Severity, Violation, BUILTIN_LOADOUT}`, `zygo_config::display::scrub_userinfo(text: &str) -> String` (`display.rs:38`; only `<scheme>://` forms), `zygo_config::layers::seg(s: &str) -> String`, `Merged::show` (`layers.rs:752`; reads `self.config` for every `--layer`; each line through `zygo_core::esc`, which keeps `\`)
- In `zygo`: `cmd::compact::parse_window(s: &str) -> Option<Change>` (`compact.rs:33`), `cmd::bad_compact(agent: &str, v: &str) -> String` (`mod.rs:127`; `zygo <agent>: bad --compact value ...`), the launch rule for `compact` (`claude.rs:32-34`, `opencode.rs:32-34`: anything but `Some(Change::Set(_))` is refused, `on` included), `config::render_show` (`config.rs:39`), `config::run_in` (`:70`; `ctx.repo` is the process folder, `:64`)

Produced: as in the contract.

### Design calls

- D1. User info (review F3, F6). `fn user_info(token: &str) -> bool` is true when (a) or (b) holds. (a) The token holds, anywhere, a scheme followed by `:/` or `:\` (regex `[A-Za-z][A-Za-z0-9+.-]+:[/\\]`: a scheme of 2 or more characters, so the drive letter `C:\` is not one), and an `@` after that `:`; `?`, `#` and `/` do not end the search. (b) The token, its `=` value (the text after its first `=`) or its attached short value (for a token that starts with exactly one `-` and has 3 or more characters, the text after its first 2 characters) holds an `@`, and the text before its last `@` holds a `:` and holds no `/` and no `\`. Reading of (b): the review text says "no `/` or `\` before that `:`"; read literally, `C:\Users\first.last@corp\x.json` would match (`C` has no slash before the `:`), but the review requires it to stay visible, so the rule is "no `/` or `\` anywhere before the last `@`", which is the user-info part of `user:pass@host`. A token for which `user_info` is true is shown as `<user info removed>`, the whole token. Accepted over-hiding: `node:20@sha256:…` (form (b)). Limit: a scheme of one letter (`x:/u:p@h`) is read as a drive letter and is shown.
- D2. JSON (review F3). Before the flags end (D5): a token whose first non-blank character is `{` or `[` is `<inline JSON not shown>`; else a token with an `=` whose `=` value starts (first non-blank) with `{` or `[` is the text up to and with its first `=`, then `<inline JSON not shown>` (`--model=<inline JSON not shown>`); else an attached short value that starts with `{` or `[` gives the first 2 characters, then `<inline JSON not shown>` (`-d<inline JSON not shown>`). The JSON does not need to parse. After the flags end, a JSON token is shown (positionals after `--` are not hidden, decision 2).
- D3. Secret-named flags (review F3). Before the flags end: a token that starts with `-` and whose text before its first `=`, lower-cased, holds `key`, `token`, `secret`, `password` or `auth`: with an `=`, it is shown as that text, `=`, `<value not shown>` (`--api-key=<value not shown>`); without an `=`, the next token is `<value not shown>` when it exists, is before the flags end and does not start with `-`. Accepted over-hiding: names such as `--author` or `--max-tokens`. Limit: a bare `Authorization: Bearer <x>` positional is shown (no flag name, no `@`).
- D4. Content values (decision 2, review F14). Before the flags end, for each `Occurrence` whose `name` is `--agents`, `--mcp-config`, `--json-schema`, `--plugin-url` or `--settings`, each value (every token of `tokens.start + 1 .. tokens.end`, or the attached value) is shown only when `std::fs::metadata(base.join(value)).is_ok_and(|m| m.is_file())` (the AU30 rule, `args.rs:171-177`; an absolute value replaces `base`; a link is followed; a folder, a dangling link and a missing path are hidden). Otherwise a value token is `<inline JSON not shown>` and an attached value gives `<flag>=<inline JSON not shown>`. The walk table is the pane's agent (`claude-code`, `opencode`); for any other kind the Claude Code table (author's call, the safe side).
- D5. One function per token (review F9, F10). `pub(crate) fn shown_args(agent: Agent, args: &[String], base: &Path) -> Vec<String>` finds the flags end (the first `--` token that no occurrence's `tokens` holds; none: the end of `args`) and the D4 value positions once, then maps each token through `fn shown_token(token: &str, role: Role) -> String`, `enum Role { Content { attached: bool, shown: bool }, SecretValue, Flag, After }` (`Flag`: before the flags end and no other role; `After`: at or after the flags end). Order inside `shown_token`: D1 first (any role); then `Content` (D4); then `SecretValue` (D3, the value token of a separate secret-named flag); then for `Flag` the D3 `=` form, then D2; `After` gives the token as typed. `loadout show` and `config show` both reach `shown_args` only through `hide_secrets` (D7); there is no second path, so their outputs are equal.
- D6. The pane folder (decision 3, review F4, F5, F16). `loadout::pane_cwd(cwd, main_root, worktrees)` is the folder rule of `resolve` (`loadout.rs:147-167`) moved out unchanged: `None` gives `main_root`; `branch:<b>` gives the path of the worktree with that branch, else `main_root` and `Some(b)` (the branch that no worktree has checked out; `resolve` builds its warning from it); an absolute text gives itself; a relative text gives `main_root.join(text)`. `up::find_checkout` and `up::GIT_BOUND` become `pub`, unchanged, and `PaneBase::from_folder(here)` calls them; it copies neither, so the bare-repository case is the same as `up`'s. `Ok((_, list, main))` gives `(PaneBase { main_root: main, worktrees: list }, None)`; `Err(e)` gives `(PaneBase { main_root: here.to_path_buf(), worktrees: vec![] }, Some(e.message))`. A caller prints `Some(m)` as one stderr line, `zygo: the pane folder could not be found (<m escaped>); paths are checked against the current folder`, and goes on. `config show` and `config check` call `from_folder(ctx.repo.as_deref().unwrap_or(Path::new("")))` only when the merged config has at least one loadout (so a config with no loadout runs no git and prints no new line); `zygo loadout show` calls `from_folder(&current_dir)` after the loadout is found and prints the same line (author's call: it is the same lookup). `PaneBase::of(&self, pane: &LoadoutPane) -> PathBuf` is `pane_cwd(pane.cwd.as_deref(), &self.main_root, &self.worktrees).0`.
- D7. Show (review F15). `pub(crate) fn hide_secrets(l: &mut Loadout, base: &PaneBase)`: per pane, first `let dir = base.of(pane)` (from the pane's own `cwd`, before any scrub), then `args = shown_args(agent, &args, &dir)`, then a `server` for which `has_user_info` is true becomes `<user info removed>` (AU98, as `up` and `config check` read it), then `window.name`, `window.layout`, `kind`, `profile`, `server`, `compact`, `config_dir` and `cwd` go through `scrub_userinfo`. `zygo loadout show <n>` takes the user's `[loadout.<n>]` first, then `builtin_solo()` when `<n>` is `solo`, else exit 1 with `zygo: <LoadoutError::Unknown { name }>`; it prints `scrub_userinfo(toml::to_string({"loadout": {<n>: <loadout after hide_secrets>}}))`. `render_show` shows a clone of the merged config whose loadouts went through `hide_secrets`.
- D8. Labels (decision 3, review F17). `args::removed_labels(agent, args, base)` comes from the walk of `filter_from(agent, args, base)` (a new `labels: Vec<String>` in `Walk`), so the labels and the removed tokens come from one walk, by index. In token order: a long table flag token (`--x` or `--x=v`) whose flag token or any value the walk removes gives the typed name before `=` (a table name); each letter of a short-flag token whose flag or value the walk removes gives `-<letter>` (`-cr` gives `-c`, `-r`); every removed token that is neither a flag nor a flag value (a positional, also `-`) gives `<positional>`; the first `--` that is not a flag value gives `--`, then `<after -->` for each later token. A flag value is never labelled. Every label is a table name or one of the three fixed texts; the list is empty only when the walk removes nothing.
- D9. `config check` (W§11, AU30, AU98, AU59, review F13): `fn loadout_checks(cfg: &Config, base: &PaneBase) -> Vec<Violation>`, per pane in loadout order, for `claude-code` (agent word `claude`) and `opencode` (`opencode`) panes only: (1) `compact` that is not `Some(Change::Set(_))` from `parse_window` → error `<path>.compact: <bad_compact(word, v) without its leading "zygo <word>: ">` (so `on` is an error, as the launch treats it); (2) `server` for which `has_user_info` is true → error `<path>.server: holds user info; zygo up starts this pane as a shell and does not save it (AU98)`; (3) `removed_labels(agent, &p.args, &base.of(p))` not empty → warning `<path>.args: removed from the saved arguments (AU30): <labels joined by one space>`. `<path>` is `loadout.<seg(name)>.window[<w>].pane[<p>]`. `Sub::Check` extends `check(&merged.config)` with these.

### Test values

These constants are written into the test files as they are here. `G`, `H` and `V` are the three replacement texts.

```rust
const G: &str = "<user info removed>";
const H: &str = "<inline JSON not shown>";
const V: &str = "<value not shown>";

/// (argument, shown) for one claude-code pane, in this order. Each hidden argument holds `SECRET`; no shown text does.
const FORMS: &[(&str, &str)] = &[
    ("https://u:SECRET1@h.example.invalid", G),
    ("DATABASE_URL=postgres://u:SECRET2@h.example.invalid/db", G),
    ("--env=URL=https://u:SECRET3@h.example.invalid", G),
    ("-d=https://u:SECRET4@h.example.invalid", G),
    ("-Xhttps://u:SECRET5@h.example.invalid", G),
    ("https://u:SECRET6#x@h.example.invalid", G),
    ("https://h.example.invalid/p?u:SECRET7@x", G),
    ("https:/u:SECRET8@h.example.invalid", G),
    (r"https:\\u:SECRET9@h.example.invalid", G),
    ("u:SECRET10@h.example.invalid", G),
    ("a@b:SECRET11@h.example.invalid", G),
    ("u:SECRET12?x@h.example.invalid", G),
    ("u:SECRET13#x@h.example.invalid", G),
    ("u:pa SECRET14@h.example.invalid", G),
    ("https://u:pa'SECRET15@h.example.invalid", G),
    ("--x=u:SECRET16@h.example.invalid", G),
    ("-du:SECRET17@h.example.invalid", G),
    ("node:20@sha256:0123abcd", G),
    ("{\"k\":\"SECRET19\"", H),
    (" [ \"SECRET20\" ]", H),
    ("--model={\"k\":\"SECRET21\"}", "--model=<inline JSON not shown>"),
    ("-d{\"k\":\"SECRET22\"}", "-d<inline JSON not shown>"),
    ("--api-key=SECRET23", "--api-key=<value not shown>"),
    ("--auth-token", "--auth-token"),
    ("SECRET24", V),
    ("--Password=SECRET25", "--Password=<value not shown>"),
    ("--model=x", "--model=x"),
    ("me@example.com", "me@example.com"),
    ("claude-sonnet-4@20250514", "claude-sonnet-4@20250514"),
    ("--model=claude-sonnet-4@20250514", "--model=claude-sonnet-4@20250514"),
    (r"C:\Users\first.last@corp\x.json", r"C:\Users\first.last@corp\x.json"),
    ("me@h.example.invalid:8080", "me@h.example.invalid:8080"),
    ("Authorization: Bearer tok-visible", "Authorization: Bearer tok-visible"),
];

/// Attempt 1 round 3 F2: AU30 content flags and `--settings` with no file behind them, and JSON after any `=`.
const CONTENT_16: [&str; 16] = [
    r#"--agents={"r":{"prompt":"SECRET1"}}"#,
    r#"--json-schema={"type":"object","description":"SECRET2"}"#,
    "--agents", "agents-SECRET3.json",
    "--json-schema", "schema-SECRET4.json",
    "--plugin-url", "plugin-SECRET5",
    "--plugin-url=plugin-SECRET6",
    "--mcp-config", "mcp-SECRET7.json",
    "--settings=settings-SECRET8.json",
    r#"--model={"k":"SECRET9"}"#,
    r#"--x=["SECRET10"]"#,
    "--model=x",
    "--name=a=b",
];
const CONTENT_16_SHOWN: [&str; 16] = [
    "--agents=<inline JSON not shown>", "--json-schema=<inline JSON not shown>",
    "--agents", H, "--json-schema", H, "--plugin-url", H,
    "--plugin-url=<inline JSON not shown>", "--mcp-config", H,
    "--settings=<inline JSON not shown>", "--model=<inline JSON not shown>", "--x=<inline JSON not shown>",
    "--model=x", "--name=a=b",
];

/// Attempt 1 round 3 F1, corrected for D1 (a whole token is replaced).
const USER_INFO_12: [&str; 12] = [
    "--x=https:/u:SECRET1@h.example.invalid",
    r"https:\\u:SECRET2@h.example.invalid",
    r"https:\u:SECRET3@h.example.invalid",
    "u:SECRET4@h.example.invalid",
    "https://u:pa'SECRET5@h.example.invalid",
    "https://u:pa\"SECRET6@h.example.invalid",
    "https://u:pa`SECRET7@h.example.invalid",
    "https://u:pa SECRET8@h.example.invalid",
    "https://u:SECRET9@h.example.invalid/x",
    "--model=x",
    "https://h.example.invalid/x",
    "plain",
];
const USER_INFO_12_SHOWN: [&str; 12] = [G, G, G, G, G, G, G, G, G, "--model=x", "https://h.example.invalid/x", "plain"];
```

```toml
# PAIR_CFG (attempt 1 `workspace_cli.rs`), a Rust raw string r#"..."# in the test file
[loadout.pair]
[[loadout.pair.window]]
name = "agents"
[[loadout.pair.window.pane]]
kind = "claude-code"
server = 'https://u:sekrit1@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "claude-code"
server = 'https:/u:sekrit2@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "claude-code"
server = 'https:\\u:sekrit3@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "opencode"
server = 'u:sekrit4@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "opencode"
server = 'https://zygo.example.invalid'
[loadout.solo]
[[loadout.solo.window]]
[[loadout.solo.window.pane]]
kind = "shell"
```

```toml
# LOADOUT_CFG (attempt 1 `cli_surface.rs`)
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
compact = "5k"
server = "https://u:sekrit@zygo.example.invalid"
args = ["--settings", "x", "-p", "hi"]
[[loadout.pair.window.pane]]
kind = "opencode"
compact = "on"
```

```toml
# SHOW_CFG (attempt 1 round 1 M3)
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
server = "https://u:SECRETA@h.example.invalid"
args = ["https://u:SECRETB@h.example.invalid/x", "--settings", '{"env":{"ANTHROPIC_API_KEY":"sk-SECRET1"}}', "--mcp-config={\"headers\":{\"Authorization\":\"Bearer SECRET2\"}}", '["SECRET3"]', "--model=x"]
```

```toml
# DASH_CFG (attempt 1 round 1 B1)
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
args = ["--system-prompt", "--api-token-is-SECRETVALUE=x", "--append-system-prompt", "-SECRET2"]
[[loadout.pair.window.pane]]
kind = "opencode"
args = ["--prompt", "--SECRET3=y", "-s", "-SECRET4"]
```

`one_pane(args: &[&str]) -> String` in the test file builds, with `toml::Table` and `toml::to_string`, the config `[loadout.pair]` with one window and one `claude-code` pane whose `args` are `args` (and, through `one_pane_at(args, cwd: Option<&str>)`, a `cwd`). `W` is the text `warning: loadout.pair.window[0].pane[0].args: removed from the saved arguments (AU30): `. `NO_FOLDER` is the text `zygo: the pane folder could not be found (zygo up needs a git repository); paths are checked against the current folder\n`.

### Steps

- [x] 1. Check the base: `docs/work/phase2/graph.md` shows M1 and FIX-19 `DONE`; `rg -n 'pub enum LoadoutCmd' crates/zygo/src/cmd/loadout.rs` and `rg -n 'fn user_config_or_exit' crates/zygo/src/cmd/mod.rs` each print one line.
- [x] 2. `crates/zygo-workspace/src/up.rs`: `pub(crate) const GIT_BOUND` (`:36`) becomes `pub const GIT_BOUND`, and `pub(crate) fn find_checkout` (`:302`) becomes `pub fn find_checkout`; bodies unchanged; each doc comment gains `Also used by zygo config check and show, and zygo loadout show (FIX-27).` Run `cargo build -p zygo-workspace --locked` → exit 0. (No test of its own: the step 8 base tests run it.)
- [x] 3. `crates/zygo-workspace/tests/loadout.rs`: add `pane_cwd_is_the_folder_rule_of_resolve` (kills: a `pane_cwd` that differs from `resolve`): with `root()` and `trees()` of the file: `pane_cwd(None, &root(), &trees()) == (root(), None)`; for each entry of `trees()` that has a branch `b`, `pane_cwd(Some(&format!("branch:{b}")), ..) == (<its path>, None)`; `pane_cwd(Some("branch:nope"), ..) == (root(), Some("nope".to_string()))`; `pane_cwd(Some(<std::env::temp_dir() as text>), ..) == (std::env::temp_dir(), None)`; `pane_cwd(Some("sub/dir"), ..) == (root().join("sub/dir"), None)`. Run `cargo test -p zygo-workspace --test loadout` → fails to compile: no `pane_cwd`. Then `crates/zygo-workspace/src/loadout.rs`: `pub fn pane_cwd(cwd: Option<&str>, main_root: &Path, worktrees: &[Worktree]) -> (PathBuf, Option<String>)` (D6, doc comment), and `resolve` calls it and builds its warning from the second item, with the same text and the same de-duplication. Run `cargo test -p zygo-workspace --test loadout` → all pass (`cwd_is_absolute_relative_or_a_branch_worktree` and `branch_without_a_worktree_gives_the_main_worktree_and_one_warning` among them).
- [x] 4. `crates/zygo-workspace/tests/args.rs`: add `removed_labels` to the `use` line and these tests (`l(agent, a) = removed_labels(agent, &s(a), Path::new(""))`; `s` is the file's helper):

```rust
// removed_labels_name_flags_never_values (attempt 1 round 1, F17: every positional is labelled)
assert_eq!(l(Agent::ClaudeCode, &["--system-prompt", "S1", "--append-system-prompt=S2", "-p", "S3", "pos", "--", "S4"]),
    s(&["--system-prompt", "--append-system-prompt", "-p", "<positional>", "<positional>", "--", "<after -->"]));
assert_eq!(l(Agent::ClaudeCode, &["-pX", "-p", "-pX"]), s(&["-p", "-p", "-p"]));
// removed_labels_never_label_a_value_that_starts_with_a_dash (round 1 B1; kills: labelling by token text)
assert_eq!(l(Agent::ClaudeCode, &["--system-prompt", "--api-token-is-SECRETVALUE=x", "--append-system-prompt", "-SECRET2", "-p", "pos"]),
    s(&["--system-prompt", "--append-system-prompt", "-p", "<positional>"]));
assert_eq!(l(Agent::OpenCode, &["--prompt", "--SECRET3=y", "-s", "-SECRET4", "--prompt", "-"]), s(&["--prompt", "-s", "--prompt"]));
// removed_labels_go_by_index_never_by_text (round 3 F5; kills: finding a removed token by its text; `--` as a value)
assert_eq!(l(Agent::ClaudeCode, &["--model", "-p", "-p"]), s(&["-p"]));
assert_eq!(l(Agent::ClaudeCode, &["--system-prompt", "--", "-p", "SECRET"]), s(&["--system-prompt", "-p", "<positional>"]));
assert_eq!(l(Agent::ClaudeCode, &["--model", "--", "pos"]), s(&["<positional>"]));
assert_eq!(l(Agent::ClaudeCode, &["--model", "-p", "--", "x"]), s(&["--", "<after -->"]));
assert_eq!(l(Agent::OpenCode, &["-m", "--prompt", "--prompt", "-m"]), s(&["--prompt"]));
// removed_labels_name_each_flag_of_a_short_group (kills: one label per token)
assert_eq!(l(Agent::ClaudeCode, &["-cr"]), s(&["-c", "-r"]));
assert_eq!(l(Agent::ClaudeCode, &["-cr", "abc"]), s(&["-c", "-r"]));
// removed_labels_label_every_positional (F17; kills: the old "no label right after a labelled flag" exception)
assert_eq!(l(Agent::ClaudeCode, &["-p", "hi"]), s(&["-p", "<positional>"]));
```

and two tests with a temp `base`: `removed_labels_name_a_flag_that_loses_only_a_value` (kills: labelling only removed flag tokens, which gives an empty line): `base` holds the file `a.json`; `removed_labels(Agent::ClaudeCode, &s(&["--mcp-config", "a.json", "{\"k\":\"SECRET\"}"]), base) == s(&["--mcp-config"])`; `removed_labels_take_content_files_from_the_base` (kills: the process folder in place of `base`): `base` holds `m.json`; `removed_labels(Agent::ClaudeCode, &s(&["--mcp-config", "m.json"]), base)` is empty, and with an empty temp folder as `base` it is `s(&["--mcp-config"])`. Add `removed_labels_are_empty_only_when_nothing_is_removed` (kills: a removal with no label): for each `(agent, args)` of the `l(..)` calls above and `(ClaudeCode, ["--model", "m"])`, `(ClaudeCode, ["--settings", "x"])`, `(ClaudeCode, ["--add-dir", "a", "b"])`: `l(agent, args).is_empty() == removable(agent, &s(args)).is_empty()`, and each label is a token of `args` that starts with `-` and holds no `=`, or `-<letter>` of such a token, or `--`, `<positional>`, `<after -->`. Run `cargo test -p zygo-workspace --test args` → fails to compile: no `removed_labels`.
- [x] 5. `crates/zygo-workspace/src/args.rs`: `labels: Vec<String>` in `Walk`, filled by the D8 rules inside `walk`; `pub fn removed_labels(agent: Agent, args: &[String], base: &Path) -> Vec<String>` returns `walk(agent, args, Some(base)).labels`; the module doc line 2 names it. Run `cargo test -p zygo-workspace --test args` → all pass (every older test among them).
- [x] 6. `crates/zygo/src/cmd/loadout.rs`: add the test module `tests` with `G`, `H`, `V`, `FORMS`, `CONTENT_16`, `CONTENT_16_SHOWN`, `USER_INFO_12`, `USER_INFO_12_SHOWN` of "Test values", the helper `fn args_in_text(text: &str, p: usize) -> Vec<String>` (the `args` of pane `p` of window 0 of the loadout `pair` in a `show` text, parsed as `toml::Table`; attempt 1 named it `shown_args`, which is now the production function), `fn base(dir: &Path) -> PaneBase` (`PaneBase { main_root: dir.to_path_buf(), worktrees: vec![] }`), `fn one(args: &[&str]) -> Loadout` (one window, one `claude-code` pane), and these tests:
  - `user_info_reads_a_scheme_anywhere_and_the_user_pass_form` (D1; kills: `has_user_info` as the rule; a search that stops at `?`, `#` or `/`; a 1-character scheme; the first `@` in place of the last; the literal "before that `:`" reading): `user_info` is true for every `FORMS` input whose shown text is `G`, and for `https://u:p@h`, `ab://u@h`, `-du:p@h`; false for `me@example.com`, `claude-sonnet-4@20250514`, `--model=claude-sonnet-4@20250514`, `r"C:\Users\first.last@corp\x.json"`, `me@h.example.invalid:8080`, `host:8080`, `https://h.example.invalid/x`, `Authorization: Bearer tok`, `--model=x`, `plain`, `x:/u:p@h` (stated limit).
  - `shown_args_hide_every_form` (D1-D3, D5; kills: a flag name dropped from a `=` JSON or secret value; JSON that must parse; a secret value in the separate form shown): with an empty temp folder `t`, `shown_args(Agent::ClaudeCode, &<FORMS inputs>, t) == <FORMS shown texts>`.
  - `shown_args_show_a_content_value_only_when_it_names_a_file` (`cfg(unix)`, D4; kills: only the first `--mcp-config` value hidden; the process folder in place of `base`; `symlink_metadata` or `exists` in place of `is_file`; the next token after a content flag with no value hidden; tokens after `--` hidden): `base` holds the file `a.json`, the folder `dir` and the symlink `dangling` → `missing`; `abs` = `<base>/a.json` as text; args `["--mcp-config", "a.json", "{\"k\":\"SECRET1\"}", "--agents", "dir", "--json-schema", "dangling", "--agents", abs, "--settings=a.json", "--settings=SECRET3.json", "--plugin-url=SECRET5", "--mcp-config", "--model", "m", "--name=a=b", "--", "--settings", "after", "{\"k\":\"after-json\"}"]` give `["--mcp-config", "a.json", H, "--agents", H, "--json-schema", H, "--agents", abs, "--settings=a.json", "--settings=<inline JSON not shown>", "--plugin-url=<inline JSON not shown>", "--mcp-config", "--model", "m", "--name=a=b", "--", "--settings", "after", "{\"k\":\"after-json\"}"]`.
  - `show_replaces_a_server_with_user_info_and_hides_args` (attempt 1 round 1 and round 3 F1): a `pair` loadout with one window and two `claude-code` panes: `server = "https://u:k1@h.example.invalid"`, `args = ["https://u:k2@h.example.invalid/x"]`; and `server = "https://h.example.invalid"`; the text of `show("pair", l, &base(t))` holds neither `k1` nor `k2`, holds `server = "<user info removed>"` and `server = "https://h.example.invalid"`, and `args_in_text(&text, 0) == [G]`.
  - `show_replaces_an_argument_with_user_info` (round 3 F1, corrected for D1): `args_in_text(&show("pair", one(&USER_INFO_12), &base(t)), 0) == USER_INFO_12_SHOWN`; the text holds no `SECRET`.
  - `show_hides_every_content_flag_value_and_inline_json_after_any_flag` (round 3 F2): `args_in_text(&show("pair", one(&CONTENT_16), &base(t)), 0) == CONTENT_16_SHOWN` with `t` an empty temp folder; the text holds no `SECRET`.
  - `show_hides_inline_json` (round 1 B2): two panes; pane 0 args `["--settings", r#"{"env":{"ANTHROPIC_API_KEY":"sk-SECRET1"}}"#, "--mcp-config", r#"{"headers":{"Authorization":"Bearer SECRET2"}}"#, "--model=x"]` show as `["--settings", H, "--mcp-config", H, "--model=x"]`; pane 1 args `[r#"--settings={"k":"SECRET3"}"#, r#"--mcp-config={"k":"SECRET4"}"#, r#"["SECRET5"]"#, "--settings", "settings.json", "plain"]` show as `["--settings=<inline JSON not shown>", "--mcp-config=<inline JSON not shown>", H, "--settings", H, "plain"]`; the text holds no `SECRET`.
  - `hide_secrets_scrubs_every_other_text_field` (F15; kills: a field left out of the scrub): one window with `name = Some("https://u:SECRET1@h.example.invalid/x")` and `layout = Some("https://u:SECRET2@h.example.invalid/x")`, one `claude-code` pane with `profile`, `compact`, `config_dir` and `cwd` each `Some("https://u:SECRET<n>@h.example.invalid/x")` (n = 3..6); after `hide_secrets(&mut l, &base(t))` each of these six fields is `Some("https://h.example.invalid/x")`.
  Run `cargo test -p zygo --lib cmd::loadout` → fails to compile.
- [x] 7. `crates/zygo/src/cmd/loadout.rs`: `const USER_INFO_REMOVED: &str = "<user info removed>"`, `const INLINE_JSON_HIDDEN: &str = "<inline JSON not shown>"`, `const VALUE_HIDDEN: &str = "<value not shown>"`, `const CONTENT_FLAGS: [&str; 5] = ["--agents", "--mcp-config", "--json-schema", "--plugin-url", "--settings"]`, `const SECRET_NAMES: [&str; 5] = ["key", "token", "secret", "password", "auth"]`, `fn user_info(token: &str) -> bool` (D1; the regex of (a) as a `std::sync::LazyLock<regex::Regex>`), `enum Role`, `fn shown_token(token: &str, role: Role) -> String`, `pub(crate) fn shown_args(agent: Agent, args: &[String], base: &Path) -> Vec<String>` (D2-D5), `pub(crate) struct PaneBase { pub(crate) main_root: PathBuf, pub(crate) worktrees: Vec<Worktree> }` with `pub(crate) fn from_folder(here: &Path) -> (PaneBase, Option<String>)` and `pub(crate) fn of(&self, pane: &LoadoutPane) -> PathBuf` (D6), `pub(crate) fn hide_secrets(l: &mut Loadout, base: &PaneBase)` and `fn show(name: &str, l: Loadout, base: &PaneBase) -> Result<String, toml::ser::Error>` (D7). Run `cargo test -p zygo --lib cmd::loadout` → all pass.
- [x] 8. Create `crates/zygo/tests/loadout_secrets.rs` with the constants of "Test values" (`G`, `H`, `V`, `FORMS`, `CONTENT_16`, `CONTENT_16_SHOWN`, `USER_INFO_12`, `USER_INFO_12_SHOWN`, `PAIR_CFG`, `LOADOUT_CFG`, `SHOW_CFG`, `DASH_CFG`, `W`, `NO_FOLDER`), `one_pane`, `one_pane_at`, and the helpers `fn zygo_at(home: &Path, cwd: &Path, config: &str, args: &[&str]) -> std::process::Output` (`<home>/config.toml` = `config`; `HOME`, `LOCALAPPDATA`, `APPDATA` = `home`; `ZYGO_CONFIG` = that file; `ZYGO_SESSION_DIR`, `ZYGO_AGENT`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` removed; current folder `cwd`; stdin null; the form of `with_config`, `crates/zygo/tests/cli_surface.rs:601`), `fn git(dir: &Path, args: &[&str])` (runs `git -c user.name=zygo-test -c user.email=zygo-test@example.invalid -c init.defaultBranch=main <args>` in `dir` and asserts success), `fn repo(home: &Path) -> PathBuf` (`git init -q <home>/r`, then `git -C <home>/r commit -q --allow-empty -m init`; returns `<home>/r`), `fn args_of_show(stdout: &str) -> Vec<String>` (pane 0 `args` of `loadout show pair`) and `fn args_line(stdout: &str) -> String` (the one `config show` line that holds `args = `). `zygo loadout` is a workspace verb (OP5: exit 2 on Windows), so its tests are `cfg(unix)`; the `config` tests are not gated. Tests:
  - `cfg(unix)` `loadout_show_hides_server_user_info` (attempt 1 `loadout_list_and_show`, show part; kills: a server shown with user info): with `PAIR_CFG`, `loadout show pair` → exit 0; stdout holds none of `sekrit1`, `sekrit2`, `sekrit3`, `sekrit4` and no `u:`; parsed as `toml::Table`, `server` of panes 0-3 is `<user info removed>` and of pane 4 `https://zygo.example.invalid`; `loadout show solo` → exit 0 and `["loadout"]["solo"]["window"][0]["pane"][0]["kind"] == "shell"`; with config `""`, `loadout show solo` → `shown["loadout"]["solo"].clone().try_into::<zygo_config::Loadout>().unwrap() == zygo_workspace::loadout::builtin_solo()`; `loadout show nope` → `(Some(1), "", "zygo: unknown loadout `nope`\n")`.
  - `cfg(unix)` `loadout_show_takes_a_relative_content_file_from_the_pane_cwd` (kills: the process folder as the base): `<home>/proj/m.json` exists; `zygo_at(home, home, &one_pane_at(&["--mcp-config", "m.json", "--agents", "gone.json"], Some(<home>/proj as text)), &["loadout", "show", "pair"])` → exit 0, `args_of_show == ["--mcp-config", "m.json", "--agents", H]`, stderr `NO_FOLDER` (home is not a repository).
  - `cfg(unix)` `loadout_show_and_config_show_print_the_same_args` (D5, F9, F10; kills: two code paths): config `one_pane(<FORMS inputs>)`; `loadout show pair` → exit 0 and `args_of_show == <FORMS shown texts>`; `config show` → exit 0 and `args_line(stdout)` holds `toml::Value::Array(<FORMS shown texts as toml strings>).to_string()`; neither stdout holds `SECRET`.
  - `config_show_hides_every_form` (not gated; F3 on `config show` alone): config `one_pane(<FORMS inputs>)`; for `["config", "show"]` and `["config", "show", "--layer", "user"]`: exit 0; `args_line` holds the array text of the shown texts; no `SECRET` on stdout or stderr; stdout holds `"--model=x"`, `"me@example.com"`, `"claude-sonnet-4@20250514"` and `'C:\Users\first.last@corp\x.json'`.
  - `config_show_hides_loadout_user_info_and_inline_json` (round 1 M3): `SHOW_CFG`; for both `show` forms: exit 0; no `SECRET` on stdout or stderr; stdout holds `server = "<user info removed>"`, `["<user info removed>", "--settings", "<inline JSON not shown>"`, `"--mcp-config=<inline JSON not shown>", "<inline JSON not shown>"` and `"--model=x"`.
  - `config_show_hides_args_with_user_info_and_content_flag_values` (round 3 F1 F2): config `one_pane(&[USER_INFO_12[..7], CONTENT_16[..13], ["--model=x"]].concat())`; for both `show` forms: exit 0; no `SECRET`; `args_line` holds the array text of `[G, G, G, G, G, G, G, "--agents=<inline JSON not shown>", "--json-schema=<inline JSON not shown>", "--agents", H, "--json-schema", H, "--plugin-url", H, "--plugin-url=<inline JSON not shown>", "--mcp-config", H, "--settings=<inline JSON not shown>", "--model=<inline JSON not shown>", "--model=x"]` (21 entries: 7 of `USER_INFO_12`, 13 of `CONTENT_16`, then `--model=x`).
  - `config_check_reports_loadout_compact_server_and_removable_args` (W§11, AU98, AU30; F13: `on` is an error): `LOADOUT_CFG` → exit `Some(11)`, stdout exactly `"error: loadout.pair.window[0].pane[0].compact: bad --compact value `5k`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\nerror: loadout.pair.window[0].pane[0].server: holds user info; zygo up starts this pane as a shell and does not save it (AU98)\nwarning: loadout.pair.window[0].pane[0].args: removed from the saved arguments (AU30): -p <positional>\nerror: loadout.pair.window[0].pane[1].compact: bad --compact value `on`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\n"`; no `sekrit`.
  - `config_check_never_labels_a_value_that_starts_with_a_dash` (round 1 B1): `DASH_CFG` → exit 0, stdout `"warning: loadout.pair.window[0].pane[0].args: removed from the saved arguments (AU30): --system-prompt --append-system-prompt\nwarning: loadout.pair.window[0].pane[1].args: removed from the saved arguments (AU30): --prompt -s\n"`; none of `SECRET`, `--api-token`, `-S` on stdout or stderr.
  - `config_check_never_prints_a_removed_argument_value`: `one_pane(&["--system-prompt", "SECRET1", "--append-system-prompt=SECRET2", "-p", "SECRET3", "--", "SECRET4"])` → exit 0, stdout `W` + `"--system-prompt --append-system-prompt -p <positional> -- <after -->\n"`; none of `SECRET1`..`SECRET4`.
  - `config_check_labels_a_removed_token_by_its_index` (round 3 F5, decision 3; kills: a label by text, one label per short group, an empty line): for `(args, labels)` in `(["--model", "-p", "-p"], "-p")`, `(["--system-prompt", "--", "-p", "SECRET"], "--system-prompt -p <positional>")`, `(["--model", "--", "-p"], "-p")`, `(["-cr"], "-c -r")`: `(code, stdout) == (Some(0), W + labels + "\n")`.
  - `config_check_takes_a_relative_content_file_from_the_pane_cwd` (kills: the process folder as the walk base; a flag that loses only a value left unnamed): `<home>/proj/m.json` exists; config `[loadout.pair]` with three `claude-code` panes: `cwd = '<home>/proj'`, `args = ["--mcp-config", "m.json"]`; `cwd = '<home>/proj'`, `args = ["--mcp-config", "m.json", '{"k":"SECRET"}']`; no `cwd`, `args = ["--mcp-config", "m.json"]`; `zygo_at(home, home, .., ["config", "check"])` → exit 0, stdout `"warning: loadout.pair.window[0].pane[1].args: removed from the saved arguments (AU30): --mcp-config\nwarning: loadout.pair.window[0].pane[2].args: removed from the saved arguments (AU30): --mcp-config\n"`, stderr `NO_FOLDER`, no `SECRET`.
  - `w18_63_config_check_does_not_report_settings` (W§18 #63, AU59): `one_pane(&["--settings", "x"])` → `(Some(0), "config ok\n")`.
- [x] 9. In the same file add the base tests (D6, review F4, F5; each with the pane config `one_pane_at(&["--mcp-config", "m.json"], <cwd>)` and the check `zygo_at(home, <from>, .., ["config", "check"])` → `(Some(0), "config ok\n", "")`):
  - `config_check_takes_the_main_worktree_from_a_repository_subfolder` (kills: the current folder as the base): `r = repo(home)`; `<r>/m.json`; `<r>/sub` created; no `cwd`; from `<r>/sub`. Also `config show` from `<r>/sub` → `args_line` holds `["--mcp-config", "m.json"]`.
  - `config_check_takes_the_main_worktree_from_a_linked_worktree` (kills: the checkout top in place of the main worktree): `r = repo(home)`; `git(r, ["worktree", "add", "-q", "-b", "lw", <home>/lw])`; `<r>/m.json` (untracked, so not in `lw`); no `cwd`; from `<home>/lw`.
  - `config_check_takes_the_main_worktree_from_a_bare_repository` (kills: a copy of `find_checkout` without its bare case): `r = repo(home)`; `git(home, ["clone", "-q", "--bare", <r>, <home>/b.git])`; `git(<home>/b.git, ["worktree", "add", "-q", <home>/bw])`; `<home>/bw/m.json`; no `cwd`; from `<home>/b.git`.
  - `config_check_takes_a_branch_cwd_from_its_worktree` (kills: `branch:` not resolved): `r = repo(home)`; `git(r, ["worktree", "add", "-q", "-b", "x", <home>/lx])`; `<home>/lx/m.json`; `cwd = "branch:x"`; from `r`.
  - `config_check_and_show_warn_when_the_pane_folder_cannot_be_found` (kills: a silent fallback; a lookup for a config with no loadout): `<home>/m.json`; no `cwd`; from `home` (no repository): `config check` → `(Some(0), "config ok\n", NO_FOLDER)`; `config show` → exit 0 and stderr `NO_FOLDER`; with config `""`, `config check` → `(Some(0), "config ok\n", "")`.
- [x] 10. Run `cargo test -p zygo --test loadout_secrets` → the `loadout show` tests fail (`zygo loadout show`: clap error, exit 2), the `config show` tests fail on shown secrets, and the `config check` tests other than `w18_63_config_check_does_not_report_settings` and the `config ""` case fail.
- [x] 11. `crates/zygo/src/cmd/loadout.rs`: `LoadoutCmd` gains `/// One loadout as TOML, with no secret` `Show { name: String }`; the `run` arm is D7, with D6 for the base (a `current_dir` error prints `zygo: cannot read the current folder: <error escaped>` and exits 1; a `toml` error prints `zygo: <error escaped>` and exits 1). Run `cargo test -p zygo --test loadout_secrets loadout_show` → 3 passed.
- [x] 12. `crates/zygo/src/cmd/config.rs`: `render_show` gains `base: &PaneBase` and shows a clone whose loadouts went through `hide_secrets` (D7); `fn loadout_checks(cfg: &Config, base: &PaneBase) -> Vec<Violation>` (D9); in `run_in`, after the merge and before `match args.sub`, when `merged.config.loadout` is not empty: `let (base, warning) = super::loadout::PaneBase::from_folder(ctx.repo.as_deref().unwrap_or(Path::new("")));` and `warning` printed by D6; with no loadout, `PaneBase { main_root: PathBuf::new(), worktrees: vec![] }`. `Sub::Show` passes `&base`; `Sub::Check` uses `let mut violations = check(&merged.config); violations.extend(loadout_checks(&merged.config, &base));`. The existing unit tests of `config.rs` that call `render_show` pass `&PaneBase { main_root: <their temp folder>, worktrees: vec![] }`. Run `cargo test -p zygo --test loadout_secrets` → all pass; `cargo test -p zygo --lib` → all pass.
- [x] 13. `docs/specs/2026-11-20-workspaces-design.md` (review F13). Replace the W§11 bullet (`:545`) in place with:

```text
- `zygo config check` validates loadouts in two places. `crates/zygo-config/src/check.rs`: a known `kind`; `passthrough` and `config_dir` only with `claude-code`; at least one pane per window; `default_loadout` names a loadout; a missing `profile` is a warning (the launch continues without it, as today). `crates/zygo/src/cmd/config.rs`, for `claude-code` and `opencode` panes (the checks that need the compact parser, AU30 or AU98, which `zygo-config` does not depend on, AU103): a `compact` that the launch refuses is an error, `on` included, as `zygo claude` and `zygo opencode` refuse it (`crates/zygo/src/cmd/claude.rs:32-34`, `crates/zygo/src/cmd/opencode.rs:32-34`); a `server` with user info is an error (AU98); `args` that AU30 would remove, taken relative to the pane's folder, give one warning that names each removed argument and never its value (W§12); a `--settings` is not reported (AU59).
```

Replace the W§16 row (`:678`) in place with:

```text
| `crates/zygo-config/src/check.rs`, `crates/zygo/src/cmd/config.rs` | Loadout validation: the structural checks in `check.rs`; `compact`, `server` user info (AU98) and the AU30 argument warning in `cmd/config.rs` (AU103, W§11). |
```

Replace the W§12 row `` | `zygo loadout list` / `zygo loadout show <n>` | `` (`:560`) in place with:

```text
| `zygo loadout list` / `zygo loadout show <n>` | Loadouts with their source (built-in or user); one loadout in TOML. `zygo loadout show` and `zygo config show` show each loadout argument through one rule set, so they print the same. An argument is `<user info removed>` when it holds a scheme of 2 or more characters followed by `:/` or `:\` with an `@` anywhere after it (`?`, `#` and `/` do not end the search; `C:\` is a drive letter, not a scheme), or when the argument, its `=` value or its attached short-flag value has the form `user:pass@host` (the text before its last `@` holds a `:` and no `/` or `\`). So `me@example.com`, `claude-sonnet-4@20250514` and `C:\Users\first.last@corp\x.json` are shown; `node:20@sha256:…` is hidden (accepted). Before the `--` that ends the flags: each value of an AU30 content flag or of `--settings` (by the arities of the pinned help; every `--mcp-config` value) is shown only when it names a regular file, taken relative to the pane's folder as `zygo up` takes it (AU30), else `<inline JSON not shown>`; the value of a flag whose name holds `key`, `token`, `secret`, `password` or `auth` (any case) is `<value not shown>`, in the `=` form and as the next token; an argument, `=` value or attached short-flag value whose first non-blank character is `{` or `[` is `<inline JSON not shown>`, also when it does not parse. Known limit: a bare `Authorization: Bearer <x>` argument is shown. A loadout `server` that holds user info (AU98) is `<user info removed>`; the other text fields lose URL user info. `zygo config check` takes each pane's arguments relative to the pane's folder, as `zygo up` does, and names each argument that AU30 removes by its flag (`-c` and `-r` for `-cr`; a flag that loses only a value is named too), `--`, `<after -->` or `<positional>`, never by a value. When the pane folder cannot be found, `zygo config show`, `zygo config check` and `zygo loadout show` print `zygo: the pane folder could not be found (<error>); paths are checked against the current folder` and go on. |
```

Append as the last row of the change-log table:

```text
| 2026-11-23 | W§11 (`zygo config check` bullet), W§12 (`zygo loadout list` / `zygo loadout show <n>` row), W§16 (`check.rs` row) | controller decisions 2026-11-23 (M1 split into M1 and FIX-27; FIX-27 plan review F3-F17); author's call on the text | `loadout show`, `config show` and `config check` print no secret and name what they remove: one rule set for both show commands (user info by a scheme anywhere or the `user:pass@host` form, JSON by its first character, secret-named flag values, content values only when they name a file relative to the pane's folder); the pane folder by `zygo up`'s own lookup, with one warning line when it fails; `config check` labels from the same AU30 walk; the `compact`, AU98 and AU30 checks live in `crates/zygo/src/cmd/config.rs`, and `compact = "on"` is an error. |
```

Check: `rg -c -F 'C:\Users\first.last@corp\x.json` are shown' docs/specs/2026-11-20-workspaces-design.md` prints `1`; `rg -c -F 'crates/zygo/src/cmd/config.rs`, for `claude-code` and `opencode` panes' docs/specs/2026-11-20-workspaces-design.md` prints `1`; `rg -n -F '| `crates/zygo-config/src/check.rs`, `crates/zygo/src/cmd/config.rs` |' docs/specs/2026-11-20-workspaces-design.md` prints one line; `rg -n '^\| 2026-11-23 \| W§11' docs/specs/2026-11-20-workspaces-design.md` prints one line, and it is the last line of the change-log table.
- [x] 14. Run the Acceptance check alone: `cargo test -p zygo-workspace --test args --test loadout && cargo test -p zygo --test loadout_secrets && cargo test -p zygo --lib cmd::loadout && cargo test --workspace --locked --no-run` → exit 0.
- [x] 15. Regression, scoped to the changed crates: `cargo test -p zygo-workspace --locked` → all pass (`up` among them: only visibility changed); `cargo test -p zygo --locked` → all pass. Lint: `cargo clippy -p zygo -p zygo-workspace --locked --tests -- -D warnings` → no warning. Windows build: `cargo check -p zygo --tests --locked --target x86_64-pc-windows-gnu` → exit 0, or a first error in a dependency build script (record it); then read the gates by hand: the `loadout show` tests and `shown_args_show_a_content_value_only_when_it_names_a_file` are `cfg(unix)`; every other test and every helper they use is not gated.
- [x] 16. Commit. Write this message to `FIX-27-commit.txt` in the session scratchpad (outside the repository):

```text
fix(workspace): loadout show, config show and check print no secret (FIX-27)

zygo loadout show and zygo config show map each loadout argument through
one function: user info by a scheme anywhere in the token or by the
user:pass@host form (model ids such as claude-sonnet-4@20250514 stay),
JSON by its first character, the values of secret-named flags, and a
content flag value only when it names a file relative to the pane's
folder. The pane folder comes from zygo up's own find_checkout. zygo
config check walks each pane's arguments with that folder and labels
each removed token by its index in the walk: -cr names -c and -r, every
positional is labelled, and no label holds a value.

Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
Zygo-Work: FIX-27
```

Then `git add crates/zygo/src/cmd/loadout.rs crates/zygo/src/cmd/config.rs crates/zygo/tests/loadout_secrets.rs crates/zygo-workspace/src/args.rs crates/zygo-workspace/tests/args.rs crates/zygo-workspace/src/loadout.rs crates/zygo-workspace/tests/loadout.rs crates/zygo-workspace/src/up.rs docs/specs/2026-11-20-workspaces-design.md docs/work/phase2/nodes/FIX-27.md` and `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com git commit -F <scratchpad>/FIX-27-commit.txt` → one commit; `git show --stat HEAD` lists only these files; `git log -1 --format=%B` ends with the two trailer lines and holds no Claude attribution.

### Live code against attempt 1

Checked 2026-11-23 on `b4482df` (the parts of attempt 1 that move here).

1. `args::has_user_info` (`args.rs:418`) is true for `me@example.com` and `claude-sonnet-4@20250514`. Attempt 1 used it for every argument; D1 is a different rule, and `has_user_info` stays for `server` (AU98).
2. `args.rs` has no form of `removable` with a base: only `filter_from` (`:346`) takes one. Attempt 1 called `removable` (process folder) on slices of the arguments; D8 adds `removed_labels` to the walk.
3. `args::occurrences` (`:375`) gives one `Occurrence` per letter of a short cluster, all with the same `tokens`; attempt 1 merged them by `tokens.start` and so gave one label for `-cr`.
4. `--mcp-config` is `Arity::Variadic` (`args.rs:65`); attempt 1 hid only the one token after a content flag, so a second value was shown and a content flag with no value hid the next flag.
5. The AU30 content check is `is_file()` (`args.rs:171-177`); attempt 1 showed no content value at all by its file state (it hid by flag name).
6. The pane folder rule lives inside `loadout::resolve` (`loadout.rs:147-167`) and `up::find_checkout` and `up::GIT_BOUND` are `pub(crate)` (`up.rs:302`, `:36`); D6 moves the rule into `pane_cwd` and makes the two items public.
7. `zygo-config`'s `check.rs` has no `compact`, `server` or AU30 check (`:151`: "this crate depends on neither", AU103), but W§11 lists `compact accepted by parse_window` under `check.rs`; step 13 rewrites W§11 and the W§16 row.
8. `Merged::show` (`layers.rs:752`) reads `self.config` for every `--layer`, and `zygo_core::esc` keeps `\`, so the `config show` line holds the same TOML array text as `loadout show` (D5 test).

### Self-review (§3.3)

1. Spec coverage. D1 user info (F3, F6): steps 6, 7, 8 (`loadout_show_and_config_show_print_the_same_args`, `config_show_hides_every_form`), 13. D2 JSON and D3 secret names (F3): steps 6, 8. D4 content values as files (decision 2, F14): steps 6, 8, 13. D5 one function, equal outputs (F9, F10): steps 6, 8. D6 the pane folder by `up`'s own lookup, the warning line, subfolder, linked worktree, bare repository, no `cwd`, `branch:x` (F4, F5, F16): steps 2, 3, 8, 9, 13. D7 show and the scrub of the other fields (F15): steps 6, 11, 12. D8 labels, every positional labelled (decision 3, F17): steps 4, 5, 8. D9 `config check` with `on` an error (F13): steps 8, 12, 13. W§11, W§12, W§16 rows and the change log (F13): step 13. F2: every value is in "Test values" or in the step text; the attempt's helper is renamed `args_in_text`. F20: `op5_exit` is not consumed. Carried round 1-3 tests, with corrected expectations: `removed_labels_*` (F17 adds `<positional>`), `show_replaces_an_argument_with_user_info` (whole token), `show_hides_every_content_flag_value_and_inline_json_after_any_flag`, `show_hides_inline_json`, `show_replaces_a_server_with_user_info_and_hides_args`, `config_show_hides_loadout_user_info_and_inline_json`, `config_show_hides_args_with_user_info_and_content_flag_values`, `config_check_reports_loadout_compact_server_and_removable_args` (`-p <positional>`), `config_check_never_labels_a_value_that_starts_with_a_dash`, `config_check_never_prints_a_removed_argument_value` (`-p <positional> -- <after -->`), `config_check_labels_a_removed_token_by_its_index`, `w18_63_config_check_does_not_report_settings`, `loadout_show_hides_server_user_info`.
2. Step scan. Each step is one action with one check. No body is given; D1-D9 state the rules that the tests alone do not fix (the scheme length, the last `@`, the flags end, the role order, the label rules, when the folder lookup runs).
3. Interface match. M1 produces `LoadoutArgs`, `LoadoutCmd { List }`, `run` and `user_config_or_exit` with these signatures; this node adds only the `Show` variant and its arm. `pane_cwd` keeps `resolve`'s output. `find_checkout` and `GIT_BOUND` change only their visibility. `removed_labels` is new; no other walk user changes. No later node consumes this node's Produces; FIX-24 shares `args.rs`, `up.rs` and the spec and runs after it.
4. Review focus. Line 4 (the session-argument list and the AU30 filter list drift apart) is tested in L2 and W4; this node changes neither list, and `removed_labels_are_empty_only_when_nothing_is_removed` ties the labels to the same walk. No line names FIX-27.
5. Proportion. The plan gives rules, signatures, values and assertions; no body.
