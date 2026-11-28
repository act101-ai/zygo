# RT-1 — Member record `first_started_at`, set by each commit of a pending start

**Spec:** docs/specs/2026-11-24-session-runtime-design.md §2.1, §3.2, §4, §5 (commit rows), §6 (rows `state.rs`, `zygo-hooks/src/workspace.rs`, `member.rs`), §7 items 4 and 5; base-spec amendment W§7.2 field table and W AU92 (docs/specs/2026-11-20-workspaces-design.md)
**Files:** `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/workspace.rs`, `crates/zygo/src/cmd/member.rs`, `docs/specs/2026-11-20-workspaces-design.md`
**Consumes:** —
**Produces:** `zygo_workspace::state::MemberRecord.first_started_at: Option<u64>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`; `MemberRecord::new` sets `None`); JSON key `members.<pane>.first_started_at` (Unix seconds) in `workspace.json` `version` 1
**Acceptance:** `test "$(cargo test -q -p zygo-workspace -p zygo-hooks -p zygo -- first_started_at --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test --locked -p zygo-workspace -p zygo-hooks -p zygo` → every count holds (at least 6 named `first_started_at…` in the zygo-workspace, zygo-hooks, zygo tests; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows CI, and the tests whose names contain `first_started_at` (at least 6: hook commit sets it, exit commit sets it, second commit keeps it, `pending.started_at = 0` leaves it absent, `NotStarted` exit leaves it absent, old record without the field parses and a write keeps every other field and a set field) run and pass

## Plan

**Premises checked against the live code (branch `phase3/RT-1` at `3a7602c`, equal to `phase-3` head):**

- `MemberRecord` (`crates/zygo-workspace/src/state.rs:96-130`) has no `first_started_at`; `grep -rn first_started_at crates` finds nothing. `pending` is at `:100-101`; `MemberRecord::new` is at `:293-315` and is the only full struct literal of `MemberRecord` in `crates/` (every other construction uses `MemberRecord::new` or `..` update syntax), so the new field needs one line there.
- The hook commit is the `Some(p) if p.zygo_session == session` arm of `record_session` (`crates/zygo-hooks/src/workspace.rs:401-418`). Both member commit paths reach it: Claude Code `SessionStart` (`workspace.rs:637`) and the OpenCode `session` event (`workspace.rs:308-310`).
- The exit commit is the `if names(rec) && let Some(p) = rec.pending.take()` block of `record_exit` (`crates/zygo/src/cmd/member.rs:480-487`); `Exit::NotStarted` returns at `:474-479` before it. The start write `record_start_fields` (`member.rs:430-438`) writes only `pending`, `config_dir` and `program`; `started_at` comes from `member.rs:197` (`map_or(0, …)`).
- Hand start: `join` (`member.rs:793-829`) removes the record unless `keep` (own session argument equal to the stored `agent_session` and the same kind, `:803-807`), then makes it with `MemberRecord::new` (`:808`). So a replaced record starts without the field and a kept one keeps it with no code change; RT-1 only tests this.
- W§7.2 field table (`docs/specs/2026-11-20-workspaces-design.md:290`, row `member.pending`) and W AU92 (`:233`) do not have the T§3.2 text yet. T§3.2 says to add it, and the node Files list names the W spec, so Step 9 amends both rows. W AU29 has no change (T§3.2).
- Test helpers: `crates/zygo-workspace/tests/state.rs` has `paths`, `put`, `raw`, `full()` (a member record with every field set, `pending.started_at = 1_700_000_100`) and `S5`. `crates/zygo-hooks/tests/workspace.rs` is `#![cfg(unix)]` and has `serial()`, `fx(agent)`, `ws_state(&f, phase, rec)` (returns the `Store`), `rec_pending()` (`pending = { SID, started_at: 7 }`, `zygo_session = OLD`), `member(&f)`, `raw(&f)`, `ws_with`, `hook`, `without`, `fixture`, `errors(&f)`. `crates/zygo/src/cmd/member.rs` has its unit tests in `#[cfg(test)] mod tests` (`:958`) with `base()` (`pending = { "S1", started_at: 1 }`, `zygo_session = "Z0"`), `rec(state, agent)`, `ws_with(&[..])` and `start()` (`Start { zygo_session: "S1", agent_session: Some("U1"), started_at: 7, .. }`).
- The hook tests that commit with a working snapshot child are `#[ignore = "timing: …"]` (`workspace.rs:895`, `:1075`). RT-1's hook tests avoid the child: the zygo program is a path that does not exist, and the payload carries no agent session id, so the id does not change and `start_snapshot` is never called (`workspace.rs:430-434`; the pattern of `first_start_without_an_id_keeps_the_stored_id`, `:970-987`, and of the first part of `opencode_session_commits_and_records`, `:1080-1088`). No RT-1 test is ignored, waits or reads the wall clock.
- CI runs `cargo test --workspace --locked --no-fail-fast` on ubuntu, macOS and Windows (`.github/workflows/ci.yml:35,45`). On Windows the hook test file compiles to nothing (`#![cfg(unix)]`); the 7 state and member tests still give a count above 6.
- Review focus assigned to RT-1: item 59 → Step 5 tests `record_exit_first_started_at_is_set_by_the_exit_commit`, `record_exit_first_started_at_is_kept_by_a_later_commit_and_after_a_hook_commit`, `record_exit_first_started_at_not_started_leaves_it_unchanged`; item 60 → Step 3 test `first_started_at_stays_absent_for_a_zero_start_until_a_later_nonzero_hook_commit`, Step 5 tests `record_exit_first_started_at_stays_absent_for_a_zero_start_until_a_later_nonzero_commit` and `join_first_started_at_is_absent_for_a_replaced_record_and_kept_for_an_accepted_resume`.

### Interfaces

Consumes: —.

Produces (`crates/zygo-workspace/src/state.rs`):

```rust
pub struct MemberRecord {
    // … existing fields; placed right after `pending` (W§7.2 order) …
    /// Unix time (seconds) of the first committed start of this record (W§7.2, W AU92 as amended by T§3.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_started_at: Option<u64>,
}
// MemberRecord::new(kind) sets `first_started_at: None`.
```

JSON key `members.<pane>.first_started_at` (Unix seconds, a JSON number) in `workspace.json`, `version` stays 1. No new function: the commit rule is two lines at each of the two commit sites.

The commit rule (W AU92 addition, T§3.2), the same at both sites, where `m` is the record and `p` the `pending` being committed:

```rust
if m.first_started_at.unwrap_or(0) == 0 && p.started_at != 0 {
    m.first_started_at = Some(p.started_at);
}
```

### Author's calls

- The rule is written inline at the two commit sites, not as a new public `MemberRecord` method: Produces names only the field, and two lines in two places cost less than a widened interface.
- A stored `first_started_at = 0` is replaced by the next non-zero commit (T§3.2 "absent or 0"); tested in Step 3 and Step 5.
- The "second commit keeps it" hook test starts from a record that already holds the field (`Some(3)`) with a pending start of `started_at = 7`: a later start is a new zygo session, and the fixture's view session is fixed (`SID`), so a preset record is the faithful form.
- The zero-start hook test makes its "later" commit by writing a new `pending` of the same `SID` with `store.update`, then calling the hook again; the rule does not read the session id.
- The hook tests use the Claude Code `SessionStart` path with no `session_id` and the OpenCode `session` path with only `cwd`, both with a missing zygo program, so no snapshot child starts (no timing, no `#[ignore]`).
- The zygo-crate tests go in the existing `#[cfg(test)] mod tests` of `crates/zygo/src/cmd/member.rs` (a file in the node Files list); no new test file.
- Step 9 amends W§7.2 and W AU92 with the T§3.2 text word for word; no other W row changes (T§3.2: W AU29 no change).

### Steps

- [x] **Step 1: write the failing state tests** in `crates/zygo-workspace/tests/state.rs` (after `update_keeps_unknown_fields`):

```rust
#[test] // T§4, T§7 item 5, W AU78: a member record of an older zygo has no first_started_at; a write keeps every field
fn first_started_at_absent_in_an_old_record_reads_as_none_and_a_write_keeps_every_field() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let v = serde_json::to_value(full()).unwrap(); // every member field set; no first_started_at key
    assert!(v["members"][KEY].get("first_started_at").is_none());
    put(&store, &v);
    assert_eq!(store.read().unwrap().unwrap().members[KEY].first_started_at, None);
    store.update(S5, &mut |ws| { ws.phase = Phase::Down; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!(after["phase"], json!("down"));
    assert_eq!(after["members"][KEY], v["members"][KEY]); // every other field unchanged, no key added
}

#[test] // T§3.2, T§7 item 5, W§7.2: MemberRecord::new has none; a set value is written as Unix seconds and kept by a write
fn first_started_at_set_is_written_as_unix_seconds_and_kept_by_a_write() {
    assert_eq!(MemberRecord::new(Agent::ClaudeCode).first_started_at, None);
    assert!(serde_json::to_value(MemberRecord::new(Agent::OpenCode)).unwrap().get("first_started_at").is_none());
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut v = serde_json::to_value(full()).unwrap();
    v["members"][KEY]["first_started_at"] = json!(1_700_000_100u64);
    put(&store, &v);
    assert_eq!(store.read().unwrap().unwrap().members[KEY].first_started_at, Some(1_700_000_100));
    store.update(S5, &mut |ws| { ws.phase = Phase::Down; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!(after["members"][KEY]["first_started_at"], json!(1_700_000_100u64));
    assert_eq!(after["members"][KEY], v["members"][KEY]);
    let m: MemberRecord = serde_json::from_value(v["members"][KEY].clone()).unwrap();
    assert_eq!(m, MemberRecord { first_started_at: Some(1_700_000_100), ..full().members[KEY].clone() });
}
```

- [x] **Step 2: run the state tests and see them fail to compile** (`no field first_started_at on type MemberRecord`): `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test state first_started_at`.

- [x] **Step 3: write the failing hook tests** in `crates/zygo-hooks/tests/workspace.rs` (after `session_start_refusals`). Helpers, then three tests:

```rust
/// The hook commit of a pending start by `agent` (T§3.2): the Claude Code `SessionStart` with no `session_id`, or the
/// OpenCode `session` call with only `cwd`. The zygo program does not exist and the agent id does not change, so no
/// snapshot child starts.
fn commit_by_hook(f: &Fx, agent: Agent) {
    let missing = f.t.path().join("missing");
    match agent {
        Agent::ClaudeCode => {
            let input = hook(Agent::ClaudeCode, &without(fixture("SessionStart-main"), "session_id"));
            session_start::run_member(&input, &f.dir, &f.paths, &missing, &mut Vec::new()).unwrap();
        }
        Agent::OpenCode => {
            ws_with(f, &missing, "session", json!({"cwd": "/repo"})).unwrap();
        }
    }
}

/// `members.<KEY>` of `workspace.json` as JSON.
fn member_json(f: &Fx) -> Value {
    serde_json::from_slice::<Value>(&raw(f)).unwrap()["members"][KEY].clone()
}

#[test] // T§3.2 (W AU92 addition), T§5 "Fresh member", T§7 item 4
fn first_started_at_is_set_by_the_hook_commit_of_each_agent() {
    let _s = serial();
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let f = fx(agent);
        ws_state(&f, Phase::Up, rec_pending()); // pending SID, started_at 7; no first_started_at
        commit_by_hook(&f, agent);
        let m = member(&f);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.first_started_at), (None, Some(SID), Some(7)), "{agent:?}");
        assert_eq!(member_json(&f)["first_started_at"], json!(7), "{agent:?}");
        assert!(errors(&f).is_empty(), "{agent:?}");
    }
}

#[test] // T§3.2 "never changed after it is set", T§5 "`zygo down`, then `zygo up`", T§7 item 4
fn first_started_at_is_kept_by_a_later_hook_commit() {
    let _s = serial();
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let f = fx(agent);
        ws_state(&f, Phase::Up, MemberRecord { first_started_at: Some(3), ..rec_pending() }); // pending started_at 7
        commit_by_hook(&f, agent);
        let m = member(&f);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.first_started_at), (None, Some(SID), Some(3)), "{agent:?}");
    }
}

#[test] // T§2.2 (0 is unknown), T§3.2, T§7 item 4; graph review focus 60
fn first_started_at_stays_absent_for_a_zero_start_until_a_later_nonzero_hook_commit() {
    let _s = serial();
    let zero = |at| MemberRecord { pending: Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: at, ..Default::default() }), ..rec_pending() };
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let f = fx(agent);
        let store = ws_state(&f, Phase::Up, zero(0));
        commit_by_hook(&f, agent);
        let m = member(&f);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.first_started_at), (None, Some(SID), None), "{agent:?}");
        assert!(member_json(&f).get("first_started_at").is_none(), "{agent:?}");
        // a later start with a non-zero time: its commit sets the field
        store.update(S5, &mut |w| {
            w.members.get_mut(KEY).unwrap().pending = Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: 11, ..Default::default() });
            Ok(())
        }).unwrap();
        commit_by_hook(&f, agent);
        assert_eq!(member(&f).first_started_at, Some(11), "{agent:?}");
        // a stored 0 counts as absent (T§3.2 "absent or 0")
        let g = fx(agent);
        ws_state(&g, Phase::Up, MemberRecord { first_started_at: Some(0), ..rec_pending() });
        commit_by_hook(&g, agent);
        assert_eq!(member(&g).first_started_at, Some(7), "{agent:?}");
    }
}
```

- [x] **Step 4: run the hook tests and see them fail to compile** (`no field first_started_at`): `env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks --test workspace first_started_at`.

- [x] **Step 5: write the failing zygo-crate tests** in `crates/zygo/src/cmd/member.rs` `mod tests` (after `record_exit_changes_nothing_for_another_session`; `join_…` after `join_refuses_with_the_reason_and_changes_nothing`, since it uses `start()`):

```rust
#[test] // T§3.2 (W AU92 addition), T§5 "Commit without the lock", T§7 item 4; graph review focus 59
fn record_exit_first_started_at_is_set_by_the_exit_commit() {
    for exit in [Exit::Ended, Exit::Resumable { failed_fast: false }, Exit::Resumable { failed_fast: true }] {
        let mut r = base(); // pending S1, started_at 1; no first_started_at
        assert!(record_exit(&mut r, "S1", exit));
        assert_eq!((r.pending.is_none(), r.zygo_session.as_deref(), r.first_started_at), (true, Some("S1"), Some(1)), "{exit:?}");
        let mut r = MemberRecord { first_started_at: Some(0), ..base() }; // 0 counts as absent
        assert!(record_exit(&mut r, "S1", exit));
        assert_eq!(r.first_started_at, Some(1), "{exit:?}");
    }
}

#[test] // T§3.2 "never changed after it is set", W AU92; graph review focus 59
fn record_exit_first_started_at_is_kept_by_a_later_commit_and_after_a_hook_commit() {
    let mut r = MemberRecord { first_started_at: Some(100), ..base() }; // a later start: pending started_at 1
    assert!(record_exit(&mut r, "S1", Exit::Ended));
    assert_eq!((r.zygo_session.as_deref(), r.first_started_at), (Some("S1"), Some(100)));
    // a hook already committed S1 and set the field: the exit takes no pending and keeps it
    let mut r = base();
    r.pending = None;
    r.zygo_session = Some("S1".into());
    r.first_started_at = Some(7);
    assert!(record_exit(&mut r, "S1", Exit::Ended));
    assert_eq!((r.state, r.first_started_at), (MemberState::Ended, Some(7)));
}

#[test] // T§3.2, T§2.2 (0 is unknown), T§7 item 4; graph review focus 60
fn record_exit_first_started_at_stays_absent_for_a_zero_start_until_a_later_nonzero_commit() {
    let mut r = base();
    r.pending.as_mut().unwrap().started_at = 0;
    assert!(record_exit(&mut r, "S1", Exit::Ended));
    assert_eq!((r.pending.is_none(), r.first_started_at), (true, None));
    // the next start writes pending (the start write never sets the field), then its exit commit sets it
    record_start_fields(&mut r, &Start { zygo_session: "S2".into(), started_at: 50, ..start() });
    assert_eq!(r.first_started_at, None);
    assert!(record_exit(&mut r, "S2", Exit::Ended));
    assert_eq!(r.first_started_at, Some(50));
}

#[test] // T§3.2 "a start that never spawns leaves no trace", T§7 item 4; graph review focus 59
fn record_exit_first_started_at_not_started_leaves_it_unchanged() {
    let mut r = base();
    assert!(record_exit(&mut r, "S1", Exit::NotStarted));
    assert_eq!((r.pending.is_none(), r.zygo_session.as_deref(), r.first_started_at), (true, Some("Z0"), None));
    let mut r = MemberRecord { first_started_at: Some(5), ..base() };
    assert!(record_exit(&mut r, "S1", Exit::NotStarted));
    assert_eq!(r.first_started_at, Some(5));
}

#[test] // T§3.2 "a hand start that replaces the record", T§5 hand start row, W§8.4; graph review focus 60
fn join_first_started_at_is_absent_for_a_replaced_record_and_kept_for_an_accepted_resume() {
    let k = "kaaaaaaaa";
    let none = |_: &MemberRecord| None;
    let l = Launch::new(Agent::ClaudeCode);
    // replaced: an ended record with no own session argument is removed and made again
    let mut old = rec(MemberState::Ended, Some("old"));
    old.first_started_at = Some(100);
    let mut w = ws_with(&[(k, old)]);
    join(&mut w, k, &l, &start(), None, &none).unwrap();
    assert_eq!((w.members[k].first_started_at, w.members[k].pending.as_ref().map(|p| p.started_at)), (None, Some(7)));
    let mut m = w.members[k].clone();
    assert!(record_exit(&mut m, "S1", Exit::Ended)); // its first commit sets it from this start
    assert_eq!(m.first_started_at, Some(7));
    // kept: the accepted resume of the saved conversation keeps the field
    let mut keep = rec(MemberState::Resumable, Some("abc"));
    keep.first_started_at = Some(100);
    let mut w = ws_with(&[(k, keep)]);
    join(&mut w, k, &l, &Start { agent_session: None, ..start() }, Some("abc"), &none).unwrap();
    assert_eq!((w.members[k].agent_session.as_deref(), w.members[k].first_started_at), (Some("abc"), Some(100)));
}
```

- [x] **Step 6: run the zygo-crate tests and see them fail to compile** (`no field first_started_at`): `env -u ZYGO_SESSION_DIR cargo test -p zygo --lib first_started_at`.

- [x] **Step 7: add the field** in `crates/zygo-workspace/src/state.rs`: in `MemberRecord`, right after `pending` (`:100-101`), the Produces field with its doc line and `#[serde(default, skip_serializing_if = "Option::is_none")]`; in `MemberRecord::new` (`:296-313`), `first_started_at: None,` after `pending: None,`. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --test state` → all pass, including the two `first_started_at…` tests and the unchanged `json_has_the_fields_of_w7_2` and `none_is_absent_and_absent_reads_as_default`.

- [x] **Step 8: set the field at both commits.** (a) `crates/zygo-hooks/src/workspace.rs`, `record_session`, arm `Some(p) if p.zygo_session == session` (`:402`): as its first statement, the commit rule of Interfaces with `m` and `p`. Extend the doc comment of `record_session` (`:381-387`) after "moves into the record (its own agent session id when it has one)" with ", `first_started_at` is set from `pending.started_at` when it is absent or 0 and that time is not 0 (T§3.2)". (b) `crates/zygo/src/cmd/member.rs`, `record_exit`, block `if names(rec) && let Some(p) = rec.pending.take()` (`:480-482`): as its first statement, the same rule with `m` = `rec`. Extend the doc comment (`:467-471`) after "commit that `pending` when no hook did" with " (setting `first_started_at` from its `started_at` when absent or 0 and that time is not 0, T§3.2)". Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks --test workspace first_started_at` → 3 pass; `env -u ZYGO_SESSION_DIR cargo test -p zygo --lib first_started_at` → 5 pass.

- [x] **Step 9: amend the base spec** `docs/specs/2026-11-20-workspaces-design.md` with the T§3.2 text word for word. (a) W§7.2 field table: a new row right after the `member.pending` row (`:290`): ``| `member.first_started_at` | Unix time (seconds) of the first committed start of this member record: the `pending.started_at` of the first commit (W AU92). Absent until then, and in a record of an older zygo (T§4). Never changed after it is set; a new record (a fresh loadout member, `up --reset`, a hand start that replaces the record, `zygo ws forget-pane`) starts without it. |`` (b) W AU92 (`:233`): before the closing ` | author |`, append the sentences: ``Each commit of a pending start (the member `SessionStart`, the OpenCode `session` call, the wrapper's exit commit) sets `first_started_at = pending.started_at` when `first_started_at` is absent or 0 and `pending.started_at` is not 0. The start write itself does not set it: a start that never spawns (`spawned = false`) leaves no trace.`` Check: `grep -o first_started_at docs/specs/2026-11-20-workspaces-design.md | wc -l` → `3` (1 in the new row, 2 in AU92); `git diff --stat -- docs` lists only that spec.

- [x] **Step 10: run the acceptance check**: `env -u ZYGO_SESSION_DIR sh -c 'test "$(cargo test -q -p zygo-workspace -p zygo-hooks -p zygo -- first_started_at --list 2>/dev/null | grep -c ": test$")" -ge 6 && cargo test --locked -p zygo-workspace -p zygo-hooks -p zygo'` → exit 0; the count is 10 on unix (2 state, 3 hook, 5 zygo) and every test passes with no new `#[ignore]`.

- [x] **Step 11: commit** the six files of the node (`crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/workspace.rs`, `crates/zygo/src/cmd/member.rs`, `docs/specs/2026-11-20-workspaces-design.md`) with message `RT-1: member record first_started_at, set by each commit of a pending start`, ending with two trailer lines: `Zygo-Session:` followed by the implementing session's id, and `Zygo-Work: phase3:RT-1`; no Claude attribution. Check: `git show --stat HEAD` lists exactly those six files.

### Review round 1

- Test gap closed: `undo_start_first_started_at_comes_back_after_a_replaced_start_that_never_spawns` (`crates/zygo/src/cmd/member.rs`) proves that a hand start which replaced a record with `first_started_at = Some(100)`, then `Exit::NotStarted` → `undo_start`, restores the old record with `Some(100)` (T§3.2 "a start that never spawns leaves no trace").
- Refuted: hostile values for the u64 field. It has the same type and source as `pending.started_at`, and the spec fixes that type.
- Refuted: `null` dropped on write. Produces fixes `skip_serializing_if = "Option::is_none"`, so `None` is absent by design.
- Refuted: `up` forget deleting a record that holds the field. This is the pre-existing W§7.2 wording, not a change of this node; reported to the operator.
