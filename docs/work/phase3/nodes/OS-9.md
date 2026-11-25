# OS-9 — `[results] shape` in config and the session view

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§9, O§11 row "`[results] shape`"
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-core/src/session_view.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/tests/launcher.rs`, `crates/zygo-hooks/src/trim.rs`
**Consumes:** `trim::trim_in_with` (OS-4)
**Produces:** `ResultsConfig.shape: bool` (default `true`), `SessionView.shape: bool` (`#[serde(default = "default_true")]`); the launcher copies `c.results.shape` into the view
**Acceptance:** `test "$(cargo test -q -p zygo-config --test config_layers -- results_shape --list 2>/dev/null | grep -c ': test$')" -ge 2 && test "$(cargo test -q -p zygo-core --test core_basics -- view_shape --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-launch --test launcher -- view_shape --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-config --test config_layers && cargo test -p zygo-core --test core_basics && cargo test -p zygo-launch --test launcher && cargo test -p zygo-hooks --test hooks_basic` → every count holds (at least 2 named `results_shape…` in zygo-config `config_layers`; at least 1 named `view_shape…` in zygo-core `core_basics`; at least 1 named `view_shape…` in zygo-launch `launcher`; a count of 0 fails before any run), then all pass, including new tests: no `[results]` table gives `shape == true`; `shape = false` in the user file gives `false`; a `view.json` without `shape` reads `true`; a launch with `shape = false` writes `"shape": false` to the view

## Plan

Spec: O§9 and O§11 row "`[results] shape`" of `docs/specs/2026-11-24-output-shaping-design.md`. `[results] shape` is a user-config key, default `true`; the launcher copies it to the session view; the trim hook reads it. This node adds the key, the view field and the copy. It changes no hook behavior: the shaping pipeline that reads the flag comes in later nodes.

Live code (at `0bd0da71`, after OS-4, ML-9 and MS-3):

- `ResultsConfig` is at `crates/zygo-config/src/schema.rs:476-487` (`#[serde(default)]`, fields `trim_threshold_bytes: u64`, `head_lines: u32`, `tail_lines: u32`; `Default` at `:483-487`). The config is read by serde (`typed.rs`), so a new field needs no key list.
- `SessionView` is at `crates/zygo-core/src/session_view.rs:6-51`; its `Default` at `:54-82`; `fn default_true()` at `:104`. ML-9 added `mail` (`:43-45`, `:78`).
- The launcher view literal is at `crates/zygo-launch/src/launcher.rs:589-613` and has a `..Default::default()` base (`:612`). Without the copy line, the launch writes `"shape": true` with no compile error, so the launcher test is the only guard.
- Struct literals with no `..` base: `ResultsConfig` only at `schema.rs:485` (its `Default`) and `crates/zygo-hooks/src/trim.rs:126` (in `trim_in_with`); `SessionView` only at `session_view.rs:56` (its `Default`). Every other literal in the workspace has a base. So `trim.rs:126` must change, or the workspace does not build.
- Every launcher test that reads `view.json` after a full launch is `#[ignore]` (timing). The CI-run DR-8 tests (`launcher.rs:1548-1580`) read the view in the stub's `build` and stop at a preflight refusal (`refused`, `:1543-1545`; `o.top` test hook, `launcher.rs:160`). The new launcher test uses the same pattern.
- No test name in the workspace contains `results_shape` or `view_shape` now.

Drifted premises:

1. The spec cites `session_view.rs:20-29` for the copied fields; they are now `:19-29`, and ML-9 added `mail` after them. No effect on the plan.
2. The node's **Acceptance** line uses pipes and `&&`. The one-command form below runs the same four test targets in one cargo call. It cannot hold the name-count guard; the guard is met by the four test names in steps 1, 3 and 5 (2 `results_shape…`, 1 `view_shape…` in `core_basics`, 1 `view_shape…` in `launcher`).

Author's calls:

1. `trim.rs:126` takes `shape: sv.shape` (not `..ResultsConfig::default()`), so the `ResultsConfig` the hook builds holds the session's value. Nothing reads `cfg.shape` yet; hook output is unchanged.
2. `SessionView.shape` goes after `tail_lines`, next to the other `[results]` copies. `ResultsConfig.shape` goes after `tail_lines`.
3. No `overrides.json` key for `shape`: O§11 names user config, copied at launch, as the only source.
4. The launcher test checks `"shape": false` by a `SessionView` load in the stub. The serde default is `true`, so a `false` read proves the file holds `"shape": false`. The core test reads the raw JSON as well.

Consumes (exact, `crates/zygo-hooks/src/trim.rs:117`):

```rust
pub fn trim_in_with(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput>
```

Its `ResultsConfig` literal at `trim.rs:126` is the only line of it this node changes.

Produces:

```rust
// crates/zygo-config/src/schema.rs, in `pub struct ResultsConfig`, after `tail_lines`:
/// O§9: the shaping pipeline for Bash (O§7); `false` gives the behavior at `97d8767`.
pub shape: bool,
// its `Default`: ResultsConfig { trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, shape: true }

// crates/zygo-core/src/session_view.rs, in `pub struct SessionView`, after `tail_lines`:
/// `[results] shape` of the launch (output shaping O§9, O§11); `true` for a `view.json` without the field.
#[serde(default = "default_true")]
pub shape: bool,
// its `Default`: shape: default_true(),

// crates/zygo-launch/src/launcher.rs, in the `SessionView { … }` literal, after `tail_lines`:
shape: c.results.shape,
```

