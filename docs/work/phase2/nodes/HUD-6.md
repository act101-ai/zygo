# HUD-6 — Proxy session spend and tokens: local, passthrough notional, counts, AU33 tee

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2 (rows `tokens_in` through `team_session`), H§7.3 steps 3-4, AU5, AU13, AU15, AU16, AU33, H§9 rows §7.2, §14. Design for AU33: `tee::Usage` keeps its four fields, and the 5-minute/1-hour split is a separate `tee::CacheSplit` read through `Tee::cache_split`, so the `const` `Usage` literals at `crates/zygo-proxy/tests/request_path.rs:52`, `:630` and `crates/zygo-proxy/tests/window.rs:434` do not change
**Files:** `crates/zygo-proxy/src/client.rs`, `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/src/passthrough.rs`, `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/tests/session_api.rs`, `crates/zygo-proxy/tests/passthrough.rs`
**Consumes:** `Prices::cost` (HUD-4), `schema::TokenUsage` (HUD-4), `SessionStatus::default` (HUD-5)
**Produces:** `tee::CacheSplit { m5: u64, h1: u64 }`, `Tee::cache_split(&self) -> Option<CacheSplit>`, `SessionStatus.tokens_in: Option<u64>`, `SessionStatus.tokens_out: Option<u64>`, `SessionStatus.spend_usd: Option<f64>`, `SessionStatus.spend_notional: Option<bool>`, `SessionStatus.requests_total: Option<u64>`, `SessionStatus.requests_priced: Option<u64>`, `SessionStatus.passthrough: Option<bool>`, `SessionStatus.team_session: Option<bool>`
**Acceptance:** `cargo test -p zygo-proxy && cargo test --workspace --locked --no-run` → all pass and the workspace builds: the row `estimated_usd` and the session spend come from one `Prices::cost` call; the AU15 counts with zero-token requests in neither; AU16 token sums; passthrough rows 0 with a notional session spend; the AU33 cache-write split read when present

## Plan

Design decisions (implementer's calls, to be checked in review):

- The passthrough stage hands the notional price to `Pending` on a new field `Resolved.notional_prices: Option<Prices>` (`crates/zygo-proxy/src/resolve.rs`, not in **Files**; the build needs its one literal). A `RequestCtx` field would break the `RequestCtx` literals of `tests/redaction_e2e.rs`, which is not in **Files**.
- The session takes one request through `SessionState::add_request(&self, u: &TokenUsage, cost: Option<f64>)` (public, so `tests/session_api.rs` can drive the counts). `add_usage` stays for its existing caller in `tests/session_api.rs`.
- A team session's reply has `team_session = true`, `spend_notional = true`, the token sums and `requests_total`, and no `spend_usd` and no `requests_priced`: until HUD-10 prices it with the built-in catalog, the only cost the server has is at its configured prices (AU14). `Pending::drop` gives the session no cost for a team session, and `status()` omits the two fields.
- The counts read the four row fields: a request is counted when input + cache read + cache write + output > 0. Its spend is added only when it is counted.
- A cost that is not finite (`Prices::cost` = `None`) writes `estimated_usd = 0` and the request is not priced.
- `crates/zygo-proxy/src/routes.rs` needed no change: the status handler serializes `SessionState::status()`.

- [x] **1. Write the failing tests.** `tests/session_api.rs`: `status_spend_tokens_and_counts` (AU15 counts with zero-token requests in neither, AU16 sums, the JSON names, an old server's reply, absent fields omitted), `status_team_session_sends_no_configured_price` (AU13, AU14), `tee_reads_the_cache_write_split` (AU33: not streamed, streamed, line mode, no split = `None`). `tests/passthrough.rs`: the fake upstream's reply can be set (`Upstream::reply`), `{UP}` in `setup`'s `extra` is the upstream URL; `e2e_passthrough_notional_spend` (H§7.3 step 4: rows 0, notional spend, no match and no priced provider not priced), `e2e_local_spend_is_the_row_cost` (the row `estimated_usd` equals the session spend, with the split and the long-context tier). Seen failing: the build stops on the missing fields, `add_request` and `cache_split`.
- [x] **2. `client.rs`.** `SessionStatus` gains the eight `Option` fields, each `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- [x] **3. `tee.rs`.** `CacheSplit { m5, h1 }`, `Tee::cache_split`; one parse per line (`line_usage`) feeds `Usage` and the split; `message_start` sets, `message_delta` takes maxima. `Usage` keeps its four fields.
- [x] **4. `passthrough.rs`, `resolve.rs`.** The stage keeps the price of the match's first provider on the type's dialect that has a price as `Resolved.notional_prices`; the candidate still has no price.
- [x] **5. `session.rs`.** `Spend` sums in `Live`; `add_request`; `status()` fills the eight fields.
- [x] **6. `forward.rs`.** `Pending::drop` builds one `TokenUsage` (with the split), calls `Prices::cost` once, writes `estimated_usd` from it (0 for a notional price) and calls `add_request`; `Flow::drop` copies `Tee::cache_split`. The old formula is removed.
- [x] **7. Acceptance.** `cargo test -p zygo-proxy` and `cargo test --workspace --locked --no-run`, each as its own command.
