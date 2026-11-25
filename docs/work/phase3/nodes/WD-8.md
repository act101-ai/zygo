# WD-8 — Member record fields `launch.job` and `launch.definition`

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§7.4 (`member.launch.definition = { commit, digest }`), R§9.3 effect 2 (`member.launch.job`; absent for no job), R§12
**Files:** `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`
**Consumes:** `Pin` (WD-4)
**Produces:** `state::Launch.job: Option<String>`, `state::Launch.definition: Option<zygo_config::repo_def::Pin>` (each `#[serde(default, skip_serializing_if = "Option::is_none")]`)
**Acceptance:** `test "$(cargo test -q -p zygo-workspace --test state -- launch_definition --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-workspace --test state` → every count holds (at least 2 named `launch_definition…` in zygo-workspace `state`; a count of 0 fails before any run), then all pass. New tests: a record with both fields round-trips; a record written without them loads with both `None` and serializes with neither key.

## Plan

### Interfaces

Consumed:
- `zygo_config::repo_def::Pin { pub commit: String, pub digest: String }` (WD-4, `crates/zygo-config/src/repo_def/check.rs:10-14`), derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; re-exported at `crates/zygo-config/src/repo_def/mod.rs:11`. `zygo-workspace` already depends on `zygo-config` (`crates/zygo-workspace/Cargo.toml`, `zygo-config.workspace = true`); `zygo-config` does not depend on `zygo-workspace`, so no cycle.
- `zygo_workspace::state::Launch` (`crates/zygo-workspace/src/state.rs:172-196`): derives `Debug, Clone, PartialEq, Serialize, Deserialize`; fields `kind`, `profile`, `passthrough`, `server`, `compact`, `yolo`, `config_dir`, `args`, `events_off`, then `#[serde(flatten)] extra: Extra`. `Launch::new(kind)` at `state.rs:301-315`.

Produced:
- `state::Launch.job: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`
- `state::Launch.definition: Option<zygo_config::repo_def::Pin>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`

Consumers (names match): WD-11, WD-14 and WD-18 consume `state::Launch.job (WD-8)`; WD-11 and WD-14 consume `state::Launch.definition (WD-8)`. WD-10's `PlanPane.job: Option<String>` and `PlanPane.definition: Option<Pin>` have the same types.

### `Launch` construction sites (grep `Launch {` over `crates/`)

- `crates/zygo-workspace/src/state.rs:303` `Launch::new`: the only full literal of `state::Launch`; it gets `job: None, definition: None` (step 3).
- `crates/zygo/src/cmd/claude.rs:52`, `crates/zygo/src/cmd/opencode.rs:48`, `crates/zygo/src/cmd/member.rs:461`: struct-update literals (`..Launch::new(..)` / `..kept.clone()`); they compile unchanged and need no edit.
- `crates/zygo/src/cmd/claude.rs:94,130,222,230` and `crates/zygo/src/cmd/ws.rs:424` build `cmd::Launch` (the CLI flags), a different type; no edit.

### Steps

- [x] **1. Write the failing tests.** In `crates/zygo-workspace/tests/state.rs`, add `use zygo_config::repo_def::Pin;` after line 7 (`use zygo_core::Agent;`), and add these two tests after `first_started_at_set_is_written_as_unix_seconds_and_kept_by_a_write`:

