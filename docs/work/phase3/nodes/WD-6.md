# WD-6 — The repository loadout from `[members]`, with the default set

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§9.1 (one window, no name, layout rule, pane order, pane fields), R§9.2 (default set; profile files without `workspace.toml` give no loadout), R§9.3 effect 1, R§18 item 10 (the plan shape)
**Files:** `crates/zygo-config/src/repo_def/mod.rs`, `crates/zygo-config/src/repo_def/loadout.rs` (new), `crates/zygo-config/tests/repo_def_loadout.rs` (new)
**Consumes:** `Definition` (WD-2), `MemberDef` (WD-2), `LoadoutPane.job` (WD-5), `LoadoutPane.pool` (WD-5), `LoadoutPane.name` (ML-1)
**Produces:** `repo_def::REPOSITORY_LOADOUT: &str` (`"repository"`), `repo_def::repository_loadout(def: &Definition) -> Option<Loadout>`
**Acceptance:** `cargo test -p zygo-config --test repo_def_loadout` → all pass. The tests cover: `version = 1` alone gives one window with `name = None`, layout `main-vertical`, and five `claude-code` panes named `orchestrator`, `worker-1`, `worker-2`, `worker-3`, `worker-4` in that order, jobs `orchestrator` then `worker`, each `pool = true`, no `profile`, no `cwd`; members `b`, `a` and `boss` (orchestrator) give pane order `boss`, `a`, `b` and layout `main-vertical`; no orchestrator gives `tiled`; a set `layout` wins; a member with no `job` gets `worker`; every other `LoadoutPane` field is at its default (`args` empty, `yolo` and `passthrough` false, `server`, `compact` and `config_dir` none); profile files and no workspace file give `None`

## Plan

### Interfaces

Consumed (live signatures):
- `Definition` — `crates/zygo-config/src/repo_def/shape.rs:11`: `pub struct Definition { pub workspace: Option<WorkspaceFile>, pub profiles: BTreeMap<String, RepoProfile> }`; `WorkspaceFile` (`shape.rs:18`): `{ pub version: u32, pub layout: Option<String>, pub members: BTreeMap<String, MemberDef> }`.
- `MemberDef` — `shape.rs:26`: `{ pub kind: Option<String>, pub job: Option<String>, pub profile: Option<String>, pub pool: bool, pub cwd: Option<String> }`. `kind` and `job` hold the file's value; the defaults (`claude-code`, `worker`) are applied here.
- `LoadoutPane.job: Option<String>`, `LoadoutPane.pool: bool`, `LoadoutPane.name: Option<String>` — `crates/zygo-config/src/schema.rs:860` (`LoadoutPane` derives `Default`; `kind: String`). Also used, unchanged: `Loadout { pub window: Vec<LoadoutWindow> }` (`schema.rs:841`) and `LoadoutWindow { pub name: Option<String>, pub layout: Option<String>, pub pane: Vec<LoadoutPane> }` (`schema.rs:848`), all re-exported at `zygo_config::*` (`lib.rs:19`).

Produced (consumers: WD-7, WD-10, WD-11, WD-15, WD-17 call them as `zygo_config::repo_def::…`):
- `pub const REPOSITORY_LOADOUT: &str = "repository";`
- `pub fn repository_loadout(def: &Definition) -> Option<Loadout>`

### Steps

