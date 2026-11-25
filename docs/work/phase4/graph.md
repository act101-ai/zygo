# Phase 4 Work Graph

<!-- work-graph -->

**Goal:** Build the phase 4 rows that start now (P4-2 conformance, P4-3 Codex premises, P4-4 proxy Responses client route), and P4-6 (the client contract, after phase 3 merges into `main`), as the approved client-model spec specifies.
**Authority:** `docs/specs/2026-11-25-client-model-design.md` (K§n), approved by the operator 2026-11-25: K§6.2 and K§11 items 1-7 (P4-2, CF nodes); K§6.3.7, K§6.2.5 (premise check) and K§11 items 8-10 (P4-3, PC nodes); K§6.4, K§6.7 and K§11 items 11-22 (P4-4, RS nodes); K§9, K§12 (BS node); K§6.1, K§6.2.7, K§6.9, K§9 (P4-6), K§12 (§8.1 phase 3 features, §8.4, §8.8), K-C7 and K-D3 (amended, lead, 2026-11-25), K-D51, K-D53 and K§11 items 23-30 (P4-6, CC nodes; planned by P4-5); base spec `docs/specs/2026-11-18-zygo-design.md` (§n); order of work `docs/roadmap/phase-4.md` and K§6.9. P4-7 and later rows are not in this graph.
**Run:** parallel=3 · worktrees=.worktrees/phase4/ · disk-per-node=—
**Workflows:** (1) Review gate per node: each node gets its own gate of up to 3 rounds; after round 3 each finding has exactly one disposition: fixed with a commit, rejected with a reason, or recorded with a DDR id (`docs/specs/2026-11-24-decision-records-design.md`); a node passes only when no finding is of a class that stops for the operator — operator, `docs/roadmap/phase-4.md` "Plan of work" item 3. (2) Stacked pull requests: each node is one layer of a GitHub stacked pull request (https://docs.github.com/en/pull-requests/get-started/about-stacked-prs) whose trunk is the rendezvous branch `phase-4`; a node branches from the top of the open stack, so its dependencies are always below it; the controller merges a stack, or its bottom part, into `phase-4` once every layer in it has passed its review gate and its CI is green (`gh stack link --base phase-4 …`; `gh stack merge <pr> --merge --yes`); a node is DONE when its layer merges into `phase-4`; `phase-4` reaches `main` only on the operator's word; K-D26's operator stop on CF-4 still applies, and so does K§6.2.1's operator stop on CC-12 (new registry standard text and tier) — operator, 2026-11-25 ("in the future i prefer you use stacked prs instead of this slow process"), replacing the rule of 2026-11-24 ("we PR after every row, including planning"; "each row merges to that branch so you can be autonomous"; `docs/roadmap/phase-4.md` "Plan of work" item 4). (3) No timing tests in CI (Global constraints) — operator, 2026-11-24 (release v0.2.1 failure, run 45356193851). (4) Stub agents only; the Codex premises run the real `codex` binary only against the mock (Global constraints) — operator rule 2026-11-19; K-C5, K-D14, K§6.3.7. (5) No Claude attribution (Global constraints) — operator, 2026-11-19. (6) Run settings `parallel=3`, `worktrees=.worktrees/phase4/` (`.worktrees/` is ignored at `.gitignore:2`) — operator, P4-1 brief, 2026-11-25.

To continue in any session: "resume the work in `docs/work/phase4/graph.md`".

## Global constraints

Project rules:

- No test that `cargo test --workspace` runs in CI may pass or fail by wall-clock time: deadlines, sleeps, `elapsed` asserts, narrow timeouts, fixed production bounds with no test hook, or a test-side wait under 60 s for a separate process. Use an injected clock. Otherwise the test is `#[ignore]` and is run locally. Never "widen the margin". Never rerun CI without the operator's word — operator, 2026-11-24 (release v0.2.1 failure, run 45356193851); K§4 K-C6
- Premise capture and harness tests use stub or mock programs and a mock upstream (`scripts/premises/mock_upstream.py`). Never use the owner's real Claude Code, OpenCode or Codex login: no `codex login`, and no Codex run against OpenAI or ChatGPT auth or a real `~/.codex`. A Codex premise runs the real `codex` binary with `CODEX_HOME` set to a scratch directory and the provider pointed at the mock upstream — operator rule, 2026-11-19 (the owner's login is never put at risk); K§4 K-C5, K-D14, K-D25, K§6.3.7
- No CI test runs a real `claude`, `opencode` or `codex` binary — K§4 K-C5
- No Claude attribution in commits, pull request bodies or files; each commit ends with the trailer `Zygo-Session: <session>` — operator, 2026-11-19; `docs/specs/2026-11-18-zygo-design.md` §8.5