```rust
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const DIGEST: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

#[test] // R§7.4, R§9.3 effect 2, W§7.2: member.launch.job and member.launch.definition = { commit, digest } round-trip through a write
fn launch_definition_and_job_set_are_written_and_kept_by_a_write() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut ws = full();
    let m = ws.members.get_mut(KEY).unwrap();
    m.launch.job = Some("worker".into());
    m.launch.definition = Some(Pin { commit: COMMIT.into(), digest: DIGEST.into() });
    let v = serde_json::to_value(&ws).unwrap();
    assert_eq!(v["members"][KEY]["launch"]["job"], json!("worker"));
    assert_eq!(v["members"][KEY]["launch"]["definition"], json!({"commit": COMMIT, "digest": DIGEST}));
    put(&store, &v);
    let read = store.read().unwrap().unwrap();
    assert_eq!(read.members[KEY].launch.job, Some("worker".to_string()));
    assert_eq!(read.members[KEY].launch.definition, Some(Pin { commit: COMMIT.into(), digest: DIGEST.into() }));
    assert!(read.members[KEY].launch.extra.is_empty()); // typed fields, not unknown keys
    store.update(S5, &mut |w| { w.phase = Phase::Down; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!(after["members"][KEY], v["members"][KEY]);
    assert_eq!(serde_json::from_value::<MemberRecord>(v["members"][KEY].clone()).unwrap(), ws.members[KEY]);
}

#[test] // R§9.3 effect 2 ("absent for no job"), W AU78: a record of an older zygo has neither key; it reads as None and a write adds neither
fn launch_definition_and_job_absent_read_as_none_and_are_not_written() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let v = serde_json::to_value(full()).unwrap();
    assert!(v["members"][KEY]["launch"].get("job").is_none());
    assert!(v["members"][KEY]["launch"].get("definition").is_none());
    put(&store, &v);
    let read = store.read().unwrap().unwrap();
    assert_eq!((read.members[KEY].launch.job.clone(), read.members[KEY].launch.definition.clone()), (None, None));
    store.update(S5, &mut |w| { w.phase = Phase::Down; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert!(after["members"][KEY]["launch"].get("job").is_none());
    assert!(after["members"][KEY]["launch"].get("definition").is_none());
    assert_eq!(after["members"][KEY], v["members"][KEY]);
    let l = Launch::new(Agent::OpenCode);
    assert_eq!((l.job, l.definition), (None, None));
    assert_eq!(serde_json::to_value(Launch::new(Agent::OpenCode)).unwrap(),
        json!({"kind": "opencode", "passthrough": false, "yolo": false, "args": []}));
}
```

  Check: the file has 2 test names that start with `launch_definition`.

- [x] **2. Run and see them fail.** `cargo test -p zygo-workspace --test state -- launch_definition` → compile error `no field 'job' on type 'Launch'` and `no field 'definition' on type 'Launch'` (E0609).

- [x] **3. Implement.** In `crates/zygo-workspace/src/state.rs`, in `pub struct Launch`, after the `events_off` field and before `#[serde(flatten)] pub extra: Extra`, add:

```rust
    /// W§7.2 member.launch.job (R§9.3 effect 2): "orchestrator" or "worker"; absent for no job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    /// W§7.2 member.launch.definition = { commit, digest } (R§7.4): the pin of the approved read of the workspace files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<zygo_config::repo_def::Pin>,
```

  In `Launch::new` (`state.rs:303`), after `events_off: false,` add `job: None,` and `definition: None,`. No other site changes (see construction sites above).

  Check: `cargo build --workspace --all-targets` → no error.

- [x] **4. Run and see pass.** `sh /private/tmp/claude-501/-Users-kagemartin-Work-zygo/3ea42e92-d626-4dbb-aa82-d5cb3dcdbb49/scratchpad/wd8acc.sh .` → exit 0 (count ≥ 2, then every `state` test passes, including the unchanged `json_has_the_fields_of_w7_2`, `none_is_absent_and_absent_reads_as_default` and `every_struct_keeps_unknown_fields`). Then `cargo test -p zygo-workspace` and `cargo test -p zygo` → all pass (the crates that build `state::Launch`).

- [x] **5. Commit.** Stage only `crates/zygo-workspace/src/state.rs` and `crates/zygo-workspace/tests/state.rs`; message `WD-8: member record launch.job and launch.definition`, with the `Zygo-Session` and `Zygo-Work` trailer lines that the implementer's brief names at the end, each on its own line. Check: `git show --stat HEAD` lists those two files only.

- [x] **6. Review fix (AU78).** The Files line was extended with `crates/zygo-config/src/repo_def/check.rs` and `crates/zygo-config/tests/repo_def_check.rs` for AU78: `Pin` gets a flattened `extra` map and `Pin::new(commit, digest)`, and `every_struct_keeps_unknown_fields` covers `/members/kabcdefgh/launch/definition`.

### Notes for later nodes (no change here)

- `member::record_start` (`crates/zygo/src/cmd/member.rs:452-465`) copies the given launch whole (`rec.launch = kept`), so a start write whose launch has `job`/`definition` unset clears saved ones, and the D4 compare at `:461` sees a difference. Keeping them at a resume (R§9.3 effect 2, R§7.4) is WD-14's scope.
