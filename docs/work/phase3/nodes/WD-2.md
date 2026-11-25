# WD-2 — Typed shapes, allow-list validation and digest of the workspace files

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§6.2, R§6.3, R§6.4, R§6.5, R-C3, R-C7, R§19 PR3, R§18 item 4 (the parse rules)
**Files:** `crates/zygo-config/src/repo_def/mod.rs`, `crates/zygo-config/src/repo_def/shape.rs` (new), `crates/zygo-config/src/repo_def/digest.rs` (new), `crates/zygo-config/tests/repo_def_shape.rs` (new)
**Consumes:** `Files` (WD-1)
**Produces:** `repo_def::Definition { workspace: Option<WorkspaceFile>, profiles: BTreeMap<String, RepoProfile> }`, `repo_def::WorkspaceFile { version: u32, layout: Option<String>, members: BTreeMap<String, MemberDef> }`, `repo_def::MemberDef { kind: Option<String>, job: Option<String>, profile: Option<String>, pool: bool, cwd: Option<String> }`, `repo_def::RepoProfile { overrides: BTreeMap<String, String>, disabled: Vec<String> }`, `repo_def::parse(files: &Files) -> Result<Definition, String>` (the `Err` is the first violation as `<path>: <key path>: <reason>`, key path passed through `zygo_core::esc`), `repo_def::digest(files: &Files) -> String` (64 lower-case hex), `repo_def::short(digest: &str) -> &str` (first 12 characters)
**Acceptance:** `cargo test -p zygo-config --test repo_def_shape` → all pass. The tests cover each R§18 item 4 input that is a parse rule (`members.a.args`, `.config_dir`, `.passthrough`, `.yolo`, `.server`, `.compact`, `kind = "shell"`, a top-level `[[model]]`, `[redaction]`, `[update]`, `[workspace]`, an unknown top-level key, `version = 2`, two orchestrators, 33 members, `pool = true` on `opencode`, `cwd` of `/tmp`, `../x`, `~/x` and `dоcs` with a Cyrillic `о`, an override value with a non-ASCII character, a profile `prompt`, `roles`, `plugins`, `features`, an empty profile) with the R§6.4 message, including exactly `.zygo/workspace.toml: members.a.args: not allowed in a repository file`; a TOML syntax error prints as `TOML parse error at line L, column C: <message>` with no quoted source line; the first violation wins in R§6.4 order (workspace file, then profiles in byte order of name); the digest of a fixed two-file input equals a hard-coded SHA-256 of the R§6.5 byte layout; PR3: a 64 KiB input of nested arrays and a 64 KiB input of nested inline tables each give `Err` or `Ok` with no stack overflow, parsed on a spawned thread with the default thread stack size

## Plan

**Premises checked against the live code (branch `phase3/WD-2` at `ca3c52ec`, equal to `origin/phase-3`):**