Spec constraints (K = `docs/specs/2026-11-25-client-model-design.md`):

- The proxy stores and logs no request or response body; the Responses dialect keeps no conversation state on the proxy — K§4 K-C1, K-D31
- Each pass over a body is O(body size) and bounded; a chained translation is two linear passes; a held partial event is bounded by `HOLD_MAX` and no buffer grows with the response — K§4 K-C2; K§6.4.3 stream obligation 9
- The proxy accepts a route by dialect, not by client; wire format keys on `Dialect` (tee usage parse, window-rewrite format, cache-marker placement); client policy (`insert_markers.<client>`, redaction harness spans, window arming) keeps today's per-client sites until P4-6 moves them to the descriptor — K§6.4.2, K-D47, K-D48
- Messages is the translation pivot; a same-dialect pair passes through with reasoning bytes unchanged and unmarked — K-D21, K§6.4.3
- Responses request fields: each top-level field in exactly one list (mapped, ignored, refused); unknown fields, unknown item types and missing fields fail closed with `400 zygo_bad_request` that names them — K§6.4.3, K-D49, K§6.7
- An upstream error before the first byte keeps its HTTP status with an OpenAI error body; `response.failed` only after the stream started; each started stream ends with exactly one terminal event — K§6.4.4, K-D50
- Registry and grade files are hostile input (the K§6.2.5 input rules); a bar exception lives only in the registry; a change of standard text, tier or bar exception stops for the operator — K§6.2.1, K§6.2.5, K-D18
- P4-2 grades the criteria as written; each derived cell that differs from today's hand grade is listed in the pull request with its reason, and that pull request stops for the operator: CF-4's layer does not merge without the operator — K-D26
- P4-6 adds registry entries for the phase 3 client features (K-D51); new standard text and its tier stop for the operator: CC-12's layer does not merge without the operator — K§6.2.1
- A change to the boundary-check exception list (`docs/conformance/boundary-exceptions.toml`) stops for the operator — K§6.1.4, K-D53 (operator, 2026-11-25)
- Codex minimum version 0.172.0 — K§4 K-C8
- Codex premises run against `mock_upstream.py` directly, with no zygo between, under `env -i` with a temporary `HOME` and `CODEX_HOME`, a dummy key built at run time, and `HTTPS_PROXY` at the mock, which refuses and records each `CONNECT` — K§6.3.7, K-D25
- The base spec's `docs/premises.md` method holds for every fixture: `scrub.py` hygiene, no token, key form, user name or home path — K§11 item 10; `docs/premises.md` "Method"

