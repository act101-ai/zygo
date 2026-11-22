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

- [x] **2.1** Read only (do not copy any file): the Claude Code transcripts of the LIV-1 runs under `<config dir>/projects/` whose folder name holds `hud-work` or `hud-repo` (LIV-1 D7). Search them for `429` and `rate_limit`. Note only the error type and message text and the time of each 429. Quote no prompt or answer text. If nothing is recorded, write `not recorded`.
- [x] **2.2** Write the section `## Root cause` in this file. It holds: the facts of Task 1 with file:line; the 429 text from 2.1 (or `not recorded`); the verdict. `zygo`: the error text names a missing header or client identity, or the request differs from main in a way the tests show. `upstream`: the error type is `rate_limit_error` with a usage or rate limit message, and zygo sends auxiliary and main the same way. `not determined`: no error text and no difference. For `zygo`, name the node that must change §12.4 and write the base-spec change-log line it needs.
- [x] **2.3** Run the acceptance check of the contract. Expected: 0 failed and `rg` exit 0. Commit this file with all steps ticked: `docs(work): FIX-8 root cause of the passthrough auxiliary 429`.

## Root cause

### Task 1: what zygo sends (tests, `crates/zygo-proxy/tests/passthrough.rs`)

Run (step 1.4): `cargo test -p zygo-proxy --locked --test passthrough -- auxiliary_request_reaches upstream_429_reaches --nocapture` → 2 passed, 0 failed. (The command of step 1.4 as written, with two test names before `--`, is refused by cargo: `error: unexpected argument 'upstream_429_reaches' found`; cargo takes one name before `--`, so both names go after `--`.)

- `auxiliary_request_reaches_upstream_as_main_does` (`passthrough.rs:409`): with the Claude Code header set (`cc_headers`, `:402`), a `main` and an `auxiliary` request reach the upstream with the same bytes and the same header map. Upstream header names, printed by the test: `accept`, `anthropic-beta`, `anthropic-version`, `authorization`, `content-length`, `content-type`, `host`. `content-length` and `host` come from the HTTP client; zygo sends no `user-agent`.
- Client headers that zygo does not send (for every request class): `user-agent`, `x-app`, `x-stainless-lang` (and every other `x-stainless-*`), `anthropic-dangerous-direct-browser-access`, `x-claude-code-session-id`, `x-claude-code-request-class` (and every other `x-claude-code-*`), `x-zygo-token`, `x-zygo-session`. Cause in code: the default list `forward.rs:72-75`, the never-list `forward.rs:48-62`, and the forwarded list `passthrough.rs:42`, added by `CredentialType::outbound` (`passthrough.rs:71-86`). No line between `forward.rs:416` and `:444` reads the request class.
- `upstream_429_reaches_the_client_once_and_unchanged` (`passthrough.rs:436`): an upstream 429 with `retry-after: 7` and a `rate_limit_error` body reaches the client once, with the same status, the same `retry-after` and the same JSON body; the upstream gets 1 request (no retry; one candidate, `passthrough.rs:231`; a 429 is kept as `last`, `forward.rs:446`, and relayed, `forward.rs:465-469`, `:503-504`); the usage row has status 429 and class `auxiliary`.

So zygo sends an `auxiliary` request exactly as it sends a `main` request, and it relays a 429 without change. zygo does not make the 429.

### Step 2.1: the 429 text

