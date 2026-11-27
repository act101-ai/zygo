use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_config::repo_def::Pin;
use zygo_workspace::loadout::{PaneKind as PlanKind, PlanPane};
use zygo_workspace::state::*;

const ID: &str = "0123456789abcdef";
const KEY: &str = "kabcdefgh";
const S5: Duration = Duration::from_secs(5);

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}
/// The identity fields that `up` sets at creation.
fn identity(ws: &mut Workspace) -> anyhow::Result<()> {
    ws.name = "app".into(); ws.common_dir = "/srv/app/.git".into(); ws.main_root = "/srv/app".into();
    ws.tmux_session = "zygo-app".into(); ws.loadout = "solo".into(); ws.phase = Phase::Building; ws.phase_since = 1_700_000_000;
    Ok(())
}
/// A valid version 1 file as JSON; a test changes one field and writes it with `put`.
fn doc() -> Value {
    json!({"version": 1, "id": ID, "name": "app", "common_dir": "/srv/app/.git", "main_root": "/srv/app",
        "tmux_session": "zygo-app", "loadout": "solo", "phase": "up", "phase_since": 1700000000u64})
}
fn put(store: &Store, v: &Value) {
    fs::create_dir_all(store.dir()).unwrap();
    fs::write(store.dir().join("workspace.json"), serde_json::to_vec(v).unwrap()).unwrap();
}
fn raw(store: &Store) -> Vec<u8> { fs::read(store.dir().join("workspace.json")).unwrap() }

fn full() -> Workspace {
    let mut ws = Workspace::new(ID);
    identity(&mut ws).unwrap();
    ws.phase = Phase::Up;
    ws.tmux_socket = Some("/tmp/tmux-1000/default".into());
    ws.snapshot_at = Some(1_700_000_050);
    ws.windows.push(Window { index: 0, name: Some("main".into()), layout: "b25d,200x50,0,0,0".into(), width: 200, height: 50,
        active: true, zoomed: false, ..Default::default() });
    ws.panes.push(Pane { key: KEY.into(), window: 0, index: 0, active: true, cwd: "/srv/app".into(), kind: PaneKind::ClaudeCode,
        ..Default::default() });
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.zygo_session = Some("01JD0000000000000000000000".into());
    m.pending = Some(Pending { zygo_session: "01JD0000000000000000000001".into(), agent_session: Some("a-2".into()),
        started_at: 1_700_000_100, ..Default::default() });
    m.agent_session = Some("a-1".into());
    m.state = MemberState::Running;
    m.launch.profile = Some("fast".into());
    m.launch.server = Some("https://zygo.example".into());
    m.launch.compact = Some("120k".into());
    m.launch.yolo = true;
    m.launch.config_dir = Some("work".into());
    m.launch.args = vec!["--model".into(), "opus".into()];
    m.config_dir = Some("/home/u/work".into());
    m.program = Some("/usr/bin/claude".into());
    m.overrides = Some(json!({"results": false, "guards": {}}));
    m.compact = Some(CompactOverride::Set("120000".into()));
    m.interrupted = vec![Subagent { agent_id: "a1".into(), agent_type: "Explore".into(), ..Default::default() }];
    m.closed_units = vec!["W2".into()];
    m.dropped_flags = vec!["--agents".into()];
    m.clean_stop = true;
    m.note_pending = true;
    m.saved_for = Some("01JD0000000000000000000000".into());
    ws.members.insert(KEY.into(), m);
    ws
}

#[test] // W§7.2: every field name and value form; W-C7: no other field exists
fn json_has_the_fields_of_w7_2() {
    let want = json!({
        "version": 1, "id": ID, "name": "app", "common_dir": "/srv/app/.git", "main_root": "/srv/app",
        "tmux_socket": "/tmp/tmux-1000/default", "tmux_session": "zygo-app", "loadout": "solo",
        "phase": "up", "phase_since": 1700000000u64, "down_all": false, "snapshot_at": 1700000050u64,
        "windows": [{"index": 0, "name": "main", "layout": "b25d,200x50,0,0,0", "width": 200, "height": 50, "active": true, "zoomed": false}],
        "panes": [{"key": KEY, "window": 0, "index": 0, "active": true, "cwd": "/srv/app", "kind": "claude-code"}],
        "members": {KEY: {
            "zygo_session": "01JD0000000000000000000000",
            "pending": {"zygo_session": "01JD0000000000000000000001", "agent_session": "a-2", "started_at": 1700000100u64},
            "agent_session": "a-1", "state": "running",
            "launch": {"kind": "claude-code", "profile": "fast", "passthrough": false, "server": "https://zygo.example",
                "compact": "120k", "yolo": true, "config_dir": "work", "args": ["--model", "opus"]},
            "config_dir": "/home/u/work", "program": "/usr/bin/claude",
            "overrides": {"results": false, "guards": {}}, "compact": "set 120000",
            "interrupted": [{"agent_id": "a1", "agent_type": "Explore"}],
            "closed_units": ["W2"], "dropped_flags": ["--agents"], "clean_stop": true, "note_pending": true,
            "saved_for": "01JD0000000000000000000000"}}
    });
    assert_eq!(serde_json::to_value(full()).unwrap(), want);
    assert_eq!(serde_json::from_value::<Workspace>(want).unwrap(), full());
}

#[test] // W§7.2: a value that is none or absent is not written; a short file reads with the defaults
fn none_is_absent_and_absent_reads_as_default() {
    let v = serde_json::to_value(MemberRecord::new(Agent::OpenCode)).unwrap();
    assert_eq!(v, json!({"state": "ended", "launch": {"kind": "opencode", "passthrough": false, "yolo": false, "args": []},
        "interrupted": [], "closed_units": [], "dropped_flags": [], "clean_stop": false, "note_pending": false}));
    let m: MemberRecord = serde_json::from_value(json!({"state": "resumable", "launch": {"kind": "opencode"}})).unwrap();
    assert_eq!(m, MemberRecord { state: MemberState::Resumable, ..MemberRecord::new(Agent::OpenCode) });
    let ws: Workspace = serde_json::from_value(doc()).unwrap();
    assert_eq!((ws.tmux_socket, ws.snapshot_at, ws.down_all, ws.windows.len(), ws.panes.len(), ws.members.len()), (None, None, false, 0, 0, 0));
}