## Verification floor

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` — `.github/workflows/ci.yml:45` (job `test`); `.github/workflows/release.yml:30`
- Install script, unix: `sh scripts/test-install.sh`; Windows: `pwsh scripts/test-install.ps1` — `.github/workflows/ci.yml` (job `test`)
- Premises, local: `sh scripts/check-premises.sh` → exit 0 (with `N2_FORBIDDEN_NAMES` set at run time for the fixture nodes) — `docs/premises.md` "Method"; K§6.2.5
- Conformance outputs, after CF-4: `ZYGO_WRITE_CONFORMANCE=1 cargo test -p zygo-core --test conformance --locked` leaves `git diff --exit-code docs/conformance.md docs/conformance.json` clean — K§6.2.5 item 10
- Live check LC-CX1: owner-run only, never by an agent, never in CI — K§6.3.7 (PC16); `docs/live-checks.md` "Method"
- Test edits, each P4-6 node after CC-2: `sh scripts/check-test-edits.sh <parent> <node file>` → exit 0, `<parent>` from the `Parent:` line of the node's plan (the parent of its layer) — K§4 K-C7 (amended), K§11 item 24

## Review focus

1. Evidence that only looks real: a `test:` function with `#[ignore = "…"]`, a function of that name in a `mod` under `#[cfg(test)]` of another file, a `premise:` row whose result is `failed` or `observed`, an id that is only in the other premise file, a fixture that is a symbolic link or `{}` — none counts — test in CF-1 — K§6.2.3, K§6.2.5 item 3
2. Hostile registry and grade input: an id like `A-7.1x`, an evidence path that is absolute, holds `..`, or leaves the repository through a link, a reason with a line break or over 300 characters, an unknown key — refused — test in CF-1 — K§6.2.5 input rules
3. Score rounding: a tier score rounded before the total is computed gives 88, not 89, on the K§6.2.6 example — round only last — test in CF-2 — K§6.2.4, K§6.2.6
4. A `|` in a reason or URL, and CR LF line ends on Windows checkout — the table stays one row; the compare is equal after CR LF → LF — test in CF-3 — K§6.2.5 item 10, input rules
5. The data-driven premise check passing on a table that lost rows (`Q7` removed, a `PW` id removed) or on an agent name with no header line — it fails — test in PC-1 — K§6.2.5 last paragraph
6. A Codex run that reaches a real host (OpenAI, ChatGPT, the npm registry) or reads the owner's `~/.codex` — every other host is a refused `CONNECT` record; `HOME` and `CODEX_HOME` are scratch, recorded in `fixtures/codex/runs/<part>/isolation.txt` — check in PC-2 (harness and its record), PC-3 to PC-6 (each run) — K§6.3.7
7. A Responses `input` item of an unknown `type`, a `function_call_output` with no `call_id`, `previous_response_id`, `text.format` of type `json_schema` toward an Anthropic upstream — `400 zygo_bad_request` naming it, never a silent drop — test in RS-1 — K§6.4.3 obligations 8-10, K§6.7
8. A client-sent zygo-marked reasoning value that does not decode, is over the size bound, or names a provider or model not among the session's candidates — dropped with an adjustment code, never selects a provider — test in RS-1 — K§6.7
9. Upstream stream faults: an event whose JSON does not parse after the first byte, a `ping`, an unknown event, an error event after text — one `response.failed` and no second terminal event; `sequence_number` stays strictly rising; a partial line or one tool call's arguments past `HOLD_MAX` end the stream and free the buffer — test in RS-2 — K§6.4.3 stream obligations 4, 6, 7, 9
10. A `529` before the first byte on a streaming request — the client gets `529`, an OpenAI error body and no SSE bytes; an over-long prompt reaches the client in the PC14 shape — test in RS-3; an error after the first event at the route — test in RS-4 — K§6.4.4
11. Same-dialect passthrough that re-serializes reasoning items or drops `include` entries — reasoning items byte-equal; `store` is `false`; the usage row has the `response.completed` counts; a mapped chat stream (chat upstream to a Messages client) is never parsed as Responses lines — test in RS-5 — K§6.4.3, K§6.4.2 (C3)
12. Cache markers added to a Responses or chat upstream body, or to the Anthropic client's own request stage twice — markers only on the Anthropic upstream body of a Responses client; no `cache_control` anywhere toward a Responses upstream — tests in RS-6, RS-5 — K§6.4.2 cache row, K-D48
13. Window rewrite applied after the encoder, or on an unarmed session — the tee rewrites the Messages `message_delta` before the encoder, or the Responses `response.completed` usage on the same-dialect path; unarmed bytes pass unchanged — test in RS-7 — K§6.4.5, K§6.4.2
14. A test edit that changes behavior while the check passes: a changed literal, a removed or renamed test function, an added `#[ignore]`, a `#[cfg(test)]` module moved to another file, a listed delta that differs from the real one — each fails unless listed exactly; a doc-comment or type-path edit passes — test in CC-2 — K-C7 (amended), K§11 item 24
15. A state file whose pane kind is `"opencode"`, `"shell"` or unknown — the first two read and write back byte-equal; the unknown one is refused as at the base — test in CC-8 — K§6.1.2 (`PaneKind`), K§11 item 29
16. A client name that escapes the boundary check: inside `format!` or `json!`, as a raw or byte string, in a non-module item with no `#[cfg(test)]` — flagged; a name inside a longer literal or a comment — not flagged; the guard passes only through its exception entry, and a planted literal elsewhere still fails — test in CC-15 — K§6.1.4, K§11 item 26
17. A launch path that skips the preview gate: `zygo ws run` for a member pane, the loadout check, `ZYGO_PREVIEW_CLIENT` set to another client's id — refused with the K§6.2.7 text — test in CC-11 — K§6.2.7
18. An `adapter_for` table with a client of `Agent::ALL` missing, or a client with two rows — the table test fails — test in CC-9 — K§6.1.1, K§11 item 27

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| CF-1 | Conformance input model and check: `Agent::ALL`, registry and grade parsing with the input rules, evidence kinds, check items 1-9 on fixture inputs | — | TODO | judgment | — |
| CF-2 | Conformance derived values: cell, tier and total score, bar, rollup, release state, on fixture inputs | CF-1 | TODO | implementation | — |
| CF-3 | Conformance outputs: feature matrix, rollup and score in Markdown and JSON, the write mode and check item 10, on fixture inputs | CF-2 | TODO | implementation | — |
| CF-4 | Registry `capabilities.toml`, grade files for Claude Code and OpenCode, generated `docs/conformance.md` and `.json`, the tree test, CI paths, old scripts deleted (PR stops for the operator, K-D26) | CF-3 | TODO | judgment | — |
| PC-1 | `scripts/check-premises.sh` driven by data, with `scripts/test-check-premises.sh` | — | IN-PROGRESS | judgment | acceptance exit 0 @ 1a89e491 |
| PC-2 | Codex premise harness: every mock and scrub change of PC-3 to PC-6, codex-only `lib.sh` with an isolation record, `run-codex.sh`, help and schema captures | PC-1 | TODO | judgment | — |
| PC-3 | Codex proxy-facing captures: PC1, PC2, PC7, PC13, PC14 (requests, hosts, headers, models, injected items, context error) | PC-2 | TODO | implementation | — |
| PC-4 | Codex config, trust and hook captures: PC3, PC4, PC5, PC12; the `--strict-config` run of a config with zygo's keys | PC-2 | TODO | implementation | — |
| PC-5 | Codex agents, compaction, MCP, status line and skills captures: PC6, PC8, PC9, PC11, PC15 | PC-2 | TODO | implementation | — |
| PC-6 | Codex resume and update captures: PC10, PC16 | PC-2 | TODO | implementation | — |
| PC-7 | `docs/premises.md` and `docs/live-checks.md`: `Codex` headers, rows PC1-PC16, LC-CX1, Codex method lines | PC-1, PC-3, PC-4, PC-5, PC-6 | TODO | mechanical | — |
| RS-1 | Responses → Messages request translator and the zygo mark on reasoning items (`translate/responses_client.rs`, `translate/reasoning.rs`) | PC-3 | TODO | judgment | — |
| RS-2 | Messages → Responses encoder: SSE stream and JSON response (`translate/responses_client.rs`) | RS-1 | TODO | judgment | — |
| RS-3 | `POST /openai/v1/responses` route, test harness, request side of forward, errors before the first byte, redaction on `input` | RS-1 | TODO | judgment | — |
| RS-4 | Response side for Anthropic and chat upstreams: stream order with the encoder after the tee, non-streaming path, errors after the first event | RS-2, RS-3 | TODO | judgment | — |
| RS-5 | Responses → Responses passthrough, `ResponseCtx.lines`, the tee's `response.completed` usage parse | RS-4 | TODO | judgment | — |
| RS-6 | Cache markers at forward time on the Anthropic upstream body of a Responses client | RS-5 | TODO | judgment | — |
| RS-7 | Window rewrite for a Responses client, Anthropic and Responses upstreams | RS-5 | TODO | judgment | — |
| RS-8 | `GET /openai/v1/models` in the PC7 shape, other PC1 paths, chat route stays 404 | RS-3 | TODO | implementation | — |
| BS-1 | Base-spec notes: §8.1 (the registry is the capability list) and §7.6 v2 (the Responses client route is built) | CF-4, RS-5, RS-6, RS-7, RS-8 | TODO | mechanical | — |
| CC-0 | Take `main` (with phase 3) into `phase-4` | CF-1, CF-2, CF-3, CF-4, PC-1, PC-2, PC-3, PC-4, PC-5, PC-6, PC-7, RS-1, RS-2, RS-3, RS-4, RS-5, RS-6, RS-7, RS-8, BS-1 | BLOCKED(operator: phase 3 merged into main) | implementation | — |
| CC-1 | Client-branch inventory at the P4-6 base commit, written under Observed (K§6.9) | CC-0 | TODO | judgment | — |
| CC-2 | `scripts/check-test-edits.sh` and its `syn` helper: the per-node test-edit check (K-C7, amended) | CC-0 | TODO | implementation | — |
| CC-3 | Core client module `zygo_core::client`: descriptor type, concerns 1 and 2, `Agent::ALL`/`DEFAULT`/`spec()`, `Dialect` in core, minimum version from the descriptor | CC-1, CC-2, CF-4 | TODO | judgment | — |
| CC-4 | Proxy, redaction, usage and work-join client facts (concern 9) read the descriptor | CC-3, RS-6, RS-7 | TODO | judgment | — |
| CC-5 | MCP tool naming and `compact` tool listing (concern 5) read the descriptor | CC-4 | TODO | implementation | — |
| CC-6 | Hook output and session text delivery (concerns 3, 4) read the descriptor | CC-5 | TODO | implementation | — |
| CC-7 | Argument grammar and session flags (concern 7) read the descriptor | CC-6 | TODO | implementation | — |
| CC-8 | Pane kind, resume kind, turn events, workspace definition kinds (concern 8); `PaneKind` = `Agent(Agent)` or `Shell` | CC-7 | TODO | judgment | — |
| CC-9 | Launch layer: `adapter_for` registry, adapters under `client/`, plugin delivery and env rows (concerns 6, 11), `Plugins` removed | CC-8 | TODO | judgment | — |
| CC-10 | CLI shims under `cmd/client/`; binary-crate client sites read the registry (concern 10) | CC-9 | TODO | implementation | — |
| CC-11 | Preview gate at launch for every entry point, loadout check, hidden help and doctor (K§6.2.7) | CC-10 | TODO | judgment | — |
| CC-12 | Phase 3 client features in the registry: output shaping, mail wake and delivery, run-time status line (K-D51; PR stops for the operator) | CC-1, CF-4 | TODO | judgment | — |
| CC-13 | Sweep the `Agent::` variant paths left in tests | CC-11 | TODO | mechanical | — |
| CC-14 | Opaque client kind `pub struct Agent(Kind)` and the `trybuild` compile-fail cases | CC-13 | TODO | judgment | — |
| CC-15 | Boundary check `client_boundary` (K§6.1.4) with its exception list | CC-14 | TODO | judgment | — |
| CC-16 | Base-spec notes: §8.1 (phase 3 features), §8.4 (`ZYGO_AGENT` ids, client denylist rows), §8.8 (superseded) | BS-1, CC-12, CC-15 | TODO | mechanical | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-25 | Graph created from `docs/specs/2026-11-25-client-model-design.md` (P4-1): 19 nodes for P4-2, P4-3, P4-4 at base `ec17582b`. |
| 2026-11-25 | Audit round 1: 18 findings fixed as the coordinator ruled; RS-3 split into RS-3 and RS-4 (RS renumbered to RS-8); 20 nodes; Workflows (2) now stacked PRs (operator, 2026-11-25). |
| 2026-11-25 | P4-5: 16 nodes CC-0 to CC-15 for P4-6 (K§6.1, K§6.2.7, K§11 items 23-30); CC-0 blocked until the operator merges phase 3 into `main`; 36 nodes. |
| 2026-11-25 | Audit of the CC nodes (3 critical, 6 important, 5 minor) fixed as ruled: K-C7, K-D3, items 23-24 amended (lead); boundary exception list (operator, K-D53); CC-13 split (sweep, opaque kind); 17 CC nodes, 37 nodes. |
