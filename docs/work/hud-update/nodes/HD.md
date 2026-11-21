# HD — Base-spec amendments: W§15, H§9, auto-update

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§9 (every row except `docs/live-checks.md`), H§17 note 1 (dated §7.2 note); `docs/specs/2026-11-21-auto-update-design.md` "Base-spec changes"; `docs/specs/2026-11-20-workspaces-design.md` W§15 (every row; moved from WS D1, now DROPPED). One owner for `docs/specs/2026-11-18-zygo-design.md`: an edge HD→D1 would make the HUD wait for D1 (against H§17 note 1), and D1→HD needs a gate row in a workspaces graph that holds the 25-row cap
**Files:** `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `for t in G11 '\[loadout' requests_in_flight WA-1 WA-2 ZYGO_WORKSPACE CLAUDE_CODE_BRIDGE_SESSION_ID StopFailure 'resume note' OP13 AU105 PW16 'statusline-hud-design' spend_notional long_context cache_write_5m '\[statusline\]' 'icon test' 'update prompt' ZYGO_NO_UPDATE_CHECK; do grep -q "$t" docs/specs/2026-11-18-zygo-design.md || echo "missing $t"; done` → prints nothing, and each row of W§15, H§9 and the auto-update "Base-spec changes" has its edit in the section it names

## Plan
