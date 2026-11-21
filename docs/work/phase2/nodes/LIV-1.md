# LIV-1 — Live check P-HUD-1, PH8/PH10 on Windows, W-SHELL part 2 text

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1; PH8, PH10), H§9 row `docs/live-checks.md:77`, H§17 note 4. Needs the operator's machine (live login); uses the HUD-9 log line and fetch-delay seam; #38 and PH8/PH10 also need a Windows machine with Git Bash: a manual check that blocks no other node. A failed PH8 or PH10 applies its H§12 fallback as a new node after this one
**Files:** `scripts/premises/live-hud.sh`, `docs/live-checks.md`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/live-hud.sh && sh scripts/premises/live-hud.sh --windows` (the second in Git Bash on Windows) → exit 0 each, with `LIVE` lines for P-HUD-1 (H§14 #37), W-SHELL part 2 with the new text (H§14 #38), PH8 and PH10; scrubbed fixtures; dated rows in `docs/live-checks.md` and dated notes in H§12

## Plan
