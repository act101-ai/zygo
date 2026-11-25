# WD-5 — Config keys: pane `job` and `pool`, `[workspace] pool`, `[limits] max_members`, `.zygo.toml` filter, `config check` rules

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§9.1 (pane keys and `config check` rules), R§9.3 (no job by default in a user loadout), R§9.4 (`max_members` 1-32, default 8; `.zygo.toml` cannot set it), R§9.5 (`[workspace] pool`; relative after `~` expansion is a `config check` error; no `pool` on `opencode`), R§13, R§14 rows W§11, §6.1 layer 3, §6.1 layer 4 and §13.5, R§18 item 12 (`.zygo.toml` part)
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/tests/definition_keys.rs` (new), `docs/specs/2026-11-20-workspaces-design.md` (W§11 amendment), `docs/specs/2026-11-18-zygo-design.md` (§6.1 layer 3, §6.1 layer 4 and §13.5 amendments)
**Consumes:** `LoadoutPane.name` (ML-1)
**Produces:** `LoadoutPane.job: Option<String>`, `LoadoutPane.pool: bool`, `WorkspaceConfig.pool: Option<String>`, `LimitsConfig.max_members: u32`, `schema::JOBS: [&str; 2]` (`["orchestrator", "worker"]`), `schema::MAX_MEMBERS_DEFAULT: u32` (8)
**Acceptance:** `cargo test -p zygo-config --test definition_keys && cargo test -p zygo-config --test config_layers` → all pass. The new tests cover: a `job` other than `orchestrator` or `worker`, two orchestrators in one loadout, `pool = true` on `opencode`, `pool` with `config_dir`, `[workspace] pool` relative after `~` expansion, and `max_members` of 0 and of 33 each give one `zygo config check` violation; a user pane with no `job` has `job = None`; `.zygo.toml` with `[limits] max_members = 30` gives the warning `.zygo.toml: limits.max_members ignored; only user or team config may set it` and the merged value stays the user value; the team layer can set `max_members`; the default is 8. The spec amendments in Files are checked in review, not by this command.

## Plan

Consumes (live): `LoadoutPane.name: Option<String>` (`crates/zygo-config/src/schema.rs:748-749`, ML-1), checked by `loadout_violations` (`crates/zygo-config/src/check.rs:195-212`). Produces (exact, after this node):
- `pub const JOBS: [&str; 2] = ["orchestrator", "worker"];` and `pub const MAX_MEMBERS_DEFAULT: u32 = 8;` (`schema.rs`, next to `LOADOUT_KINDS`).
- `LimitsConfig { pub subagent_max_turns: u32, pub max_members: u32 }`; `LimitsConfig::default() == LimitsConfig { subagent_max_turns: 0, max_members: 8 }`.
- `WorkspaceConfig.pool: Option<String>` (default `None`, skipped when `None`).
- `LoadoutPane.job: Option<String>` (after `name`, skipped when `None`) and `LoadoutPane.pool: bool` (after `config_dir`, default `false`).
- `#[doc(hidden)] pub fn load_with_repo_reader(ctx: &LoadContext, builtin: Table, read: impl FnOnce(&Path) -> RepoFile) -> Result<Merged, ConfigError>` in `crates/zygo-config/src/layers.rs`: the body of `load_with_builtin` with `read(repo)` in place of `read_default_branch(repo)` (`layers.rs:492`); `load_with_builtin(ctx, builtin)` becomes `load_with_repo_reader(ctx, builtin, read_default_branch)`. It is a test seam only: the `.zygo.toml` test runs with no git call and no time bound.

Steps:

