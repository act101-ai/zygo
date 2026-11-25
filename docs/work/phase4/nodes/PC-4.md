# PC-4 — Codex config, trust and hook captures: PC3, PC4, PC5, PC12

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC3, PC4, PC5, PC12; K§6.3.6 (what K-D13 needs from PC3 and PC4); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/config.sh` (new), `fixtures/codex/config/` (new), `fixtures/codex/trust/` (new), `fixtures/codex/hooks/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2)
**Produces:** `fixtures/codex/config/` (PC3: `CODEX_HOME` against `~/.codex`, `-c` against a project `.codex/config.toml` per key, project agent files and hooks, `-C`/`--cd`; PC12: `shell_environment_policy` in tool shells), `fixtures/codex/trust/` (PC4), `fixtures/codex/hooks/` (PC5: one payload per hook event and the output shapes Codex accepts), and in this node file under "Observed": the date, result and fixture paths of PC3, PC4, PC5, PC12
**Acceptance:** `sh scripts/premises/run-codex.sh config && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(find fixtures/codex/hooks -type f -size +2c | wc -l)" -ge 3` → every command exits 0 and at least 3 hook captures exist; each of PC3, PC4, PC5, PC12 has a dated result and fixture paths under "Observed" in this file

## Planning notes

- The "home" config of PC3 is a planted file inside the scratch `HOME`, never the owner's `~/.codex` (Global constraints).
- PC-7 writes the rows from "Observed".

## Observed

Written when the node runs.

## Plan

Written when the node starts.
