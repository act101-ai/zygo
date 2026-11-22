# HUD-10 — Team-server session spend at built-in list prices (AU14)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` AU14, H§7.3 step 5, H§9 row §7.2 (no configured price leaves the server), H§17 note 2. Owns the `crates/zygo-hooks/Cargo.toml` dev-dependency additions its team-server test needs. Every new `SessionView` or `SessionStatus` literal in its tests ends in `..Default::default` (H§17 note 1). The H§14 #15 test calls `collect` in process with an `env` closure that returns the loopback `ZYGO_URL`
**Files:** `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/serve.rs`, `crates/zygo-hooks/tests/statusline_team.rs`, `crates/zygo-hooks/Cargo.toml`, `Cargo.lock`
**Consumes:** `Prices::cost` (HUD-4), `SessionStatus.team_session` (HUD-6), `statusline::render::render` (HUD-8), `statusline::data::collect` (HUD-9)
**Produces:** —
**Acceptance:** `cargo test -p zygo-proxy --test serve && cargo test -p zygo-hooks --test statusline_team && cargo test --workspace --locked --no-run` → all pass, including H§14 #15

## Plan

Steps:

- [x] **1. Read.** The contract, AU14, AU15, H§7.2, H§7.3 step 5, H§9 row §7.2, H§14 #15, the graph constraints and review focus 8, the HUD-6 plan (the two guards) and the HUD-9 plan (data path, cache, seams).
- [x] **2. Failing proxy test.** `crates/zygo-proxy/tests/serve.rs`: the fake upstream's reply can be set (`Upstream::set_usage`); `h14_15_team_session_spend_at_builtin_list_prices`. Seen failing on the HUD-6 guards: `spend_usd` and `requests_priced` absent from a team session's reply.
- [x] **3. `forward.rs`.** `list_prices(id)`: the first price of the row `id` in the built-in catalog of the binary (`zygo_config::builtin::defaults()`, read once). `Pending::drop` gives a team session `list_prices(row.model)` × the request's tokens through `Prices::cost`; the row's `estimated_usd` stays at the configured prices. The guard `cost.filter(team_label.is_none())` is removed.
- [x] **4. `session.rs`.** `status()` sends `spend_usd` and `requests_priced` for a team session too. The two `(!team).then_some(…)` guards are removed. `spend_notional` stays `passthrough || team`.
- [x] **5. Proxy test passes.** One wrong expectation of mine first: a configured rate of 3.0 was equal to `requests_total = 3` in the "no configured number in the reply" check; the test's rates are now not whole numbers.
- [x] **6. HUD test.** `crates/zygo-hooks/tests/statusline_team.rs`: `h14_15_team_session_hud_shows_notional_spend_at_list_prices`. Honest order: this test was written after step 3 and 4, so its first run passed. Mutation check in its place: with the team spend summed at the configured cost in `Pending::drop`, this test failed (`Some(7.5075) != 0.584`) and the proxy test failed (`2.235 != 0.017075 + 0.1025`). The mutant is removed.
- [x] **7. `crates/zygo-proxy/tests/session_api.rs`** (not in **Files**): `status_team_session_sends_no_configured_price` asserted the two HUD-6 guards (`spend_usd` and `requests_priced` absent for a team session). The guards are gone by this node's contract, so the test failed. Its two guard assertions now expect the sum that the session holds; the AU13 assertions are unchanged.
- [x] **8. Acceptance.** `cargo test -p zygo-proxy --test serve`, `cargo test -p zygo-hooks --test statusline_team`, `cargo test --workspace --locked --no-run`, each as its own command; then `cargo test -p zygo-proxy --locked`, `cargo test --workspace --locked --test statusline_team` (the workspace feature set, as CI builds it) and `cargo clippy -p zygo-proxy -p zygo-hooks --all-targets --locked -- -D warnings`.

H§14 items and their tests:

| Item | Test |
|---|---|
| #15, proxy part (review focus 8) | `h14_15_team_session_spend_at_builtin_list_prices` (`crates/zygo-proxy/tests/serve.rs`): `team_session = true`, `spend_notional = true`, `spend_usd` = Σ `Prices::cost` at the built-in prices with one request above `long_context.above` at the long rates; no number of the reply is a configured rate or a cost at the configured rates; the stored rows' `estimated_usd` are at the configured prices; a served model that the built-in catalog does not have is counted and not priced; a local proxy reply has `team_session = false` and `spend_notional = false`. |
| #15, HUD part | `h14_15_team_session_hud_shows_notional_spend_at_list_prices` (`crates/zygo-hooks/tests/statusline_team.rs`): `≈$X` with `≈` in SGR `2` and the amount in SGR `32`, compact and verbose; `≈$X+` after a request for a model missing from the built-in catalog; mode `serve 127.0.0.1` with `ZYGO_URL=https://127.0.0.1:<port>`; a local proxy session shows `local` and no `≈`. |

Design calls:

1. **Where the list price is read.** In `Pending::drop` (`forward.rs`), by the row's `model` (the served catalog id that the resolution stage wrote), from `zygo_config::builtin::defaults().catalog`, kept in a `OnceLock`. The serving config is not read for the session spend of a team session, so a configured price cannot be in the sum. A reload does not change the built-in catalog, so one read for the life of the process is correct.
2. **`add_request` keeps its signature.** The session sums the cost that its caller gives. The rule "never a configured price for a team session" is in the one production caller (`Pending::drop`) and is tested end to end; `SessionState` has no price table.
3. **A model that the built-in catalog does not price** (no row, or a row with no price) is counted in `requests_total` and not in `requests_priced` (AU15); its spend is 0.
4. **No new dev-dependency.** The HUD test talks to the team server and plays the upstream over plain loopback TCP (`std::net`), so `crates/zygo-hooks/Cargo.toml` and `Cargo.lock` do not change.
5. **No `test-hooks` call of `zygo-proxy` in the HUD test.** `ServeHandle::usage_written` exists only with that feature, which `cargo test -p zygo-hooks` does not enable. The test waits on the status reply (`requests_total`), with a 10 s bound, because a request's usage is added when its response body ends.
6. **The loopback tunnel** is the `ZYGO_URL` value only (`https://127.0.0.1:<port>`): the HUD reads its host for the mode, and fetches the status from the `url` file of the session directory (HUD-9). No TLS listener is needed for the mode text.
7. **`session_api.rs` edit** (step 7) is outside **Files**. The alternative was a failing `cargo test -p zygo-proxy`.

Found outside the node (reported, not changed):

- `fallback_all_failed_removes_pin` (`crates/zygo-proxy/tests/request_path.rs`) failed once in a full `cargo test -p zygo-proxy` run and passed in the two other full runs. Cause: `refused_port()` binds a port and releases it; the test uses that port several steps later, while the other tests of the binary bind `127.0.0.1:0`. Another test's upstream got the port, so the "dead" provider answered `200` with that test's SSE body (the failure text is that body). This node changes no code on that path.
