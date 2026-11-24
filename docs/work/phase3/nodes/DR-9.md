# DR-9 — The `dispositions` guard key, default `enforce`

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§9.3 (default level `enforce`, added at `crates/zygo-config/src/schema.rs` beside the other work-guard defaults), D§13 items 2 and 3, D-C7
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-work/src/policy.rs`, `crates/zygo-work/tests/policy.rs`, `crates/zygo/src/cmd/guards.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo/tests/serve_cli.rs`, `crates/zygo-work/tests/collection.rs`
**Consumes:** —
**Produces:** `zygo_config::GUARDS` with `"dispositions"` (7 entries); `zygo_config::GuardsConfig.dispositions: Level` (default `Level::Enforce`; a valid `[guards]` key, clamped by `min_level`); `zygo_work::policy::Guard::Dispositions` (in `Guard::CONFIGURED`, name `"dispositions"`, level from `[guards]`)
**Acceptance:** `test "$(cargo test -q -p zygo-work --test policy -- dispositions --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-config -p zygo-work -p zygo --locked --no-fail-fast` → every count holds (at least 3 named `dispositions…` in zygo-work `policy`; a count of 0 fails before any run), then all pass, including a new test in `crates/zygo-work/tests/policy.rs` that the default policy gives `Guard::Dispositions` the level `enforce`, that `.zygo.toml` `[guards] dispositions = "notify"` gives `notify`, and that `min_level = "notify"` lifts a `monitor` setting to `notify`; the `describe` line and the `zygo guards` output list `dispositions enforce`.

## Plan

Consumes (live): — . Produces (exact, after this node):
- `pub const GUARDS: [&str; 7] = ["claims", "scope", "navigation", "output", "trailers", "repeat", "dispositions"];` (`crates/zygo-config/src/schema.rs`)
- `pub struct GuardsConfig { pub min_level: Level, pub claims: Level, pub scope: Level, pub navigation: Level, pub output: Level, pub repeat: Level, pub trailers: Level, pub dispositions: Level }`; `GuardsConfig::default().dispositions == Level::Enforce`; `get`/`set` accept `"dispositions"`. The `min_level` floor (`Merged::raise_to_floor`, `layers.rs:613`) and the session-override check (`with_overrides`, `layers.rs:729`) iterate `GUARDS`, so they need no change.
- `zygo_work::policy::Guard::Dispositions`; `Guard::CONFIGURED: [Guard; 7]` ending in `Guard::Dispositions`; `Guard::Dispositions.name() == "dispositions"`; `Policy::level(Guard::Dispositions)` takes the `_ => self.guards.get(g.name())` arm (`policy.rs:72`), so `level` needs no change.

Steps:

- [ ] 1. In `crates/zygo-work/tests/policy.rs`: import `RepoFile` from `zygo_config`; append `, dispositions enforce` to `DEFAULT_LEVELS` and to the `line(...)` level strings in `file_on_default_branch_sets_levels` and `session_override_is_shown_and_floor_refused`; in `guard_names_and_fixed_levels` add `(Guard::Dispositions, "dispositions")` and make the `CONFIGURED` array end in `"dispositions"`. Add:
  ```rust
  #[test]
  fn dispositions_defaults_to_enforce() {
      let r = repo(None, None);
      let mut m = merged("", r.path());
      assert_eq!(Policy::resolve(&m, r.path()).level(Guard::Dispositions), Level::Enforce);
      // describe prints levels only for a found file; set one without a git read (no timing bound)
      m.repo_file = RepoFile::Found { branch: "main".into(), table: Default::default(), ignored: vec![] };
      assert_eq!(Policy::resolve(&m, r.path()).describe(), line(r.path(), DEFAULT_LEVELS));
  }

  #[test]
  fn dispositions_is_a_guards_key() {
      let r = repo(None, None);
      assert_eq!(policy("[guards]\ndispositions = \"notify\"\n", r.path()).level(Guard::Dispositions), Level::Notify);
      let mut o = Overrides::default();
      o.guards.insert("dispositions".into(), "monitor".into());
      let p = Policy::resolve(&merged("", r.path()).with_overrides(&o).unwrap(), r.path());
      assert_eq!(p.level(Guard::Dispositions), Level::Monitor);
  }

  #[test]
  fn dispositions_min_level_lifts_monitor() {
      let r = repo(None, None);
      let user = "[guards]\nmin_level = \"notify\"\ndispositions = \"monitor\"\n";
      assert_eq!(policy(user, r.path()).level(Guard::Dispositions), Level::Notify);
      let mut o = Overrides::default();
      o.guards.insert("dispositions".into(), "monitor".into());
      assert_eq!(merged(user, r.path()).with_overrides(&o).unwrap_err(),
          Refusal::BelowFloor { guard: "dispositions".into(), min: Level::Notify });
  }

  #[test]
  #[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
  fn dispositions_from_repo_file() {
      let r = repo(Some("version = 1\n[guards]\ndispositions = \"notify\"\n"), None);
      let p = policy("", r.path());
      assert_eq!(p.level(Guard::Dispositions), Level::Notify);
      assert_eq!(p.describe(), line(r.path(),
          "claims monitor, scope monitor, navigation monitor, output monitor, trailers notify, repeat monitor, dispositions notify"));
  }
  ```