#[test] // W§7.2 value sets
fn enums_use_the_spec_words() {
    for (p, s) in [(Phase::Building, "building"), (Phase::Up, "up"), (Phase::Draining, "draining"), (Phase::Stopping, "stopping"), (Phase::Down, "down")] {
        assert_eq!(serde_json::to_value(p).unwrap(), json!(s));
    }
    for (m, s) in [(MemberState::Running, "running"), (MemberState::Resumable, "resumable"), (MemberState::Ended, "ended")] {
        assert_eq!(serde_json::to_value(m).unwrap(), json!(s));
    }
    for (k, s) in [(PaneKind::ClaudeCode, "claude-code"), (PaneKind::OpenCode, "opencode"), (PaneKind::Shell, "shell")] {
        assert_eq!(serde_json::to_value(k).unwrap(), json!(s));
    }
    assert_eq!(serde_json::to_value(CompactOverride::Cleared).unwrap(), json!("cleared"));
    assert_eq!(serde_json::from_value::<CompactOverride>(json!("set off")).unwrap(), CompactOverride::Set("off".into()));
    for bad in ["", "set", "set ", "clear", "SET 1"] {
        assert!(serde_json::from_value::<CompactOverride>(json!(bad)).is_err(), "{bad:?}");
    }
    assert!(serde_json::from_value::<Phase>(json!("paused")).is_err());
}

#[test] // AU78: every struct keeps the fields it does not know
fn every_struct_keeps_unknown_fields() {
    let mut ws = full();
    ws.members.get_mut(KEY).unwrap().launch.definition = Some(Pin::new(COMMIT.into(), DIGEST.into()));
    let mut v = serde_json::to_value(ws).unwrap();
    for at in ["", "/windows/0", "/panes/0", "/members/kabcdefgh", "/members/kabcdefgh/pending", "/members/kabcdefgh/launch",
        "/members/kabcdefgh/launch/definition", "/members/kabcdefgh/interrupted/0"] {
        v.pointer_mut(at).unwrap().as_object_mut().unwrap().insert("future".into(), json!({"n": [1, "x"]}));
    }
    let ws: Workspace = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(ws.extra["future"], json!({"n": [1, "x"]}));
    assert_eq!(serde_json::to_value(&ws).unwrap(), v);
}

#[test]
fn read_of_a_missing_state_is_none_and_creates_nothing() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    assert_eq!(store.dir(), p.workspaces_dir().join(ID));
    assert!(matches!(store.read(), Ok(None)));
    assert_eq!(store.phase_unlocked(), None);
    assert!(!p.workspaces_dir().exists());
}

#[test]
fn read_gives_the_state_and_its_phase() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    put(&store, &doc());
    let ws = store.read().unwrap().unwrap();
    assert_eq!((ws.version, ws.id.as_str(), ws.name.as_str(), ws.phase), (1, ID, "app", Phase::Up));
    assert_eq!(store.phase_unlocked(), Some(Phase::Up));
}

#[test] // AU29: also when the newer form does not fit the version 1 types
fn read_refuses_a_newer_version() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut v = doc();
    v["version"] = json!(2);
    v["panes"] = json!("another form");
    put(&store, &v);
    let e = store.read().unwrap_err();
    assert!(matches!(&e, StateError::Newer { path } if path == &store.dir().join("workspace.json")));
    assert_eq!(e.to_string(), "workspace state is from a newer zygo; run zygo update");
    assert_eq!(store.phase_unlocked(), None);
}

#[test] // AU84 with hostile input, W§10 "does not parse"
fn read_validates_each_field() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let long = "a".repeat(33);
    let cases: Vec<(&str, Value, &str)> = vec![
        ("/id", json!("abc"), "invalid id"),
        ("/id", json!("0123456789ABCDEF"), "invalid id"),
        ("/id", json!("../../../../etc"), "invalid id"),
        ("/id", json!("fedcba9876543210"), "invalid id"), // valid form, not this directory
        ("/name", json!("a b"), "invalid name"),
        ("/name", json!(""), "invalid name"),
        ("/name", json!("App"), "invalid name"),
        ("/name", json!("a;rm -rf"), "invalid name"),
        ("/name", json!(long), "invalid name"),
        ("/tmux_session", json!("other"), "invalid tmux_session"),
        ("/tmux_session", json!("zygo-other"), "invalid tmux_session"),
        ("/tmux_session", json!("zygo-app\n"), "invalid tmux_session"),
        ("/tmux_session", json!("app"), "invalid tmux_session"),
        ("/version", json!(0), "invalid version"),
    ];
    for (at, value, reason) in cases {
        let mut v = doc();
        *v.pointer_mut(at).unwrap() = value.clone();
        put(&store, &v);
        let e = store.read().unwrap_err();
        assert!(matches!(&e, StateError::Invalid { reason: r, .. } if r == reason), "{at} = {value}: {e}");
        assert_eq!(store.phase_unlocked(), None);
    }
    for key in ["k$(x)", "#{pane_id}", "kabcdefg", "kabcdefghi", "kABCDEFGH", "kabcdef01", "xabcdefgh", ""] {
        let mut v = doc();
        v["panes"] = json!([{"key": key, "window": 0, "index": 0, "cwd": "/srv/app", "kind": "shell"}]);
        put(&store, &v);
        assert!(matches!(store.read(), Err(StateError::Invalid { reason, .. }) if reason == "invalid pane key"), "panes {key:?}");
        let mut v = doc();
        v["members"] = json!({key: {"state": "ended", "launch": {"kind": "opencode"}}});
        put(&store, &v);
        assert!(matches!(store.read(), Err(StateError::Invalid { reason, .. }) if reason == "invalid pane key"), "members {key:?}");
    }
    for text in ["", "{", "[]", "{\"version\": 1}", "\u{0}"] {
        fs::write(store.dir().join("workspace.json"), text).unwrap();
        let e = store.read().unwrap_err();
        assert!(matches!(&e, StateError::Invalid { path, .. } if path == &store.dir().join("workspace.json")), "{text:?}");
        assert!(e.to_string().starts_with(&store.dir().join("workspace.json").display().to_string()));
    }
}

#[test] // AU84 on the id given to `open`: no path outside the state directory is read
fn an_invalid_store_id_reads_nothing() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    fs::create_dir_all(p.workspaces_dir().join("x")).unwrap();
    fs::write(p.workspaces_dir().join("x/workspace.json"), serde_json::to_vec(&doc()).unwrap()).unwrap();
    for id in ["x", "../workspaces/x", "", "0123456789abcdeg"] {
        let store = Store::open(&p, id);
        assert!(matches!(store.read(), Err(StateError::Invalid { reason, .. }) if reason == "invalid id"), "{id:?}");
        assert_eq!(store.phase_unlocked(), None);
    }
}