- Claude Code transcripts of LIV-1 (`<config dir>/projects/`, folders with `hud-work` or `hud-repo`): 3 transcripts, one per run (runs 2, 3, 4, session a). Search for `429`, `rate_limit`, `error`, `isApiErrorMessage`: no API error record; the only matches are `429` inside UUIDs and the word in hook text. Each transcript has one assistant answer with `stop_reason` `end_turn`. **429 text: not recorded.**
- zygo's session folders of the LIV-1 sessions: not present (the script removes its scratch dir; `$HOME/.cache/zygo/sessions/` holds only the operator's sessions). zygo does not store an upstream error body (`crates/zygo-usage/src/schema.rs`: status only).
- zygo usage rows (`$HOME/.local/share/zygo/usage.db`, read with `sqlite3 -readonly`), LIV-1 sessions, 2026-11-22 UTC, model `claude-opus-5-5`:

  | session (start) | first row | then |
  |---|---|---|
  | 01M8167WRH (run 2 a) | 08:33:57 `auxiliary` 429, 274 ms, 0 tokens | 08:33:59 `auxiliary` 200 (1176 in / 15 out); `main` 503, 503; 08:34:02 `main` 200 |
  | 01M8169SEY, 01M8169ZYG, 01M816BCZQ (run 2 b, c, d) | 08:34:57, 08:35:04, 08:35:50 `auxiliary` 429 | no other row |
  | 01M816HH1B (run 3 a) | 08:39:13 `auxiliary` 429, 274 ms | 08:39:15 `auxiliary` 200; 08:39:17 `main` 200 |
  | 01M816JYCK, 01M816K4AK, 01M816MHB9 (run 3 b, c, d) | 08:39:57, 08:40:03, 08:40:49 `auxiliary` 429 | no other row |
  | 01M816SW4G (run 4 a) | 08:43:48 `auxiliary` 429, 294 ms | 08:43:50 `auxiliary` 200; 08:43:53 `main` 200 |
  | 01M816VB76, 01M816VHM8, 01M816WZJM (run 4 b, c, d) | 08:44:32, 08:44:39, 08:45:26 `auxiliary` 429 | no other row |

  In session a the 429 request starts about 2 s before the prompt record of the transcript (08:33:59.77 in run 2): it is a request that Claude Code sends at start, before the first prompt. Sessions b, c, d end before a prompt, so this start request is their only request.
- The same pattern in all passthrough sessions of the store: 24 passthrough sessions have an `auxiliary` request (2026-11-21 06:14 to 2026-11-22 08:45 UTC); in **24 of 24** the first `auxiliary` request is a 429 (latency 135-403 ms, mean 256 ms, 0 tokens), and in each long session `auxiliary` requests after it are answered 200 (2135 `auxiliary` 200 rows in all). Only 26 `auxiliary` 429 rows exist in all; 24 of them are the first `auxiliary` request of a session.

### Verdict: `not determined`

- Not `zygo` by the definition of step 2.2: no error text names a missing header or a client identity (no error text is recorded), and the tests show no difference between `main` and `auxiliary` in what zygo sends. The headers that zygo drops are dropped from every request, also from the `auxiliary` and `main` requests of the same session that get 200 two seconds later; a header difference alone does not explain a refusal of only the first request.
- Not `upstream` by the definition of step 2.2: no error text with `rate_limit_error` and a rate or usage limit message is recorded.
- The 5-hour usage limit of the operator's account is not the cause: the same 429 comes first in every passthrough session for 26 hours (from 2026-11-21 06:14 UTC), and in each LIV-1 session a, `main` gets 200 3 to 5 s after the 429 (08:34:02, 08:39:17, 08:43:53); a usage limit refuses those requests too. Passthrough `main` and `subagent` 429 rows exist only at 2026-11-22 02:22-02:23 and 05:51-05:52 (6 rows), not in the LIV-1 window.
- What the evidence shows: the refused request is the start request of Claude Code (first request of each session, sent before the first prompt), not the `auxiliary` class in general. Its body is not recorded; zygo sends the received bytes when no stage changed the body (`forward.rs:426`, `:827`, `:839-840`), so whether the vendor refuses it in a direct Claude Code session too, or only without the dropped client headers, is not shown by any record.

What a next node needs to decide it (no change to zygo in this unit): record, for a non-2xx upstream answer on a passthrough request, the upstream `error.type` and `error.message` (no content of the request) in the session log or the usage row, then run one passthrough session start; or compare one start of Claude Code with and without zygo, through the mock-free live check of §20 #28. If the text then names a client identity or a missing header, the change is to §12.4 (the forwarded list `passthrough.rs:42`), with a base-spec change-log line such as "§12.4: passthrough also forwards `<header>` to the type's host; LIV-1 D8, FIX-8."

