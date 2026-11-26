# RS-6 — Cache markers at forward time on the Anthropic upstream body of a Responses client

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stage table row cache markers), K§11 item 19 (the Responses → Anthropic half); base spec `docs/specs/2026-11-18-zygo-design.md` §10.4 (marker placement); K-D48
**Files:** `crates/zygo-proxy/src/cache.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/responses_cache.rs` (new)
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3)
**Produces:** `cache::mark_upstream(body: &mut Value) -> bool` (the base-spec §10.4 marker placement on a Messages body built for an Anthropic upstream, with a string `system` first made one text block (K§6.4.2); `true` when it added a marker; `false` when the body already holds a `cache_control` key, or has no `system` and no `tools`)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_cache -- --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-proxy --locked` → at least 3 tests in `responses_cache` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: Responses → Anthropic with `[cache] insert_markers.<session client> = true` puts the markers in the upstream body; with it off, none; Responses → chat, none; the Anthropic client's request-time stage (`cache.rs:33-40`) is unchanged (existing cache tests pass unchanged)

## Planning notes

- Client policy stays the per-client config key `insert_markers.<agent>` read as today (`cache.rs:34`); the Codex default is P4-9 (K§6.4.2).
- The marker is placed in `responses_attempt` (`forward.rs:977-1013`) in its Anthropic branch, after facts and the schema rewrite. `attempt_body` calls `responses_attempt` only for an Openai client and a non-Openai candidate (`forward.rs:897-899`), so that branch is the only place where the candidate is Anthropic and the client is Openai.
- This node and RS-7 share no file (RS-7 changes `window.rs` and `tee.rs`), so they run in parallel after RS-5.

## Plan

### Premise check (2026-11-25, base `8c7e3290`)

- Holds: the cache stage acts only on the Messages route with an Anthropic client dialect (`crates/zygo-proxy/src/cache.rs:33-42`). A Responses body reaches an Anthropic candidate only through `responses_attempt` (`crates/zygo-proxy/src/forward.rs:897-899`, `:982-1013`). A Responses upstream already gets no marker (RS-5 test `no_cache_markers_toward_responses_upstream`, `tests/responses_passthrough.rs:175`); RS-6 does not repeat that test.
- Drifted: `responses_client::request` builds `system` as one string (`crates/zygo-proxy/src/translate/responses_client.rs:179-181`). Codex sends its base text as leading `developer` items, which are joined into that string (`:461-465`; both `fixtures/codex/requests/first-turn.json` and `turn-with-reasoning.json` start with two `developer` items and have no `instructions`). §10.4 marks `system` only when it is an array, so the plain rule would mark only the last tool and leave the instructions outside the cached prefix. Spec K§6.4.2 row "Cache markers" is rewritten as final text (the string becomes one marked text block; the Anthropic client's string `system` stays unmarked), with a change-log line. The old citation `forward.rs:788-797` now reads `responses_attempt`, `forward.rs:977-1013`.
- Client policy: the lookup stays the per-client key, read as at `cache.rs:34`. Defaults (`crates/zygo-config/src/schema.rs:465-470`): `claude-code` false, `opencode` true. A `[cache] insert_markers` table in the config replaces the default map.

### Interfaces (live code)

Consumed:

```rust
// crates/zygo-proxy/tests/support/responses.rs (RS-3)
pub fn start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness       // :211
pub fn post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes)   // :331
pub fn upstream_bodies(&self) -> Vec<Value>                                      // :353
pub fn upstream_raw(&self) -> Vec<Vec<u8>>                                       // :358
pub fn codex_request(name: &str) -> Value                                        // :412
// crates/zygo-proxy/src/pipeline.rs: ModelRoute::Responses (RS-3)
// crates/zygo-proxy/src/cache.rs (private, reused)
fn has_marker(v: &Value) -> bool                                                 // :12
fn mark_last(body: &mut Value, field: &str) -> bool                              // :21
// crates/zygo-proxy/src/forward.rs
struct Rewrite<'a> { facts: Option<&'a Facts>, rules: &'a [SchemaRule], strict: bool }   // :875
fn responses_attempt(body: &Value, raw: &Bytes, c: &Candidate, rw: &Rewrite, rules: &[SchemaRule])
    -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError>                      // :982
