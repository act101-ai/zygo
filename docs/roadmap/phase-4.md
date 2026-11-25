# zygo phase 4 roadmap

**Status:** started 2026-11-25. One spec, proposed 2026-11-25: `docs/specs/2026-11-25-client-model-design.md` (`K§n`). Next: operator approval of the spec, then one work graph for the phase (see "Plan of work").
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-21-auto-update-design.md`, `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`).

## Intent

The operator's request (2026-11-25): "i would like to add codex support to zygo. with adding codex support, we need two things. we need to have a clear architectural model for client implementations so they are straightforward to implement, update, and grade. we will maintain a feature matrix conformance score. for every feature that any zygo feature a client supports, all clients get graded against their support for that client, and a rubric is built, clients are measured and scored and found wanting. but sufficient. :)"

Two additions of the same day (operator): "we need to produce a feature matrix, a rollup, and a score for each client." And: "we want background update checks and post to the hud when there's an update." On the wrapped clients' own updates: each client has a flag in the client abstraction that says whether zygo announces its updates; the default follows whether the client announces its own updates by default.

A **client** is a coding agent that zygo wraps: today Claude Code and OpenCode, in phase 4 also Codex CLI.

The gap today: client behavior is spread over 35 source files as branches on the client kind (K§1); the conformance document is two hand-written tables that no CI job checks; the proxy serves only the Anthropic Messages dialect, and Codex speaks only the OpenAI Responses API; a session never learns of a zygo release after its start.

The direction: one client contract with a check that keeps client branches inside it; a capability registry with a rubric of checkable criteria, per-client grade files with measured evidence, and a generated feature matrix, rollup and score with a sufficiency bar; the Responses API as a proxy client dialect; `zygo codex`; a background update check in the launcher that posts to the HUD.

## Items

Numbered in causal order.

| Id | Item | Depends | Spec |
|---|---|---|---|
| P4-1 | Capability registry and rubric (A-1 to A-21, WA-1, WA-2 split into criteria), grade files for Claude Code and OpenCode, the generator and check (a Rust test), the three outputs (feature matrix, rollup, score) in `docs/conformance.md` and `docs/conformance.json`, the sufficiency bar | none | `2026-11-25-client-model-design.md` (K§6.2) |
| P4-2 | Codex premises and fixtures (PC1 to PC16) against the mock upstream; no login | none | `2026-11-25-client-model-design.md` (K§6.3.7) |
| P4-3 | Proxy: the OpenAI Responses API as a client dialect (`/openai/v1/responses` with SSE, `/openai/v1/models`), to Anthropic, Responses and chat upstreams | P4-2 (captured Codex requests are its test input) | `2026-11-25-client-model-design.md` (K§6.4) |
| P4-4 | Client contract: descriptor and adapter registries, Claude Code and OpenCode moved onto it with no behavior change, the boundary check, the `supported` flag and the preview gate | phase 3 merged into `main` and taken by `phase-4`; P4-1 | `2026-11-25-client-model-design.md` (K§6.1, K§6.2.7) |
| P4-5 | Codex client: `zygo codex`, per-session `CODEX_HOME`, hooks with the trust rule, workspace members, stub launch and premise tests, a grade file with `supported = false` | P4-1, P4-2, P4-3, P4-4 | `2026-11-25-client-model-design.md` (K§6.3) |
| P4-6 | Codex grades measured; the bar decision; Codex listed in `zygo doctor` and the install docs when it meets the bar | P4-5 | `2026-11-25-client-model-design.md` (K§6.3.8, K-D16) |
| P4-7 | Update notices: a background zygo update check in the launcher, a HUD segment, `zygo status` and exit lines; the per-client `announce_client_updates` flag and the `announces_own_updates` premises; capability A-22 added to the registry and graded for each client | phase 3 merged into `main` and taken by `phase-4` (status-line segment order); P4-1, P4-4 | `2026-11-25-client-model-design.md` (K§6.5) |

## Plan of work (operator rules as in phase 3; order input from the lead, 2026-11-25)

1. **One spec.** `docs/specs/2026-11-25-client-model-design.md` covers P4-1 to P4-7.
2. **One work graph.** One work graph covers all of Phase 4. It is made after the operator approves the spec. The spec does not plan nodes.
3. **A review gate per node.** Each work node gets its own review gate of up to 3 rounds, as in phase 3. The P3-8 disposition rule applies at each gate: after round 3, a node passes only when each finding has exactly one disposition (fixed with a commit, rejected with a reason, or recorded with a DDR id), and no finding is of a class that stops for the operator.
4. **A pull request after every row.** Each row of the work graph, planning included, is delivered in its own pull request into the rendezvous branch `phase-4`. The lead merges a row after its gate and green CI. `phase-4` reaches `main` only on the operator's word.
5. **Order against phase 3.** Phase 3 still runs on `origin/phase-3` and changes many client-branch sites (session start, status line, OpenCode plugin, member launch, workspace hooks, Claude Code launch, trim hook). It does not change the proxy sources, the conformance scripts or `docs/conformance.md`.
   - Start at approval, in parallel with phase 3, based on `phase-4`: P4-1, P4-2, then P4-3.
   - Wait until phase 3 merges into `main` and `phase-4` takes it: P4-4, then P4-5 and P4-6, and P4-7.
   - P4-4 starts by taking the client-branch inventory again at its base commit (K§6.9). The inventory in the spec, at `bf3726bd`, shows the pattern; it is not the work list.

## Not in phase 4 unless the operator adds them

- ChatGPT-login passthrough for Codex (§12.2 `chatgpt` row stays v2).
- The Grok CLI and Pi.
- The chat/completions client route.
- An install during a session; a change to the start-check prompt of the auto-update design.
