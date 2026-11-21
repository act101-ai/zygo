# HP — Probe: temporary statusLine command for PH1, PH2, PH4, PH5, PH9, PH11, PH12

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (probe first, PH1, PH2, PH4, PH5, PH9, PH11, PH12), H§6.3 (the two OP13 `printf` commands), H§17 note 4. Results are dated notes in the H§12 table
**Files:** `scripts/premises/live-hud-probe.sh`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/live-hud-probe.sh` → exit 0 on the pinned Claude Code: one `LIVE <id> verified|failed|observed` line each for PH1, PH2, PH4, PH5, PH9, PH11, PH12; stdin and environment logs written as fixtures through `scripts/premises/scrub.py`; a dated note per id in H§12. H§12 and H§17 note 4: on a `failed` line the node still passes; it records the result, and before any listed dependent runs the executor adds the fallback node and re-points those dependents to it: PH1 → 16-color only or no escapes (HR); PH2 → verbose joined into one row (HR); PH4 → skip the failing width source, W = 120 (HD2); PH5 → effort part dropped for another shape (HD2); PH9 → plain `zygo` logo (HR); PH11 → wait for the discard thread (HD2); PH12 → none (observation)

## Plan