- [x] 1. Create `crates/zygo-config/tests/definition_keys.rs`:
  ```rust
  use std::path::PathBuf;

  use zygo_config::layers::load_with_repo_reader;
  use zygo_config::*;
  use zygo_core::paths::Paths;

  const IGNORED: &str = ".zygo.toml: limits.max_members ignored; only user or team config may set it";
  const LO: &str = "[loadout.pair]\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\n";
  const PANE: &str = "[[loadout.pair.window.pane]]\n";
  const BAD_JOB: &str = "must be \"orchestrator\" or \"worker\"";

  #[must_use = "the directory is removed when this value drops; bind it"]
  struct Cx {
      _d: tempfile::TempDir,
      c: LoadContext,
  }

  fn ctx(user: &str, team: Option<&str>, repo: Option<PathBuf>) -> Cx {
      let d = tempfile::Builder::new().prefix("zygo-wd5-").tempdir().unwrap();
      let p = d.path();
      std::fs::write(p.join("config.toml"), user).unwrap();
      let paths = Paths {
          config_file: p.join("config.toml"),
          cache_dir: p.join("cache"),
          data_dir: p.join("data"),
          sessions_dir: p.join("cache/sessions"),
          marketplaces_dir: p.join("cache/marketplaces"),
          plugin_store: p.join("data/plugins"),
          approvals_file: p.join("data/approvals.toml"),
          usage_db: p.join("data/usage.db"),
      };
      Cx { c: LoadContext { paths, repo, team: team.map(|t| toml::from_str(t).unwrap()) }, _d: d }
  }

  fn loaded(user: &str) -> Merged {
      load(&ctx(user, None, None).c).unwrap()
  }

  /// The one `config check` violation of `user`, as `<severity> <path>: <message>`.
  fn one(user: &str) -> String {
      let v = check(&loaded(user).config);
      assert_eq!(v.len(), 1, "{v:?}");
      format!("{:?} {}", v[0].severity, v[0])
  }

  fn none(user: &str) {
      assert_eq!(check(&loaded(user).config), vec![]);
  }

  /// The merged config when the `.zygo.toml` read returns `repo` as found (no git call, no time bound).
  fn with_repo_file(user: &str, team: Option<&str>, repo: &str) -> Merged {
      let cx = ctx(user, team, Some(PathBuf::from("repo")));
      let table: toml::Table = toml::from_str(repo).unwrap();
      let builtin = toml::Table::try_from(builtin::defaults()).unwrap();
      load_with_repo_reader(&cx.c, builtin, |_| RepoFile::Found { branch: "main".into(), table, ignored: vec![] }).unwrap()
  }

  #[test]
  fn constants_and_defaults() { // R§9.3, R§9.4
      assert_eq!(JOBS, ["orchestrator", "worker"]);
      assert_eq!(MAX_MEMBERS_DEFAULT, 8);
      assert_eq!(LimitsConfig::default(), LimitsConfig { subagent_max_turns: 0, max_members: 8 });
      assert_eq!(WorkspaceConfig::default().pool, None);
      let p = LoadoutPane::default();
      assert_eq!((p.job, p.pool), (None, false));
      let m = loaded("");
      assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (8, Layer::Builtin));
      assert_eq!(check(&m.config), vec![]);
  }

  #[test]
  fn keys_parse_from_user_config() { // R§13
      let user = format!(
          "[workspace]\npool = \"~/.claude-interns\"\n[limits]\nmax_members = 12\n{LO}kind = \"claude-code\"\nname = \"elena\"\njob = \"worker\"\npool = true\n"
      );
      let m = loaded(&user);
      assert_eq!(m.warnings, Vec::<String>::new());
      assert_eq!(m.config.workspace.pool.as_deref(), Some("~/.claude-interns"));
      assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (12, Layer::User));
      let p = &m.config.loadout["pair"].window[0].pane[0];
      assert_eq!((p.name.as_deref(), p.job.as_deref(), p.pool), (Some("elena"), Some("worker"), true));
      assert_eq!(check(&m.config), vec![]);
  }

  #[test]
  fn user_pane_without_job_has_no_job() { // R§9.3 (lead: ruling 9)
      let m = loaded(&format!("{LO}kind = \"claude-code\"\nname = \"elena\"\n"));
      let p = &m.config.loadout["pair"].window[0].pane[0];
      assert_eq!((p.job.as_deref(), p.pool), (None, false));
      assert_eq!(check(&m.config), vec![]);
  }

  #[test]
  fn check_job_is_orchestrator_or_worker() { // R§9.1
      let pane = |job: &str| format!("{LO}kind = \"claude-code\"\njob = \"{job}\"\n");
      none(&pane("orchestrator"));
      none(&pane("worker"));
      for bad in ["lead", "Worker", ""] {
          assert_eq!(one(&pane(bad)), format!("Error loadout.pair.window[0].pane[0].job: {BAD_JOB}"), "{bad:?}");
      }
  }

  #[test]
  fn check_one_orchestrator_per_loadout() { // R§9.1
      let orch = "kind = \"claude-code\"\njob = \"orchestrator\"\n";
      let two = format!("{LO}{orch}{PANE}kind = \"claude-code\"\njob = \"worker\"\n{PANE}{orch}");
      assert_eq!(one(&two), "Error loadout.pair.window[0].pane[2].job: only one orchestrator per loadout");
      let two_windows = format!("{LO}{orch}[[loadout.pair.window]]\n{PANE}{orch}");
      assert_eq!(one(&two_windows), "Error loadout.pair.window[1].pane[0].job: only one orchestrator per loadout");
      none(&format!("{LO}{orch}[loadout.other]\n[[loadout.other.window]]\n[[loadout.other.window.pane]]\n{orch}"));
  }

  #[test]
  fn check_pool_rules() { // R§9.1, R§9.5
      none(&format!("{LO}kind = \"claude-code\"\npool = true\n"));
      none(&format!("{LO}kind = \"claude-code\"\npool = false\nconfig_dir = \"~/.claude-work\"\n"));
      assert_eq!(
          one(&format!("{LO}kind = \"opencode\"\npool = true\n")),
          "Error loadout.pair.window[0].pane[0].pool: only with kind = \"claude-code\""
      );
      assert_eq!(
          one(&format!("{LO}kind = \"claude-code\"\npool = true\nconfig_dir = \"~/.claude-work\"\n")),
          "Error loadout.pair.window[0].pane[0].pool: not with config_dir"
      );
  }

  #[test]
  fn check_workspace_pool_absolute_after_tilde() { // R§9.5
      let w = |p: &str| format!("[workspace]\npool = '{p}'\n");
      for ok in ["~", "~/.claude-interns"] {
          none(&w(ok));
      }
      none(&w(&std::env::temp_dir().display().to_string()));
      for bad in ["interns", "./interns", "~interns", ""] {
          assert_eq!(one(&w(bad)), "Error workspace.pool: must be absolute after ~ expansion", "{bad:?}");
      }
  }

  #[test]
  fn check_max_members_is_1_to_32() { // R§9.4
      for ok in [1, 8, 32] {
          none(&format!("[limits]\nmax_members = {ok}\n"));
      }
      for bad in [0, 33] {
          assert_eq!(one(&format!("[limits]\nmax_members = {bad}\n")), "Error limits.max_members: must be 1-32", "{bad}");
      }
  }

  #[test]
  fn repo_file_cannot_set_max_members() { // R§9.4, R§18 item 12 (.zygo.toml part)
      let repo = "version = 1\n[limits]\nmax_members = 30\nsubagent_max_turns = 12\n";
      let m = with_repo_file("[limits]\nmax_members = 4\n", None, repo);
      assert_eq!(m.warnings, [IGNORED]);
      assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (4, Layer::User));
      assert_eq!((m.config.limits.subagent_max_turns, m.source_of("limits.subagent_max_turns")), (12, Layer::Repository));
      let m = with_repo_file("", None, repo);
      assert_eq!(m.warnings, [IGNORED]);
      assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (8, Layer::Builtin));
      assert_eq!(with_repo_file("", None, "[limits]\nsubagent_max_turns = 12\n").warnings, Vec::<String>::new());
  }

  #[test]
  fn team_layer_sets_max_members() { // R§9.4
      let m = load(&ctx("", Some("[limits]\nmax_members = 12\n"), None).c).unwrap();
      assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (12, Layer::Team));
      assert_eq!(m.warnings, Vec::<String>::new());
      assert!(m.team_keys.contains("limits.max_members"));
      let m = with_repo_file("", Some("[limits]\nmax_members = 12\n"), "[limits]\nmax_members = 30\n");
      assert_eq!(m.config.limits.max_members, 12);
      assert_eq!(m.warnings, [IGNORED]);
  }
  ```