#[test] // W§7.3: `update` creates the state from `Workspace::new`, and each later call reads, changes and writes
fn update_creates_then_reads_modifies_and_writes() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let made = store.update(S5, &mut identity).unwrap();
    assert_eq!((made.version, made.id.as_str(), made.phase), (1, ID, Phase::Building));
    assert_eq!(store.read().unwrap().unwrap(), made);
    store.update(S5, &mut |ws| { ws.members.insert(KEY.into(), MemberRecord::new(Agent::ClaudeCode)); Ok(()) }).unwrap();
    let got = store.update(S5, &mut |ws| { ws.snapshot_at = Some(7); Ok(()) }).unwrap();
    assert!(got.members.contains_key(KEY)); // the second writer kept the first writer's change
    assert_eq!(got.snapshot_at, Some(7));
    assert_eq!(store.read().unwrap().unwrap(), got);
}

#[test] // AU84, W§7.3: a writer refuses a result that no reader would accept, and writes nothing
fn update_refuses_a_result_larger_than_the_read_limit() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    store.update(S5, &mut identity).unwrap();
    store.update(S5, &mut |ws| { ws.members.insert(KEY.into(), MemberRecord::new(Agent::ClaudeCode)); Ok(()) }).unwrap();
    let before = raw(&store);
    let r = store.update(S5, &mut |ws| {
        ws.members.get_mut(KEY).unwrap().zygo_session = Some("x".repeat(2 * 1024 * 1024));
        Ok(())
    });
    let Err(e) = r else { panic!("a state larger than the read limit was written") };
    assert!(
        matches!(&e, StateError::Invalid { reason, .. } if reason.starts_with("the new state is ")
            && reason.ends_with(" bytes, more than the 1048576 bytes that every reader accepts (AU84); not written")),
        "{e}"
    );
    assert_eq!(raw(&store), before);
    assert!(matches!(store.read(), Ok(Some(_))));
}

#[cfg(unix)]
#[test] // AU2, W§7.1: directory 0700, files 0600
fn modes_are_0700_and_0600() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    store.update(S5, &mut identity).unwrap();
    let _op = store.op_lock().unwrap();
    store.log_error("x");
    let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(store.dir().to_path_buf()), 0o700);
    for f in ["workspace.json", "lock", "op.lock", "errors.log"] {
        assert_eq!(mode(store.dir().join(f)), 0o600, "{f}");
    }
}

#[test] // AU78, W§18 #11 second half: a version 1 file with extra fields keeps them after a write
fn update_keeps_unknown_fields() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut v = doc();
    v["future"] = json!({"a": 1});
    v["windows"] = json!([{"index": 0, "layout": "l", "width": 80, "height": 24, "glow": true}]);
    v["panes"] = json!([{"key": KEY, "window": 0, "index": 0, "cwd": "/srv/app", "kind": "shell", "title": "t"}]);
    v["members"] = json!({KEY: {"state": "resumable", "launch": {"kind": "opencode", "tier": 3}, "badge": [1],
        "pending": {"zygo_session": "s1", "started_at": 5, "why": "w"}, "interrupted": [{"agent_id": "a", "agent_type": "b", "depth": 2}]}});
    put(&store, &v);
    store.update(S5, &mut |ws| { ws.phase = Phase::Down; Ok(()) }).unwrap();
    let after: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!(after["phase"], json!("down"));
    assert_eq!(after["future"], json!({"a": 1}));
    assert_eq!(after["windows"][0]["glow"], json!(true));
    assert_eq!(after["panes"][0]["title"], json!("t"));
    assert_eq!(after["members"][KEY]["badge"], json!([1]));
    assert_eq!(after["members"][KEY]["launch"]["tier"], json!(3));
    assert_eq!(after["members"][KEY]["pending"]["why"], json!("w"));
    assert_eq!(after["members"][KEY]["interrupted"][0]["depth"], json!(2));
}

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

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const DIGEST: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

#[test] // R§7.4, R§9.3 effect 2, W§7.2: member.launch.job and member.launch.definition = { commit, digest } round-trip through a write
fn launch_definition_and_job_set_are_written_and_kept_by_a_write() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut ws = full();
    let m = ws.members.get_mut(KEY).unwrap();
    m.launch.job = Some("worker".into());
    m.launch.definition = Some(Pin::new(COMMIT.into(), DIGEST.into()));
    let v = serde_json::to_value(&ws).unwrap();
    assert_eq!(v["members"][KEY]["launch"]["job"], json!("worker"));
    assert_eq!(v["members"][KEY]["launch"]["definition"], json!({"commit": COMMIT, "digest": DIGEST}));
    put(&store, &v);
    let read = store.read().unwrap().unwrap();
    assert_eq!(read.members[KEY].launch.job, Some("worker".to_string()));
    assert_eq!(read.members[KEY].launch.definition, Some(Pin::new(COMMIT.into(), DIGEST.into())));
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

#[test] // M§6: up copies the loadout pane name into the fresh member record
fn member_name_fresh_member_takes_the_plan_pane_name() {
    let pp = |name: Option<&str>| PlanPane { kind: PlanKind::Agent(Agent::ClaudeCode), name: name.map(String::from), cwd: "/srv/app".into(),
        profile: None, args: vec![], yolo: false, passthrough: false, server: None, compact: None, config_dir: None,
        job: None, pool: false, definition: None };
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

#[test] // AU84, M§6, ML-3 review round 1: a stored pane name that fails the grammar reads as no name through Pane::valid_name
fn member_name_pane_valid_name_checks_the_grammar() {
    for (n, want) in [("lead", Some("lead")), ("a/b", None), ("le\u{7}ad", None), ("lead\n", None), ("all", None)] {
        assert_eq!(Pane { name: Some(n.into()), ..Pane::default() }.valid_name(), want, "{n:?}");
    }
    assert_eq!(Pane::default().valid_name(), None);
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    for n in ["a/b", "le\u{1b}ad"] {
        let mut v = doc();
        v["panes"] = json!([{"key": KEY, "window": 0, "index": 0, "cwd": "/srv/app", "kind": "claude-code", "name": n}]);
        put(&store, &v);
        let ws = store.read().unwrap().unwrap(); // the file is not refused
        assert_eq!((ws.panes[0].name.as_deref(), ws.panes[0].valid_name()), (Some(n), None), "{n:?}");
    }
}

#[test] // AU84, ML-3 review round 1: a `name` of the wrong JSON type loads as no name; the file and the other fields stay
fn member_name_of_the_wrong_json_type_reads_as_none() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    for bad in [json!(5), json!(["a"]), json!({"a": 1}), json!(true), json!(null)] {
        let mut v = doc();
        v["panes"] = json!([{"key": KEY, "window": 0, "index": 2, "cwd": "/srv/app", "kind": "claude-code", "name": bad}]);
        v["members"] = json!({KEY: {"state": "running", "launch": {"kind": "claude-code"}, "name": bad, "badge": [1]}});
        put(&store, &v);
        let ws = store.read().unwrap().unwrap();
        let (pane, m) = (&ws.panes[0], &ws.members[KEY]);
        assert_eq!((pane.name.as_deref(), pane.valid_name(), m.name.as_deref(), m.valid_name()), (None, None, None, None), "{bad}");
        assert_eq!((pane.index, pane.kind, m.state), (2, PaneKind::ClaudeCode, MemberState::Running), "{bad}");
        assert_eq!(m.extra.get("badge"), Some(&json!([1])), "{bad}");
    }
}

