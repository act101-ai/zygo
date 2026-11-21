# LIV-1 — Live check P-HUD-1 on Linux or macOS

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1), H§17 note 4. Needs the operator's machine (live login); Linux or macOS. Builds the binary with `cargo build -p zygo --features test-hooks` to use the HUD-9 seams (stdin/environment dump, width-source log, fetch delay). Windows parts are LIV-2. The executor stops and asks the operator to run this node on their machine
**Files:** `scripts/premises/live-hud.sh`, `docs/live-checks.md`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** `ZYGO_TEST_HUD_DUMP` (HUD-9), `ZYGO_TEST_FETCH_DELAY_MS` (HUD-9), `ZYGO_TEST_REFRESH_INTERVAL` (HUD-9)
**Produces:** —
**Acceptance:** `cargo build -p zygo --features test-hooks && sh scripts/premises/live-hud.sh --zygo target/debug/zygo && cargo test -p zygo --test forbidden_content` → exit 0 with `LIVE` lines for P-HUD-1 (H§14 #37); scrubbed fixtures; dated rows in `docs/live-checks.md` and dated notes in H§12. A failed P-HUD-1 line changes the contract of each node it names: PH1 (colors) → HUD-8, HUD-9, HUD-10, H2, LIV-2; PH2 (two rows) → HUD-8, HUD-11, H2; PH3 (cut or wrap at 70 columns) → HUD-8; PH4 (width source, status area width) → HUD-8; PH5 (stdin and environment dump) → HUD-9; PH9 (logo and reflection column) → HUD-8, H2; PH12 (cancellation, observation) → HUD-9; C7 (HUD text in a recorded upstream request) → HUD-9, the hook entry in `cmd/hook.rs` that is the only writer of HUD text. For a DONE node the failure creates a new node (execute-work §5.4) before DOC-1 runs; a TODO node (H2, LIV-2) is re-planned before it starts

## Plan
