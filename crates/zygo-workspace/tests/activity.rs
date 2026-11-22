//! `activity.json` under `activity.lock` (W§9.1, AU26 rule 2, AU43, AU56, W§18 #57, #85; PW7 7.5).
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::session_dir::SessionDir;
use zygo_workspace::activity::{self, Activity, LOCK_BOUND};

const S10: Duration = Duration::from_secs(10);

fn dir() -> (TempDir, SessionDir) {
    let t = TempDir::new().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    (t, d)
}

fn file(d: &SessionDir) -> Value {
    serde_json::from_slice(&fs::read(d.activity_json()).unwrap()).unwrap()
}

fn fixture(rel: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code").join(rel);
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn empty() -> Activity {
    Activity { version: 1, busy: false, running: BTreeMap::new(), updated_at: 0 }
}

#[test]
fn read_missing_file_is_default() {
    let (_t, d) = dir();
    assert_eq!(activity::read(&d).unwrap(), empty());
    assert_eq!(Activity::default(), empty());
}

#[test]
fn read_unparsable_file_is_an_error_naming_the_file() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), b"{not json").unwrap();
    let e = activity::read(&d).unwrap_err();
    assert!(format!("{e:#}").starts_with(&format!("{}: ", d.activity_json().display())), "{e:#}");
}

#[cfg(unix)]
#[test]
fn read_unreadable_file_is_an_error_naming_the_file() {
    let (_t, d) = dir();
    fs::create_dir(d.activity_json()).unwrap();
    let e = activity::read(&d).unwrap_err();
    assert!(format!("{e:#}").starts_with(&format!("{}: ", d.activity_json().display())), "{e:#}");
}

#[test]
fn read_takes_known_fields_and_drops_unknown_ones() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), br#"{"version":1,"busy":true,"running":{"agent-id-1":"pw-sub"},"updated_at":7,"x":1}"#)
        .unwrap();
    assert_eq!(
        activity::read(&d).unwrap(),
        Activity {
            version: 1,
            busy: true,
            running: BTreeMap::from([("agent-id-1".into(), "pw-sub".into())]),
            updated_at: 7
        }
    );
}

#[test]
fn read_of_a_newer_version_reads_its_known_fields() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), br#"{"version":2,"busy":true,"running":{},"updated_at":5,"phase":"x"}"#).unwrap();
    assert_eq!(
        activity::read(&d).unwrap(),
        Activity { version: 2, busy: true, running: BTreeMap::new(), updated_at: 5 }
    );
}

#[test]
fn lock_bound_is_one_second() {
    assert_eq!(LOCK_BOUND, Duration::from_secs(1));
}

#[test]
fn update_writes_the_version_1_schema() {
    let (_t, d) = dir();
    let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    activity::update(&d, S10, &mut |a| {
        a.busy = true;
        a.subagent_start("agent-id-1", "pw-sub");
    })
    .unwrap();
    let v = file(&d);
    let at = v["updated_at"].as_u64().unwrap();
    assert!(at >= before, "{v}");
    assert_eq!(v, json!({"version": 1, "busy": true, "running": {"agent-id-1": "pw-sub"}, "updated_at": at}));
}

#[test]
fn update_keeps_earlier_changes() {
    let (_t, d) = dir();
    activity::update(&d, S10, &mut |a| a.busy = true).unwrap();
    activity::update(&d, S10, &mut |a| a.subagent_start("a1", "t1")).unwrap();
    let a = activity::read(&d).unwrap();
    assert!(a.busy);
    assert_eq!(a.running, BTreeMap::from([("a1".into(), "t1".into())]));
}

#[test]
fn update_replaces_an_unparsable_file_with_the_default_and_the_change() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), b"garbage").unwrap();
    activity::update(&d, S10, &mut |a| a.subagent_start("a1", "t1")).unwrap();
    let a = activity::read(&d).unwrap();
    assert_eq!((a.version, a.busy, a.running), (1, false, BTreeMap::from([("a1".into(), "t1".into())])));
}