#[test] // M§6, ML-3 review round 1: the panes[] entry of a loadout up takes the name of an agent pane; a shell pane and an AU98 pane get none
fn member_name_loadout_pane_entry_takes_the_name_of_an_agent_pane_only() {
    let pp = |kind: PlanKind, server: Option<&str>| PlanPane { kind, name: Some("lead".into()), cwd: "/srv/app".into(),
        profile: None, args: vec![], yolo: false, passthrough: false, server: server.map(String::from), compact: None, config_dir: None,
        job: None, pool: false, definition: None };
    let entry = |p: &PlanPane| zygo_workspace::up::testing::loadout_pane(p);
    let agent = entry(&pp(PlanKind::Agent(Agent::ClaudeCode), Some("https://zygo.example")));
    assert_eq!((agent.kind, agent.name.as_deref(), agent.valid_name()), (PaneKind::ClaudeCode, Some("lead"), Some("lead")));
    let shell = entry(&pp(PlanKind::Shell, None));
    assert_eq!((shell.kind, shell.name), (PaneKind::Shell, None));
    let au98 = entry(&pp(PlanKind::Agent(Agent::OpenCode), Some("https://u:p@zygo.example")));
    assert_eq!((au98.kind, au98.name), (PaneKind::Shell, None));
}

#[test] // M§6, D18-D20, ML-3 review round 1: `ws forget-pane` drops the record and clears the name of its pane
fn member_name_forget_pane_clears_the_pane_name() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    let mut v = doc();
    v["phase"] = json!("down");
    v["panes"] = json!([{"key": KEY, "window": 0, "index": 0, "cwd": "/srv/app", "kind": "claude-code", "name": "lead"}]);
    v["members"] = json!({KEY: {"state": "resumable", "launch": {"kind": "claude-code"}, "name": "lead"}});
    put(&store, &v);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = zygo_workspace::admin::forget_pane(&p, &t.path().join("no-tmux"), Some(ID), t.path(), KEY, &mut out, &mut err);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let ws = store.read().unwrap().unwrap();
    assert!(ws.members.is_empty());
    assert_eq!((ws.panes[0].kind, ws.panes[0].name.as_deref()), (PaneKind::Shell, None));
}

#[test] // AU29, W§18 #11 first half; W§10: a state that does not parse is not changed
fn update_changes_no_file_it_cannot_read() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut newer = doc();
    newer["version"] = json!(2);
    let mut hostile = doc();
    hostile["name"] = json!("a b");
    for (v, is_newer) in [(newer, true), (hostile, false)] {
        put(&store, &v);
        let before = raw(&store);
        let mut calls = 0;
        let e = store.update(S5, &mut |ws| { calls += 1; ws.phase = Phase::Down; Ok(()) }).unwrap_err();
        assert_eq!(matches!(e, StateError::Newer { .. }), is_newer);
        assert_eq!(matches!(e, StateError::Invalid { .. }), !is_newer);
        assert_eq!(calls, 0);
        assert_eq!(raw(&store), before);
    }
}

#[test] // AU84 at the write; a closure error writes nothing
fn update_refuses_an_invalid_result_and_a_closure_error() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    // a member write for a workspace with no state creates none (design call b)
    let e = store.update(S5, &mut |ws| { ws.members.insert(KEY.into(), MemberRecord::new(Agent::ClaudeCode)); Ok(()) }).unwrap_err();
    assert!(matches!(e, StateError::Invalid { reason, .. } if reason == "invalid name"));
    assert!(!store.dir().join("workspace.json").exists());
    store.update(S5, &mut identity).unwrap();
    let before = raw(&store);
    type Change = fn(&mut Workspace);
    let hostile: [(&str, Change); 6] = [
        ("invalid version", |ws| ws.version = 2),
        ("invalid id", |ws| ws.id = "fedcba9876543210".into()),
        ("invalid name", |ws| ws.name = "a b".into()),
        ("invalid tmux_session", |ws| ws.tmux_session = "other".into()),
        ("invalid pane key", |ws| ws.panes.push(Pane { key: "k$(x)".into(), ..Default::default() })),
        ("invalid pane key", |ws| { ws.members.insert("#{pane_id}".into(), MemberRecord::new(Agent::OpenCode)); }),
    ];
    for (reason, change) in hostile {
        let e = store.update(S5, &mut |ws| { change(ws); Ok(()) }).unwrap_err();
        assert!(matches!(&e, StateError::Invalid { reason: r, .. } if r == reason), "{reason}: {e}");
        assert_eq!(raw(&store), before);
    }
    let e = store.update(S5, &mut |ws| { ws.phase = Phase::Down; anyhow::bail!("pane gone") }).unwrap_err();
    assert!(matches!(e, StateError::Refused(_)));
    assert_eq!(e.to_string(), "pane gone");
    assert_eq!(raw(&store), before);
    // an invalid store id: no directory is made
    let p = paths(t.path());
    let bad = Store::open(&p, "../x");
    assert!(matches!(bad.update(S5, &mut identity), Err(StateError::Invalid { .. })));
    assert!(matches!(bad.op_lock(), Err(StateError::Invalid { .. })));
    bad.log_error("x");
    assert!(!p.workspaces_dir().join("../x").exists());
}

fn hold(path: PathBuf) -> fs::File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}

#[test] // W§7.3 wait bound: the caller's bound, then Busy; the closure never runs; readers need no lock
#[ignore = "timing: asserts the 100 ms lock wait ends in under 2 s"]
fn update_waits_for_the_lock_up_to_the_bound() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    store.update(S5, &mut identity).unwrap();
    let before = raw(&store);
    let held = hold(store.dir().join("lock"));
    let mut calls = 0;
    let start = Instant::now();
    let r = store.update(Duration::from_millis(100), &mut |_| { calls += 1; Ok(()) });
    let waited = start.elapsed();
    assert!(matches!(r, Err(StateError::Busy)));
    assert!(waited >= Duration::from_millis(100) && waited < Duration::from_secs(2), "{waited:?}");
    assert_eq!(calls, 0);
    assert_eq!(raw(&store), before);
    assert_eq!(store.phase_unlocked(), Some(Phase::Building)); // AU83: the phase read takes no lock
    assert!(store.read().unwrap().is_some());
    drop(held);
    store.update(Duration::ZERO, &mut |_| Ok(())).unwrap(); // a free lock needs no wait
}

