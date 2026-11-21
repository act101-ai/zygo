# HSP — Proxy session spend and tokens: local, passthrough notional, counts, AU33 tee

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2 (rows `tokens_in` through `team_session`), H§7.3 steps 3-4, AU5, AU13, AU15, AU16, AU33, H§9 rows §7.2, §14
**Files:** `crates/zygo-proxy/src/client.rs`, `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/src/passthrough.rs`, `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/tests/session_api.rs`, `crates/zygo-proxy/tests/passthrough.rs`, `crates/zygo-hooks/src/statusline.rs`
**Consumes:** `Prices::cost` (HPR), `schema::TokenUsage` (HPR), `SessionStatus.model` (HS)
**Produces:** `SessionStatus.tokens_in: Option<u64>`, `SessionStatus.tokens_out: Option<u64>`, `SessionStatus.spend_usd: Option<f64>`, `SessionStatus.spend_notional: Option<bool>`, `SessionStatus.requests_total: Option<u64>`, `SessionStatus.requests_priced: Option<u64>`, `SessionStatus.passthrough: Option<bool>`, `SessionStatus.team_session: Option<bool>`
**Acceptance:** `cargo test -p zygo-proxy --test session_api --test passthrough && cargo test --workspace --locked --no-run` → all pass and the workspace builds: the row `estimated_usd` and the session spend come from one `Prices::cost` call; the AU15 counts with zero-token requests in neither; AU16 token sums; passthrough rows 0 with a notional session spend; the AU33 5-minute and 1-hour cache-write fields read when present

## Plan