#[cfg(unix)]
#[test]
fn update_returns_a_read_error_and_writes_nothing() {
    let (_t, d) = dir();
    fs::create_dir(d.activity_json()).unwrap();
    let e = activity::update(&d, S10, &mut |a| a.busy = true).unwrap_err();
    assert!(format!("{e:#}").starts_with(&format!("{}: ", d.activity_json().display())), "{e:#}");
    assert!(d.activity_json().is_dir());
}

#[test]
fn update_refuses_a_newer_version_and_writes_nothing() {
    let (_t, d) = dir();
    let newer = br#"{"version":2,"busy":false,"running":{},"updated_at":5}"#;
    fs::write(d.activity_json(), newer).unwrap();
    let e = activity::update(&d, S10, &mut |a| a.busy = true).unwrap_err();
    assert_eq!(e.to_string(), format!("{}: written by a newer zygo (version 2)", d.activity_json().display()));
    assert_eq!(fs::read(d.activity_json()).unwrap(), newer.to_vec());
}

#[test]
fn update_gives_up_after_its_bound_when_the_lock_is_held() {
    let (_t, d) = dir();
    let held =
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(d.activity_lock()).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    let start = Instant::now();
    let e = activity::update(&d, Duration::from_millis(50), &mut |a| a.busy = true).unwrap_err();
    assert!(start.elapsed() >= Duration::from_millis(50));
    assert_eq!(e.to_string(), format!("{}: lock not taken within 50 ms", d.activity_lock().display()));
    assert!(!d.activity_json().exists());
}

#[test]
fn update_waits_for_a_lock_released_within_its_bound() {
    let (_t, d) = dir();
    let (tx, rx) = std::sync::mpsc::channel();
    let lock = d.activity_lock();
    let holder = std::thread::spawn(move || {
        let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock).unwrap();
        fs4::FileExt::try_lock(&f).unwrap();
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
    });
    rx.recv().unwrap();
    let start = Instant::now();
    activity::update(&d, LOCK_BOUND, &mut |a| a.busy = true).unwrap();
    assert!(start.elapsed() >= Duration::from_millis(50));
    assert!(activity::read(&d).unwrap().busy);
    holder.join().unwrap();
}

#[test]
fn update_fails_when_the_session_dir_is_missing_and_names_the_lock_file() {
    let (t, _) = dir();
    let d = SessionDir::new(t.path().join("gone"));
    let e = activity::update(&d, S10, &mut |a| a.busy = true).unwrap_err();
    assert!(format!("{e:#}").starts_with(&format!("{}: open lock file", d.activity_lock().display())), "{e:#}");
    assert!(!d.root.exists());
}

#[test]
fn update_removes_stale_temp_files_only() {
    let (_t, d) = dir();
    let (old, young) = (d.root.join(".tmpOLD"), d.root.join(".tmpYOUNG"));
    fs::write(&old, b"").unwrap();
    fs::write(&young, b"").unwrap();
    fs::File::options().write(true).open(&old).unwrap().set_modified(SystemTime::now() - Duration::from_secs(120)).unwrap();
    activity::update(&d, S10, &mut |a| a.busy = true).unwrap();
    assert!(!old.exists() && young.exists());
}

#[test]
fn start_then_stop_of_the_fixture_ids_empties_the_running_set() {
    let s = fixture("workspace/SubagentStart-last-dispatched.json");
    let e = fixture("workspace/SubagentStop-last-dispatched.json");
    let mut a = Activity::default();
    a.subagent_start(s["agent_id"].as_str().unwrap(), s["agent_type"].as_str().unwrap());
    assert_eq!(a.running, BTreeMap::from([("agent-id-1".into(), "pw-sub".into())]));
    assert!(a.subagent_stop(e["agent_id"].as_str().unwrap()));
    assert!(a.running.is_empty());
}

#[test]
fn internal_stop_with_no_start_returns_false_and_changes_nothing() {
    let e = fixture("hooks/SubagentStop-internal-sub.json");
    assert_eq!(e["agent_type"], json!(""));
    let mut a = Activity::default();
    assert!(!a.subagent_stop(e["agent_id"].as_str().unwrap()));
    assert_eq!(a, Activity::default());
}