#[test] // W§7.3, graph review focus 5: two kinds of writers at once; both changes are kept
fn concurrent_writers_keep_both_changes() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    Store::open(&p, ID).update(S5, &mut identity).unwrap();
    let start = std::sync::Barrier::new(2);
    std::thread::scope(|s| {
        s.spawn(|| { // the snapshot kind: windows[]
            let store = Store::open(&p, ID);
            start.wait();
            for i in 0..25u32 {
                store.update(Duration::MAX, &mut |ws| { ws.windows.push(Window { index: i, ..Default::default() }); Ok(()) }).unwrap();
            }
        });
        s.spawn(|| { // the member kind: members{}
            let store = Store::open(&p, ID);
            start.wait();
            for i in 0..25u8 {
                let key = format!("kaaaaaaa{}", (b'a' + i) as char);
                store.update(Duration::MAX, &mut |ws| { ws.members.insert(key.clone(), MemberRecord::new(Agent::ClaudeCode)); Ok(()) }).unwrap();
            }
        });
    });
    let ws = Store::open(&p, ID).read().unwrap().unwrap();
    assert_eq!(ws.windows.iter().map(|w| w.index).collect::<Vec<_>>(), (0..25).collect::<Vec<_>>());
    assert_eq!(ws.members.len(), 25);
}

#[test] // W§18 #7
fn a_killed_write_leaves_the_state_whole_and_the_next_write_removes_old_tmp_files() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let whole = store.update(S5, &mut identity).unwrap();
    let before = raw(&store);
    let aged = |name: &str, secs: u64| {
        let path = store.dir().join(name);
        fs::write(&path, b"{\"version\": 1, \"id\": \"01").unwrap(); // half of a write
        fs::File::options().write(true).open(&path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(secs)).unwrap();
        path
    };
    let old = aged(".tmpAbC123", 61);
    let young = aged(".tmpXyZ789", 30);
    let other = aged("down.log", 3600);
    assert_eq!(raw(&store), before); // the old workspace.json is unchanged
    assert_eq!(store.read().unwrap().unwrap(), whole); // and readable
    assert!(old.exists()); // a read deletes nothing
    store.update(S5, &mut |ws| { ws.phase = Phase::Up; Ok(()) }).unwrap();
    assert!(!old.exists()); // older than 1 min: gone after the next write
    assert!(young.exists()); // it can be a live writer's file
    assert!(other.exists()); // only `.tmp*`
    assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
}

#[test] // AU83: 200 ms of retries before "held"
#[ignore = "timing: asserts the 200 ms retry ends in under 2 s"]
fn op_lock_reports_held_after_200_ms() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    assert!(store.op_lock_free()); // no file yet
    assert!(!store.dir().exists()); // and the check made none
    let held = Store::open(&p, ID).op_lock().unwrap();
    assert!(!store.op_lock_free());
    let start = Instant::now();
    let r = store.op_lock();
    let waited = start.elapsed();
    assert!(matches!(r, Err(StateError::OpHeld)));
    assert_eq!(StateError::OpHeld.to_string(), "up or down is in progress");
    assert!(waited >= Duration::from_millis(200) && waited < Duration::from_secs(2), "{waited:?}");
    assert!(!store.op_lock_free()); // the check took nothing from the holder
    drop(held);
    assert!(store.op_lock_free());
    assert!(store.op_lock_free()); // the check released its own short hold
    let mine = store.op_lock().unwrap();
    assert!(!Store::open(&p, ID).op_lock_free());
    drop(mine);
    assert!(Store::open(&p, "nothex").op_lock_free()); // design call a
}

#[test] // AU83: a short hold by a hook does not make `up` or `down` fail
#[ignore = "timing: a holder sleeps 20 ms against the 200 ms op.lock retry"]
fn op_lock_gets_a_lock_released_inside_the_retry_time() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let other = Store::open(&p, ID);
    let holder = std::thread::spawn(move || {
        let lock = other.op_lock().unwrap();
        held_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(lock);
    });
    held_rx.recv().unwrap();
    assert!(!store.op_lock_free());
    go_tx.send(()).unwrap();
    assert!(store.op_lock().is_ok());
    holder.join().unwrap();
}

#[test] // W§7.1: one line per call, at most 64 KiB, the newest lines stay
fn errors_log_has_one_line_per_call_and_at_most_64_kib() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let log = store.dir().join("errors.log");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    store.log_error("snapshot: tmux\nfailed\r");
    let text = fs::read_to_string(&log).unwrap();
    let (secs, rest) = text.split_once(' ').unwrap();
    assert!(secs.parse::<u64>().unwrap() >= now);
    assert_eq!(rest, "snapshot: tmux failed \n");
    store.log_error(&"é".repeat(2000)); // 4000 bytes: cut to 1024 at a char boundary
    let text = fs::read_to_string(&log).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert_eq!(text.lines().nth(1).unwrap().split_once(' ').unwrap().1, "é".repeat(512));
    for i in 0..1500 {
        store.log_error(&format!("line {i:04} {}", "x".repeat(90)));
        assert!(fs::metadata(&log).unwrap().len() <= 64 * 1024, "after line {i}");
    }
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.ends_with(&format!(" line 1499 {}\n", "x".repeat(90))));
    assert!(text.lines().count() > 200 && text.len() > 30 * 1024); // the limit cuts old lines; it does not empty the file
    for l in text.lines() { // no line is cut in two
        let (secs, rest) = l.split_once(' ').unwrap();
        assert!(secs.parse::<u64>().is_ok() && rest.starts_with("line ") && rest.ends_with(&"x".repeat(90)), "{l:?}");
    }
}

