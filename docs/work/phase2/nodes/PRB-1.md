# PRB-1 — Probe: temporary statusLine command for PH1-PH7, PH9, PH11-PH13

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (probe first), H§6.3 (the two OP13 `printf` commands), H§17 note 4. Needs the operator's machine (live login); pinned Claude Code in a terminal. Adds PH3, PH6, PH7 (visible in the same capture) and PH13 (OpenCode help) to the H§12 list; PH8 and PH10 need Windows and Git Bash, so LIV-2 records them. Results are dated notes in the H§12 table. The executor stops and asks the operator to run this node on their machine. PH4 includes the status area width, read from the probe capture, so HUD-8 is planned with it. LIV-2 records PH8 and PH10
**Files:** `scripts/premises/live-hud-probe.sh`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/live-hud-probe.sh && cargo test -p zygo --test forbidden_content` → exit 0 with one `LIVE <id> verified|failed|observed` line each for PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13; stdin and environment logs written as fixtures through `scripts/premises/scrub.py`; a dated note per id in H§12. A `failed` line still passes this node; its H§12 fallback re-plans every node whose acceptance it changes before that node starts: PH1 → HUD-8, HUD-9, HUD-10, H2, LIV-1, LIV-2; PH2 → HUD-8, HUD-11, LIV-1, H2; PH3 → HUD-8; PH9 → HUD-8, LIV-2; PH4, PH5, PH11 → HUD-8, HUD-9; PH7 → HUD-8, HUD-12; PH6, PH12, PH13 → none

## Plan