#[test]
fn stop_with_no_start_leaves_another_running_id() {
    let mut a = Activity::default();
    a.subagent_start("agent-id-1", "pw-sub");
    let before = a.clone();
    assert!(!a.subagent_stop("a47a5756291c7f553"));
    assert_eq!(a, before);
}

#[test]
fn second_stop_of_one_id_returns_false() {
    let mut a = Activity::default();
    a.subagent_start("a1", "t1");
    assert!(a.subagent_stop("a1"));
    assert!(!a.subagent_stop("a1"));
    assert!(a.running.is_empty());
}

#[test]
fn repeat_start_keeps_one_entry_with_the_newest_type() {
    let mut a = Activity::default();
    a.subagent_start("a1", "t1");
    a.subagent_start("a1", "t2");
    assert_eq!(a.running, BTreeMap::from([("a1".into(), "t2".into())]));
}

#[test]
fn running_methods_leave_busy_unchanged() {
    let mut a = Activity { busy: true, ..Activity::default() };
    a.subagent_start("a1", "t1");
    a.subagent_stop("a1");
    a.subagent_stop("a2");
    assert!(a.busy);
}

// Round 2, B1: a newer file whose known fields have new types, and a version that is not an integer.
const P1: &[u8] = br#"{"version":2,"busy":false,"running":{"a1":{"type":"pw-sub","started":5}},"updated_at":5}"#;
const P2: &[u8] = br#"{"version":"2.0","busy":false,"running":{"a1":{"type":"pw-sub","started":5}},"updated_at":5}"#;

fn a1_pw_sub() -> BTreeMap<String, String> {
    BTreeMap::from([("a1".into(), "pw-sub".into())])
}

#[test]
fn update_refuses_a_newer_file_with_new_field_types_and_writes_nothing() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), P1).unwrap();
    let e = activity::update(&d, S10, &mut |a| a.busy = true).unwrap_err();
    assert_eq!(e.to_string(), format!("{}: written by a newer zygo (version 2)", d.activity_json().display()));
    assert_eq!(fs::read(d.activity_json()).unwrap(), P1.to_vec());
}

#[test]
fn update_refuses_a_version_that_is_not_an_integer_and_writes_nothing() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), P2).unwrap();
    let e = activity::update(&d, S10, &mut |a| a.busy = true).unwrap_err();
    assert_eq!(e.to_string(), format!("{}: written by a newer zygo (version \"2.0\")", d.activity_json().display()));
    assert_eq!(fs::read(d.activity_json()).unwrap(), P2.to_vec());
}

#[test]
fn read_of_a_newer_file_with_new_field_types_takes_its_known_fields() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), P1).unwrap();
    assert_eq!(activity::read(&d).unwrap(), Activity { version: 2, busy: false, running: a1_pw_sub(), updated_at: 5 });
}

#[test]
fn read_of_a_version_that_is_not_an_integer_takes_the_known_fields_as_newest() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), P2).unwrap();
    assert_eq!(
        activity::read(&d).unwrap(),
        Activity { version: u32::MAX, busy: false, running: a1_pw_sub(), updated_at: 5 }
    );
}

#[test]
fn read_of_a_newer_file_takes_fields_of_other_types_leniently() {
    let (_t, d) = dir();
    fs::write(
        d.activity_json(),
        br#"{"version":3,"busy":"yes","running":{"a":"t","b":{"type":7},"c":[1]},"updated_at":"x"}"#,
    )
    .unwrap();
    let running = BTreeMap::from([("a".into(), "t".into()), ("b".into(), String::new()), ("c".into(), String::new())]);
    assert_eq!(activity::read(&d).unwrap(), Activity { version: 3, busy: true, running, updated_at: 0 });
}

#[test]
fn update_rewrites_a_version_0_file_as_version_1() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), br#"{"version":0,"busy":true,"running":{"a1":"t1"},"updated_at":1}"#).unwrap();
    activity::update(&d, S10, &mut |a| a.subagent_start("a2", "t2")).unwrap();
    let v = file(&d);
    assert_eq!(v["version"], json!(1), "{v}");
    assert_eq!(v["busy"], json!(true), "{v}");
    assert_eq!(v["running"], json!({"a1": "t1", "a2": "t2"}), "{v}");
}

