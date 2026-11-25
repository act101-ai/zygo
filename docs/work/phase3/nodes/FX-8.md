# FX-8 — zygo usage reads summary and savings from one snapshot

**Spec:** — (found in the OS-8 review, F3). `crates/zygo/src/cmd/usage.rs` calls `st.summary(&f)?` and then `st.savings(&f, &price)?` (one line, `Ok((st.summary(&f)?, st.savings(&f, &price)?))`); each opens its own read transaction (`crates/zygo-usage/src/query.rs:146`, `crates/zygo-usage/src/savings.rs:142`). A trim or read-back written between the two makes `bytes trimmed` and the savings lines describe different row sets. O§6.6 prints them as one report.
**Files:** `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/src/savings.rs`, `crates/zygo-usage/src/lib.rs`, `crates/zygo-usage/tests/report_snapshot.rs` (new), `crates/zygo/src/cmd/usage.rs`
**Consumes:** Store::summary (existing, crates/zygo-usage/src/query.rs), `Store::savings` (OS-7), Filter (existing, crates/zygo-usage/src/query.rs), Summary (existing, crates/zygo-usage/src/query.rs), `Savings` (OS-7), `Rates` (OS-6)
**Produces:** one `Store` method that returns both `Summary` and `Savings` from one read transaction (the plan fixes its name and signature)
**Acceptance:** `cargo test -p zygo-usage --test report_snapshot` → all pass: a trim row written by a second connection while the report runs (from inside the rates callback) shows in neither the summary nor the savings figures; and `cargo test -p zygo --test usage_doctor` still passes with no expected line changed.

## Plan