// crates/zygo-proxy/src/session.rs:231
pub struct SessionState { pub agent: Agent, .. }
```

Produced:

```rust
// crates/zygo-proxy/src/cache.rs
/// A Messages body built for an Anthropic upstream (K§6.4.2, base §10.4): with no `cache_control` key anywhere, a
/// string `system` becomes one text block, then the last `system` block and the last tool get
/// `{"type":"ephemeral","ttl":"1h"}`. `true` when it added a marker.
pub fn mark_upstream(body: &mut Value) -> bool
```

`CacheStage::on_request` does not change. Its tests stay byte-equal: `tests/request_path.rs` `cache_markers_opencode_only` asserts that a string `system` stays unmarked on the Messages route.

### Steps

- [ ] **1. Unit tests for `mark_upstream`.** Append to `crates/zygo-proxy/src/cache.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn mark() -> Value {
        json!({"type":"ephemeral","ttl":"1h"})
    }

    #[test]
    fn mark_upstream_wraps_string_system_and_marks_last_tool() {
        let mut b = json!({"system":"S","tools":[{"name":"a"},{"name":"b"}],"messages":[]});
        assert!(mark_upstream(&mut b));
        assert_eq!(
            b,
            json!({"system":[{"type":"text","text":"S","cache_control":mark()}],
                   "tools":[{"name":"a"},{"name":"b","cache_control":mark()}],"messages":[]})
        );
    }

    #[test]
    fn mark_upstream_marks_last_block_of_array_system() {
        let mut b = json!({"system":[{"type":"text","text":"a"},{"type":"text","text":"b"}]});
        assert!(mark_upstream(&mut b));
        assert_eq!(b, json!({"system":[{"type":"text","text":"a"},{"type":"text","text":"b","cache_control":mark()}]}));
    }

    #[test]
    fn mark_upstream_leaves_a_marked_body() {
        let before = json!({"system":"S","tools":[{"name":"a"}],
            "messages":[{"role":"user","content":[{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]}]});
        let mut b = before.clone();
        assert!(!mark_upstream(&mut b));
        assert_eq!(b, before);
    }

    #[test]
    fn mark_upstream_without_system_or_tools_is_false() {
        let before = json!({"messages":[{"role":"user","content":"hi"}]});
        let mut b = before.clone();
        assert!(!mark_upstream(&mut b));
        assert_eq!(b, before);
    }
}
```

- [ ] **2. Route tests.** Create `crates/zygo-proxy/tests/responses_cache.rs`:

```rust
//! RS-6: cache markers at forward time on the Anthropic upstream body of a Responses client (K§6.4.2 row cache
//! markers, K§11 item 19, K-D48, base §10.4). Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{codex_request, Harness};
use zygo_config::Dialect::{Anthropic as A, OpenaiChat as C};
use zygo_core::Agent;

const R: &str = "openai/v1/responses";
const ON: &str = "[cache]\ninsert_markers.claude-code = true\n";

/// Occurrences of `pat` in `raw`.
fn count(raw: &[u8], pat: &[u8]) -> usize {
    raw.windows(pat.len()).filter(|w| *w == pat).count()
}

/// The upstream body (parsed, raw) of one `first-turn` request through `h`. The mock's default reply (400) is not
/// under test.
fn sent(h: &Harness) -> (Value, Vec<u8>) {
    h.post(R, &codex_request("first-turn"));
    (h.upstream_bodies().remove(0), h.upstream_raw().remove(0))
}

/// K§11 item 19: on for the session's client → the string system is one marked text block, the last tool is marked,
/// every other value is the unmarked body's; two markers, not more (review focus 12).
#[test]
fn anthropic_upstream_markers_when_on() {
    let (off, _) = sent(&Harness::start(A, Agent::ClaudeCode, ""));
    let (on, raw) = sent(&Harness::start(A, Agent::ClaudeCode, ON));
    let mark = json!({"type":"ephemeral","ttl":"1h"});
    let mut want = off.clone();
    want["system"] = json!([{"type":"text","text": off["system"].as_str().expect("first-turn has developer text"),
                             "cache_control": mark}]);
    let n = want["tools"].as_array().expect("first-turn has tools").len();
    want["tools"][n - 1]["cache_control"] = mark;
    assert_eq!(on, want);
    assert_eq!(count(&raw, b"\"cache_control\""), 2);
}

/// Off (Claude Code's default) → no marker.
#[test]
fn anthropic_upstream_no_markers_when_off() {
    let (_, raw) = sent(&Harness::start(A, Agent::ClaudeCode, ""));
    assert_eq!(count(&raw, b"cache_control"), 0);
}

/// The key is read per client: OpenCode's default `true` marks.
#[test]
fn opencode_default_marks_anthropic_upstream() {
    let (on, raw) = sent(&Harness::start(A, Agent::OpenCode, ""));
    assert_eq!(on["system"][0]["cache_control"], json!({"type":"ephemeral","ttl":"1h"}));
    assert_eq!(count(&raw, b"\"cache_control\""), 2);
}

