# WD-6 — The repository loadout from `[members]`, with the default set

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§9.1 (one window, no name, layout rule, pane order, pane fields), R§9.2 (default set; profile files without `workspace.toml` give no loadout), R§9.3 effect 1, R§18 item 10 (the plan shape)
**Files:** `crates/zygo-config/src/repo_def/mod.rs`, `crates/zygo-config/src/repo_def/loadout.rs` (new), `crates/zygo-config/tests/repo_def_loadout.rs` (new)
**Consumes:** `Definition`, `MemberDef` (WD-2); `LoadoutPane.job`, `LoadoutPane.pool` (WD-5); `LoadoutPane.name` (ML: M§6)
**Produces:** `repo_def::REPOSITORY_LOADOUT: &str` (`"repository"`), `repo_def::repository_loadout(def: &Definition) -> Option<Loadout>`
**Acceptance:** `cargo test -p zygo-config --test repo_def_loadout` → all pass. The tests cover: `version = 1` alone gives one window with `name = None`, layout `main-vertical`, and five `claude-code` panes named `orchestrator`, `worker-1`, `worker-2`, `worker-3`, `worker-4` in that order, jobs `orchestrator` then `worker`, each `pool = true`, no `profile`, no `cwd`; members `b`, `a` and `boss` (orchestrator) give pane order `boss`, `a`, `b` and layout `main-vertical`; no orchestrator gives `tiled`; a set `layout` wins; a member with no `job` gets `worker`; every other `LoadoutPane` field is at its default (`args` empty, `yolo` and `passthrough` false, `server`, `compact` and `config_dir` none); profile files and no workspace file give `None`

## Plan
