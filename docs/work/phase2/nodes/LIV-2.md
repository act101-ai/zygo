# LIV-2 — Windows live part: PH8, PH10, W-SHELL part 2 (manual)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (PH8, PH10), H§14 #38, H§9 row `docs/live-checks.md:77`. Needs a Windows machine with Git Bash and the operator's live login. No node depends on it. A failed PH8 or PH10 applies its H§12 fallback as a new node after this one. Edge to LIV-1 for the shared script, fixtures and docs. The executor stops and asks the operator to run this node on their machine
**Files:** `scripts/premises/live-hud.sh`, `docs/live-checks.md`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/live-hud.sh --windows && cargo test -p zygo --test forbidden_content` (the script in Git Bash on Windows) → exit 0 with `LIVE` lines for W-SHELL part 2 with the new text (H§14 #38), PH8 and PH10; scrubbed fixtures; dated rows in `docs/live-checks.md` and dated notes in H§12. A failed W-SHELL part 2 line (H§14 #38) creates a new node after this one (execute-work §5.4): it changes the W-SHELL part 2 text in `docs/live-checks.md:77` that this node wrote (H§9 row `docs/live-checks.md:77`) when the text is wrong, or re-opens HUD-8 (rows, logo) or HUD-9 (hook entry) when the output is wrong

## Plan
