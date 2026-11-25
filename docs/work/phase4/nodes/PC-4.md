# PC-4 — Codex config, trust and hook captures: PC3, PC4, PC5, PC12; the `--strict-config` run

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC3, PC4, PC5, PC12 and the last paragraph (the `--strict-config` run); K§6.3.1 item 2 (the keys zygo writes), K§6.3.6 (what K-D13 needs from PC3 and PC4); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/config.sh` (new), `fixtures/codex/config/` (new), `fixtures/codex/trust/` (new), `fixtures/codex/hooks/` (new), `fixtures/codex/runs/config/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/config/` (PC3: `CODEX_HOME` against `~/.codex`, `-c` against a project `.codex/config.toml` per key, project agent files and hooks, `-C`/`--cd`; PC12: `shell_environment_policy` in tool shells; `strict-config.txt`: one `--strict-config` run of a config that holds each key K§6.3.1 item 2 says zygo writes, with Codex's verdict per key), `fixtures/codex/trust/` (PC4), `fixtures/codex/hooks/` (PC5: one payload per hook event and the output shapes Codex accepts), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh config && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(find fixtures/codex/hooks -type f -size +2c | wc -l)" -ge 3 && test "$(wc -c < fixtures/codex/config/strict-config.txt)" -gt 2 && grep -qx 'home=scratch' fixtures/codex/runs/config/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/config/isolation.txt && test -f fixtures/codex/runs/config/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/config/connect.txt)" && ! grep -q 'Written when the node runs' docs/work/phase4/nodes/PC-4.md && test "$(grep -cE '^- (PC3|PC4|PC5|PC12): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-4.md)" -eq 4` → every command exits 0: at least 3 hook captures and the `--strict-config` record; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` was refused; "Observed" has no placeholder and one dated result line for each of PC3, PC4, PC5, PC12

## Planning notes

- The "home" config of PC3 is a planted file inside the scratch `HOME`, never the owner's `~/.codex` (Global constraints).
- The `--strict-config` run here uses a planted config with zygo's keys. The run of zygo's own generated config (K§11 item 34, the premise test of `crates/zygo-launch/tests/premise.rs`) needs the Codex client and is P4-9.
- Every mock and scrub change this node needs is a PC-2 deliverable; this node edits neither file. PC-7 writes the rows from "Observed".

## Observed

Written when the node runs.

## Plan

Written when the node starts.
