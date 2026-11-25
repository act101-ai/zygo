# zygo phase 4 roadmap

**Status:** started 2026-11-25. The operator approved the spec on 2026-11-25, with the instruction to plan and build: `docs/specs/2026-11-25-client-model-design.md` (`K§n`). Next: P4-1 (the plan for the rows that start now) and P4-5 (the plan for the client contract).
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-21-auto-update-design.md`, `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`).

## Intent

The operator's request (2026-11-25): "i would like to add codex support to zygo. with adding codex support, we need two things. we need to have a clear architectural model for client implementations so they are straightforward to implement, update, and grade. we will maintain a feature matrix conformance score. for every feature that any zygo feature a client supports, all clients get graded against their support for that client, and a rubric is built, clients are measured and scored and found wanting. but sufficient. :)"

Two additions of the same day (operator): "we need to produce a feature matrix, a rollup, and a score for each client." And: "we want background update checks and post to the hud when there's an update." On the wrapped clients' own updates: each client has a flag in the client abstraction that says whether zygo announces its updates; the default follows whether the client announces its own updates by default.

A **client** is a coding agent that zygo wraps: today Claude Code and OpenCode, in phase 4 also Codex CLI.

The gap today: client behavior is spread over 35 source files as branches on the client kind (K§1); the conformance document is two hand-written tables that no CI job checks; the proxy serves only the Anthropic Messages dialect, and Codex speaks only the OpenAI Responses API; a session never learns of a zygo release after its start.

The direction: one client contract whose client kind is opaque, so the compiler keeps client branches inside it; a capability registry with a rubric of checkable criteria, per-client grade files with measured evidence, and a generated feature matrix, rollup and score with a sufficiency bar; the Responses API as a proxy client dialect; Codex, designed in its own document against the built contract; a background update check in the launcher that posts to the HUD.

## Items

Numbered in causal order (operator, 2026-11-25).

| Id | Item | Depends | Spec |
|---|---|---|---|
| P4-1 | Plan the rows that start now: P4-2, P4-3, P4-4 (one planning row) | spec approval | K§6.9 |
| P4-2 | Conformance: capability registry and rubric (A-1 to A-21, WA-1, WA-2 as criteria; bar exceptions in the registry), grade files for Claude Code and OpenCode, the generator and check (a Rust test), the feature matrix, rollup and score in `docs/conformance.md` and `docs/conformance.json`, the bar | P4-1 | K§6.2 |
| P4-3 | Codex premises and fixtures (PC1 to PC16) against the mock upstream, no login; the premise check driven by data | P4-1 | K§6.3.7, K§6.2.5 |
| P4-4 | Proxy: the OpenAI Responses API as a client dialect (`/openai/v1/responses` with SSE, `/openai/v1/models`) to Anthropic, Responses and chat upstreams; routes by dialect, tested with an existing client's session | P4-1, P4-3 (captured Codex requests) | K§6.4 |
| P4-5 | Plan the client contract | spec approval | K§6.1 |
| P4-6 | Build the client contract: opaque client kind, descriptor and adapter registries, Claude Code and OpenCode moved onto it with no behavior change, the boundary check, the `supported` flag and the launch gate; capabilities or criteria for the client-facing features of phase 3 | phase 3 merged into `main` and taken by `phase-4`; P4-2, P4-5 | K§6.1, K§6.2.7 |
| P4-7 | Codex client design document, designed against the built contract; K§6.3 and the Codex parts of K§6.4 are its requirements | P4-3, P4-4, P4-6 | K§6.3 (requirements) |
| P4-8 | Plan the Codex client | P4-7 | the P4-7 design |
| P4-9 | Build the Codex client: `zygo codex`, its grade file in the same commit as its client entry, Codex client policy in the proxy, the bar decision, release listing when it meets the bar | P4-8 | the P4-7 design |
| P4-10 | zygo's own update notices: background check in the launcher, HUD segment 16, `zygo status` and exit lines, `update_state`; A-22 added | phase 3 complete (shared status-line segment order); P4-2 | K§6.5.2, K§6.5.3, K§6.5.5 |
| P4-11 | Client update announcements: `announces_own_updates` premises and facts, the per-client flag, release sources; A-22.2 and A-22.3 graded | P4-6, P4-10 | K§6.5.4, K§6.5.6 |

## Plan of work (operator, 2026-11-25; order input from the lead)

1. **One spec.** `docs/specs/2026-11-25-client-model-design.md`, approved 2026-11-25. The Codex client gets a second spec (P4-7), designed against the built contract.
2. **Planning rows.** P4-1 plans P4-2 to P4-4; P4-5 plans P4-6; P4-8 plans P4-9. A spec does not plan nodes.
3. **A review gate per node.** Each work node gets its own review gate of up to 3 rounds, as in phase 3. The P3-8 disposition rule applies at each gate: after round 3, a node passes only when each finding has exactly one disposition (fixed with a commit, rejected with a reason, or recorded with a DDR id), and no finding is of a class that stops for the operator.
4. **A pull request after every row.** Each row, planning included, is delivered in its own pull request into the rendezvous branch `phase-4`. The lead merges a row after its gate and green CI. `phase-4` reaches `main` only on the operator's word.
5. **Order against phase 3.** Phase 3 still runs on `origin/phase-3` and changes many client-branch sites (session start, status line, OpenCode plugin, member launch, workspace hooks, Claude Code launch, trim hook). It does not change the proxy sources, the conformance scripts or `docs/conformance.md`.
   - Now, in parallel with phase 3, based on `phase-4`: P4-1, then P4-2, P4-3, P4-4; and P4-5.
   - After phase 3 merges into `main` and `phase-4` takes it: P4-6, then P4-7, P4-8, P4-9, and P4-11.
   - Opportunistic: P4-10 is dispatched in parallel as soon as phase 3 is complete (operator).
   - P4-6 starts by taking the client-branch inventory again at its base commit (K§6.9). The inventory in the spec, at `bf3726bd`, shows the pattern; it is not the work list.

## Not in phase 4 unless the operator adds them

- ChatGPT-login passthrough for Codex (§12.2 `chatgpt` row stays v2).
- The Grok CLI and Pi.
- The chat/completions client route.
- An install during a session; a change to the start-check prompt of the auto-update design.