- [x] **1. Write the failing tests** in `crates/zygo-config/tests/repo_def_loadout.rs` (new). Header `//! WD-6: the repository loadout from [members], with the default set (R§9.1, R§9.2, R§9.3 effect 1, R§18 item 10).` Helpers build the `Definition` through the real parse:
  ```rust
  use std::collections::BTreeMap;
  use zygo_config::repo_def::{self, Definition, Files, REPOSITORY_LOADOUT};
  use zygo_config::LoadoutPane;

  fn def(ws: Option<&str>, profiles: &[(&str, &str)]) -> Definition {
      repo_def::parse(&Files {
          full_ref: Some("refs/remotes/origin/main".into()),
          commit: "a".repeat(40),
          workspace: ws.map(|t| t.as_bytes().to_vec()),
          profiles: profiles.iter().map(|(n, t)| (n.to_string(), t.as_bytes().to_vec())).collect::<BTreeMap<_, _>>(),
          ignored: vec![],
      })
      .unwrap()
  }
  fn window(ws: &str) -> zygo_config::LoadoutWindow {
      let l = repo_def::repository_loadout(&def(Some(ws), &[])).unwrap();
      assert_eq!(l.window.len(), 1);
      l.window.into_iter().next().unwrap()
  }
  fn names(w: &zygo_config::LoadoutWindow) -> Vec<&str> { w.pane.iter().map(|p| p.name.as_deref().unwrap()).collect() }
  fn agent(name: &str, job: &str, pool: bool) -> LoadoutPane {
      LoadoutPane { kind: "claude-code".into(), name: Some(name.into()), job: Some(job.into()), pool, ..LoadoutPane::default() }
  }
  ```
  Tests, one per Acceptance case:
  - `name_is_repository`: `assert_eq!(REPOSITORY_LOADOUT, "repository");`
  - `default_set_from_version_alone`: for each of `"version = 1\n"` and `"version = 1\n[members]\n"`: `let w = window(text);` `assert_eq!(w.name, None);` `assert_eq!(w.layout.as_deref(), Some("main-vertical"));` `assert_eq!(w.pane, vec![agent("orchestrator", "orchestrator", true), agent("worker-1", "worker", true), agent("worker-2", "worker", true), agent("worker-3", "worker", true), agent("worker-4", "worker", true)]);` (full-struct equality also proves no `profile`, no `cwd`).
  - `orchestrator_first_then_byte_order`: `let w = window("version = 1\n[members.b]\n[members.a]\n[members.boss]\njob = \"orchestrator\"\n");` `assert_eq!(names(&w), ["boss", "a", "b"]);` `assert_eq!(w.layout.as_deref(), Some("main-vertical"));` `assert_eq!(w.name, None);`
  - `no_orchestrator_gives_tiled`: `let w = window("version = 1\n[members.b]\n[members.a]\n");` `assert_eq!(names(&w), ["a", "b"]);` `assert_eq!(w.layout.as_deref(), Some("tiled"));`
  - `set_layout_wins`: `window("version = 1\nlayout = \"even-horizontal\"\n[members.boss]\njob = \"orchestrator\"\n").layout.as_deref() == Some("even-horizontal")`; `window("version = 1\nlayout = \"main-horizontal\"\n[members.a]\n").layout.as_deref() == Some("main-horizontal")`; `window("version = 1\nlayout = \"tiled\"\n").layout.as_deref() == Some("tiled")` (default set with a set layout).
  - `member_without_job_is_worker`: `let w = window("version = 1\n[members.a]\n");` `assert_eq!(w.pane[0].job.as_deref(), Some("worker"));`
  - `member_fields_and_unset_defaults`: `let w = window("version = 1\n[members.a]\nkind = \"opencode\"\nprofile = \"rust\"\ncwd = \"branch:feature-x\"\n[members.b]\npool = true\ncwd = \"src/app\"\n");` then `assert_eq!(w.pane, vec![LoadoutPane { kind: "opencode".into(), name: Some("a".into()), job: Some("worker".into()), profile: Some("rust".into()), cwd: Some("branch:feature-x".into()), ..LoadoutPane::default() }, LoadoutPane { kind: "claude-code".into(), name: Some("b".into()), job: Some("worker".into()), pool: true, cwd: Some("src/app".into()), ..LoadoutPane::default() }]);` and for each pane: `assert!(p.args.is_empty()); assert!(!p.yolo); assert!(!p.passthrough); assert_eq!(p.server, None); assert_eq!(p.compact, None); assert_eq!(p.config_dir, None);`
  - `no_workspace_file_gives_none`: `assert_eq!(repo_def::repository_loadout(&def(None, &[("rust", "disabled = [\"design\"]\n")])), None);` `assert_eq!(repo_def::repository_loadout(&def(None, &[])), None);`
- [x] **2. Run and see them fail:** `cargo test -p zygo-config --test repo_def_loadout` fails to compile with unresolved `repo_def::REPOSITORY_LOADOUT` and `repo_def::repository_loadout`.
- [x] **3. Implement** `crates/zygo-config/src/repo_def/loadout.rs` (new), doc line `//! The repository loadout from [members] (R§9.1, R§9.2, R§9.3 effect 1).`:
  - `pub const REPOSITORY_LOADOUT: &str = "repository";`
  - `pub fn repository_loadout(def: &Definition) -> Option<Loadout>`. Body rules (spec-fixed): `def.workspace` is `None` → `None`. Members are `ws.members`, or, when that map is empty, the default set: `orchestrator` (`job = "orchestrator"`) and `worker-1` … `worker-4` (`job = "worker"`), each `kind = "claude-code"`, `pool = true`, no `profile`, no `cwd`. Each member gives one `LoadoutPane { kind: m.kind or "claude-code", name: Some(name), job: Some(m.job or "worker"), profile: m.profile, pool: m.pool, cwd: m.cwd, ..LoadoutPane::default() }`. Order: the pane with job `orchestrator` first, then the others in `BTreeMap` order (byte order of `String`). One `LoadoutWindow { name: None, layout: ws.layout, or Some("main-vertical") when a pane has job `orchestrator`, else Some("tiled"), pane }`.
  - In `crates/zygo-config/src/repo_def/mod.rs`: add `mod loadout;` and `pub use loadout::{repository_loadout, REPOSITORY_LOADOUT};`, and add "the repository loadout (R§9.1-R§9.2)" to the module doc line.
- [x] **4. Run and see pass:** `cargo test -p zygo-config --test repo_def_loadout` → 8 passed; `cargo test -p zygo-config` → all pass; `cargo clippy -p zygo-config --lib --test repo_def_loadout -- -D warnings` clean. `cargo clippy -p zygo-config --all-targets -- -D warnings` and `cargo fmt --check -p zygo-config` fail on the base (clippy `collapsible_if` at `crates/zygo-config/tests/repo_def_check.rs:196`; no `rustfmt.toml`, so rustfmt reports crate-wide diffs against the 120-column hand style) and were not used as the gate. Review round 2 added four tests (the loadout check through `zygo_config::check`, one member replaces the default set, byte order, an opencode orchestrator): 12 passed.
- [x] **5. Commit** the three files with message `WD-6: repository loadout from [members], with the default set` and the session trailers.