- [x] 2. Run `cargo test -p zygo-config --locked --test definition_keys`. Fail looks like: compile errors `cannot find value JOBS`, `no field job on type LoadoutPane`, `cannot find function load_with_repo_reader`.
- [x] 3. In `crates/zygo-config/src/schema.rs`:
  - After `LOADOUT_KINDS` (`:702`): `/// The values of loadout.<name>.window.pane.job (workspace definition design R§9.3); config check refuses any other value.` `pub const JOBS: [&str; 2] = ["orchestrator", "worker"];` and `/// [limits] max_members when no layer sets it (R§9.4).` `pub const MAX_MEMBERS_DEFAULT: u32 = 8;`.
  - `LimitsConfig` (`:603-608`): drop `Default` from the derive list; add field `/// The most agent panes a repository loadout may start, 1-32 (R§9.4); built-in, team and user layers only.` `pub max_members: u32`; add `impl Default for LimitsConfig { fn default() -> Self { LimitsConfig { subagent_max_turns: 0, max_members: MAX_MEMBERS_DEFAULT } } }`.
  - `WorkspaceConfig` (`:705-714`, `Default` at `:716-720`): field `/// The config-directory pool (R§9.5); \`~\` expanded as --config-dir does; absolute after that.` `#[serde(skip_serializing_if = "Option::is_none")] pub pool: Option<String>` after `drain_timeout`; `pool: None` in `Default`.
  - `LoadoutPane` (`:743-766`): after `name`, `/// One of [\`JOBS\`] (R§9.3); absent: no job.` `#[serde(skip_serializing_if = "Option::is_none")] pub job: Option<String>`; after `config_dir`, `/// The config directory from [workspace] pool (R§9.5). Only with kind = "claude-code"; not with config_dir.` `pub pool: bool`.
