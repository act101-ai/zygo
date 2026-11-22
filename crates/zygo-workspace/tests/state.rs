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
