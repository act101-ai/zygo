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
| CF-1 | Conformance input model and check: `Agent::ALL`, registry and grade parsing with the input rules, evidence kinds, check items 1-9 on fixture inputs | — | DONE | judgment | PR #87 merged 74562e3b; acceptance 65 check_/input_ tests pass @ 3aceaac3; CI run 45605170068 green (ubuntu, macos, windows) |
| CF-2 | Conformance derived values: cell, tier and total score, bar, rollup, release state, on fixture inputs | CF-1 | DONE | implementation | PR #100 merged 3e98ae7c; acceptance 15 derive_ tests, conformance 84 pass @ 11c0dd70; CI run 45610449986 green (ubuntu, macos, windows) |
| CF-3 | Conformance outputs: feature matrix, rollup and score in Markdown and JSON, the write mode and check item 10, on fixture inputs | CF-2 | DONE | implementation | PR #102 merged 1c147219; acceptance 25 render_ tests, conformance 109 pass @ 80118dc2; CI run 45618690411 green (ubuntu, macos, windows) |
| CF-4 | Registry `capabilities.toml`, grade files for Claude Code and OpenCode, generated `docs/conformance.md` and `.json`, the tree test, CI paths, old scripts deleted (PR stops for the operator, K-D26) | CF-3, OC-1 | DONE | judgment | PR #104 merged 278f2cd4; acceptance exit 0 on merged tree (conformance 120 pass); CI run 45707423597 green (ubuntu, macos, windows) |
| OC-1 | OpenCode: session token out of the environment and config (apiKey from the token file), providers limited to zygo | — | DONE | judgment | PR #128 merged 53eabe25; acceptance exit 0 on merged tree; CI run 45693613400 green (ubuntu, macos, windows) |
| PC-1 | `scripts/check-premises.sh` driven by data, with `scripts/test-check-premises.sh` | — | DONE | judgment | PR #91 merged adac6bc0; acceptance exit 0 @ 54039f94 (test-check-premises 50 cases); CI run 45678686867 green (ubuntu, macos, windows) |
| PC-2 | Codex premise harness: every mock and scrub change of PC-3 to PC-6, codex-only `lib.sh` with an isolation record, `run-codex.sh`, help and schema captures | PC-1 | DONE | judgment | PR #126 merged 1206befa; acceptance exit 0 @ 4784e06b (real codex 0.172.0 run, ~/.codex unchanged); CI run 45698780636 green (ubuntu, macos, windows) |
| PC-3 | Codex proxy-facing captures: PC1, PC2, PC7, PC13, PC14 (requests, hosts, headers, models, injected items, context error) | PC-2, PC-10 | DONE | implementation | PR #137 merged 0d665799; acceptance exit 0 @ e554d2ca (real codex 0.172.0; PC1 PC2 PC7 PC13 PC14 verified); CI run 45725231426 green (ubuntu, macos, windows) |
| PC-4 | Codex config, trust and hook captures: PC3, PC4, PC5, PC12; the `--strict-config` run of a config with zygo's keys | PC-2, PC-10 | TODO | implementation | — |
| PC-5 | Codex agents, compaction, MCP, status line and skills captures: PC6, PC8, PC9, PC11, PC15 | PC-2, PC-10 | DONE | implementation | PR #147 merged 08774771; acceptance exit 0 @ 6cbfc673 (real codex 0.172.0; PC6 PC8 PC9 PC11 verified, PC15 failed); CI run 45735709999 green (ubuntu, macos, windows) |
| PC-6 | Codex resume and update captures: PC10, PC16 | PC-2, PC-10 | DONE | implementation | PR #138 merged 7ecef460; acceptance exit 0 @ 86ddb8e3 (real codex 0.172.0; PC10 PC16 verified); CI run 45735047521 green (ubuntu, macos, windows) |
| PC-7 | `docs/premises.md` and `docs/live-checks.md`: `Codex` headers, rows PC1-PC16, LC-CX1, Codex method lines | PC-1, PC-3, PC-4, PC-5, PC-6, OC-1, PC-11 | TODO | mechanical | — |
| PC-8 | check-premises.sh: the name scan exempts the verbatim schemas; an indented table row is not a premise row | PC-1 | DONE | implementation | PR #127 merged 73a4ff40; acceptance exit 0 on merged tree (test-check-premises 99 cases); CI run 45704926916 green (ubuntu, macos, windows) |
| PC-9 | `scripts/premises/ver_test.sh`: its repo copy includes every evidence path the premise rows cite | PC-1 | DONE | mechanical | PR #136 merged 256e5cae; acceptance exit 0 on merged tree (ver_test 62, test-check-premises 102); CI run 45716799629 green (ubuntu, macos, windows) |
| PC-10 | Premise harness privacy: TZ=UTC for Codex runs; scrub OS in user agents and double-slash scratch paths | PC-2 | DONE | implementation | PR #141 merged a1c0ce8a; acceptance exit 0 on merged tree (scrub 70, lib 156, mock 56); CI run 45716982108 green (ubuntu, macos, windows) |
| PC-11 | Scrub: platformOs, login shell and macOS writable roots in Codex captures; namespace tool descriptions; re-scrub committed Codex fixtures | PC-3, PC-4, PC-5, PC-6, PC-12 | TODO | implementation | — |
| PC-12 | Codex premise: the Responses event sequence zygo's encoder emits (empty terminal output, extra item and delta events) is accepted | PC-10, RS-2 | TODO | judgment | — |
| RS-1 | Responses → Messages request translator and the zygo mark on reasoning items (`translate/responses_client.rs`, `translate/reasoning.rs`) | PC-3 | DONE | judgment | PR #152 merged fe18f3dd; acceptance exit 0 on merged tree (responses_request 49, translate 48); CI run 45733206649 green (ubuntu, macos, windows) |
| RS-2 | Messages → Responses encoder: SSE stream and JSON response (`translate/responses_client.rs`) | RS-1 | DONE | judgment | PR #156 merged 1ad733f0; acceptance exit 0 on merged tree (responses_stream 29); CI run 45738657210 green (ubuntu, macos, windows) |
| RS-3 | `POST /openai/v1/responses` route, test harness, request side of forward, errors before the first byte, redaction on `input` | RS-1 | TODO | judgment | — |
| RS-4 | Response side for Anthropic and chat upstreams: stream order with the encoder after the tee, non-streaming path, errors after the first event | RS-2, RS-3, PC-12 | TODO | judgment | — |
| RS-5 | Responses → Responses passthrough, `ResponseCtx.lines`, the tee's `response.completed` usage parse | RS-4 | TODO | judgment | — |
| RS-6 | Cache markers at forward time on the Anthropic upstream body of a Responses client | RS-5 | TODO | judgment | — |
| RS-7 | Window rewrite for a Responses client, Anthropic and Responses upstreams | RS-5 | TODO | judgment | — |
| RS-8 | `GET /openai/v1/models` in the PC7 shape, other PC1 paths, chat route stays 404 | RS-3 | TODO | implementation | — |
| RS-9 | Codex namespace tools: flatten into function tools for Anthropic and chat upstreams, map their calls back | RS-1, RS-2, PC-5 | TODO | judgment | — |
| BS-1 | Base-spec notes: §8.1 (the registry is the capability list) and §7.6 v2 (the Responses client route is built) | CF-4, RS-5, RS-6, RS-7, RS-8, RS-9 | TODO | mechanical | — |
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
| 2026-11-25 | CF-1 DONE: gate 3 rounds; round 3 review findings F1, F3-F7 fixed, F2 rejected (reason in `nodes/CF-1.md`); two implementer-found defects fixed; PR #87 merged `74562e3b`. |
| 2026-11-25 | CF-2 DONE: gate 2 rounds; round 1 F1-F4 fixed (F4: K§6.2.4 failing-core sentence, author's call), round 2 N1-N2 fixed; PR #100 merged `3e98ae7c`. |
| 2026-11-25 | CF-3 DONE: gate 3 rounds; round 1 F1-F6 fixed, round 2 D1-D5 fixed (one md_text escape rule, cell URL as a link), round 3 no findings; PR #102 merged `1c147219`. |
| 2026-11-25 | PC-1 DONE: gate 3 rounds; round 3 F1-F5, F7-F11 fixed, F6 rejected (reason in nodes/PC-1.md); PR #91 merged adac6bc0. |
| 2026-11-25 | PC-8 added (author's call): two check-premises.sh defects the PC-1 round 3 implementer found. |
| 2026-11-25 | Operator: OpenCode A-1.2, A-1.3, A-10.2 fixed in code (OC-1, before CF-4); OpenCode A-1.4 and Claude Code A-1.4 bar exceptions; Claude Code A-1.2 and A-10.2 reworded "passthrough excepted (§12.3)". CF-4 applies the registry and spec changes. |
| 2026-11-25 | PC-7 depends on OC-1 (author's call): both change `docs/premises.md` (OC-1 adds the OpenCode rows PO1, PO2). |
| 2026-11-25 | OC-1 DONE: gate 3 rounds; round 1 F1-F12 (F3 rejected: CF-4 regrade), round 2 G1-G5, round 3 H1-H6 fixed (H3 narrowed), H7 rejected (reasons in nodes/OC-1.md); PR #128 merged 53eabe25. |
| 2026-11-25 | PC-2 DONE: gate 3 rounds; round 1 F1-F11, round 2 G1-G10, round 3 H1-H7 fixed, thin-launcher limit recorded (nodes/PC-2.md); PR #126 merged 1206befa. |
| 2026-11-25 | PC-9 added (author's call): ver_test.sh K1/K4 fail since OC-1's PO1 evidence path. |
| 2026-11-25 | PC-10 added (author's call): Codex fixtures leaked the host timezone and OS (PC-3 review F4) and scrub mis-mapped double-slash scratch paths (PC-6); PC-3..PC-6 captures re-taken after it. |
| 2026-11-25 | PC-8 DONE: gate 3 rounds; round 1 F1-F5, round 2 G1-G7, round 3 H1-H4 fixed, H5 recorded (nodes/PC-8.md); PR #127 merged 73a4ff40. |
| 2026-11-25 | CF-4 DONE: operator stop (A-1.2/A-10.2 passthrough excepted; OpenCode A-1.4 bar exception; OpenCode A-1.2/A-1.3/A-10.2 fixed by OC-1); gate 3 rounds, findings fixed/recorded (nodes/CF-4.md); lead's calls: OpenCode A-1.3 met, A-1 key-helper reading (K-D18); PR #104 merged 278f2cd4. |
| 2026-11-25 | PC-10 DONE: gate 3 rounds; TZ=UTC, OS and timezone scrub, daemon-socket guard, leftover-process check, refused-path records, second mock key; limits recorded (nodes/PC-10.md); PR #141 merged a1c0ce8a. |
| 2026-11-25 | PC-9 DONE: gate 2 rounds + nit round; evidence paths must be plain repo paths (no symlinked dir), ver_test copies cited evidence; G5 case-variant limit recorded (nodes/PC-9.md); PR #136 merged 256e5cae. |
| 2026-11-25 | PC-11 added (author's call): Codex captures keep platformOs, the login shell, macOS writable roots and namespace tool descriptions (PC-3 review G3); scrub and re-scrub in place before PC-7. |
| 2026-11-25 | PC-3 DONE: gate 3 rounds; recaptured on the PC-10 harness; apply_patch via a minimal catalog (run p) only; item-id/platformOs/shell scrub gaps to PC-11; PR #137 merged 0d665799. |
| 2026-11-25 | RS-9 added (author's call): RS-1 drops Codex namespace tools (multi_agent_v1 subagent tools) on Anthropic/chat upstreams; RS-9 flattens them and maps calls back from PC-5's captures. Remote-compaction items noted for P4-9. |
| 2026-11-26 | RS-1 DONE: gate 3 rounds; K§6.4.3 field/tool/item classes (client_metadata ignored; namespace dropped → RS-9); user-first, pairing, adjacency, history-only tools for compaction; PR #152 merged fe18f3dd. |
| 2026-11-26 | PC-6 DONE: gate 3 rounds; PC10 verified; PC16 verified by a controlled count (+1 api.github.com CONNECT, key on vs off); item 52 now cites the P4-11 zygo-launch row for Codex; K§6.5.4 records the measured source; PR #138 merged 7ecef460. |
| 2026-11-26 | PC-12 added (author's call): confirm against real Codex that the encoder's event sequence (empty terminal output, extra events) is accepted, before RS-4 wires it in. |
| 2026-11-26 | RS-1 Files corrected: the PC-5 compaction fixture was read, not changed. |
| 2026-11-26 | PC-11 now depends on PC-12 (author's call): PC-12 adds premise rows and encoder fixtures that PC-11 re-scrubs and PC-7 reads. |
| 2026-11-26 | PC-5 DONE: gate 3 rounds; PC8 verified on end-of-turn compaction in one live TUI process (plus cold resume and mid-turn); PC15 failed (skills.config path selects, does not add); TUI /mcp POST is Codex's codex_tui MCP client; PR #147 merged 08774771. |
| 2026-11-26 | RS-2 DONE: gate 1 round + nit round; Messages→Responses encoder and JSON form; custom_tool_call for client custom tools; marks bounded by MARK_MAX; HOLD_MAX holds within a push; stop-reason mapping; event sequence documented for PC-12; PR #156 merged 1ad733f0. |
