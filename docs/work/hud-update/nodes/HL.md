# HL — Live check P-HUD-1, W-SHELL part 2 text, remaining PH results

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1; PH3, PH6, PH7, PH8, PH10, PH13 recorded here), H§9 row `docs/live-checks.md:77`, H§17 note 4. The live script uses tmux by hand; no automated test here
**Files:** `scripts/premises/live-hud.sh`, `docs/live-checks.md`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/live-hud.sh` → exit 0 on the pinned Claude Code with `LIVE` lines for P-HUD-1 and PH3, PH6, PH7, PH8, PH10, PH13, scrubbed fixtures, dated rows in `docs/live-checks.md` (H§14 #37) and dated notes in H§12; W-SHELL part 2 is recorded on Windows with the new text (H§14 #38). A failed PH applies its H§12 fallback as a new node before HD runs

## Plan