- [ ] 2. Run `cargo test -p zygo-work --test policy --locked`. Fail looks like: compile error `no variant ... named Dispositions`.
- [ ] 3. In `crates/zygo-config/src/schema.rs`: `GUARDS` as in Produces; field `pub dispositions: Level` after `trailers`; `dispositions: Level::Enforce` in `Default`; arm `"dispositions" => &mut self.dispositions` in `slot` and `"dispositions" => self.dispositions` in `get`. `config_layers.rs` `defaults_match_spec` checks only the six old keys and needs no change.
- [ ] 4. In `crates/zygo-work/src/policy.rs`: variant `Dispositions` after `Repeat` with doc `/// D§9.3: checked in work_verdict; default enforce.`; `CONFIGURED: [Guard; 7]` with `Guard::Dispositions` last; `Guard::Dispositions => "dispositions"` in `name`.
- [ ] 5. Run `cargo test -p zygo-work --test policy --locked`. Pass: all non-ignored tests pass, the three new CI tests included.
- [ ] 6. Update the fixed lists that now gain `dispositions`:
  - `crates/zygo/src/cmd/guards.rs` `lists_levels_with_sources`: insert `\ndispositions enforce (builtin)` after `repeat monitor (builtin)`; in `each_session_level_applies_on_its_own` and `team_floor_holds_over_user_config` the `min_level` line moves from `lines[6]` to `lines[7]`.
  - `crates/zygo/tests/slash_parity.rs:412`: the `now:` text ends `, repeat monitor, dispositions enforce`.
  - `crates/zygo/tests/serve_cli.rs:431`: `levels` ends `, repeat monitor, dispositions enforce)`; `:466`: the expected suffix ends `, repeat monitor, dispositions enforce)`.
  - `crates/zygo-work/tests/collection.rs:440-441` (`begin_writes_the_levels_in_force`, outside the node's Files; see Drift): add `("dispositions", "enforce")` to `want`.
- [ ] 7. Run `cargo test -p zygo --locked --lib cmd::guards` and `cargo test -p zygo-work --locked --test collection begin_writes_the_levels_in_force`. Pass: all selected tests pass.
- [ ] 8. On demand, local only (not CI): `cargo test -p zygo-work --test policy --locked -- --ignored` and `cargo test -p zygo --test serve_cli --locked -- --ignored status_reads_the_team_layer`. Pass: all pass (`dispositions_from_repo_file` shows the `.zygo.toml` key; the `describe` line lists `dispositions`).
- [ ] 9. Acceptance: `test "$(cargo test -q -p zygo-work --test policy -- dispositions --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-config -p zygo-work -p zygo --locked --no-fail-fast`. Pass: count is 4, exit 0, no failed test.
- [ ] 10. Floor: `cargo test --workspace --locked --no-fail-fast`. Pass: exit 0, `0 failed` in every result line.
- [ ] 11. Commit the changed files: `feat(guards): dispositions guard key, default enforce (DR-9)`, trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:DR-9:implement`, no Claude attribution.

Drift and author's calls:
- Files omits `crates/zygo-work/tests/collection.rs`: `begin_writes_the_levels_in_force` (`collection.rs:440-441`) compares the whole `Guard::CONFIGURED` levels map written by `hooks::levels` (`crates/zygo-work/src/hooks.rs:205`); a seventh guard breaks it. Step 6 edits that one expected map.
- The Acceptance names a test where `.zygo.toml` sets `dispositions = "notify"`. Reading `.zygo.toml` runs git under the 2 s `GIT_BOUND` (`crates/zygo-config/src/repo_file.rs:40`), and every such test here is `#[ignore = "timing: …"]` (commit `96c1cbc`; graph constraint "No wall-clock or deadline test runs in CI"). So `dispositions_from_repo_file` is ignored and runs in step 8. The CI tests prove the same `[guards]` key through the user layer and the session override, and the defaults `describe` line through a `RepoFile::Found` set on `Merged` (no git read).
- D§13 items 2 and 3 are base-spec text (§13.4 row and defaults sentence, §13.5 key). The text goes in DR-13 (its Files list `docs/specs/2026-11-18-zygo-design.md` §13.4, §13.5). DR-9 does the code half only.
- `GUARDS` gets `dispositions` as its last entry, so the order of `describe`, `zygo guards` and the `now:` line stays the same for the six old guards.
