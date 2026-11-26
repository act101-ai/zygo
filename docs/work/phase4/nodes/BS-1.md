# BS-1 — Base-spec notes: §8.1 (the registry is the capability list) and §7.6 v2 (the Responses client route is built)

**Spec:** docs/specs/2026-11-25-client-model-design.md K§12 (rows §8.1, first clause, and §7.6 v2), K§9 (P4-2: the §8.1 note)
**Files:** `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `grep -q 'docs/conformance/capabilities.toml' docs/specs/2026-11-18-zygo-design.md && test "$(grep -c '2026-11-25-client-model-design.md' docs/specs/2026-11-18-zygo-design.md)" -ge 3 && sh scripts/check-premises.sh && cargo test --workspace --locked --no-fail-fast` → every command exits 0 (the base spec names the client-model spec 0 times at `ec17582b`; each note cites it), on ubuntu, macOS and Windows CI for the cargo run (the base spec is read by tests; `.github/workflows/ci.yml:14`)

## Planning notes

- Depends on CF-4 (the registry exists) and on every RS node whose work the §7.6 note states as built (RS-5 to RS-8, and through them RS-1 to RS-4).
- One node owns the base-spec file for this graph, so the CF and RS nodes need no edge between them (author).
- The other K§12 rows are later rows: §8.1 A-22 (P4-10), §8.1 phase 3 features (P4-6), §3 (P4-11), §8.4 and §8.8 (P4-6), H§ and U (P4-10).
- A dated note in the existing note form of the base spec; no other text of the base spec changes.

## Plan

Capability: `mechanical` (three dated notes in one file; no code, no test change).

### Premise check (2026-11-26, `phase-4` at `736ce1b3`)

- Holds: base spec §7.6 (`docs/specs/2026-11-18-zygo-design.md:292`) has the v2 bullet at `:295` ("**v2 (with Codex):** the client routes `POST /openai/v1/responses`, `POST /openai/v1/chat/completions`, `GET /openai/v1/models`, the `openai-chat` client → `anthropic` upstream pair, and `dialect_models` (§6.4). v1 answers these routes `404 zygo_route_not_in_v1`. …").
- Holds: base spec §8.1 (`:322`) opens with the capability-list paragraph at `:324` ("Every agent has a conformance row with each capability marked **full**, **partial** … or **none** …").
- Holds: the dated-note form is `*(Note <date>, <node> <stage>: <text>)*` at the end of the line it amends, for example `:299` `*(Note 2026-11-19, N13 planning (author's call): …)*` and `:352` `*(Note 2026-11-18, N39 planning: …)*`.
- Holds: `grep -c '2026-11-25-client-model-design.md' docs/specs/2026-11-18-zygo-design.md` → `0`. `docs/conformance/capabilities.toml` exists (23 capabilities, A-1 to A-21, WA-1, WA-2).
- Holds: base spec §7.4 (`:273`) has at `:282` "The served model must have a provider that speaks the request's dialect, or one whose dialect `zygo` can translate to (§7.6); else the role's `dialect_models` entry is tried; else `400 zygo_no_provider`. Candidates are the model's enabled providers in config order, the pinned one first (§10.4)." `crates/zygo-proxy/src/resolve.rs` `resolve` (`:29-75`) does not read `dialect_models`: when no provider of the served model gives an upstream dialect for the request (`serves`, `upstream_dialect`), it answers `400 zygo_no_provider` (`:64-66`). Scope added (author's call, 2026-11-26): a third dated note at the end of `:282` that states this and that the `dialect_models` step stays v2.
- Fails in part: the Acceptance clause `grep -q 'docs/conformance/capabilities.toml'` passes before this node, because `:354` (the WA-1/WA-2 paragraph) already names the registry. Only the count clause (`-ge 3`) shows that the notes are present. Step 5 adds a check that names each note.
- Code facts for the §7.6 note (`crates/zygo-proxy`, at `736ce1b3`):
  - `src/routes.rs` `router` (`:48-50`): `POST /openai/v1/responses` → `responses`; `GET /openai/v1/models` → `openai_models`; `POST /openai/v1/chat/completions` → `not_in_v1` (`:76-79`, `404 zygo_route_not_in_v1`).
  - `src/resolve.rs` `upstream_dialect` (`:81-89`): an `openai` client takes, per provider, the first of `openai`, `anthropic`, `openai-chat`; an `openai-chat` client takes only `openai-chat`.
  - `src/forward.rs` `responses_attempt` (`:1055-`): toward `anthropic` and `openai-chat`, `translate/responses_client.rs` `request` builds the Messages body; toward `openai-chat` the existing Messages → chat translator runs after it. Toward `openai` the body passes with no pivot (K§6.4.3).
  - `dialect_models`: `crates/zygo-config/src/schema.rs:380` (`Role.dialect_models`) parses it; `crates/zygo-proxy/src/resolve.rs` `resolve` (`:29-75`) does not read it. No route and no pair is built for an `openai-chat` client.
- Tests that read the base spec: `cargo test` — only `crates/zygo-config/tests/builtin.rs` `prompts_equal_spec_6_5_verbatim` (`:136-150`; it reads §6.5 to `## 7.`, so these notes do not change its input). Scripts — `scripts/check-premises.sh` (`:14`; reads §19, §19a, §10.5 and the lines that name `N38`; the notes name no `N38`), `scripts/test-check-premises.sh:34` and `scripts/premises/ver_test.sh:204-212` (copy the file).
- Dependency state: CF-4 and RS-1 to RS-16 are DONE; TF-2 is PLANNED. The node starts only when TF-2 is DONE.

### Note texts (the change)

§7.6 note: append one space and this text to the end of line `:295` (after "A request to a provider that speaks the client's dialect is not translated."), on the same line:

```
*(Note 2026-11-26, BS-1 (client-model design `docs/specs/2026-11-25-client-model-design.md` K§6.4, K§12): the Responses client route is built. The proxy serves `POST /openai/v1/responses`, streamed and not streamed, and `GET /openai/v1/models`, the session's role list in the shape Codex reads (`crates/zygo-proxy/src/routes.rs` `router`). For a Responses client, each provider is a candidate in the first dialect it speaks of `openai`, `anthropic`, then `openai-chat` (`crates/zygo-proxy/src/resolve.rs` `upstream_dialect`). Toward `openai` the body goes with no translation (K§6.4.3). Toward `anthropic` the proxy translates Responses → Messages and the answer Messages → Responses (`crates/zygo-proxy/src/translate/responses_client.rs`). Toward `openai-chat` Messages is the pivot: Responses → Messages → chat, and the answer back the same way (`crates/zygo-proxy/src/forward.rs` `responses_attempt`). These stay v2: `POST /openai/v1/chat/completions`, which answers `404 zygo_route_not_in_v1` (`routes.rs` `not_in_v1`); the `openai-chat` client → `anthropic` upstream pair; and `dialect_models`, which the config accepts (`crates/zygo-config/src/schema.rs` `Role`) and resolution does not read.)*
```

§8.1 note: append one space and this text to the end of line `:324` (after "and published in the docs."), on the same line:

```
*(Note 2026-11-26, BS-1 (client-model design `docs/specs/2026-11-25-client-model-design.md` K§6.2, K§9 P4-2, K§12): the capability list is the registry `docs/conformance/capabilities.toml`. It holds each capability's id, name, standard text, tier and criteria, and the bar exceptions. Each client's grades are in `docs/conformance/<client-id>.toml`. The generator writes the cells to `docs/conformance.md` and `docs/conformance.json`. Where the table below and the registry differ, the registry is correct. The cell form (**full**, **partial**, **none**) and C9 stand.)*
```

§7.4 note: append one space and this text to the end of line `:282` (after "the pinned one first (§10.4)."), on the same line:

```
*(Note 2026-11-26, BS-1 (client-model design `docs/specs/2026-11-25-client-model-design.md` K§12): resolution does not read `dialect_models` now. When no provider of the served model speaks the request's dialect or one `zygo` can translate to, `crates/zygo-proxy/src/resolve.rs` `resolve` answers `400 zygo_no_provider`; the config accepts `dialect_models` (`crates/zygo-config/src/schema.rs` `Role`). The `dialect_models` step stays v2; see the §7.6 note.)*
```

### Steps

- [ ] 1. Confirm the node can start: TF-2, CF-4 and RS-1 to RS-16 are `DONE` in `docs/work/phase4/graph.md`. Run `grep -c '2026-11-25-client-model-design.md' docs/specs/2026-11-18-zygo-design.md`. Pass: every dependency is `DONE`; the count is `0`; `sed -n '282p;295p;324p'` shows the three lines quoted in the premise check.
- [ ] 2. Append the §7.6 note to the end of line `:295` (one space before it, same line). Pass: `wc -l` of the base spec is unchanged (1118); `sed -n '295p'` ends with `does not read.)*`.
- [ ] 3. Append the §8.1 note to the end of line `:324` (one space before it, same line). Pass: `wc -l` is unchanged (1118); `sed -n '324p'` ends with `C9 stand.)*`.
- [ ] 4. Append the §7.4 note to the end of line `:282` (one space before it, same line). Pass: `wc -l` is unchanged (1118); `sed -n '282p'` ends with `see the §7.6 note.)*`.
- [ ] 5. Check that only these three lines changed and each note is in its place. Run `git diff --numstat -- docs/specs/2026-11-18-zygo-design.md`. Pass: `3	3	docs/specs/2026-11-18-zygo-design.md`. Run `git diff --word-diff=porcelain -- docs/specs/2026-11-18-zygo-design.md`. Pass: the only line that starts with `-` is the `--- a/` header (words are only added). Run `sed -n '282p' docs/specs/2026-11-18-zygo-design.md`, `sed -n '295p' docs/specs/2026-11-18-zygo-design.md` and `sed -n '324p' docs/specs/2026-11-18-zygo-design.md`. Pass: line 282 has `Note 2026-11-26, BS-1`, `2026-11-25-client-model-design.md` and `dialect_models`; line 295 has `Note 2026-11-26, BS-1` and `2026-11-25-client-model-design.md`; line 324 has `Note 2026-11-26, BS-1`, `2026-11-25-client-model-design.md` and `docs/conformance/capabilities.toml`.
- [ ] 6. Run the Acceptance command alone, from the repository root: `grep -q 'docs/conformance/capabilities.toml' docs/specs/2026-11-18-zygo-design.md && test "$(grep -c '2026-11-25-client-model-design.md' docs/specs/2026-11-18-zygo-design.md)" -ge 3 && sh scripts/check-premises.sh && cargo test --workspace --locked --no-fail-fast`. Pass: exit 0; the count is `3`.
- [ ] 7. Verification floor. Run `N2_FORBIDDEN_NAMES=zzcinameq sh scripts/check-premises.sh`. Pass: exit 0, last line starts `check-premises: ok (`. Run `cargo test --workspace --locked --no-fail-fast`. Pass: exit 0, and `prompts_equal_spec_6_5_verbatim` (`zygo-config`, test `builtin`) is `ok`. Run `sh scripts/test-check-premises.sh` (it copies the base spec). Pass: exit 0. (On a host where `/bin/sh` is bash ≥4.4 and TF-3 is not DONE, case "H1: a binary file with a forbidden name" fails on `phase-4` too; that is the TF-3 defect; record it here.) If a test fails, run the same test on `phase-4` without the change; record the result in this file and do not continue.
- [ ] 8. Commit `docs(spec): BS-1 base-spec notes §8.1, §7.6 v2 and §7.4` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:BS-1`, no Claude attribution. Push the node branch (no force) and open the pull request against `phase-4`. Pass: CI green on ubuntu, macOS and Windows (`.github/workflows/ci.yml:14` and `:28` start CI on this file). Do not rerun CI without the operator's word.
