# FIX-3 — stub_member test: no cloned_ref_to_slice_refs warning

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§18 preamble ("Stub"); found defect from L3, graph Log 2026-11-21
**Files:** `crates/zygo-launch/tests/stub_member.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo clippy -p zygo-launch --all-targets --locked -- -D warnings && cargo test -p zygo-launch --locked --test stub_member` → no warning and no error from clippy; the `stub_member` tests all pass

## Plan

Work in the node's checkout. Run each command from the repository root. The commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-3`.

Facts of the live code (read before you start):

- `crates/zygo-launch/tests/stub_member.rs:297` is `assert_eq!(wait_done(&s.ctl, 1), [start.clone()], "the request step is finished while its reply is held");`. `wait_done` returns `Vec<String>` (`:40`); `start` is a `String` (`:292`). `assert_eq!` takes a reference to the array, which is the form the lint names.
- `Vec<String>` compares with `&[String]`, so `std::slice::from_ref(&start)` fits with no other change.
- `cargo clippy -p` also lints the workspace crates that `zygo-launch` depends on.

Steps:

- [ ] **1** Run `cargo clippy -p zygo-launch --all-targets --locked -- -D warnings`. Expected: FAIL with one error, `clippy::cloned_ref_to_slice_refs` at `crates/zygo-launch/tests/stub_member.rs:297`. If clippy reports any other warning, stop and report it: it is not in this node.
- [ ] **2** `stub_member.rs:297` becomes:

```rust
        assert_eq!(wait_done(&s.ctl, 1), std::slice::from_ref(&start), "the request step is finished while its reply is held");
```

- [ ] **3** Acceptance. Run `cargo clippy -p zygo-launch --all-targets --locked -- -D warnings && cargo test -p zygo-launch --locked --test stub_member`. Expected: no warning and no error from clippy; the `stub_member` tests all pass.
- [ ] **4** Commit `stub_member.rs` and this file with its steps ticked: `test(launch): compare the done lines with a slice of the start line (clippy)`.
