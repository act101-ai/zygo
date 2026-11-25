# OS-8 — `zygo usage` prints the net figures, breakdown and ratio

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.6, O§6.2 (names), O§6.3 rule "Prices", O-C5, O§10 row "Config does not load in `zygo usage`", O§16 item 15 (`zygo usage` part)
**Files:** `crates/zygo/src/cmd/usage.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo/Cargo.toml`, `Cargo.lock`, `docs/specs/2026-11-24-output-shaping-design.md`
**Consumes:** `Store::savings` (OS-7), `Savings` (OS-7), `SavingsKey` (OS-7), `Rates` (OS-6)
**Produces:** —
**Acceptance:** `test "$(cargo test -q -p zygo --test usage_doctor -- savings --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo --test usage_doctor` → every count holds (at least 6 named `savings…` in zygo `usage_doctor`; a count of 0 fails before any run), then all pass: the existing H§14 item 18 test keeps every assertion, except that `compaction signals: 0` is asserted present, not at `at + 2`; the existing full-output tests in `usage_doctor.rs` gain the O§6.6 block and change no other line; on a fixture database the lines after `tokens trimmed (estimate)` equal the O§6.6 block line for line; a negative amount prints `≈-$`; unpriced requests add ` (<n> requests had no price)`; a config that does not load prints `estimated spend avoided: unknown (config does not load)` and every other line; a database built from the `97d8767` schema exits 0 and prints `bytes read back: 0` and `-` keys; a tool, command or model name with a control character passes through `esc`

