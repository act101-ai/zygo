# DOC-1 — Base-spec notes for the HUD (H§9) and auto-update

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§9 (every row except `docs/live-checks.md`; the status-reply change is recorded in §7.2), H§17 note 1; `docs/specs/2026-11-21-auto-update-design.md` "Base-spec changes". The W§15 rows, its §7.2 row included, stay with workspaces D1, which runs later. Records only the `zygo claude`/`zygo opencode` start check in the `zygo update` §17 row; the `zygo up` check text is D1's (after M5)
**Files:** `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `for t in statusline-hud-design spend_notional long_context cache_write_5m '\[statusline\]' 'icon test' 'update prompt' ZYGO_NO_UPDATE_CHECK 'zygo statusline' '⇣off' 'H§6.1 AU28' 'last main-loop request' 'long-context tier' 'start check'; do grep -q "$t" docs/specs/2026-11-18-zygo-design.md || echo "missing $t"; done` → prints nothing, and each H§9 row and each auto-update "Base-spec changes" row has its dated edit in the section it names

## Plan
