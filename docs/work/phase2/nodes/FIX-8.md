# FIX-8 — Triage: one `auxiliary` 429 in each passthrough session (LIV-1 D8)

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §7.3 ("Outbound headers" and its note: fallback only on a connect error, a 5xx or a 429), §12.4; found by LIV-1 (node file "Live run findings" D8: sessions a, b, c, d each had one `auxiliary` request answered 429, model `claude-opus-5-5`; session a also had `main` 200 and `auxiliary` 200)
**Files:** `crates/zygo-proxy/tests/passthrough.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-proxy --locked --test passthrough && rg -q '^## Root cause' docs/work/phase2/nodes/FIX-8.md` → 0 failed, with `auxiliary_request_reaches_upstream_as_main_does ... ok` and `upstream_429_reaches_the_client_once_and_unchanged ... ok`; the node file has a `## Root cause` section with file:line evidence and a verdict `zygo`, `upstream` or `not determined`

## Plan

Work in the node's checkout. Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-8`. In each commit, tick (`- [x]`) the steps of this file that are done. No step starts an agent or sends a request to a real vendor.

Facts of the live code (read 2026-11-22, main at 0be75eb):

- Stage 2 for passthrough (`passthrough.rs:186-241`) has no branch on the request class; `upstream_model` is the received `model` (`:227`; `forward.rs:751-756`). The body sent is the received bytes unless the stages changed it (`forward.rs:827`, `:839-840`).
- Outbound headers for every passthrough request: `outbound_headers` (`forward.rs:71-101`: `content-type`, `accept`, `anthropic-version`, `anthropic-beta`, minus `NEVER`/`NEVER_PREFIX` `:48-66`) plus the client's `authorization` and `anthropic-beta` (`passthrough.rs:71-86`, `CLAUDE_AI.forwarded` `:42`). Every other client header is not sent. zygo-proxy sets no user agent of its own.
- One candidate (`passthrough.rs:231`); a 429 is kept as `last` (`forward.rs:446`) and sent to the client (`:465-469`); response headers pass except hop-by-hop, provider-only and `x-zygo-*` (`:130-147`).
- The request class is read only at `window.rs:162` (main only), `redact_stage.rs:148` (`admits`: `main`, `compaction`, `auxiliary` or absent, `crates/zygo-redact/src/content.rs:70-71`) and the usage row (`forward.rs:291`).
- Test harness: `Upstream` (`passthrough.rs:45-89`) records each request and always answers 200; `setup` resolves `api.anthropic.com` to the fake `anth` (`:136-152`); `post_pt` (`:234-239`); `e2e_login_reaches_upstream_unchanged` (`:362-383`) is the model test.
- The usage rows store the status, not the upstream error body (`crates/zygo-usage/src/schema.rs`).

### Task 1: what zygo sends (tests)

- [x] **1.1** Give `Upstream` a status and extra reply headers: add the fields `status: Arc<Mutex<u16>>` (200 at start) and `headers: Arc<Mutex<Vec<(String, String)>>>` (empty at start), use them in the reply builder of `start`, and add `fn answer(&self, status: u16, headers: &[(&str, &str)], body: &Value)` that sets all three for every later request.
- [x] **1.2** Add the test `auxiliary_request_reaches_upstream_as_main_does`. It sends one `main` request and one `auxiliary` request with `post_pt` to `M`, body `mb("claude-test-1")`, and the Claude Code header set `anthropic-beta: oauth-2025-04-20`, `anthropic-version: 2023-06-01`, `accept: application/json`, `user-agent: claude-cli/2.1.330 (external, cli)`, `x-app: cli`, `x-stainless-lang: js`, `anthropic-dangerous-direct-browser-access: true`, `x-claude-code-session-id: s1`, and `x-claude-code-request-class: main` or `auxiliary`. Assertions: both answers 200; `t.anth.got().len() == 2`; each recorded body is exactly the sent bytes; the two recorded header maps are equal; `user-agent`, `x-app`, `x-stainless-lang`, `anthropic-dangerous-direct-browser-access` and every `x-claude-code-*` name are absent from both; `authorization` is `BEARER` and `anthropic-beta` is `oauth-2025-04-20`; the two usage rows have `request_class` `main` and `auxiliary`, with the same `model`. Print the sorted recorded header names with `eprintln!`.
- [x] **1.3** Add the test `upstream_429_reaches_the_client_once_and_unchanged`: `t.anth.answer(429, &[("retry-after", "7")], &json!({"type":"error","error":{"type":"rate_limit_error","message":"limit"}}))`, then one `auxiliary` request. Assertions: the client gets 429, `retry-after: 7` and the same JSON body; `t.anth.got().len() == 1` (no retry); one usage row with status 429 and `request_class` `auxiliary`.
- [x] **1.4** Run `cargo test -p zygo-proxy --locked --test passthrough auxiliary_request_reaches upstream_429_reaches -- --nocapture`. Expected: both pass. Copy the printed header names into this file under `## Root cause`, together with the list of the client headers that zygo did not send.
- [x] **1.5** Commit `passthrough.rs` and this file: `test(proxy): what passthrough sends for an auxiliary request, and how it relays a 429`.

