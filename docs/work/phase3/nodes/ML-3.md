# ML-3 — Member name in the plan, the member record and `panes[]`

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§6 (`up` copies the name); `docs/specs/2026-11-20-workspaces-design.md` W§7.2, AU78, AU84
**Files:** `crates/zygo-workspace/src/loadout.rs`, `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/src/snapshot.rs`, `crates/zygo-workspace/tests/loadout.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-workspace/tests/snapshot.rs`
**Consumes:** `LoadoutPane.name` (ML-1), `valid_member_name` (ML-1)
**Produces:** `zygo_workspace::loadout::PlanPane.name: Option<String>`, `zygo_workspace::state::MemberRecord.name: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `zygo_workspace::state::Pane.name: Option<String>` (same attributes), `zygo_workspace::state::MemberRecord::valid_name(&self) -> Option<&str>` (the name only when it passes `valid_member_name`: AU84 at each read)
**Acceptance:** `test "$(cargo test -q -p zygo-workspace --test loadout -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-workspace --test state -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 3 && test "$(cargo test -q -p zygo-workspace --test snapshot -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-workspace --test loadout --test state --test snapshot` → every count holds (at least 1 named `member_name…` in zygo-workspace `loadout`; at least 3 named `member_name…` in zygo-workspace `state`; at least 1 named `member_name…` in zygo-workspace `snapshot`; a count of 0 fails before any run), then all pass: `loadout::resolve` copies `name` to `PlanPane`; `fresh_member` (`up.rs`) writes it to the record; a snapshot keeps `Pane.name` of the stored pane with the same key after every `%N` changed; a `workspace.json` without `name` loads, and a write keeps unknown fields; a stored name that fails the grammar reads as no name

## Plan

### Premise check (live code at `2518f103`)

- ML-1 produced `zygo_config::schema::LoadoutPane.name: Option<String>` (`crates/zygo-config/src/schema.rs:747-749`, `#[serde(skip_serializing_if = "Option::is_none")]`, under the struct-level `#[serde(default)]` at `:743`).
- Drift (path only): `valid_member_name` lives in `zygo-core`, not in `zygo-config`: `zygo_core::ids::valid_member_name(s: &str) -> bool` (`crates/zygo-core/src/ids.rs:19`; `^[a-z][a-z0-9-]{0,31}$` and not `reserved_member_name`, `:11`, which also refuses the pane-key form `^k[a-z2-7]{8}$` and `all`, `cli`). `zygo-workspace` already depends on `zygo-core` (`crates/zygo-workspace/src/loadout.rs:11`), so the crate graph does not change.
- `config check` refuses a bad, reserved, repeated or shell-pane name (`crates/zygo-config/src/check.rs:196-211`), but `zygo up` does not run `config check` (no call of `zygo_config::check::check` in `crates/zygo-workspace/src/up.rs` or `crates/zygo/src/cmd/up.rs`). So `resolve` copies a name only when `valid_member_name` passes (M§6: names are checked at each read, as AU84). Uniqueness in the loadout stays a `config check` rule; ML-3 does not repeat it.
- `PlanPane` (`crates/zygo-workspace/src/loadout.rs:35-46`) has no `name`; its only struct literals are `loadout.rs:175-185`, `tests/loadout.rs:19` and `tests/loadout.rs:56-59`. All three change.
- `state::Pane` (`crates/zygo-workspace/src/state.rs:82-93`) derives `Default`; every literal outside `state.rs` ends in `..Default::default()` / `..Pane::default()` (`up.rs:1130-1138`, test files), so no other file breaks. `snapshot.rs:340-352` sets every field and gets `name`.
- `state::MemberRecord` (`state.rs:96-133`) has one full literal, `MemberRecord::new` (`state.rs:298-318`); every other construction in the workspace goes through `MemberRecord::new` or `..base`, so only `new` changes.
- `fresh_member` (`crates/zygo-workspace/src/up.rs:661-674`) is private, and `crates/zygo-workspace/tests/up.rs` (tmux) is not on the Files line. The test reaches it through a new wrapper in the existing `test-hooks` module `up::testing` (`up.rs:101-165`, precedent `run_with`, `up.rs:153`); `[dev-dependencies]` enables `test-hooks` for every integration test (`crates/zygo-workspace/Cargo.toml:29-30`).
- `up` writes `panes[]` from `plan.fresh` and `plan.kinds` at `up.rs:1123-1138` (loadout plan only); the restore plan keeps `cur.panes` (`up.rs:1143-1148`), so a stored `Pane.name` stays. The name goes into `panes[]` from the fresh record of the same key, so a shell pane and an AU98 pane (`up.rs:744-747`) get none.
- Spec drift, reported, not edited: the W§7.2 table (`docs/specs/2026-11-20-workspaces-design.md:269-306`) has no `panes[].name` or `member.name` row; M§6 (`docs/specs/2026-11-22-workspace-mail-design.md:68`) adds both.
- Coverage gap, reported: the `panes[]` copy at `up.rs:1130-1138` runs only under tmux (`tests/up.rs`, not on the Files line). Its value is `fresh_member`'s `name`, which `member_name_fresh_member_takes_the_plan_pane_name` checks; the wiring line itself has no name assertion in this node.

### Interfaces

Consumes (exact, live):

```rust
// crates/zygo-config/src/schema.rs:744
pub struct LoadoutPane { pub kind: String, pub name: Option<String>, /* profile, args, yolo, passthrough, server, compact, config_dir, cwd */ }
// crates/zygo-core/src/ids.rs:19
pub fn valid_member_name(s: &str) -> bool
```

Produces (exact):

```rust
// crates/zygo-workspace/src/loadout.rs, PlanPane
pub name: Option<String>,
// crates/zygo-workspace/src/state.rs, Pane and MemberRecord, each:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub name: Option<String>,
// crates/zygo-workspace/src/state.rs
impl MemberRecord { pub fn valid_name(&self) -> Option<&str> }
// crates/zygo-workspace/src/up.rs, mod testing (feature test-hooks)
pub fn fresh_member(agent: zygo_core::Agent, pp: &crate::loadout::PlanPane) -> crate::state::MemberRecord
```

### Steps

- [x] 1. Write the failing loadout test. In `crates/zygo-workspace/tests/loadout.rs`, add `name: None` to `plain()` (line 19) and to the `PlanPane` literal of `unset_pane_fields_take_the_launch_defaults_and_set_fields_are_kept` (line 56), and add:

```rust
#[test] // M§6, AU84: resolve copies a valid pane name; a name that fails the grammar is not copied
fn member_name_resolve_copies_the_pane_name() {
    let named = |kind: &str, n: &str| LoadoutPane { name: Some(n.into()), ..pane(kind) };
    let w = window(vec![named("claude-code", "lead"), pane("opencode"), named("opencode", "Bad"), named("claude-code", "all"), named("claude-code", "kabcdefgh")]);
    let p = resolve(&cfg("team", vec![("team", vec![w])]), None, &root(), &[]).unwrap();
    let names: Vec<Option<&str>> = p.windows[0].panes.iter().map(|pp| pp.name.as_deref()).collect();
    assert_eq!(names, [Some("lead"), None, None, None, None]);
    assert_eq!(p.windows[0].panes[0], PlanPane { name: Some("lead".into()), ..plain(PaneKind::Agent(Agent::ClaudeCode)) });
}
```

- [x] 2. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test loadout member_name` and see it fail to compile (`struct PlanPane has no field named name`).

- [x] 3. In `crates/zygo-workspace/src/loadout.rs`: add `pub name: Option<String>,` to `PlanPane` after `kind` (doc line: `/// The member name (M§6), only when zygo_core::ids::valid_member_name passes (AU84).`), and in `resolve`'s literal (line 175) add `name: p.name.clone().filter(|n| zygo_core::ids::valid_member_name(n)),`.

- [x] 4. Run the step 2 command and see `member_name_resolve_copies_the_pane_name` pass; then `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test loadout` passes whole.

- [x] 5. Write the failing state tests in `crates/zygo-workspace/tests/state.rs` after `first_started_at_set_is_written_as_unix_seconds_and_kept_by_a_write`; add `use zygo_workspace::loadout::{PaneKind as PlanKind, PlanPane};` to the imports:

```rust
#[test] // M§6: up copies the loadout pane name into the fresh member record
fn member_name_fresh_member_takes_the_plan_pane_name() {
    let pp = |name: Option<&str>| PlanPane { kind: PlanKind::Agent(Agent::ClaudeCode), name: name.map(String::from), cwd: "/srv/app".into(),
        profile: None, args: vec![], yolo: false, passthrough: false, server: None, compact: None, config_dir: None };
    let lead = zygo_workspace::up::testing::fresh_member(Agent::ClaudeCode, &pp(Some("lead")));
    assert_eq!(lead.name.as_deref(), Some("lead"));
    assert_eq!(lead.valid_name(), Some("lead"));
    assert_eq!(lead.state, MemberState::Running);
    assert_eq!(serde_json::to_value(&lead).unwrap()["name"], json!("lead"));
    let none = zygo_workspace::up::testing::fresh_member(Agent::ClaudeCode, &pp(None));
    assert_eq!((none.name.as_deref(), none.valid_name()), (None, None));
    assert!(serde_json::to_value(&none).unwrap().get("name").is_none());
    assert_eq!(MemberRecord::new(Agent::OpenCode).name, None);
}

#[test] // AU78, M§6: a workspace.json without `name` loads; a write keeps the name and every unknown field
fn member_name_absent_reads_as_none_and_a_write_keeps_unknown_fields() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut v = doc();
    v["panes"] = json!([{"key": KEY, "window": 0, "index": 0, "cwd": "/srv/app", "kind": "claude-code", "title": "t"}]);
    v["members"] = json!({KEY: {"state": "running", "launch": {"kind": "claude-code"}, "badge": [1]}});
    put(&store, &v);
    let ws = store.read().unwrap().unwrap();
    assert_eq!((ws.panes[0].name.as_deref(), ws.members[KEY].name.as_deref()), (None, None));
    store.update(S5, &mut |ws| { ws.phase = Phase::Down; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert!(after["panes"][0].get("name").is_none());
    assert!(after["members"][KEY].get("name").is_none());
    store.update(S5, &mut |ws| {
        ws.panes[0].name = Some("lead".into());
        ws.members.get_mut(KEY).unwrap().name = Some("lead".into());
        Ok(())
    }).unwrap();
    store.update(S5, &mut |ws| { ws.phase = Phase::Up; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!((&after["panes"][0]["name"], &after["members"][KEY]["name"]), (&json!("lead"), &json!("lead")));
    assert_eq!(after["panes"][0]["title"], json!("t"));
    assert_eq!(after["members"][KEY]["badge"], json!([1]));
    let ws = store.read().unwrap().unwrap();
    assert_eq!((ws.panes[0].name.as_deref(), ws.members[KEY].valid_name()), (Some("lead"), Some("lead")));
}

#[test] // AU84, M§6: a stored name that fails the grammar loads but reads as no name
fn member_name_that_fails_the_grammar_reads_as_no_name() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let long_ok = "a".repeat(32);
    let too_long = "a".repeat(33);
    for (n, want) in [("lead-2", Some("lead-2")), (long_ok.as_str(), Some(long_ok.as_str())), ("Lead", None), ("9lead", None),
        ("-lead", None), ("", None), ("lead_2", None), ("all", None), ("cli", None), ("kabcdefgh", None), (too_long.as_str(), None)] {
        let mut v = doc();
        v["members"] = json!({KEY: {"state": "running", "launch": {"kind": "claude-code"}, "name": n}});
        put(&store, &v);
        let ws = store.read().unwrap().unwrap(); // the file is not refused
        assert_eq!(ws.members[KEY].name.as_deref(), Some(n), "{n:?}");
        assert_eq!(ws.members[KEY].valid_name(), want, "{n:?}");
    }
}
```

- [x] 6. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test state member_name` and see it fail to compile (`no field name on type MemberRecord`, `no field name on type Pane`, `no method named valid_name`, `cannot find function fresh_member in module testing`).

- [x] 7. In `crates/zygo-workspace/src/state.rs`: add to `Pane` after `kind` and to `MemberRecord` after `launch`:

```rust
    /// The member name (M§6); checked at each read by [`MemberRecord::valid_name`] (AU84).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
```

  add `name: None,` to the literal in `MemberRecord::new` (line 299), and in `impl MemberRecord` add:

```rust
    /// The stored name only when it passes `zygo_core::ids::valid_member_name` (M§6, AU84); a name that fails reads as none.
    pub fn valid_name(&self) -> Option<&str> {
        self.name.as_deref().filter(|n| zygo_core::ids::valid_member_name(n))
    }
```

- [x] 8. In `crates/zygo-workspace/src/up.rs`: in `fresh_member` (line 661) add `m.name = pp.name.clone();` after `m.state = MemberState::Running;`; in the `panes[]` literal at line 1130 add `name: plan.fresh.get(&p.key).and_then(|m| m.name.clone()),` before `..Pane::default()`; in `mod testing` add:

```rust
    /// [`super::fresh_member`]: the record that `up` writes for a fresh loadout member (ML-3).
    pub fn fresh_member(agent: zygo_core::Agent, pp: &crate::loadout::PlanPane) -> crate::state::MemberRecord {
        super::fresh_member(agent, pp)
    }
```

- [x] 9. Run the step 6 command and see the three `member_name` tests pass; then `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test state` passes whole (`json_has_the_fields_of_w7_2` and `none_is_absent_and_absent_reads_as_default` still hold: no `name` key when none).

- [x] 10. Write the failing snapshot test in `crates/zygo-workspace/tests/snapshot.rs` after `unknown_fields_of_kept_panes_and_windows_survive`:

```rust
#[test] // M§6, W§7.4 step 5: a snapshot keeps Pane.name of the stored pane with the same key after every %N changed
fn member_name_snapshot_keeps_the_stored_pane_name_by_key() {
    let (_d, p, store) = up(&["kaaaaaaaa", "kbbbbbbbb"]);
    store.update(S5, &mut |ws| { ws.panes[0].name = Some("lead".into()); Ok(()) }).unwrap();
    let read = vec![line(0, "main", 0, "%7", 0, 1, "kbbbbbbbb", "/srv/app"), line(0, "main", 0, "%9", 1, 0, "kaaaaaaaa", "/srv/app")];
    let fake = Fake::new(Some(ID), vec![Ok(read)]);
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    let ws = store.read().unwrap().unwrap();
    let name = |k: &str| ws.panes.iter().find(|x| x.key == k).unwrap().name.clone();
    assert_eq!((name("kaaaaaaaa").as_deref(), name("kbbbbbbbb").as_deref()), (Some("lead"), None));
    assert_eq!(ws.panes.iter().find(|x| x.key == "kaaaaaaaa").unwrap().index, 1); // the read wins for the layout
}
```

- [x] 11. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test snapshot member_name` and see it fail on the assertion (`left: (None, None)`, `right: (Some("lead"), None)`): `apply` builds `Pane` without the stored name.

- [x] 12. In `crates/zygo-workspace/src/snapshot.rs` `apply` (line 340 literal), add `name: ws.panes.iter().find(|x| x.key == *key).and_then(|x| x.name.clone()),` and extend the doc comment at lines 309-313 with "and its `name` (M§6)" after "keeps that pane's unknown fields (AU78)".

- [x] 13. Run the step 11 command and see it pass; then `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test snapshot` passes whole.

- [x] 14. Run `env -u ZYGO_SESSION_DIR cargo build --workspace --all-targets` and `env -u ZYGO_SESSION_DIR cargo clippy -p zygo-workspace --all-targets -- -D warnings`; both exit 0 (no other crate builds a `PlanPane` or a full `MemberRecord` literal).

- [x] 15. Commit the four source files and three test files with subject `ML-3: member name in the plan, the member record and panes[]` and two trailers, each on its own line at the end: `Zygo-Session:` with the id of the session that implements, and `Zygo-Work: phase3:ML-3`. No Claude attribution.

- [ ] 16. Verify: run the Acceptance command from the contract and see every count hold and all tests pass:

```sh
test "$(cargo test -q -p zygo-workspace --test loadout -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-workspace --test state -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 3 && test "$(cargo test -q -p zygo-workspace --test snapshot -- member_name --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-workspace --test loadout --test state --test snapshot
```

  Expected counts: loadout 1, state 3, snapshot 1.