#[test]
fn read_of_a_file_that_is_not_an_object_is_an_error() {
    let (_t, d) = dir();
    for text in [&b"[]"[..], b"2", b"null", b"\"x\""] {
        fs::write(d.activity_json(), text).unwrap();
        let e = activity::read(&d).unwrap_err();
        assert!(format!("{e:#}").starts_with(&format!("{}: ", d.activity_json().display())), "{e:#}");
    }
}

// Round 2, M2: an empty agent id is never in the running set.
#[test]
fn empty_agent_id_is_ignored() {
    let mut a = Activity::default();
    a.subagent_start("", "pw-sub");
    assert_eq!(a, Activity::default());
    assert!(!a.subagent_stop(""));
    let mut b = Activity { running: BTreeMap::from([(String::new(), "t".into())]), ..Activity::default() };
    let before = b.clone();
    assert!(!b.subagent_stop(""));
    assert_eq!(b, before);
}

// Round 2, M3: the read stops after 1 MiB + 1 byte.
fn two_mib_file(d: &SessionDir) {
    let mut text = br#"{"version":1,"busy":false,"running":{"x":"t"},"updated_at":1}"#.to_vec();
    text.resize(2 << 20, b' ');
    fs::write(d.activity_json(), text).unwrap();
}

#[test]
fn read_of_a_file_over_1_mib_is_an_error() {
    let (_t, d) = dir();
    two_mib_file(&d);
    let e = format!("{:#}", activity::read(&d).unwrap_err());
    assert_eq!(e, format!("{}: activity.json larger than 1 MiB", d.activity_json().display()));
}

#[test]
fn update_replaces_a_file_over_1_mib() {
    let (_t, d) = dir();
    two_mib_file(&d);
    activity::update(&d, S10, &mut |a| a.subagent_start("a1", "t1")).unwrap();
    let a = activity::read(&d).unwrap();
    assert_eq!((a.version, a.busy, a.running), (1, false, BTreeMap::from([("a1".into(), "t1".into())])));
}

/// Child half of #57; the parent runs it with `--ignored` and sets both variables on the child only. With no
/// variables (`--include-ignored` in the parent) it returns at once.
#[test]
#[ignore]
fn writer_helper() {
    let (Some(root), Ok(prefix)) = (std::env::var_os("ZYGO_W9_WRITER_DIR"), std::env::var("ZYGO_W9_WRITER_PREFIX"))
    else {
        return;
    };
    let root = PathBuf::from(root);
    let until = Instant::now() + S10;
    while !root.join("start").exists() {
        assert!(Instant::now() < until, "no start file");
        std::thread::sleep(Duration::from_millis(1));
    }
    let d = SessionDir::new(root);
    let ok = (0..100)
        .filter(|i| activity::update(&d, LOCK_BOUND, &mut |a| a.subagent_start(&format!("{prefix}{i}"), "pw-sub")).is_ok())
        .count();
    println!("W9_OK {prefix} {ok}");
}

/// W§18 #57: two hook processes, 100 updates each; every update that returned `Ok` is in the running set.
#[cfg(unix)]
#[test]
fn two_hook_processes_writing_100_times_each_lose_no_ok_update() {
    let (_t, d) = dir();
    let spawn = |p: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["writer_helper", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env("ZYGO_W9_WRITER_DIR", &d.root)
            .env("ZYGO_W9_WRITER_PREFIX", p)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let (a, b) = (spawn("a"), spawn("b"));
    fs::write(d.root.join("start"), b"").unwrap();
    let count = |c: Child, p: &str| -> usize {
        let out = c.wait_with_output().unwrap();
        let s = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success() && s.contains("1 passed"), "{s}{}", String::from_utf8_lossy(&out.stderr));
        s.split(&format!("W9_OK {p} ")).nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap()
    };
    let total = count(a, "a") + count(b, "b");
    assert!(total > 0);
    assert_eq!(activity::read(&d).unwrap().running.len(), total);
}