#[cfg(unix)]
#[test] // AU87 (2): `zygo_session` and `pending.zygo_session`
fn live_launcher_reads_committed_and_pending_sessions() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    let session = |name: &str, token: bool, locked: bool| -> Option<fs::File> {
        let dir = p.sessions_dir.join(name);
        fs::create_dir_all(&dir).unwrap();
        if token { fs::write(dir.join("token"), "t").unwrap(); }
        let pid = hold(dir.join("pid"));
        if locked { Some(pid) } else { None } // dropping the file ends the lock
    };
    let _live = session("01LIVE", true, true);
    session("01DEAD", true, false);
    let _no_token = session("01NOTOKEN", false, true);
    fs::write(t.path().join("cache/token"), "t").unwrap(); // what `sessions/..` would name
    let _outside = hold(t.path().join("cache/pid"));
    let member = |committed: Option<&str>, pending: Option<&str>| {
        let mut m = MemberRecord::new(Agent::ClaudeCode);
        m.zygo_session = committed.map(String::from);
        m.pending = pending.map(|s| Pending { zygo_session: s.into(), ..Default::default() });
        m
    };
    let ws = |members: Vec<(&str, MemberRecord)>| {
        let mut ws = Workspace::new(ID);
        ws.members = members.into_iter().map(|(k, m)| (k.to_string(), m)).collect();
        ws
    };
    assert_eq!(store.live_launcher(&ws(vec![])), None);
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(None, None))])), None);
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(Some("01DEAD"), None))])), None);
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(Some("01NOTOKEN"), None))])), None);
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(Some("01MISSING"), Some("01MISSING2")))])), None);
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(Some("01LIVE"), None))])), Some("kaaaaaaaa".into()));
    // only the start that is not committed yet is live
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(Some("01DEAD"), Some("01LIVE")))])), Some("kaaaaaaaa".into()));
    assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(None, Some("01LIVE")))])), Some("kaaaaaaaa".into()));
    // the first live member in key order
    assert_eq!(store.live_launcher(&ws(vec![("kccccccca", member(Some("01LIVE"), None)), ("kaaaaaaaa", member(Some("01DEAD"), None)),
        ("kbbbbbbbb", member(None, Some("01LIVE")))])), Some("kbbbbbbbb".into()));
    // hostile ids name no path
    for bad in ["..", "../sessions/01LIVE", "", "01LIVE/", "/"] {
        assert_eq!(store.live_launcher(&ws(vec![("kaaaaaaaa", member(Some(bad), Some(bad)))])), None, "{bad:?}");
    }
}

#[test] // each workspace directory with its read result; a bad state does not hide the others
fn list_all_gives_each_workspace_with_its_read_result() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    assert!(list_all(&p).is_empty()); // no state directory
    let id = |c: char| c.to_string().repeat(16);
    let mk = |i: &str, change: &dyn Fn(&mut Value)| {
        let mut v = doc();
        v["id"] = json!(i);
        change(&mut v);
        put(&Store::open(&p, i), &v);
    };
    mk(&id('c'), &|_| {});
    mk(&id('a'), &|v| v["version"] = json!(2));
    mk(&id('b'), &|v| v["name"] = json!("a b"));
    Store::open(&p, &id('d')).op_lock().unwrap(); // a directory with no workspace.json
    fs::create_dir_all(p.workspaces_dir().join("notes")).unwrap(); // not a workspace id
    fs::write(p.workspaces_dir().join("notes/workspace.json"), "{}").unwrap();
    fs::write(p.workspaces_dir().join(id('e')), "a file").unwrap(); // not a directory
    let all = list_all(&p);
    assert_eq!(all.iter().map(|(i, _)| i.clone()).collect::<Vec<_>>(), vec![id('a'), id('b'), id('c')]);
    assert!(matches!(all[0].1, Err(StateError::Newer { .. })));
    assert!(matches!(all[1].1, Err(StateError::Invalid { .. })));
    assert_eq!(all[2].1.as_ref().unwrap().name, "app");
}

#[test] // W7 Round 3 R3-2: `build_plan` and `fresh_members`; absent when empty, so an older file reads; AU84 on the keys
fn build_plan_and_fresh_members() {
    let v = serde_json::to_value(full()).unwrap();
    assert!(v.get("build_plan").is_none() && v.get("fresh_members").is_none(), "{v}");
    let mut ws = full();
    ws.build_plan = Some(BuildPlan::Loadout);
    ws.fresh_members = vec![KEY.into()];
    let v = serde_json::to_value(&ws).unwrap();
    assert_eq!((&v["build_plan"], &v["fresh_members"]), (&json!("loadout"), &json!([KEY])));
    assert_eq!(serde_json::from_value::<Workspace>(v).unwrap(), ws);
    assert_eq!(serde_json::to_value(BuildPlan::Restore).unwrap(), json!("restore"));
    let old: Workspace = serde_json::from_value(doc()).unwrap();
    assert_eq!((old.build_plan, old.fresh_members.len()), (None, 0));
    let new = Workspace::new(ID);
    assert_eq!((new.build_plan, new.fresh_members.len()), (None, 0));
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut v = doc();
    v["fresh_members"] = json!(["../x"]);
    put(&store, &v);
    assert!(matches!(store.read(), Err(StateError::Invalid { reason, .. }) if reason == "invalid pane key"));
}

/// 1 MiB, the largest `workspace.json` that reads (AU84, FIX-17).
const MIB: usize = 1024 * 1024;
/// A valid document padded with spaces to exactly `len` bytes.
fn padded(len: usize) -> Vec<u8> {
    let mut b = serde_json::to_vec(&doc()).unwrap();
    assert!(b.len() <= len);
    b.resize(len, b' ');
    b
}
/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

#[cfg(unix)]
#[test] // FIX-17 (a): a linked state folder is refused before any create, chmod, lock or write
fn linked_state_folder_is_refused_and_not_changed() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let target = t.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let body = serde_json::to_vec(&doc()).unwrap();
    fs::write(target.join("workspace.json"), &body).unwrap();
    fs::create_dir_all(p.workspaces_dir()).unwrap();
    std::os::unix::fs::symlink(&target, p.workspaces_dir().join(ID)).unwrap();
    let store = Store::open(&p, ID);
    let link_err = |e: &StateError| {
        matches!(e, StateError::NotADirectory { path, link: true } if path == store.dir())
            && e.to_string() == format!("{}: state folder is a link", store.dir().display())
    };
    let e = store.op_lock().unwrap_err();
    assert!(link_err(&e), "op_lock: {e}");
    let mut ran = false;
    let e = store.update(S5, &mut |_| { ran = true; Ok(()) }).unwrap_err();
    assert!(link_err(&e) && !ran, "update: {e}");
    let e = store.read().unwrap_err();
    assert!(link_err(&e), "read: {e}");
    store.log_error("must not be written");
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
    assert_eq!(names(&target), vec!["workspace.json".to_string()]);
    assert_eq!(fs::read(target.join("workspace.json")).unwrap(), body);
}

#[test] // FIX-17 (a): a state folder that is a file is refused; the file is not changed
fn state_folder_that_is_a_file_is_refused() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    fs::create_dir_all(p.workspaces_dir()).unwrap();
    let file = p.workspaces_dir().join(ID);
    fs::write(&file, "a file").unwrap();
    let store = Store::open(&p, ID);
    let file_err = |e: &StateError| {
        matches!(e, StateError::NotADirectory { path, link: false } if path == store.dir())
            && e.to_string() == format!("{}: state folder is not a folder", store.dir().display())
    };
    let e = store.op_lock().unwrap_err();
    assert!(file_err(&e), "op_lock: {e}");
    let e = store.update(S5, &mut identity).unwrap_err();
    assert!(file_err(&e), "update: {e}");
    let e = store.read().unwrap_err();
    assert!(file_err(&e), "read: {e}");
    store.log_error("must not be written");
    assert_eq!(fs::read(&file).unwrap(), b"a file");
    assert_eq!(names(&p.workspaces_dir()), vec![ID.to_string()]);
}