- [x] 4. In `crates/zygo-config/src/layers.rs`:
  - Rename the body of `load_with_builtin` (`:429-533`) to `#[doc(hidden)] pub fn load_with_repo_reader(ctx: &LoadContext, mut builtin: Table, read: impl FnOnce(&Path) -> RepoFile) -> Result<Merged, ConfigError>`; at `:492` write `m.repo_file = read(repo);`. `load_with_builtin` keeps its signature and `#[doc(hidden)]` and returns `load_with_repo_reader(ctx, builtin, read_default_branch)`.
  - In the `.zygo.toml` filter, after the `guards.min_level` block (`:505-510`), the same pattern for `limits`:
    ```rust
    if let Some(l) = t.get_mut("limits").and_then(Value::as_table_mut)
        && l.remove("max_members").is_some()
    {
        m.warnings.push(".zygo.toml: limits.max_members ignored; only user or team config may set it".into());
    }
    ```
- [x] 5. In `crates/zygo-config/src/check.rs`:
  - Import `JOBS` from `crate::schema`.
  - In `check`, after the `statusline.style` rule (`:107-110`): `if !(1..=32).contains(&cfg.limits.max_members) { e("limits.max_members".into(), "must be 1-32".into()); }`.
  - In `loadout_violations`, after the `default_loadout` rule (`:163-166`): `if cfg.workspace.pool.as_deref().is_some_and(|p| !absolute_after_tilde(p)) { v.push((Severity::Error, "workspace.pool".into(), "must be absolute after ~ expansion".into())); }`, with the private helper `fn absolute_after_tilde(p: &str) -> bool { p == "~" || p.starts_with("~/") || std::path::Path::new(p).is_absolute() }`. It reads the text only, as `--config-dir` expands `~` and `~/` (`crates/zygo/src/cmd/claude.rs:66-79`); no home lookup, no file access.
  - Per loadout, next to `seen` (`:172`): `let mut orchestrator = false;`. In the pane loop, after the `config_dir` rule (`:188-190`), in this order:
    - `job` set and not in `JOBS`: `({pp}.job, "must be \"orchestrator\" or \"worker\"")`, the text built from `let [orchestrator, worker] = JOBS;` as the `kind` message is built from `LOADOUT_KINDS`.
    - `job == Some("orchestrator")` and `orchestrator` already true: `({pp}.job, "only one orchestrator per loadout")`; else set `orchestrator = true` for an orchestrator pane.
    - `p.pool && !claude`: `({pp}.pool, CLAUDE_ONLY)`.
    - `p.pool && p.config_dir.is_some()`: `({pp}.pool, "not with config_dir")`.
