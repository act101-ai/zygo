# FIX-15 — HUD number form rolls over to b after 999m

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§6.6, first bullet, as amended 2026-11-22 (operator: HUD numbers roll over to `b` after `999m`). Defect reported by the operator 2026-11-22: `short(n: u64)` (`crates/zygo-hooks/src/statusline/mod.rs:37`) has steps for plain, `k` and `m` only, so 1,000,000,000 renders `1000m`. Fix: n ≥ 1,000,000,000 gives `<n ÷ 1e9>b` when n is an exact multiple of 1,000,000,000, else `<whole>.<tenth>b` (tenths, floor); `u64::MAX` does not panic. The forms below 1,000,000,000 do not change
**Files:** `crates/zygo-hooks/src/statusline/mod.rs`, `crates/zygo-hooks/tests/statusline_render.rs`
**Consumes:** —
**Produces:** `statusline::short` rolls over to b
**Acceptance:** `cargo test -p zygo-hooks --locked --lib statusline && cargo test -p zygo-hooks --locked --test statusline_render` → all pass, including `short_forms` with 1,000,000,000 → `1b`, 1,250,000,000 → `1.2b`, 999,999,999 → `999.9m`, 12,000,000,000 → `12b` and `u64::MAX` → `18446744073.7b`, and the trimmed-tokens cases of `h14_18_trimmed_bytes_show_as_estimated_tokens` with `1b` and `1.2b`

## Plan

The fix is one function, `short` in `crates/zygo-hooks/src/statusline/mod.rs`, and its two test lists: `short_forms` in the same file and the trimmed-tokens `cases` of `h14_18_trimmed_bytes_show_as_estimated_tokens` in `crates/zygo-hooks/tests/statusline_render.rs:982`, which list the `short` forms through the render.

**Builds on:** (signatures in the code now)
- `pub fn short(n: u64) -> String` (`crates/zygo-hooks/src/statusline/mod.rs:37`), from HUD-8. Callers: `render.rs:661` (threshold), `:668` (tokens in and out), `:696` (trimmed tokens); `crates/zygo/src/cmd/status.rs:48`, `:61` (context, compact window, trimmed tokens); `crates/zygo/src/cmd/help.rs:43` (window).

**Plan produces:**
- `pub fn short(n: u64) -> String`, same signature, with the `b` step of H§6.6.

Decisions (author's call):
- D1. One unit table for `m` and `b`: below 1,000 the number; below 1,000,000 `<n ÷ 1000>k` (floor, as today); else the unit is 1,000,000 with `m` below 1,000,000,000 and 1,000,000,000 with `b` from there; an exact multiple of the unit has no decimal, else `<n ÷ unit>.<n mod unit ÷ (unit ÷ 10)>`. The integer division cannot overflow or panic for any `u64`.
- D2. No unit above `b`: `u64::MAX` renders `18446744073.7b`. The operator's rule names `b` only, and a session does not reach 1e12 tokens.
- D3. Other formatters: none of the same kind. A search of `crates/` for `1_000_000`, `}m"`, `}k"` and `short(` finds only `short`. `zygo status` (`status.rs`) and `zygo help` (`help.rs`) call `zygo_hooks::statusline::short`, so they get the `b` step with no change. `zygo usage` (`usage.rs:101`, `:104`) prints whole numbers, not a short form. `parse_window` (`crates/zygo/src/cmd/compact.rs`) reads `k` and `m` input, is bounded at 10,000,000, and is a parser, not a formatter. No other file changes.
- D4. The render test gains bytes 3,999,999,996 → `999.9m`, 4,000,000,000 → `1b` and 4,800,000,001 → `1.2b` (`ceil(bytes ÷ 4)` = 999,999,999, 1,000,000,000 and 1,200,000,001), so the render path shows the new step.
- D5. The spec citation `statusline.rs:143-154` in H§6.6 was stale (the file moved to `statusline/mod.rs`); the amended bullet names `zygo_hooks::statusline::short`.
- D6. Consumes is `—`, not `statusline::short` from HUD-8: the validator refuses a Consumes item whose producer is not in Depends, and Depends stays `—` because HUD-8 is DONE and archived. The function this node changes is named under Builds on.

Steps:

- [ ] Write the failing tests. In `short_forms` (`crates/zygo-hooks/src/statusline/mod.rs`), after `(10_000_000, "10m"),` add `(999_999_999, "999.9m"), (1_000_000_000, "1b"), (1_250_000_000, "1.2b"), (12_000_000_000, "12b"), (u64::MAX, "18446744073.7b"),`. In `crates/zygo-hooks/tests/statusline_render.rs:982`, append to `cases`: `(3_999_999_996, "999.9m"), (4_000_000_000, "1b"), (4_800_000_001, "1.2b")`.
- [ ] Run them and see them fail: `cargo test -p zygo-hooks --locked --lib statusline::tests::short_forms` → 1 failed, `left: "1000m"`, `right: "1b"` for 1000000000; `cargo test -p zygo-hooks --locked --test statusline_render h14_18_trimmed_bytes_show_as_estimated_tokens` → 1 failed at 4000000000 (`✂ 1000m`).
- [ ] Implement D1: replace the doc line and body of `short` with

  ```rust
  /// `999`, `412k` (floor), `1m`, `1.2m`, `1b`, `1.2b` (tenths, floor; H§6.6).
  pub fn short(n: u64) -> String {
      let (unit, suffix) = match n {
          0..1000 => return n.to_string(),
          1000..1_000_000 => return format!("{}k", n / 1000),
          1_000_000..1_000_000_000 => (1_000_000, 'm'),
          _ => (1_000_000_000, 'b'),
      };
      if n.is_multiple_of(unit) {
          format!("{}{suffix}", n / unit)
      } else {
          format!("{}.{}{suffix}", n / unit, n % unit / (unit / 10))
      }
  }
  ```
- [ ] Run them and see them pass: the two commands of step 2 → 1 passed each.
- [ ] Run the Acceptance: `cargo test -p zygo-hooks --locked --lib statusline && cargo test -p zygo-hooks --locked --test statusline_render` → all pass. Then `cargo clippy -p zygo-hooks --locked --tests -- -D warnings` → exit 0.
- [ ] Commit, in the FIX-15 checkout: `git add crates/zygo-hooks/src/statusline/mod.rs crates/zygo-hooks/tests/statusline_render.rs docs/work/phase2/nodes/FIX-15.md`, with the identity env and the message `fix(hud): short rolls over to b after 999m`, one blank line, `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-15`.