#[test] // FIX-17 (b), AU84, AU29 (Round 2 F1): a `workspace.json` larger than 1 MiB is `TooLarge` for read, list_all, update
fn state_file_over_1_mib_is_too_large() {
    for len in [MIB + 1, MIB + 2, 2 * MIB] {
        let t = TempDir::new().unwrap();
        let p = paths(t.path());
        let store = Store::open(&p, ID);
        fs::create_dir_all(store.dir()).unwrap();
        let body = padded(len);
        fs::write(store.dir().join("workspace.json"), &body).unwrap();
        let file = store.dir().join("workspace.json");
        let text = format!("{}: state file too large; it may be from a newer zygo, so zygo does not change it", file.display());
        let too_large = |r: &Result<Workspace, StateError>| {
            matches!(r, Err(e @ StateError::TooLarge { path }) if path == &file && e.to_string() == text)
        };
        let r = store.read().map(|w| w.unwrap());
        assert!(too_large(&r), "{len}: read {r:?}");
        let all = list_all(&p);
        assert_eq!(all.len(), 1, "{len}");
        assert!(too_large(&all[0].1), "{len}: list_all {:?}", all[0].1);
        let r = store.update(S5, &mut identity);
        assert!(too_large(&r), "{len}: update {r:?}");
        assert!(raw(&store) == body, "{len}: update changed the file");
    }
}

#[test] // FIX-17 (b): a `workspace.json` of exactly 1 MiB reads and updates
fn state_file_of_1_mib_reads() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    fs::create_dir_all(store.dir()).unwrap();
    fs::write(store.dir().join("workspace.json"), padded(MIB)).unwrap();
    assert_eq!(store.read().unwrap().unwrap().name, "app");
    let ws = store.update(S5, &mut |w| { w.loadout = "pair".into(); Ok(()) }).unwrap();
    assert_eq!(ws.loadout, "pair");
    assert_eq!(store.read().unwrap().unwrap().loadout, "pair");
}

/// Runs `f` in a thread and waits for it at most 10 s: a call that blocks fails the test (FIX-17 Round 2 F4).
fn bounded<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("{what} blocked"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("{what} panicked (see the thread's message above)"),
    }
}

#[cfg(unix)]
fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

#[cfg(unix)]
#[test] // FIX-17 Round 2 F3, F4: a link at `lock`, `op.lock`, `errors.log` or `workspace.json` is never followed
fn links_inside_the_state_folder_are_not_followed() {
    use std::os::unix::fs::symlink;
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    store.update(S5, &mut identity).unwrap();
    let dir = store.dir().to_path_buf();
    let target = t.path().join("target");
    fs::create_dir(&target).unwrap();
    let is_link = |e: &StateError, name: &str| e.to_string() == format!("{}: is a link; it is not followed", dir.join(name).display());
    // `lock` and `op.lock`: links to files that do not exist; nothing is created at the target.
    fs::remove_file(dir.join("lock")).unwrap();
    symlink(target.join("lock"), dir.join("lock")).unwrap();
    symlink(target.join("op.lock"), dir.join("op.lock")).unwrap();
    let before = raw(&store);
    let s = store.clone();
    let e = bounded("update", move || s.update(S5, &mut identity).unwrap_err());
    assert!(is_link(&e, "lock"), "update: {e}");
    let s = store.clone();
    let e = bounded("op_lock", move || s.op_lock().unwrap_err());
    assert!(is_link(&e, "op.lock"), "op_lock: {e}");
    let s = store.clone();
    assert!(bounded("op_lock_free", move || s.op_lock_free()));
    assert_eq!(raw(&store), before);
    // `errors.log`: a link to a file with content, below and above the log cap; the target keeps its bytes.
    fs::write(target.join("log"), "keep").unwrap();
    symlink(target.join("log"), dir.join("errors.log")).unwrap();
    let s = store.clone();
    bounded("log_error", move || s.log_error("must not be written"));
    assert_eq!(fs::read(target.join("log")).unwrap(), b"keep");
    let full = vec![b'x'; ERRORS_LOG_MAX as usize];
    fs::write(target.join("log"), &full).unwrap();
    let s = store.clone();
    bounded("log_error", move || s.log_error("must not be written"));
    assert!(fs::symlink_metadata(dir.join("errors.log")).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(target.join("log")).unwrap(), full);
    // `workspace.json`: a link to a valid state is not read.
    fs::write(target.join("ws.json"), &before).unwrap();
    fs::remove_file(dir.join("workspace.json")).unwrap();
    symlink(target.join("ws.json"), dir.join("workspace.json")).unwrap();
    let s = store.clone();
    let r = bounded("read", move || s.read());
    assert!(matches!(&r, Err(StateError::Invalid { reason, .. }) if reason == "state file is a link"), "{r:?}");
    assert_eq!(names(&target), vec!["log".to_string(), "ws.json".to_string()]);
    assert_eq!(fs::read(target.join("ws.json")).unwrap(), before);
}

#[cfg(unix)]
#[test] // FIX-17 Round 2 F4: a FIFO at `workspace.json`, `lock`, `op.lock` or `errors.log` is refused or ignored, never waited on
fn a_fifo_in_the_state_folder_never_blocks() {
    use std::os::unix::fs::FileTypeExt;
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    store.update(S5, &mut identity).unwrap();
    let dir = store.dir().to_path_buf();
    let not_regular = |e: &StateError, name: &str| e.to_string() == format!("{}: is not a regular file", dir.join(name).display());
    // `workspace.json`
    fs::remove_file(dir.join("workspace.json")).unwrap();
    mkfifo(&dir.join("workspace.json"));
    let s = store.clone();
    let r = bounded("read", move || s.read());
    assert!(matches!(&r, Err(StateError::Invalid { reason, .. }) if reason == "state file is not a regular file"), "{r:?}");
    let s = store.clone();
    let r = bounded("update", move || s.update(S5, &mut identity));
    assert!(matches!(&r, Err(StateError::Invalid { reason, .. }) if reason == "state file is not a regular file"), "{r:?}");
    assert!(fs::symlink_metadata(dir.join("workspace.json")).unwrap().file_type().is_fifo());
    // `lock`
    fs::remove_file(dir.join("lock")).unwrap();
    mkfifo(&dir.join("lock"));
    let s = store.clone();
    let e = bounded("update", move || s.update(S5, &mut identity).unwrap_err());
    assert!(not_regular(&e, "lock"), "update: {e}");
    // `op.lock`
    mkfifo(&dir.join("op.lock"));
    let s = store.clone();
    let e = bounded("op_lock", move || s.op_lock().unwrap_err());
    assert!(not_regular(&e, "op.lock"), "op_lock: {e}");
    let s = store.clone();
    assert!(bounded("op_lock_free", move || s.op_lock_free()));
    // `errors.log`
    mkfifo(&dir.join("errors.log"));
    let s = store.clone();
    bounded("log_error", move || s.log_error("must not be written"));
    assert!(fs::symlink_metadata(dir.join("errors.log")).unwrap().file_type().is_fifo());
}

