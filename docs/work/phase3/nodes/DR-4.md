# DR-4 — `zygo decision new`: write a record template

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§10.2, D§10.1 (row `new`), D§6.1, D§6.2, D§6.3 (keys that `new` fills), D§6.8 (directory text)
**Files:** `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/new.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_new.rs`
**Consumes:** `run_with` (DR-3), `DecisionIo` (DR-3), `read_dir` (DR-2), `Kind` (DR-1), `Class` (DR-1), `valid_id` (DR-1), `FILL_MARKER` (DR-1), `DIR` (DR-1)
**Produces:** `decisions::new::NewArgs { pub kind: Kind, pub title: String, pub unit: String, pub round: u32, pub role: String, pub class: Class, pub depends_on: Vec<String>, pub supersedes: Option<String> }`; `decisions::new::create(top: &Path, a: &NewArgs, model: &str, commit: &str, date: &str) -> Result<PathBuf, String>` (writes `<id>.tmp`, renames it to `<id>.md`, creates `docs/decisions/` when absent, does not commit); the `zygo decision new` subcommand
**Acceptance:** `cargo test -p zygo --locked --test decision_new` → all pass. Covers spec Acceptance #1 (`check` reports the new file as malformed until every `<!-- zygo: fill -->` line is replaced; then `check` exits 0) and #2 (two records created in two linked checkouts get different ids; merging both branches gives no conflict); `model` is `unknown` outside a session and is the newest `requests` row's `model` for `role_served = <r>` inside one; each D§10.2 argument refusal (a bad `--unit`, `--round 0`, a role not in the session legend and not `lead`, a `--depends-on` id that is not a valid record); `docs/decisions` a regular file → exit 1 with `zygo decision: docs/decisions is not a directory`.

## Plan
