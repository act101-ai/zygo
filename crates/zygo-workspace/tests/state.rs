use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::Agent;
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
    m.last_run_at = Some(1_700_000_000);
    m.overrides = Some(json!({"results": false, "guards": {}}));
    m.compact = Some(CompactOverride::Set("120000".into()));
    m.interrupted = vec![Subagent { agent_id: "a1".into(), agent_type: "Explore".into(), ..Default::default() }];
    m.closed_units = vec!["W2".into()];
    m.dropped_flags = vec!["--agents".into()];
    m.clean_stop = true;
    m.note_pending = true;
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
            "config_dir": "/home/u/work", "program": "/usr/bin/claude", "last_run_at": 1700000000u64,
            "overrides": {"results": false, "guards": {}}, "compact": "set 120000",
            "interrupted": [{"agent_id": "a1", "agent_type": "Explore"}],
            "closed_units": ["W2"], "dropped_flags": ["--agents"], "clean_stop": true, "note_pending": true}}
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
    let mut v = serde_json::to_value(full()).unwrap();
    for at in ["", "/windows/0", "/panes/0", "/members/kabcdefgh", "/members/kabcdefgh/pending", "/members/kabcdefgh/launch",
        "/members/kabcdefgh/interrupted/0"] {
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
    let hostile: [(&str, fn(&mut Workspace)); 6] = [
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
                store.update(S5, &mut |ws| { ws.windows.push(Window { index: i, ..Default::default() }); Ok(()) }).unwrap();
            }
        });
        s.spawn(|| { // the member kind: members{}
            let store = Store::open(&p, ID);
            start.wait();
            for i in 0..25u8 {
                let key = format!("kaaaaaaa{}", (b'a' + i) as char);
                store.update(S5, &mut |ws| { ws.members.insert(key.clone(), MemberRecord::new(Agent::ClaudeCode)); Ok(()) }).unwrap();
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
