# LIV-1 — Live check P-HUD-1 on Linux or macOS

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1), H§17 note 4. Needs the operator's machine (live login); Linux or macOS. Builds the binary with `cargo build -p zygo --features test-hooks` to use the HUD-9 seams (stdin/environment dump, width-source log, fetch delay). Windows parts are LIV-2. The executor stops and asks the operator to run this node on their machine
**Files:** `scripts/premises/live-hud.sh`, `docs/live-checks.md`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** `ZYGO_TEST_HUD_DUMP` (HUD-9), `ZYGO_TEST_FETCH_DELAY_MS` (HUD-9)
**Produces:** —
**Acceptance:** `cargo build -p zygo --features test-hooks && sh scripts/premises/live-hud.sh --zygo target/debug/zygo && cargo test -p zygo --test forbidden_content` → exit 0 with `LIVE` lines for P-HUD-1 (H§14 #37); scrubbed fixtures; dated rows in `docs/live-checks.md` and dated notes in H§12. A failed P-HUD-1 line re-opens its code as a new node (execute-work §5.4) before DOC-1 runs: colors or two rows → HUD-8; cut or wrap, or a width source → HUD-8 (width) and HUD-9; overlapping runs → none (observation)

## Plan