Acceptance changed 2026-11-25 (controller's call): O§6.6 puts the block after `tokens trimmed (estimate)`, which moves `compaction signals` asserted at `usage_doctor.rs:134`.

## Plan

**Premises checked against the live code (`phase3/OS-8` at `268b0a07`):**
- OS-6 (PR #30) and OS-7 (PR #71) are merged. The consumed items, exactly as in the code:
  - `pub fn savings(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<Savings>` — `crates/zygo-usage/src/savings.rs:140`; the closure takes `(model, provider, prompt tokens)`.
  - `pub struct Savings { pub bytes_read_back: u64, pub kept_out_bytes: i64, pub kept_out_tokens: i64, pub spend_usd: f64, pub unpriced: u64, pub views: u64, pub readbacks: u64, pub by_key: Vec<SavingsKey> }` — `savings.rs:12-23`, `by_key` largest `kept_out_bytes` first.
  - `pub struct SavingsKey { pub tool: String, pub command: String, pub shaper: String, pub views: u64, pub readbacks: u64, pub kept_out_bytes: i64, pub spend_usd: f64 }` — `savings.rs:27-35`, null `command`/`shaper` already `"-"`.
  - `pub struct Rates { pub input: f64, pub cache_read: f64, pub write: f64 }` — `crates/zygo-usage/src/carry.rs:5-9`, USD per Mtok.
  - All re-exported from `zygo_usage` (`crates/zygo-usage/src/lib.rs:7,10-11`), with `SCHEMA` and `ReadbackRow` (`schema.rs:5`, `:156`).
- No code maps the catalog to `Rates`: the only `Rates` values are in `crates/zygo-usage/tests/{carry,savings}.rs`. The nearest code, `list_prices` (`crates/zygo-proxy/src/forward.rs:201-205`), is private and reads the built-in catalog only. OS-8 adds the map in `usage.rs`.
- `base(ctx, verb, None, None)` (`crates/zygo/src/cmd/verbs.rs:130`) loads the merged config (built-in rows included); a user file that does not parse is an `Err` (`crates/zygo-config/src/layers.rs:447-453`). `usage.rs:52` already uses it for `--sync`.
- Seeded `RequestRow`s in `usage_doctor.rs` (`req`, line 33) have `status` 0, so `Store::savings` gives them no carry (`savings.rs:66`: `status BETWEEN 200 AND 299`): spend `≈$0.0000`, no unpriced count.
- `crates/zygo` has no `rusqlite` dependency (`crates/zygo/Cargo.toml:10-44`). The `97d8767` database (Acceptance) can only be built with it, so step 1 adds one dev-dependency line (controller approved 2026-11-25; `Cargo.toml` and `Cargo.lock` added to **Files**).
- The O§6.6 sample block as first written could come from no database (its key amounts summed to `≈$0.1070`, not its total `≈$0.1146`; its `Read` key kept out `-4300` bytes with 4,000 bytes read back in all). This plan commit replaces its key lines with the figures of the step-3 fixture and records it in the spec's change log (controller's call 2026-11-25). Step 3 compares against the corrected block, line for line.
- No Review focus item names OS-8 (`docs/work/phase3/graph.md` "Review focus").

**Decisions (author, from the spec text):**
- Price row (O§6.3 step 3 and rule "Prices"): the model's catalog row (`Catalog::get`, `crates/zygo-config/src/schema.rs:83`); in it, the price of the provider whose `name` equals the request's `provider` (the price that priced `estimated_usd` at write time, `forward.rs:561`), else the first provider with a price (a team row has no provider, N16; a passthrough provider has no price, `crates/zygo-proxy/src/passthrough.rs:229`; same rule as `list_prices`). Tier: `long_context` when the prompt is greater than its `above` (`Prices::cost`, `schema.rs:288-291`). `write` = the tier's `cache_write_5m`, absent: the tier's `cache_write` (O§6.3 step 3; `LongContextPrices` doc, `schema.rs:257`).
- `views` and `read-backs` in a key line are singular for 1 (O§6.6 sample: `1 read-back`). The unpriced suffix keeps the spec's fixed `requests`.
- An empty breakdown prints `by tool and command: none`, as `by role and model: none` (`usage.rs:108`).

### Steps

- [x] **1. Dev-dependency.** In `crates/zygo/Cargo.toml` `[dev-dependencies]` add `rusqlite.workspace = true`. Check: `cargo test -p zygo --test usage_doctor --no-run` builds.

- [x] **2. Test helpers** in `crates/zygo/tests/usage_doctor.rs`, after `run` (line 53). Add `ReadbackRow` to the `zygo_usage` import (line 23).
  ```rust
  /// The O§6.3 test row: input 4, cache_read 0.4, cache_write_5m 5, cache_write 8 USD per Mtok.
  const PRICED: &str = "[model.provider.price_usd_per_mtok]\ninput = 4.0\ncache_read = 0.4\ncache_write = 8.0\ncache_write_5m = 5.0";
  fn creq(s: &str, ts: i64, model: &str, provider: &str, cr: u64, cw: u64, compaction: bool) -> RequestRow {
      RequestRow { ts, session: s.into(), agent: "claude-code".into(), route: "/anthropic/v1/messages".into(), model: model.into(),
          provider: provider.into(), status: 200, input_tokens: 100, cache_read_tokens: cr, cache_write_tokens: cw, compaction,
          request_class: compaction.then(|| "compaction".into()), ..Default::default() } }
  fn ktrim(s: &str, ts: i64, key: (&str, Option<&str>, Option<&str>), kept: u64, rid: &str) -> TrimRow {
      TrimRow { ts, session: s.into(), tool: key.0.into(), kind: "shaped".into(), stored_bytes: kept + 100, view_bytes: 100,
          agent_id: None, result_id: Some(rid.into()), command: key.1.map(Into::into), shaper: key.2.map(Into::into) } }
  fn rb(s: &str, ts: i64, bytes: u64, rid: &str) -> ReadbackRow {
      ReadbackRow { ts, session: s.into(), read_in: s.into(), result_id: rid.into(), tool: "result_view".into(), bytes } }
  fn after_trimmed(out: &str) -> Vec<&str> { out.lines().skip_while(|l| !l.starts_with("tokens trimmed (estimate): ")).skip(1).collect() }
  fn spend_line(p: &Paths, a: &[&str]) -> String {
      run(p, a, None).unwrap().lines().find(|l| l.starts_with("estimated spend avoided: ")).unwrap().to_string() }
  ```
  Check: `cargo test -p zygo --test usage_doctor --no-run` builds.

- [x] **3. Test `savings_usage_prints_o66_block`** (O§6.6 block, line for line as corrected in this node's plan commit; O§6.3 worked example; negative amount). Rows of the worked example (`crates/zygo-usage/tests/savings.rs` `worked_example`) in session `S`; 14 trims at ts 100 summing 40,000 bytes; 3 read-backs at ts 155 summing 4,000 bytes.
  ```rust
  let d = TempDir::new().unwrap();
  let p = paths(d.path());
  std::fs::write(&p.config_file, cfg(&[("p1", PRICED)])).unwrap();
  let s = Store::open(&p.usage_db).unwrap();
  for (ts, cr, cw) in [(90, 10_000, 2_000), (110, 10_000, 12_000), (120, 22_000, 1_000), (130, 23_000, 1_000), (140, 24_000, 1_000),
      (150, 25_000, 1_000), (160, 0, 27_000), (170, 27_000, 500), (180, 27_500, 100)] {
      s.insert_request(&creq("S", ts, "m1", "p1", cr, cw, ts == 170)).unwrap();
  }
  let (lib, diff, size) = (("Bash", Some("cargo test"), Some("libtest")), ("Bash", Some("git diff"), Some("diff")), ("Read", None, Some("size")));
  for (i, kept) in [5_000, 5_000, 5_000, 5_000, 5_000, 5_500].into_iter().enumerate() { s.insert_trim(&ktrim("S", 100, lib, kept, &format!("L{i}"))).unwrap(); }
  for i in 0..3 { s.insert_trim(&ktrim("S", 100, diff, 3_000, &format!("D{i}"))).unwrap(); }
  for i in 0..5 { s.insert_trim(&ktrim("S", 100, size, 100, &format!("Z{i}"))).unwrap(); }
  for (rid, bytes) in [("L0", 1_000), ("L1", 1_000), ("Z0", 2_000)] { s.insert_readback(&rb("S", 155, bytes, rid)).unwrap(); }
  let out = run(&p, &[], None).unwrap();
  let b = after_trimmed(&out);
  assert_eq!(b[..8], [
      "bytes read back: 4000",
      "tokens kept out of context (estimate): 9000",
      "estimated spend avoided: ≈$0.1146",
      "views: 14, read-backs: 3, view to read-back ratio: 4.67",
      "by tool and command:",
      "  Bash cargo test libtest: 6 views, 2 read-backs, 28500 bytes kept out, ≈$0.0888",
      "  Bash git diff diff: 3 views, 0 read-backs, 9000 bytes kept out, ≈$0.0270",
      "  Read - size: 5 views, 1 read-back, -1500 bytes kept out, ≈-$0.0012",
  ], "{out}");
  assert!(b[8].starts_with("compaction signals: "), "{out}");
  ```
  (libtest: 7,625 tok × 12 − 500 tok × 5.4 per Mtok = 0.0888; diff: 2,250 × 12 = 0.0270; size: 125 × 12 − 500 × 5.4 = −0.0012.)

- [x] **4. Test `savings_usage_rates_follow_catalog_provider_and_tier`** (O§6.3 step 3, rule "Prices", O-C5). One request per session at ts 20, a trim of 4,000,000 bytes (10^6 tokens) at ts 10, so the line shows the write rate in USD.
  ```rust
  let d = TempDir::new().unwrap();
  let p = paths(d.path());
  let p2 = |k: f64| format!("[model.provider.price_usd_per_mtok]\ninput = {:?}\ncache_read = {:?}\ncache_write = {:?}\ncache_write_5m = {:?}", 400.0 * k, 40.0 * k, 800.0 * k, 500.0 * k);
  let long = format!("{PRICED}\nlong_context = {{ above = 5000, input = 40.0, output = 0.0, cache_read = 4.0, cache_write = 80.0 }}");
  let write = |k: f64| std::fs::write(&p.config_file, cfg(&[("p0", ""), ("p1", &long), ("p2", &p2(k))])).unwrap();
  write(1.0);
  let s = Store::open(&p.usage_db).unwrap();
  for (sess, prov, c) in [("A", "p2", 1_000), ("B", "", 1_000), ("C", "p0", 1_000), ("D", "p1", 3_000)] {
      s.insert_request(&creq(sess, 20, "m1", prov, c, c, false)).unwrap();
      s.insert_trim(&ktrim(sess, 10, ("Bash", None, None), 4_000_000, sess)).unwrap();
  }
  assert_eq!(spend_line(&p, &["--session", "A"]), "estimated spend avoided: ≈$500.0000");  // the named provider's cache_write_5m
  assert_eq!(spend_line(&p, &["--session", "B"]), "estimated spend avoided: ≈$5.0000");    // no provider: first priced (p1)
  assert_eq!(spend_line(&p, &["--session", "C"]), "estimated spend avoided: ≈$5.0000");    // p0 has no price: first priced (p1)
  assert_eq!(spend_line(&p, &["--session", "D"]), "estimated spend avoided: ≈$80.0000");   // prompt 6100 > 5000: tier cache_write
  write(2.0);
  assert_eq!(spend_line(&p, &["--session", "A"]), "estimated spend avoided: ≈$1000.0000"); // O-C5: doubled catalog rates
  ```

- [x] **5. Test `savings_usage_names_unpriced_requests`** (O§6.6, O§10 "A model with no price row"). No config file; model `m-none` is in no catalog row.
  ```rust
  let d = TempDir::new().unwrap();
  let p = paths(d.path());
  let s = Store::open(&p.usage_db).unwrap();
  s.insert_request(&creq("E", 20, "m-none", "p1", 1_000, 1_000, false)).unwrap();
  s.insert_request(&creq("E", 30, "m-none", "p1", 2_000, 1_000, false)).unwrap();
  s.insert_trim(&ktrim("E", 10, ("Bash", None, None), 4_000, "E0")).unwrap();
  assert_eq!(spend_line(&p, &[]), "estimated spend avoided: ≈$0.0000 (2 requests had no price)");
  ```

- [x] **6. Test `savings_usage_config_does_not_load_prints_unknown`** (O§10 row "Config does not load"). Uses `ALL` as updated in step 10.
  ```rust
  let d = TempDir::new().unwrap();
  let p = paths(d.path());
  seed(&p);
  std::fs::write(&p.config_file, "[[model]\n").unwrap();
  assert!(ALL.contains("\nestimated spend avoided: ≈$0.0000\n"));
  let want = ALL.replace("\nestimated spend avoided: ≈$0.0000\n", "\nestimated spend avoided: unknown (config does not load)\n");
  assert_eq!(run(&p, &[], None).unwrap(), want);
  ```

- [x] **7. Test `savings_usage_97d8767_database_prints_dash_keys`** (O§16 item 15, O§6.5 item 5). The `97d8767` build as in `crates/zygo-usage/tests/savings.rs:174-182`.
  ```rust
  let d = TempDir::new().unwrap();
  let p = paths(d.path());
  std::fs::create_dir_all(&p.data_dir).unwrap();
  let c = rusqlite::Connection::open(&p.usage_db).unwrap();
  c.execute_batch(zygo_usage::SCHEMA).unwrap();
  c.execute_batch("DROP TABLE readbacks; ALTER TABLE trims DROP COLUMN result_id; ALTER TABLE trims DROP COLUMN command; \
      ALTER TABLE trims DROP COLUMN shaper; PRAGMA user_version = 1").unwrap();
  c.execute("INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (5, 'S1', NULL, 'Bash', 'head', 1000, 100)", []).unwrap();
  drop(c);
  let out = run(&p, &[], None).unwrap(); // Ok is exit 0
  assert_eq!(after_trimmed(&out)[..6], [
      "bytes read back: 0",
      "tokens kept out of context (estimate): 225",
      "estimated spend avoided: ≈$0.0000",
      "views: 1, read-backs: 0, view to read-back ratio: -",
      "by tool and command:",
      "  Bash - -: 1 view, 0 read-backs, 900 bytes kept out, ≈$0.0000",
  ], "{out}");
  ```

- [x] **8. Test `savings_usage_escapes_tool_command_shaper`** (O§6.6 "Every name passes `esc`"; model names are covered by `usage_report_escapes_role_and_model`).
  ```rust
  let d = TempDir::new().unwrap();
  let p = paths(d.path());
  let s = Store::open(&p.usage_db).unwrap();
  s.insert_trim(&ktrim("S1", 1, ("Bash\nforged: t", Some("c\u{1b}"), Some("s\nforged: s")), 4_000, "X0")).unwrap();
  let out = run(&p, &[], None).unwrap();
  assert!(!out.lines().any(|l| l.starts_with("forged:")), "{out}");
  assert!(out.contains("\n  Bash\\nforged: t c\\u{1b} s\\nforged: s: 1 view, 0 read-backs, 4000 bytes kept out, ≈$0.0000\n"), "{out}");
  ```

- [x] **9. H§14 #18 test** (`usage_doctor.rs:134`): replace `assert_eq!(one[at + 2], "compaction signals: 0");` with `assert!(has(&one, "compaction signals: 0"), "{one:?}");`. No other line of the test changes.

- [x] **10. `ALL` and `S1`** (`usage_doctor.rs:69-70`): insert after `tokens trimmed (estimate): 1375\n` in `ALL`:
  `bytes read back: 0\ntokens kept out of context (estimate): 1375\nestimated spend avoided: ≈$0.0000\nviews: 2, read-backs: 0, view to read-back ratio: -\nby tool and command:\n  Bash - -: 2 views, 0 read-backs, 5500 bytes kept out, ≈$0.0000\n`
  and after `tokens trimmed (estimate): 1000\n` in `S1`:
  `bytes read back: 0\ntokens kept out of context (estimate): 1000\nestimated spend avoided: ≈$0.0000\nviews: 1, read-backs: 0, view to read-back ratio: -\nby tool and command:\n  Bash - -: 1 view, 0 read-backs, 4000 bytes kept out, ≈$0.0000\n`
  (These cover `usage_report_all_sessions`, `usage_report_one_session`, `usage_report_env_session_default`, `usage_report_slash_equals_cli`.)

- [x] **11. `usage_report_since_boundary` and `usage_report_empty_store_creates_nothing`** (`usage_doctor.rs:96`, `:116`): in each `want`, insert after `tokens trimmed (estimate): 0\n`:
  `bytes read back: 0\ntokens kept out of context (estimate): 0\nestimated spend avoided: ≈$0.0000\nviews: 0, read-backs: 0, view to read-back ratio: -\nby tool and command: none\n`

- [x] **12. `usage_report_escapes_role_and_model`** (`usage_doctor.rs:179`): `assert_eq!(out.lines().count(), 12, "{out}")` → `17` (the five block lines of step 11).

- [x] **13. Run, see fail:** `cargo test -p zygo --test usage_doctor`. Expected failures: the six `savings_usage_*` tests and the five full-output tests of steps 10-12 (no block yet). `h14_18_usage_keeps_bytes_and_adds_estimated_tokens` passes.

- [x] **14. Catalog to `Rates`** in `crates/zygo/src/cmd/usage.rs`; imports `zygo_config::Catalog`, `zygo_usage::{Rates, Savings}`:
  ```rust
  /// O§6.3 step 3 and rule "Prices": the served provider's price, else the model row's first price (a team row has
  /// no provider; a passthrough provider has no price); the `long_context` tier when the prompt is over its `above`.
  fn rates(catalog: &Catalog, model: &str, provider: &str, prompt: u64) -> Option<Rates> {
      let row = catalog.get(model)?;
      let p = row.provider.iter().find(|p| p.name == provider).and_then(|p| p.price_usd_per_mtok.as_ref())
          .or_else(|| row.provider.iter().find_map(|p| p.price_usd_per_mtok.as_ref()))?;
      Some(match &p.long_context {
          Some(l) if prompt > l.above => Rates { input: l.input, cache_read: l.cache_read, write: l.cache_write_5m.unwrap_or(l.cache_write) },
          _ => Rates { input: p.input, cache_read: p.cache_read, write: p.cache_write_5m.unwrap_or(p.cache_write) },
      })
  }
  ```

- [x] **15. The block writer** in `usage.rs`:
  - `fn usd(v: f64) -> String`: `≈$` and four decimals; a negative value `≈-$` and the four decimals of `-v` (O§6.6).
  - `fn savings_lines(o: &mut String, v: &Savings, priced: bool)`: writes, each ending in `\n`:
    `bytes read back: {v.bytes_read_back}`; `tokens kept out of context (estimate): {v.kept_out_tokens}`;
    `estimated spend avoided: ` + (`unknown (config does not load)` when `!priced`, else `usd(v.spend_usd)` + ` ({v.unpriced} requests had no price)` when `v.unpriced > 0`);
    `views: {v.views}, read-backs: {v.readbacks}, view to read-back ratio: {r}` with `r` = `-` when `v.readbacks == 0`, else `{:.2}` of `views ÷ readbacks`;
    `by tool and command: none` when `v.by_key` is empty, else `by tool and command:` and per key `  {esc(tool)} {esc(command)} {esc(shaper)}: {views} view[s], {readbacks} read-back[s], {kept_out_bytes} bytes kept out, {usd(spend_usd)}` (no `s` for 1).

- [x] **16. Wire into `run`** (`usage.rs:72`), after the `--sync` return:
  - `let catalog = base(ctx, "usage", None, None).ok().map(|m| m.config.catalog);` and `let price = |m: &str, pv: &str, n: u64| catalog.as_ref().and_then(|c| rates(c, m, pv, n));`
  - Build `Filter { session, since }` once; when `usage_db` exists, open read-only once and take `summary(&f)` and `savings(&f, &price)`, both errors through `fail`; else `(Summary::default(), Savings::default())`.
  - Call `savings_lines(&mut o, &v, catalog.is_some())` right after the `tokens trimmed (estimate)` line (`usage.rs:104`), before `compaction signals`.

- [x] **17. Run, see pass:** `cargo test -p zygo --test usage_doctor` → all pass; then the Acceptance command → count ≥ 6 (six `savings_usage_*`) and all pass.

- [x] **18. Lint and crate tests:** `cargo clippy -p zygo --tests --locked -- -D warnings` → no warning; `cargo test -p zygo --locked` → all pass.

- [x] **19. Commit** `crates/zygo/src/cmd/usage.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo/Cargo.toml` and `Cargo.lock` (when step 1 changed it) with `-m "OS-8: zygo usage prints the net figures, breakdown and ratio"` and the trailers `Zygo-Session: <session>` and `Zygo-Work: <unit>` from the implementer's brief. Check: `git show --stat HEAD` lists no other file.
