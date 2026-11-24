# DR-8 — Session view `repo_root`, written by the launcher

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§13 item 6
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-launch/tests/launcher.rs`
**Consumes:** —
**Produces:** `zygo_core::session_view::SessionView.repo_root: Option<String>` (`#[serde(default)]`; the repository top level the launcher holds, `self.top`)
**Acceptance:** `test "$(cargo test -q -p zygo-core --test core_basics -- repo_root --list 2>/dev/null | grep -c ': test$')" -ge 2 && test "$(cargo test -q -p zygo-launch --test launcher -- repo_root --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-core --locked --test core_basics repo_root && cargo test -p zygo-launch --locked --test launcher repo_root` → every count holds (at least 2 named `repo_root…` in zygo-core `core_basics`; at least 2 named `repo_root…` in zygo-launch `launcher`; a count of 0 fails before any run), then all pass, with at least two tests run: a `view.json` without `repo_root` loads as `None` and a view with it round-trips; a launch in a repository writes the top level into `view.json`, a launch outside one writes `None`.

## Plan

Spec: D§13 item 6 of `docs/specs/2026-11-24-decision-records-design.md`. DR-13 runs git in `repo_root` and gives `cannot check: no repository root` when it is `None`.

Live code (at `8b33d38`): `SessionView` `crates/zygo-core/src/session_view.rs:7-43`, its `Default` `:46-72`; every other `SessionView { … }` literal has a `..` base. The launcher holds `top: Option<PathBuf>` (`crates/zygo-launch/src/launcher.rs:220`), set at `:258` by `checkout(&opts.cwd, Instant::now() + BOUND)` (`BOUND` = 2 s, `:41`; `top` is canonical, `crates/zygo-work/src/repo.rs:204`), and passes it as `LaunchCtx.repo_root` (`:633`). View literal `:584-606`, saved at `:607`. Every launcher test that reads `view.json` today is `#[ignore]` (timing: the stub child's 10 s status call, or the 2 s `checkout`). The new tests read the file in the stub's `build` (`ctx.session_dir`, after the save) and stop at a preflight refusal, as the CI test `failed_preflight_exits_2_and_starts_nothing` does.

Author's calls:
1. **Test hook for the top level.** The operator rule bars CI tests on a fixed production bound without a test hook, and bars widening the bound. `LaunchOpts` gains a `test-hooks` field `top`, read by `launch_top` in the pattern of `heartbeat_every` (`launcher.rs:812`). The outside test uses the real `checkout`: its result is `None` whether git ends in time or not.
2. **Non-UTF-8 top level gives `None`** (`Path::to_str`), so DR-13 reports no root; a lossy string could name another directory.
3. **`None` is written as `null`**, as `workspace` is (`session_view.rs:36-37`); the spec gives `#[serde(default)]` only.

Consumes: —

Produces:

```rust
// crates/zygo-core/src/session_view.rs, in `pub struct SessionView`, after `workspace`, before `extra`:
/// The repository top level of the launch directory, where `work_verdict` runs git (decision records D§13 item 6);
/// `None` outside a repository, for a non-UTF-8 path, and for a `view.json` without the field (an older launcher).
#[serde(default)]
pub repo_root: Option<String>,
```

Consumer checked: DR-13 (`docs/work/phase3/nodes/DR-13.md:5`, `SessionView.repo_root (DR-8)`). ML-1, ML-7 and ML-9 depend on DR-8 for the shared files only; ML-9 adds `SessionView.mail` next to this field in the same two places.

### Steps

- [x] **1. Write the failing core tests.** Append to `crates/zygo-core/tests/core_basics.rs`:

```rust
// ---- DR-8: session view `repo_root` (decision records D§13 item 6) ----

#[test]
fn repo_root_absent_from_view_json_reads_as_none() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    std::fs::write(d.view_json(), br#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":true,"results":true,"navigation":false}"#).unwrap();
    let v = SessionView::load(&d).unwrap();
    assert_eq!(v.repo_root, None);
    assert_eq!(v.extra, Default::default());
    assert_eq!(SessionView::default().repo_root, None);
}

#[test]
fn repo_root_round_trips_through_save_and_load() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let v = SessionView { session: "s".into(), repo_root: Some("/r/top".into()), ..Default::default() };
    v.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), v);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(raw["repo_root"], "/r/top");
}
```

- [x] **2. See them fail.** `cargo test -p zygo-core --locked --test core_basics repo_root` → compile error `no field `repo_root``.
- [x] **3. Add the field.** In `crates/zygo-core/src/session_view.rs`: the field of Produces, and `repo_root: None,` in `impl Default for SessionView` after `workspace: None,`.
- [x] **4. See them pass.** `cargo test -p zygo-core --locked --test core_basics` → all pass, the 2 new tests among them; `session_view_default_equals_the_serde_defaults` still passes.
- [x] **5. Write the failing launcher tests.** In `crates/zygo-launch/tests/launcher.rs`: add `view: Option<SessionView>,` to `struct Seen`, and `view: SessionView::load(&ctx.session_dir).ok(),` to the `Seen { … }` push in `Stub::build` (the file as the launcher wrote it). Then add:

```rust
// ---- DR-8: the launcher writes `repo_root` into view.json (decision records D§13 item 6) ----
// The preflight refusal stops the launch after `build`: no child, no status call, no timing (as
// `failed_preflight_exits_2_and_starts_nothing`).

fn refused(fx: &Fx) -> Stub {
    Stub { preflight: Some(Preflight { file: fx.root.join("settings.json"), key: "env.ANTHROPIC_API_KEY".into() }), ..stub() }
}

#[test]
fn repo_root_is_the_top_the_launcher_holds() {
    let fx = fx();
    let st = refused(&fx);
    let mut o = opts(&fx, "dump", &[]);
    o.top = Some(Some(fx.cwd.clone())); // the test hook in place of the 2 s `checkout`
    let (code, err) = run(&st, o);
    assert_eq!(code, 2, "{err}");
    assert_eq!(st.last().view.expect("view.json was written").repo_root.as_deref(), Some(fx.cwd.to_str().unwrap()));
}

#[test]
fn repo_root_is_none_outside_a_repository() {
    // The real `checkout`: the temporary directory is in no repository; a git call past the bound also gives `None`.
    let fx = fx();
    let st = refused(&fx);
    let (code, err) = run(&st, opts(&fx, "dump", &[]));
    assert_eq!(code, 2, "{err}");
    assert_eq!(st.last().view.expect("view.json was written").repo_root, None);
}
```

- [x] **6. See them fail.** `cargo test -p zygo-launch --locked --test launcher repo_root` → compile error `no field `top` on type `LaunchOpts``.
- [x] **7. Add the hook and write the field.** In `crates/zygo-launch/src/launcher.rs`:
  - `LaunchOpts`, after `serve_bounds`, with the doc line "`Some`: the repository top level in place of the `checkout` call; `Some(None)`: no repository.": `#[cfg(feature = "test-hooks")] pub top: Option<Option<PathBuf>>,`; and `#[cfg(feature = "test-hooks")] top: None,` in `LaunchOpts::new`.
  - Next to `heartbeat_every`: `fn launch_top(#[allow(unused_variables)] opts: &LaunchOpts) -> Option<PathBuf>` — with `test-hooks`, `if let Some(t) = &opts.top { return t.clone(); }`; then `checkout(&opts.cwd, Instant::now() + BOUND).map(|c| c.top)`.
  - `:258`: `let top = launch_top(&opts);`.
  - In the view literal, after `workspace: …,`: `repo_root: self.top.as_deref().and_then(Path::to_str).map(String::from),`.
- [x] **8. See them pass.** `cargo test -p zygo-launch --locked --test launcher repo_root` → `2 passed`, `0 ignored`; then `cargo test -p zygo-launch --locked --test launcher` → no test that passed before fails.
- [x] **9. Acceptance.** Run the node's Acceptance command (header) → exit 0; both counts are 2; both runs pass with 2 tests run each.
- [x] **10. Floor.** `cargo test --workspace --locked --no-fail-fast` → no failure (doc tests included: `session_view.rs:86-89` still compiles). No install script changes, so `sh scripts/test-install.sh` is not run. Review the diff: no new test spawns `claude`/`opencode` or reads a clock (review focus 75; no other review-focus line names DR-8).
- [x] **11. Commit.** Stage the four files of the header only; message `feat(work): session view repo_root, written by the launcher (DR-8)` with the trailers `Zygo-Session:` (the session id of the implementer's brief) and `Zygo-Work: phase3:DR-8:implement`, each on its own line at the end. No Claude attribution.