- WD-1 landed: `crates/zygo-config/src/repo_def/mod.rs` declares `mod read;` and re-exports `read_at, read_default, Files, GitRunner, Read, SystemGit, PROFILES_MAX, PROFILE_MAX, WORKSPACE_MAX`. `Files` is at `crates/zygo-config/src/repo_def/read.rs:46-58` (copied below). All its fields are `pub`, so the tests build `Files` values directly.
- `zygo_core::esc(s: &str) -> String` (`crates/zygo-core/src/text.rs:4`, re-exported at `crates/zygo-core/src/lib.rs:18`) writes `\n`, `\r`, `\t` and every other control or bidi character as `\u{1b}`, `\u{202e}` and so on. `zygo-core` is already a dependency of `zygo-config`.
- TOML crate: `toml = "1.2.1"` (`Cargo.toml:68`; the spec's `Cargo.toml:66` is the line at its pinned commit `97d8767`), locked at `toml 1.2.1+spec-1.1.0` (`Cargo.lock:2707-2708`), with `toml_parser 1.1.5+spec-1.1.0`. `cargo tree -e features -i toml` shows only the features `default`, `std`, `serde`, `parse` and `display`. So `preserve_order` is off, `toml::Table` is a `BTreeMap` and keys come in byte order. `unbounded` is off too.
- SHA-256: `sha2 = "0.11.1"` (`Cargo.lock:2409-2410`) and `hex 0.4.3` (`Cargo.lock:1084-1085`) are already dependencies of `zygo-config` (`crates/zygo-config/Cargo.toml:16-17`). No new dependency is needed. The same pattern `hex::encode(Sha256::digest(..))` is at `crates/zygo-workspace/src/identity.rs:18`.
- Parse errors: `toml::de::Error::message() -> &str` and `::span() -> Option<Range<usize>>`. The existing `zygo_config::repo_file::syntax_error(text, message, span)` (`crates/zygo-config/src/repo_file.rs:28-37`, the rule that R-C7 names) turns them into `TOML parse error at line L, column C: <message>` with no source line. It already has 4 callers (`repo_file.rs:273`, `profile.rs:284`, `layers.rs:452`, `zygo-plugins`). WD-2 reuses it. Probe on toml 1.2.1: `version = 1\n\nfoo = \n` gives message `string values must be quoted, expected literal string` at span `19..19`, which is line 3, column 7.
- PR3: toml 1.2.1 has a recursion limit. `toml-1.2.1/src/de/parser/mod.rs:36-38,70-71` wraps the event parser in `toml_parser::parser::RecursionGuard` with `LIMIT: u32 = 80`, unless the feature `unbounded` is on, and it is off. `key.rs:66-67` applies the same limit to dotted keys. Probe with toml 1.2.1 on a spawned thread with the default stack: 64 KiB of nested arrays (`a = [[[…]]]`) gives `Err` "cannot recurse further; max recursion depth met" at span `84..85`. 64 KiB of nested inline tables (`a = {b = {b = …}}`) gives the same at span `404..405`. Dotted keys and table headers 20 000 deep give the same. Depth 70 parses `Ok`, depth 90 gives `Err`. So the parse never recurses deeper than 80. The WD-2 walk descends only into the fixed levels of R§6.2 and R§6.3 (at most `members.<name>.<key>`) and never into a value of unknown shape. So the walk is bounded too.
- Reuse: `zygo_core::ids::valid_member_name(s: &str) -> bool` (`crates/zygo-core/src/ids.rs:19-27`: `^[a-z][a-z0-9-]{0,31}$`, not `all`, `cli` or the pane-key form, which is M§6 as R§6.2 cites). `zygo_config::profile::valid_name(s: &str) -> bool` (`crates/zygo-config/src/profile.rs:49-51`: 1-32 of `[a-z0-9-]`, which is the §11.3 profile name rule and also the R§6.3 role-key rule).
- Not reusable or not present: `zygo_config::typed::from_value` (`typed.rs:130`) is serde-based and reports `unknown key`, not R§6.4's reason, so WD-2 walks a `toml::Table` by hand. No layout-name list, no `cwd` validator and no allow-list walker exists in `zygo-config` (`grep -rn "even-horizontal\|branch:\|not allowed" crates/zygo-config/src`: nothing). The loadout `cwd` resolver is in `zygo-workspace` (`loadout.rs:116-128`), and `zygo-config` must not depend on it (graph Global constraints, crate graph). `zygo_workspace::pool::tilde` does not exist (`grep -rn "fn tilde" crates`: nothing), and R§6.2 refuses a leading `~` and does not expand it, so none is needed.
- No spec premise drifted. The spec is not changed.
- Review focus assigned to WD-2 (graph "Review focus" item 31: a key outside the allow list at any depth, inline tables included, and `cwd` look-alike characters) is covered by Step 1 tests `allow_list_holds_at_every_depth` and `parse_rules_of_r18_item_4` (the `dоcs` case).

### Interfaces

Consumes (as in the code now):

```rust
// crates/zygo-config/src/repo_def/read.rs:46-58 (WD-1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    pub full_ref: Option<String>,
    pub commit: String,
    pub workspace: Option<Vec<u8>>,           // .zygo/workspace.toml
    pub profiles: BTreeMap<String, Vec<u8>>,  // key: the stem, `rust` for .zygo/profiles/rust.toml
    pub ignored: Vec<String>,
}
// existing helpers
pub fn zygo_core::esc(s: &str) -> String;
pub fn zygo_core::ids::valid_member_name(s: &str) -> bool;
pub fn zygo_config::profile::valid_name(s: &str) -> bool;
pub fn zygo_config::repo_file::syntax_error(text: &str, message: &str, span: Option<std::ops::Range<usize>>) -> String;
```

Produces (`crates/zygo-config/src/repo_def/shape.rs` and `digest.rs`, re-exported from `repo_def/mod.rs`):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition { pub workspace: Option<WorkspaceFile>, pub profiles: BTreeMap<String, RepoProfile> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile { pub version: u32, pub layout: Option<String>, pub members: BTreeMap<String, MemberDef> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberDef { pub kind: Option<String>, pub job: Option<String>, pub profile: Option<String>, pub pool: bool, pub cwd: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoProfile { pub overrides: BTreeMap<String, String>, pub disabled: Vec<String> }

pub fn parse(files: &Files) -> Result<Definition, String>;
pub fn digest(files: &Files) -> String;      // 64 lower-case hex
pub fn short(digest: &str) -> &str;          // first 12 characters
```

`MemberDef.kind` and `.job` hold the file's value, and `None` when the key is absent. The defaults (`claude-code`, `worker`) belong to the consumers WD-6 and WD-10, so `parse` does not fill them in. `RepoProfile.disabled` keeps the order of the file.

### Author's calls

- Reason texts (R§17 leaves them to the builder, apart from R§6.4's "not allowed in a repository file" and the TOML form): see the tables in Step 1. A wrong type and a wrong value of a key get the same reason, so the reason never quotes the value.
- Three violations are about the whole file, not one key: a TOML syntax error (R§6.4's own example), bytes that are not UTF-8, and an empty profile. These have no key-path segment: `<path>: <reason>`. All other violations are `<path>: <esc(key path)>: <reason>`. The key path joins its keys with `.`, and an array item is written `disabled[<index>]`. The `<path>` of a profile is `.zygo/profiles/<esc(name)>.toml`.
- Order inside the workspace file: (1) `version`; (2) the other top-level keys in byte order. For `members` the order is: its type, then its count (at most 32, checked before any entry), then each member in byte order of name. For each member: its name, its type, its keys in byte order, then the cross-key rule `pool = true` needs a `claude-code` kind, then the orchestrator rule, which fires on the second orchestrator in name order. Inside a profile file: the top-level keys in byte order, then the rule that the profile is not empty.
- `cwd` checks, in this order: string; every byte `0x20`-`0x7e`; does not start with `~`; not absolute (`starts_with('/')` or `Path::is_absolute`, so `/tmp` is refused on Windows as well); then, after a `branch:` prefix, 1-255 bytes; else at most 1024 bytes, no `\`, and each `/` part 1-255 bytes and not `.` or `..`.
- `parse` does not check the profile names again: WD-1's read already refuses names that do not match the pattern and the name `none` (R§6.1 step 4). The R§18 item 4 `None.toml` case is a WD-1 read case.
- `short(d)` is `d.get(..12).unwrap_or(d)`, so a shorter input does not panic.

### Steps

- [ ] **Step 1: write the failing tests** in `crates/zygo-config/tests/repo_def_shape.rs` (new):

```rust
//! WD-2: shapes, allow-list validation and digest of `.zygo/` (R§6.2-R§6.5, R-C3, R-C7, R§19 PR3, R§18 item 4).
use std::collections::BTreeMap;
use zygo_config::repo_def::{self, Definition, Files, MemberDef, RepoProfile, WorkspaceFile};

const W: &str = ".zygo/workspace.toml";
const P: &str = ".zygo/profiles/rust.toml";

fn files(ws: Option<&str>, profiles: &[(&str, &str)]) -> Files {
    Files { full_ref: Some("refs/remotes/origin/main".into()), commit: "a".repeat(40),
            workspace: ws.map(|t| t.as_bytes().to_vec()),
            profiles: profiles.iter().map(|(n, t)| (n.to_string(), t.as_bytes().to_vec())).collect(),
            ignored: vec![] }
}
fn ws_err(text: &str) -> String { repo_def::parse(&files(Some(text), &[])).unwrap_err() }
fn prof_err(text: &str) -> String { repo_def::parse(&files(None, &[("rust", text)])).unwrap_err() }
fn member(body: &str) -> String { format!("version = 1\n[members.a]\n{body}\n") }
```

`parse_rules_of_r18_item_4`: every R§18 item 4 parse-rule input gives exactly these messages:

```rust
assert_eq!(ws_err(&member("args = [\"x\"]")), ".zygo/workspace.toml: members.a.args: not allowed in a repository file");
let many: String = (0..33).map(|i| format!("[members.m{i}]\n")).collect();
let ws: Vec<(String, &str)> = vec![
    (member("config_dir = \"/x\""), "members.a.config_dir: not allowed in a repository file"),
    (member("passthrough = true"), "members.a.passthrough: not allowed in a repository file"),
    (member("yolo = true"), "members.a.yolo: not allowed in a repository file"),
    (member("server = \"x\""), "members.a.server: not allowed in a repository file"),
    (member("compact = true"), "members.a.compact: not allowed in a repository file"),
    (member("kind = \"shell\""), "members.a.kind: must be claude-code or opencode"),
    ("version = 1\n[[model]]\nid = \"x\"\n".into(), "model: not allowed in a repository file"),
    ("version = 1\n[redaction]\ncontent = false\n".into(), "redaction: not allowed in a repository file"),
    ("version = 1\n[update]\nchannel = \"x\"\n".into(), "update: not allowed in a repository file"),
    ("version = 1\n[workspace]\npool = \"/x\"\n".into(), "workspace: not allowed in a repository file"),
    ("version = 1\nfoo = 1\n".into(), "foo: not allowed in a repository file"),
    ("version = 2\n".into(), "version: must be the integer 1"),
    ("version = 1\n[members.a]\njob = \"orchestrator\"\n[members.b]\njob = \"orchestrator\"\n".into(),
     "members.b.job: at most one member can be the orchestrator"),
    (format!("version = 1\n{many}"), "members: at most 32 members"),
    (member("kind = \"opencode\"\npool = true"), "members.a.pool: only a claude-code member can take a pool directory"),
    (member("cwd = \"/tmp\""), "members.a.cwd: must not be an absolute path"),
    (member("cwd = \"../x\""), "members.a.cwd: must not hold a . or .. part"),
    (member("cwd = \"~/x\""), "members.a.cwd: must not start with ~"),
    (member("cwd = \"d\u{43e}cs\""), "members.a.cwd: must be printable ASCII (bytes 0x20-0x7e)"),
];
for (text, want) in &ws { assert_eq!(ws_err(text), format!("{W}: {want}"), "input: {text}"); }
let prof = [
    ("overrides = { review = \"gpt-6-s\u{f3}l\" }\n", "overrides.review: must be 1-128 bytes of printable ASCII"),
    ("prompt = \"x\"\n", "prompt: not allowed in a repository file"),
    ("[roles.x]\nprompt = \"y\"\n", "roles: not allowed in a repository file"),
    ("plugins = [\"x\"]\n", "plugins: not allowed in a repository file"),
    ("[features]\nresults = false\n", "features: not allowed in a repository file"),
    ("", "a profile must set overrides or disabled"),
    ("overrides = {}\ndisabled = []\n", "a profile must set overrides or disabled"),
];
for (text, want) in prof { assert_eq!(prof_err(text), format!("{P}: {want}"), "input: {text}"); }
```

`allow_list_holds_at_every_depth` (review focus 31):

```rust
assert_eq!(ws_err("version = 1\nmembers = { a = { kind = \"claude-code\", args = [\"x\"] } }\n"),
           ".zygo/workspace.toml: members.a.args: not allowed in a repository file");
assert_eq!(ws_err(&member("[members.a.extra]\nx = 1")), ".zygo/workspace.toml: members.a.extra: not allowed in a repository file");
assert_eq!(prof_err("overrides = { review = { model = \"x\" } }\n"),
           ".zygo/profiles/rust.toml: overrides.review: must be 1-128 bytes of printable ASCII");
assert_eq!(prof_err("disabled = [[\"x\"]]\n"), ".zygo/profiles/rust.toml: disabled[0]: a role name must be 1-32 of [a-z0-9-]");
```

`value_rules` (the other R§6.2 and R§6.3 rules; inputs built with `member(..)` unless shown):

```rust
let cases: Vec<(String, &str)> = vec![
    ("version = 1\nlayout = \"tiled,abc\"\n".into(), "layout: must be one of even-horizontal, even-vertical, main-horizontal, main-vertical, tiled"),
    ("[members.a]\n".into(), "version: must be the integer 1"),
    ("version = 1.0\n".into(), "version: must be the integer 1"),
    ("version = 1\nmembers = 1\n".into(), "members: must be a table"),
    ("version = 1\nmembers = { a = 1 }\n".into(), "members.a: must be a table"),
    ("version = 1\n[members.Bad]\n".into(), "members.Bad: not a valid member name"),
    ("version = 1\n[members.all]\n".into(), "members.all: not a valid member name"),
    (member("job = \"lead\""), "members.a.job: must be orchestrator or worker"),
    (member("profile = \"none\""), "members.a.profile: must be 1-32 of [a-z0-9-] and not none"),
    (member("profile = \"Rust\""), "members.a.profile: must be 1-32 of [a-z0-9-] and not none"),
    (member("pool = \"yes\""), "members.a.pool: must be true or false"),
    (member("cwd = 1"), "members.a.cwd: must be a string"),
    (member("cwd = \"branch:\""), "members.a.cwd: a branch name must be 1-255 bytes"),
    (member(&format!("cwd = \"branch:{}\"", "b".repeat(256))), "members.a.cwd: a branch name must be 1-255 bytes"),
    (member(&format!("cwd = \"{}\"", vec!["a"; 513].join("/"))), "members.a.cwd: must be at most 1024 bytes"),
    (member(&format!("cwd = \"x/{}\"", "a".repeat(256))), "members.a.cwd: each part must be 1-255 bytes"),
    (member("cwd = \"a//b\""), "members.a.cwd: each part must be 1-255 bytes"),
    (member("cwd = 'a\\b'"), "members.a.cwd: must not hold \\"),
    (member("cwd = \"a/./b\""), "members.a.cwd: must not hold a . or .. part"),
];
for (text, want) in &cases { assert_eq!(ws_err(text), format!("{W}: {want}"), "input: {text}"); }
let o17: String = (0..17).map(|i| format!("r{i} = \"m\"\n")).collect();
let d17 = (0..17).map(|i| format!("\"r{i}\"")).collect::<Vec<_>>().join(", ");
let prof: Vec<(String, &str)> = vec![
    ("overrides = 1\n".into(), "overrides: must be a table"),
    (format!("[overrides]\n{o17}"), "overrides: at most 16 entries"),
    ("overrides = { Review = \"m\" }\n".into(), "overrides.Review: a role name must be 1-32 of [a-z0-9-]"),
    ("overrides = { review = \"\" }\n".into(), "overrides.review: must be 1-128 bytes of printable ASCII"),
    (format!("overrides = {{ review = \"{}\" }}\n", "m".repeat(129)), "overrides.review: must be 1-128 bytes of printable ASCII"),
    ("disabled = \"design\"\n".into(), "disabled: must be an array"),
    (format!("disabled = [{d17}]\n"), "disabled: at most 16 entries"),
    ("disabled = [\"design\", \"x y\"]\n".into(), "disabled[1]: a role name must be 1-32 of [a-z0-9-]"),
];
for (text, want) in &prof { assert_eq!(prof_err(text), format!("{P}: {want}"), "input: {text}"); }
let mut f = files(None, &[]); f.workspace = Some(vec![0xff]);
assert_eq!(repo_def::parse(&f).unwrap_err(), ".zygo/workspace.toml: not UTF-8");
let ok32: String = (0..32).map(|i| format!("[members.m{i}]\n")).collect();
assert_eq!(repo_def::parse(&files(Some(&format!("version = 1\n{ok32}")), &[])).unwrap().workspace.unwrap().members.len(), 32);
```

`toml_syntax_error_is_message_and_position_only`:

```rust
let e = ws_err("version = 1\n\nfoo = \n");
assert_eq!(e, ".zygo/workspace.toml: TOML parse error at line 3, column 7: string values must be quoted, expected literal string");
let e = prof_err("disabled = [\"design\"]\ntoken = sk-live-123\n");
assert!(e.starts_with(".zygo/profiles/rust.toml: TOML parse error at line 2, column "), "{e}");
assert!(!e.contains('\n') && !e.contains('|') && !e.contains("sk-live-123"), "{e}");
```

`first_violation_wins_in_r6_4_order`:

```rust
assert_eq!(repo_def::parse(&files(Some("version = 2\n"), &[("a", "prompt = \"x\"\n")])).unwrap_err(),
           ".zygo/workspace.toml: version: must be the integer 1");
assert_eq!(repo_def::parse(&files(Some("version = 1\n"), &[("b", "prompt = \"x\"\n"), ("a", "roles = 1\n")])).unwrap_err(),
           ".zygo/profiles/a.toml: roles: not allowed in a repository file");
assert_eq!(ws_err("version = 1\nfoo = 1\nbar = 2\n"), ".zygo/workspace.toml: bar: not allowed in a repository file");
```

`key_path_is_escaped` (R-C7):

```rust
assert_eq!(ws_err("version = 1\n[members.\"a\\u001b\"]\n"), ".zygo/workspace.toml: members.a\\u{1b}: not a valid member name");
assert_eq!(prof_err("\"x\\u202e\" = 1\n"), ".zygo/profiles/rust.toml: x\\u{202e}: not allowed in a repository file");
```

`valid_files_parse_to_the_typed_shape`:

```rust
let ws = "version = 1\nlayout = \"tiled\"\n[members.lead]\nkind = \"claude-code\"\njob = \"orchestrator\"\nprofile = \"rust\"\npool = true\ncwd = \"branch:feature-x\"\n[members.w]\nkind = \"opencode\"\ncwd = \"crates/zygo\"\n";
let p = "overrides = { review = \"gpt-6-sol\", code = \"claude-sonnet-5-5\" }\ndisabled = [\"design\"]\n";
assert_eq!(repo_def::parse(&files(Some(ws), &[("rust", p)])).unwrap(), Definition {
    workspace: Some(WorkspaceFile { version: 1, layout: Some("tiled".into()), members: BTreeMap::from([
        ("lead".into(), MemberDef { kind: Some("claude-code".into()), job: Some("orchestrator".into()), profile: Some("rust".into()), pool: true, cwd: Some("branch:feature-x".into()) }),
        ("w".into(), MemberDef { kind: Some("opencode".into()), job: None, profile: None, pool: false, cwd: Some("crates/zygo".into()) }),
    ]) }),
    profiles: BTreeMap::from([("rust".into(), RepoProfile {
        overrides: BTreeMap::from([("code".into(), "claude-sonnet-5-5".into()), ("review".into(), "gpt-6-sol".into())]),
        disabled: vec!["design".into()] })]),
});
assert_eq!(repo_def::parse(&files(Some("version = 1\n"), &[])).unwrap().workspace.unwrap().members, BTreeMap::new());
assert_eq!(repo_def::parse(&files(None, &[])).unwrap(), Definition { workspace: None, profiles: BTreeMap::new() });
```

`digest_is_sha256_of_the_r6_5_layout` (expected values computed with `shasum -a 256` over the bytes `.zygo/workspace.toml\0` `12\0` `version = 1\n` `.zygo/profiles/rust.toml\0` `22\0` `disabled = ["design"]\n`):

```rust
let f = files(Some("version = 1\n"), &[("rust", "disabled = [\"design\"]\n")]);
let d = repo_def::digest(&f);
assert_eq!(d, "2019b3cde22d08cf3f77e15dcd2e8983da90fc3f48fec869e020e20490454e37");
assert_eq!(repo_def::short(&d), "2019b3cde22d");
assert_eq!(repo_def::digest(&files(None, &[])), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
let mut g = f.clone(); g.full_ref = None; g.commit = "b".repeat(40); g.ignored = vec![".zygo/README".into()];
assert_eq!(repo_def::digest(&g), d);
```

`pr3_deep_nesting_does_not_overflow_the_stack` (each input is exactly 65536 bytes, `WORKSPACE_MAX`; the parse runs on `std::thread::spawn`, which has the default stack size):

```rust
let arrays = format!("a = {}{}", "[".repeat(32766), "]".repeat(32766));
let tables = format!("a = {}{}", "{b = ".repeat(10922), "}".repeat(10922));
for text in [arrays, tables] {
    assert_eq!(text.len() as u64, repo_def::WORKSPACE_MAX);
    let f = files(Some(&text), &[]);
    let r = std::thread::spawn(move || repo_def::parse(&f)).join().expect("no panic, no overflow");
    let e = r.unwrap_err();
    assert!(e.starts_with(".zygo/workspace.toml: TOML parse error at line 1, column "), "{e}");
}
```

- [ ] **Step 2: run it and see it fail.** `cargo test -p zygo-config --test repo_def_shape` fails to compile: `Definition`, `MemberDef`, `RepoProfile`, `WorkspaceFile`, `parse`, `digest` and `short` are not in `zygo_config::repo_def`.

- [ ] **Step 3: write `crates/zygo-config/src/repo_def/shape.rs`** (new). It holds the four structs of Interfaces and `pub fn parse(files: &Files) -> Result<Definition, String>`, plus these private items:
  - `const NOT_ALLOWED: &str = "not allowed in a repository file";`
  - `const LAYOUTS: [&str; 5] = ["even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled"];`
  - `const MEMBERS_MAX: usize = 32;` and `const ENTRIES_MAX: usize = 16;`
  - `fn text_of(path: &str, bytes: &[u8]) -> Result<toml::Table, String>`: `std::str::from_utf8`, else `<path>: not UTF-8`; then `text.parse::<toml::Table>()`, else `format!("{path}: {}", crate::repo_file::syntax_error(text, e.message(), e.span()))`.
  - `fn workspace(t: &toml::Table) -> Result<WorkspaceFile, (String, &'static str)>` and `fn profile(t: &toml::Table) -> Result<RepoProfile, (String, &'static str)>`. Each returns `(key path, reason)`, and the key path is unescaped. They follow the order of Author's calls, with the reasons of Step 1. A member name must pass `zygo_core::ids::valid_member_name`. `profile` must pass `crate::profile::valid_name(s) && s != "none"`. An override key and a `disabled` item must pass `crate::profile::valid_name`. An override value is a string of 1-128 bytes, all in `0x20..=0x7e`. A whole-file violation (the empty profile) returns an empty key path.
  - `fn cwd_rule(v: &toml::Value) -> Result<String, &'static str>` implements the `cwd` order of Author's calls.
  - `parse` checks `.zygo/workspace.toml` first and then each `files.profiles` entry in map order (byte order of name). It builds each error as `format!("{path}: {}: {reason}", zygo_core::esc(&key))`, or as `format!("{path}: {reason}")` when the key path is empty. The profile path is `format!(".zygo/profiles/{}.toml", zygo_core::esc(name))`.
  Result: `cargo build -p zygo-config` compiles.

- [ ] **Step 4: write `crates/zygo-config/src/repo_def/digest.rs`** (new). The R§6.5 byte layout, exactly:

```rust
use sha2::{Digest, Sha256};
use super::Files;

/// R§6.5: SHA-256 over, per file in R§6.4 order, `path 0x00 len-decimal 0x00 bytes`; 64 lower-case hex.
pub fn digest(files: &Files) -> String {
    let mut h = Sha256::new();
    let ws = files.workspace.iter().map(|b| (".zygo/workspace.toml".to_string(), b));
    let ps = files.profiles.iter().map(|(n, b)| (format!(".zygo/profiles/{n}.toml"), b));
    for (path, bytes) in ws.chain(ps) {
        h.update(path.as_bytes());
        h.update([0u8]);
        h.update(bytes.len().to_string().as_bytes());
        h.update([0u8]);
        h.update(bytes);
    }
    hex::encode(h.finalize())
}

/// The first 12 characters, for one-line messages (R§6.5).
pub fn short(digest: &str) -> &str { digest.get(..12).unwrap_or(digest) }
```

- [ ] **Step 5: re-export from `crates/zygo-config/src/repo_def/mod.rs`.** Add `mod digest;`, `mod shape;`, `pub use digest::{digest, short};` and `pub use shape::{parse, Definition, MemberDef, RepoProfile, WorkspaceFile};`. Keep the WD-1 `pub use read::{…}` line as it is. Update the module doc comment to name R§6.2-R§6.5. Result: `cargo build -p zygo-config` compiles.

- [ ] **Step 6: verify.** `cargo test -p zygo-config --test repo_def_shape` passes with 9 passed and 0 failed (`parse_rules_of_r18_item_4`, `allow_list_holds_at_every_depth`, `value_rules`, `toml_syntax_error_is_message_and_position_only`, `first_violation_wins_in_r6_4_order`, `key_path_is_escaped`, `valid_files_parse_to_the_typed_shape`, `digest_is_sha256_of_the_r6_5_layout`, `pr3_deep_nesting_does_not_overflow_the_stack`). After that, `cargo test -p zygo-config --locked` shows no failure, so WD-1's `repo_def_read` and the existing suites still pass.

- [ ] **Step 7: commit.** Commit the four files in the Files line with the message `WD-2: shapes, allow-list validation and digest of .zygo/`.
