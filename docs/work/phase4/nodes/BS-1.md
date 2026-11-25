# BS-1 — Base-spec notes: §8.1 (the registry is the capability list) and §7.6 v2 (the Responses client route is built)

**Spec:** docs/specs/2026-11-25-client-model-design.md K§12 (rows §8.1, first clause, and §7.6 v2), K§9 (P4-2: the §8.1 note)
**Files:** `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `grep -q 'docs/conformance/capabilities.toml' docs/specs/2026-11-18-zygo-design.md && test "$(grep -c '2026-11-25-client-model-design.md' docs/specs/2026-11-18-zygo-design.md)" -ge 2 && sh scripts/check-premises.sh && cargo test --workspace --locked --no-fail-fast` → every command exits 0 (the base spec names the client-model spec 0 times at `ec17582b`; each note cites it), on ubuntu, macOS and Windows CI for the cargo run (the base spec is read by tests; `.github/workflows/ci.yml:14`)

## Planning notes

- Depends on CF-4 (the registry exists) and on every RS node whose work the §7.6 note states as built (RS-5 to RS-8, and through them RS-1 to RS-4).
- One node owns the base-spec file for this graph, so the CF and RS nodes need no edge between them (author).
- The other K§12 rows are later rows: §8.1 A-22 (P4-10), §8.1 phase 3 features (P4-6), §3 (P4-11), §8.4 and §8.8 (P4-6), H§ and U (P4-10).
- A dated note in the existing note form of the base spec; no other text of the base spec changes.

## Plan

Written when the node starts.