#[cfg(unix)]
#[test] // FX-28 (re-plan R3-F4): a folder at `lock`, `op.lock` or `mail/lock` is refused with a line that names the path,
        // as a FIFO is; the open for write fails with EISDIR before the handle can be checked
fn a_folder_at_a_lock_file_is_refused_with_its_path() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let store = Store::open(&p, ID);
    store.update(S5, &mut identity).unwrap();
    let dir = store.dir().to_path_buf();
    let not_regular = |name: &str| format!("{}: is not a regular file", dir.join(name).display());
    // `lock` (`Store::update`)
    fs::remove_file(dir.join("lock")).unwrap();
    fs::create_dir(dir.join("lock")).unwrap();
    assert_eq!(store.update(S5, &mut identity).unwrap_err().to_string(), not_regular("lock"));
    // `op.lock` (`Store::op_lock`); `op_lock_free` gives `true` with no wait, as for a FIFO
    assert!(fs::symlink_metadata(dir.join("op.lock")).is_err());
    fs::create_dir(dir.join("op.lock")).unwrap();
    assert_eq!(store.op_lock().unwrap_err().to_string(), not_regular("op.lock"));
    assert!(store.op_lock_free());
    // `mail/lock` (`mail::Delivery::begin`, through `lock_file_as`)
    fs::create_dir_all(dir.join("mail/lock")).unwrap();
    let e = zygo_workspace::mail::Delivery::begin(&store, Duration::ZERO).unwrap_err();
    assert_eq!(e.to_string(), format!("zygo: mail: {}", not_regular("mail/lock")));
    for name in ["lock", "op.lock", "mail/lock"] {
        assert!(dir.join(name).is_dir(), "{name}");
    }
}

/// M4: `update_existing` makes no folder and no state when none exists. With no state folder it creates nothing,
/// not even the folder above it. With a state folder and no `workspace.json` (the case that `update` would fill with
/// `Workspace::new`), it gives `Ok(None)`, runs no closure and writes no `workspace.json`; the only file it leaves is
/// the `lock` file that taking the lock creates (M4 review F2b, F4).
#[test]
fn update_existing_makes_nothing_when_the_state_is_gone() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    let mut calls = 0;
    assert!(matches!(store.update_existing(S5, &mut |_| { calls += 1; Ok(()) }), Ok(None)));
    assert_eq!(calls, 0);
    assert!(!store.dir().exists());
    assert!(!store.dir().parent().unwrap().exists());
    fs::create_dir_all(store.dir()).unwrap();
    assert!(matches!(store.update_existing(S5, &mut |_| { calls += 1; Ok(()) }), Ok(None)));
    assert_eq!(calls, 0);
    assert_eq!(names(store.dir()), ["lock"]);
}

/// M4: on an existing state it is `update`; a closure error writes nothing.
#[test]
fn update_existing_changes_an_existing_state() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    store.update(S5, &mut identity).unwrap();
    let before = raw(&store);
    assert!(matches!(store.update_existing(S5, &mut |_| anyhow::bail!("no record")), Err(StateError::Refused(_))));
    assert_eq!(raw(&store), before);
    let got = store.update_existing(S5, &mut |w| { w.loadout = "other".into(); Ok(()) }).unwrap().unwrap();
    assert_eq!(got.loadout, "other");
    assert_eq!(store.read().unwrap().unwrap().loadout, "other");
}

/// M4, `ws forget` racing the mirror: the folder goes away after the read, inside the closure; the write then fails
/// and no folder is made. It does not tell `update_existing` from `update`, which also makes no folder after its
/// read; `update_existing_makes_nothing_when_the_state_is_gone` does.
#[test]
fn update_existing_does_not_make_a_forgotten_folder_again() {
    let t = TempDir::new().unwrap();
    let store = Store::open(&paths(t.path()), ID);
    store.update(S5, &mut identity).unwrap();
    let dir = store.dir().to_path_buf();
    let r = store.update_existing(S5, &mut |w| {
        fs::remove_dir_all(&dir)?;
        w.loadout = "other".into();
        Ok(())
    });
    assert!(r.is_err(), "{r:?}");
    assert!(!store.dir().exists());
}

/// M4: the session id rule of `live_launcher`.
#[test]
fn valid_session_ids() {
    for ok in ["S1", "01JTESTSESSIONAAAAAAAAAAAA"] {
        assert!(valid_session_id(ok), "{ok}");
    }
    for bad in ["", "..", "../x", "/tmp/x", "a/b", "a.b", "a b", "é"] {
        assert!(!valid_session_id(bad), "{bad}");
    }
}

#[test] // FIX-24 D1, W§7.2 member.launch.events_off: absent when false, an old file reads false, never in `extra`
fn launch_events_off_defaults_to_false_and_is_left_out_when_false() {
    let on: Launch = serde_json::from_value(json!({"kind": "opencode", "events_off": true})).unwrap();
    assert!(on.events_off);
    assert!(on.extra.is_empty(), "the key must not land in extra: {:?}", on.extra);
    let old: Launch = serde_json::from_value(json!({"kind": "opencode"})).unwrap();
    assert!(!old.events_off, "an old file reads false");
    assert!(serde_json::to_value(Launch::new(Agent::OpenCode)).unwrap().get("events_off").is_none());
    let mut l = Launch::new(Agent::OpenCode);
    l.events_off = true;
    assert_eq!(serde_json::to_value(&l).unwrap()["events_off"], json!(true));
    let t = TempDir::new().unwrap();
    let paths = paths(t.path());
    let store = Store::open(&paths, ID);
    store.update(S5, &mut identity).unwrap();
    let mut m = MemberRecord::new(Agent::OpenCode);
    m.launch.events_off = true;
    store.update(S5, &mut |ws| { ws.members.insert(KEY.into(), m.clone()); Ok(()) }).unwrap();
    assert!(store.read().unwrap().unwrap().members[KEY].launch.events_off);
}