### Task 2: the root cause

- [ ] **2.1** Read only (do not copy any file): the Claude Code transcripts of the LIV-1 runs under `<config dir>/projects/` whose folder name holds `hud-work` or `hud-repo` (LIV-1 D7). Search them for `429` and `rate_limit`. Note only the error type and message text and the time of each 429. Quote no prompt or answer text. If nothing is recorded, write `not recorded`.
- [ ] **2.2** Write the section `## Root cause` in this file. It holds: the facts of Task 1 with file:line; the 429 text from 2.1 (or `not recorded`); the verdict. `zygo`: the error text names a missing header or client identity, or the request differs from main in a way the tests show. `upstream`: the error type is `rate_limit_error` with a usage or rate limit message, and zygo sends auxiliary and main the same way. `not determined`: no error text and no difference. For `zygo`, name the node that must change §12.4 and write the base-spec change-log line it needs.
- [ ] **2.3** Run the acceptance check of the contract. Expected: 0 failed and `rg` exit 0. Commit this file with all steps ticked: `docs(work): FIX-8 root cause of the passthrough auxiliary 429`.

## Root cause

### Task 1: what zygo sends (tests, `crates/zygo-proxy/tests/passthrough.rs`)

Run (step 1.4): `cargo test -p zygo-proxy --locked --test passthrough -- auxiliary_request_reaches upstream_429_reaches --nocapture` → 2 passed, 0 failed. (The command of step 1.4 as written, with two test names before `--`, is refused by cargo: `error: unexpected argument 'upstream_429_reaches' found`; cargo takes one name before `--`, so both names go after `--`.)

- `auxiliary_request_reaches_upstream_as_main_does` (`passthrough.rs:409`): with the Claude Code header set (`cc_headers`, `:402`), a `main` and an `auxiliary` request reach the upstream with the same bytes and the same header map. Upstream header names, printed by the test: `accept`, `anthropic-beta`, `anthropic-version`, `authorization`, `content-length`, `content-type`, `host`. `content-length` and `host` come from the HTTP client; zygo sends no `user-agent`.
- Client headers that zygo does not send (for every request class): `user-agent`, `x-app`, `x-stainless-lang` (and every other `x-stainless-*`), `anthropic-dangerous-direct-browser-access`, `x-claude-code-session-id`, `x-claude-code-request-class` (and every other `x-claude-code-*`), `x-zygo-token`, `x-zygo-session`. Cause in code: the default list `forward.rs:72-75`, the never-list `forward.rs:48-62`, and the forwarded list `passthrough.rs:42`, added by `CredentialType::outbound` (`passthrough.rs:71-86`). No line between `forward.rs:416` and `:444` reads the request class.
- `upstream_429_reaches_the_client_once_and_unchanged` (`passthrough.rs:436`): an upstream 429 with `retry-after: 7` and a `rate_limit_error` body reaches the client once, with the same status, the same `retry-after` and the same JSON body; the upstream gets 1 request (no retry; one candidate, `passthrough.rs:231`; a 429 is kept as `last`, `forward.rs:446`, and relayed, `forward.rs:465-469`, `:503-504`); the usage row has status 429 and class `auxiliary`.

So zygo sends an `auxiliary` request exactly as it sends a `main` request, and it relays a 429 without change. zygo does not make the 429.