/// A chat upstream gets none, even with the key on (OpenCode's default).
#[test]
fn chat_upstream_no_markers() {
    let (_, raw) = sent(&Harness::start(C, Agent::OpenCode, ""));
    assert_eq!(count(&raw, b"cache_control"), 0);
}
```

- [ ] **3. See them fail.** `cargo test -p zygo-proxy --lib cache::tests` → does not compile (`mark_upstream` not found). `cargo test -p zygo-proxy --test responses_cache` → `anthropic_upstream_markers_when_on` and `opencode_default_marks_anthropic_upstream` fail (no `cache_control` in the body); the two "no markers" tests pass.

- [ ] **4. `mark_upstream`.** In `crates/zygo-proxy/src/cache.rs`, add `pub fn mark_upstream(body: &mut Value) -> bool` with the doc comment above. Rule: `has_marker(body)` → `false`; a `system` that is a JSON string is replaced in place by `[{"type":"text","text":<that string>}]`; then both `mark_last(body, "system")` and `mark_last(body, "tools")` run, and the result is their OR (as `on_request` does at `:38-40`). Leave `CacheStage` as it is.

- [ ] **5. Wire it in `forward.rs`.**
  - Add `use crate::cache;` with the other `crate::` imports (`:27-41`).
  - `struct Rewrite` (`:875`): add the field `markers: bool`, doc ``/// `[cache] insert_markers.<agent>` for the session's agent (§10.4); read only toward an `anthropic` candidate of a Responses client (K§6.4.2).``
  - `run`, the `Rewrite` literal (`:423-427`): add `markers: cfg.config.cache.insert_markers.get(s.agent.as_str()).copied().unwrap_or(false),`
  - `responses_attempt`, the Anthropic branch (`} else { (m, Vec::new()) }`, `:1003-1005`): `if rw.markers { cache::mark_upstream(&mut m); }` before `(m, Vec::new())`. Add to its doc comment: "to `anthropic`, the §10.4 markers when `rw.markers` (K§6.4.2), last, so they sit on the final `system` text".

- [ ] **6. See them pass.** `cargo test -p zygo-proxy --lib cache::tests` → 4 passed. `cargo test -p zygo-proxy --test responses_cache` → 4 passed.

- [ ] **7. Acceptance.** `test "$(cargo test -q -p zygo-proxy --test responses_cache -- --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-proxy --locked` → exit 0 (count 4; the cache tests in `request_path`, `redaction_e2e` and `responses_passthrough` pass unchanged). Also `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings` → no warning.

- [ ] **8. Commit** `feat(proxy): RS-6 cache markers on the Anthropic upstream body of a Responses client`, with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:RS-6`.

### Author's checks

1. Spec coverage: K§6.4.2 row (forward time, Anthropic only, per-client key, chat none, Anthropic client unchanged) → steps 2, 4, 5, and `request_path::cache_markers_opencode_only` unchanged. Responses upstream none → RS-5's test. K§11 item 19 → `anthropic_upstream_markers_when_on`. §10.4 "no `cache_control` anywhere" → `mark_upstream_leaves_a_marked_body`. String `system` (K§6.4.2 rewrite) → `mark_upstream_wraps_string_system_and_marks_last_tool`.
2. Step scan: each code step names one signature or one line. The only body rule given (wrap a string `system`) is the spec rewrite, stated in words.
3. Interface match: the Produces signature is the contract's. RS-14 (`forward.rs`) depends on RS-6 already. CC-4 (depends on RS-6) moves client policy to the descriptor; the new `insert_markers` read in `run` is one more per-client site for it, next to `cache.rs:34`.
4. Review focus 12 (graph header): markers only on the Anthropic upstream body of a Responses client → `anthropic_upstream_markers_when_on` (exactly 2), `chat_upstream_no_markers`; not twice on the Anthropic client's stage → `CacheStage` unchanged and `responses_attempt` is not on the Messages route; Responses upstream → RS-5's test.
5. Proportion: about 25 lines of production code against about 110 lines of tests; no production body is written here.
6. Live code: every path, line and symbol above was opened at `8c7e3290`. No existing function wraps a string `system` into a block (`grep -rn '"system"' crates/zygo-proxy/src`).

### Overlap

Files: `cache.rs`, `forward.rs`, `tests/responses_cache.rs`. Open nodes: RS-14 (`forward.rs`) already depends on RS-6. RS-7 (`window.rs`, `tee.rs`; its node says no `forward.rs` change), RS-13, RS-12, PC-4, PC-7 and PC-11 share no file. No edge added.