- [x] 6. Run `cargo test -p zygo-config --locked --test definition_keys`. Pass: 10 passed, 0 failed, 0 ignored.
- [x] 7. Fix the full struct literals that the new fields break (see Drift): in `crates/zygo-config/tests/config_layers.rs` add `pool: None` to the `WorkspaceConfig { … }` literal at `:1587`, add `job: None` and `pool: false` to the `LoadoutPane { … }` literals at `:1596` and `:1612-1616`; in `crates/zygo-workspace/tests/loadout.rs:46-50` add `job: None` and `pool: false`. No assertion changes.
- [x] 8. Run `cargo test -p zygo-config --locked --test config_layers` and `cargo test -p zygo-workspace --locked --test loadout`. Pass: 0 failed in each (the `#[ignore]` timing tests stay ignored).
- [x] 9. Rewrite W§11 in `docs/specs/2026-11-20-workspaces-design.md` (`:516-550`) as final text. Other lines stay byte for byte; these change:
  - In the TOML block, after `drain_timeout = "10m"           # AU4; <n>s | <n>m | <n>h` add the line `pool = "~/.claude-interns"       # optional; config-directory pool (workspace definition design R§9.5); absolute after ~ expansion`. After `kind = "claude-code"            # claude-code | opencode | shell` add `job = "worker"                  # orchestrator | worker; optional; absent: no job (R§9.3)`. After `config_dir = "~/.claude-work"   # claude-code only; optional; as \`--config-dir\`` add `pool = false                    # claude-code only; optional; not with config_dir (R§9.5)`.
  - After the bullet `- Agent pane fields map to \`LaunchOpts\` …` add the bullet: ``- `job` and `pool` (workspace definition design R§9.3, R§9.5). A pane with no `job` has no job; zygo does not guess one. A pane with `pool = true` takes its config directory from `[workspace] pool` at a fresh build and runs in passthrough; with no `[workspace] pool` it starts as any other pane. The pane key `name` is defined in the workspace mail design (M§6).``
  - In the `zygo config check` bullet, the `check.rs` clause becomes: ``a known `kind`; `passthrough`, `config_dir` and `pool` only with `claude-code`; `pool` and `config_dir` not both set; `job` is `orchestrator` or `worker`; at most one pane with `job = "orchestrator"` per loadout; at least one pane per window; `default_loadout` names a loadout; `[workspace] pool` absolute after the `~` expansion; a missing `profile` is a warning (the launch continues without it, as today).`` The rest of the bullet does not change.
  - Append to its `## Change log` table: ``| 2026-11-24 | W§11 | operator for the keys (workspace definition design R§9.3, R§9.5); author's call for the config check rules (R§9.1, R§9.5); node WD-5 | Workspace definition design R§14 row W§11: pane keys `job` and `pool`, key `[workspace] pool`, and their `config check` rules. |``