Consumers checked: OS-27 (shape-off baseline) and WD-13 depend on OS-9.

### Steps

- [x] **1. Write the failing config tests.** Append to `crates/zygo-config/tests/config_layers.rs`:

```rust
// ---- OS-9: `[results] shape` (output shaping O§9) ----

#[test]
fn results_shape_is_true_without_a_results_table() {
    assert!(Config::default().results.shape);
    assert!(cfg("").results.shape);
    let m = loaded("");
    assert!(m.config.results.shape);
    assert_eq!(m.source_of("results.shape"), Layer::Builtin);
}

#[test]
fn results_shape_false_in_the_user_file_reads_false() {
    let m = loaded("[results]\nshape = false\n");
    assert!(!m.config.results.shape);
    assert_eq!(m.source_of("results.shape"), Layer::User);
    let r = &m.config.results;
    assert_eq!((r.trim_threshold_bytes, r.head_lines, r.tail_lines), (16384, 60, 20));
}
```

  Check: `cargo test -p zygo-config --test config_layers results_shape` fails to compile (`no field shape on type ResultsConfig`).

- [x] **2. Add `ResultsConfig.shape`.** In `crates/zygo-config/src/schema.rs`: the field and `Default` as in **Produces**.

  Check: `cargo test -p zygo-config --test config_layers results_shape` passes 2 of 2. `cargo build -p zygo-hooks` fails at `trim.rs:126` (missing field `shape`); step 6 fixes it.

- [x] **3. Write the failing core test.** Append to `crates/zygo-core/tests/core_basics.rs`:

```rust
// ---- OS-9: session view `shape` (output shaping O§9, O§11) ----

#[test]
fn view_shape_absent_from_view_json_reads_true_and_false_round_trips() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    // a view.json written before the field existed
    std::fs::write(d.view_json(), br#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":true,"results":true,"navigation":false}"#).unwrap();
    let v = SessionView::load(&d).unwrap();
    assert!(v.shape);
    assert_eq!(v.extra, Default::default());
    assert!(SessionView::default().shape);
    SessionView { session: "s".into(), shape: false, ..Default::default() }.save(&d).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(raw["shape"], false);
    assert!(!SessionView::load(&d).unwrap().shape);
}
```

  Check: `cargo test -p zygo-core --test core_basics view_shape` fails to compile (`no field shape on type SessionView`).

- [x] **4. Add `SessionView.shape`.** In `crates/zygo-core/src/session_view.rs`: the field and the `Default` line as in **Produces**.

  Check: `cargo test -p zygo-core --test core_basics` passes, including `view_shape_absent_from_view_json_reads_true_and_false_round_trips` and the existing `session_view_default_equals_the_serde_defaults` (`Default` equals the serde defaults).

- [x] **5. Write the failing launcher test.** Append to `crates/zygo-launch/tests/launcher.rs` (no `#[ignore]`: the launch stops at the preflight refusal, as in the DR-8 tests):

```rust
// ---- OS-9: the launcher copies `[results] shape` into view.json (output shaping O§9, O§11) ----

#[test]
fn view_shape_of_the_user_config_reaches_the_view() {
    let fx = fx();
    let st = refused(&fx);
    let mut o = opts(&fx, "dump", &[]);
    o.top = Some(None); // the test hook in place of the 2 s `checkout`
    let (code, err) = run(&st, o);
    assert_eq!(code, 2, "{err}");
    assert!(st.last().view.expect("view.json was written").shape, "the default");
    fx.config("[results]\nshape = false\n");
    let st = refused(&fx);
    let mut o = opts(&fx, "dump", &[]);
    o.top = Some(None);
    let (code, err) = run(&st, o);
    assert_eq!(code, 2, "{err}");
    // The serde default is `true`: a `false` read means the file holds `"shape": false`.
    assert!(!st.last().view.expect("view.json was written").shape);
}
```

  Check: after step 6 builds `zygo-hooks`, `cargo test -p zygo-launch --test launcher view_shape` fails at the last assertion (the view literal's `..Default::default()` gives `true`).

- [x] **6. Fix the hook's `ResultsConfig` literal.** In `crates/zygo-hooks/src/trim.rs:126`, inside `trim_in_with`:

```rust
let cfg = ResultsConfig { trim_threshold_bytes: threshold, head_lines: sv.head_lines, tail_lines: sv.tail_lines, shape: sv.shape };
```

  Check: `cargo test -p zygo-hooks --test hooks_basic` passes with no change to any assertion (hook output unchanged).

- [x] **7. Copy the key in the launcher.** In `crates/zygo-launch/src/launcher.rs`, the `SessionView { … }` literal (`:589-613`): add `shape: c.results.shape,` after `tail_lines: c.results.tail_lines,`.

  Check: `cargo test -p zygo-launch --test launcher view_shape` passes 1 of 1.

- [x] **8. Whole build.** Check: `cargo build --workspace --all-targets --locked` succeeds (no other literal lacks `shape`).

### Acceptance

```
cargo test -p zygo-config -p zygo-core -p zygo-launch -p zygo-hooks --test config_layers --test core_basics --test launcher --test hooks_basic
```

All pass, including `results_shape_is_true_without_a_results_table`, `results_shape_false_in_the_user_file_reads_false`, `view_shape_absent_from_view_json_reads_true_and_false_round_trips` and `view_shape_of_the_user_config_reaches_the_view`. (Confirmed at `0bd0da71`: this one cargo call builds and runs exactly these four test targets.)