- [x] 10. Rewrite §6.1 layer 3 in `docs/specs/2026-11-18-zygo-design.md` (`:109`) as: ``3. **User config** `~/.config/zygo/config.toml` (Windows `%APPDATA%\zygo\config.toml`; `ZYGO_CONFIG` overrides the path). It is the only layer, with the built-in defaults, that sets `[workspace]` (`default_loadout`, default `solo`, the built-in loadout of one Claude Code pane; `drain_block_dispatch`, default `true`; `drain_timeout`, default `10m`; `pool`, the config-directory pool, no default, absolute after `~` expansion, whose members run in passthrough) and `[loadout.<name>]` (windows with a layout and agent or shell panes) (workspaces design W§11; workspace definition design R§9.5). With the built-in defaults and the team config, it sets `[limits] max_members`, the most agent panes a repository loadout may start, 1-32, default 8 (workspace definition design R§9.4).`` Append to `## Change log`: ``| 2026-11-24 | §6.1 layer 3 | operator (workspace definition design R§9.4, R§9.5: user config holds the pool and the maximum); author's call for the team layer (R§9.4); node WD-5 | `[workspace] pool` and `[limits] max_members` are user config keys. |``
- [x] 11. Rewrite §6.1 layer 4 (`:110`) as: ``4. **Repository file** `.zygo.toml` at the repository root, read from the default branch (§13.5). It may set only `[guards]`, `[navigation]`, `[limits]` and `[work]`; any other table is ignored with one stderr line. It cannot set `limits.max_members`: the load removes it with one stderr line, `.zygo.toml: limits.max_members ignored; only user or team config may set it` (workspace definition design R§9.4).`` Append: ``| 2026-11-24 | §6.1 layer 4 | operator (workspace definition design R§9.4: a repository file cannot exceed the user maximum); author's call for the filter; node WD-5 | `.zygo.toml` cannot set `limits.max_members`; its four tables do not change. |``
- [x] 12. Rewrite the last paragraph of §13.5 (`:688`) as: ``Per guard: the file's level, else the config default; then the higher of that and `min_level`. Turn cap in force: the lower non-zero of the file's and the config's `[limits] subagent_max_turns` (user or team config, default 0). Member maximum: `[limits] max_members` comes from the built-in, team and user layers only; the load removes it from the file with the line `.zygo.toml: limits.max_members ignored; only user or team config may set it` (workspace definition design R§9.4). An invalid file is ignored with one stderr line; `zygo doctor` and `zygo status` print `repo policy: <root>@<branch> (claims enforce, …)`, `defaults` or `invalid (<why>)`.`` The TOML example does not change. Append: ``| 2026-11-24 | §13.5 | operator (workspace definition design R§9.4); author's call for the filter; node WD-5 | The repository file cannot set `limits.max_members`. |``
- [x] 13. Final verification: `cargo test -p zygo-config --locked --test definition_keys && cargo test -p zygo-config --locked --test config_layers && cargo test -p zygo-workspace --locked --test loadout && cargo build --workspace --locked`. Pass: exit 0, `0 failed` in each test result line, the build ends with `Finished`.
- [x] 14. Commit the changed files (`crates/zygo-config/src/{schema,layers,check}.rs`, `crates/zygo-config/tests/definition_keys.rs`, `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-workspace/tests/loadout.rs`, the two spec files) as `WD-5: config keys job, pool, [workspace] pool, [limits] max_members`, with trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:WD-5`, each on its own line, no Claude attribution.

Drift and author's calls:
- Spec line cites in R§9.4 and R§16 have moved: `LimitsConfig` is at `schema.rs:603-608` (cited `:599-602`), `WorkspaceConfig` at `:705-714` (cited `:685-698`), `LoadoutPane` at `:743-766` (cited `:722-741`); the `guards.min_level` filter is at `layers.rs:505-510` (cited `:505-509`). The code is as the spec describes; only the numbers moved.
- `LimitsConfig` derives `Default` (`schema.rs:603`); a default of 8 needs a hand `impl Default`, as `WorkspaceConfig` has. The built-in layer is `Config::default()` serialized (`builtin.rs:30-37`, `layers.rs:420-422`), so `limits.max_members = 8` shows as `# builtin` with no other change.
- Files gap: `crates/zygo-config/tests/config_layers.rs` (`:1587`, `:1596`, `:1612-1616`) and `crates/zygo-workspace/tests/loadout.rs` (`:46-50`) build `WorkspaceConfig` and `LoadoutPane` with every field and no `..`; the new fields break their compile, and the Acceptance runs `config_layers`. Step 7 adds the new fields there and nothing else.
- Every existing `.zygo.toml` test in `config_layers.rs` is `#[ignore = "timing: …"]` because the read runs git under the 2 s `GIT_BOUND` (graph rule: no wall-clock test in CI). The `max_members` case must run in CI, so step 4 adds the `load_with_repo_reader` seam and the test passes a `RepoFile::Found` to it. Production calls the same body with `read_default_branch`.
- `[workspace] pool` "relative after the `~` expansion" is checked on the text: `~` and `~/…` expand to the home directory, which is absolute (`claude.rs:71`, `.filter(|h| h.is_absolute())`); any other value must be an absolute path. A missing home is a `zygo up` failure (R§9.5), not a `config check` rule.
- The violation texts `must be 1-32`, `only one orchestrator per loadout`, `not with config_dir`, `must be absolute after ~ expansion` are the builder's (R§17, "the text of lines this spec does not fix"); the `job` and `pool`-kind texts reuse the existing `kind` and `CLAUDE_ONLY` forms.
- The W§11 TOML example gets `job` and `pool` but not `name`: R§14 says `name` is P3-1's (M§6), and ML-1 did not add it to W§11. The new bullet names M§6 for it.
- Spec gap, out of Files: `zygo loadout show` and `zygo config show` scrub each loadout text field through `hide_secrets` (`crates/zygo/src/cmd/loadout.rs:319-345`, W§12). After this node `job` is a new text field that it does not scrub. Its value is user text, but `config check` refuses any value other than `orchestrator` or `worker`. It is reported to the lead, not planned here.
