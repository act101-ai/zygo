//! `activity.json` under `activity.lock` (W§9.1, AU26 rule 2, AU43, AU56, W§18 #57, #85; PW7 7.5).
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::session_dir::SessionDir;
use zygo_workspace::activity::{self, Activity, LOCK_BOUND};

const S10: Duration = Duration::from_secs(10);
/// A bound that does not end during a test: load never decides a result.
const LONG: Duration = Duration::from_secs(3600);

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
    Activity { version: 1, busy: false, running: BTreeMap::new(), updated_at: 0, tools_running: 0 }
}

#[test]
fn read_missing_file_is_default() {
    let (_t, d) = dir();
    assert_eq!(activity::read(&d).unwrap(), empty());
    assert_eq!(Activity::default(), empty());
}

#[test] // W10 round 2 item 1: a file of an older zygo has no `tools_running`; it reads 0
fn tools_running_absent_reads_0_and_is_written_only_when_not_0() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), br#"{"version":1,"busy":true,"running":{},"updated_at":7}"#).unwrap();
    assert_eq!(activity::read(&d).unwrap().tools_running, 0);
    activity::update(&d, S10, &mut |a| a.tools_running = 2).unwrap();
    assert_eq!(file(&d)["tools_running"], json!(2));
    assert_eq!(activity::read(&d).unwrap().tools_running, 2);
    activity::update(&d, S10, &mut |a| a.tools_running = 0).unwrap();
    assert!(file(&d).get("tools_running").is_none(), "{}", file(&d));
    // A newer file: a number is taken; any other type reads 1, so the drain never counts the member as idle on a
    // field it cannot read.
    fs::write(d.activity_json(), br#"{"version":2,"busy":true,"running":{},"updated_at":7,"tools_running":3}"#).unwrap();
    assert_eq!(activity::read(&d).unwrap().tools_running, 3);
    fs::write(d.activity_json(), br#"{"version":2,"busy":true,"running":{},"updated_at":7,"tools_running":"x"}"#).unwrap();
    assert_eq!(activity::read(&d).unwrap().tools_running, 1);
    fs::write(d.activity_json(), br#"{"version":2,"busy":true,"running":{},"updated_at":7}"#).unwrap();
    assert_eq!(activity::read(&d).unwrap().tools_running, 0);
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
            updated_at: 7,
            tools_running: 0
        }
    );
}

#[test]
fn read_of_a_newer_version_reads_its_known_fields() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), br#"{"version":2,"busy":true,"running":{},"updated_at":5,"phase":"x"}"#).unwrap();
    assert_eq!(
        activity::read(&d).unwrap(),
        Activity { version: 2, busy: true, running: BTreeMap::new(), updated_at: 5, tools_running: 0 }
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

// H1 attempt-2 review round 1, F1: the drain counts an unparsable file as busy (AU26), so a write that started from
// the default would end an open turn. The file is kept and the update returns the read error.
#[test]
fn update_refuses_an_unparsable_file_and_writes_nothing() {
    let (_t, d) = dir();
    for text in [&b"garbage"[..], b"[]", br#"{"version":1,"busy":"yes"}"#] {
        fs::write(d.activity_json(), text).unwrap();
        let e = format!("{:#}", activity::update(&d, S10, &mut |a| a.subagent_start("a1", "t1")).unwrap_err());
        assert!(e.starts_with(&format!("{}: ", d.activity_json().display())), "{e}");
        assert_eq!(fs::read(d.activity_json()).unwrap(), text.to_vec());
    }
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
#[ignore = "timing: a holder thread sleeps 100 ms against the 1 s lock bound and elapsed time is asserted"]
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
    assert_eq!(activity::read(&d).unwrap(), Activity { version: 2, busy: false, running: a1_pw_sub(), updated_at: 5, tools_running: 0 });
}

#[test]
fn read_of_a_version_that_is_not_an_integer_takes_the_known_fields_as_newest() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), P2).unwrap();
    assert_eq!(
        activity::read(&d).unwrap(),
        Activity { version: u32::MAX, busy: false, running: a1_pw_sub(), updated_at: 5, tools_running: 0 }
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
    assert_eq!(activity::read(&d).unwrap(), Activity { version: 3, busy: true, running, updated_at: 0, tools_running: 0 });
}

#[test]
fn read_of_a_newer_file_whose_running_is_not_an_object_is_busy() {
    let (_t, d) = dir();
    fs::write(d.activity_json(), br#"{"version":2,"busy":false,"running":["a1"],"updated_at":5}"#).unwrap();
    assert_eq!(activity::read(&d).unwrap(), Activity { version: 2, busy: true, running: BTreeMap::new(), updated_at: 5, tools_running: 0 });
    fs::write(d.activity_json(), br#"{"version":2,"busy":false,"updated_at":5}"#).unwrap();
    assert_eq!(activity::read(&d).unwrap(), Activity { version: 2, busy: false, running: BTreeMap::new(), updated_at: 5, tools_running: 0 });
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

// H1 attempt-2 review round 1, F1: a file over the read limit is kept; starting from the default would end an open
// turn.
#[test]
fn update_refuses_a_file_over_1_mib_and_writes_nothing() {
    let (_t, d) = dir();
    two_mib_file(&d);
    let before = fs::read(d.activity_json()).unwrap();
    let e = format!("{:#}", activity::update(&d, S10, &mut |a| a.subagent_start("a1", "t1")).unwrap_err());
    assert_eq!(e, format!("{}: activity.json larger than 1 MiB", d.activity_json().display()));
    assert_eq!(fs::read(d.activity_json()).unwrap(), before);
}

// H1 attempt-2 review round 1, F1: a result over the read limit (MAX_FILE, 1 MiB) is refused, so no reader ever
// finds a file it cannot read.
#[test]
fn update_refuses_a_result_larger_than_the_read_limit() {
    let (_t, d) = dir();
    activity::update(&d, S10, &mut |a| {
        a.busy = true;
        a.tools_running = 1;
    })
    .unwrap();
    let before = fs::read(d.activity_json()).unwrap();
    let id = "x".repeat(2 << 20);
    let r = activity::update(&d, S10, &mut |a| a.subagent_start(&id, "t1"));
    let Err(e) = r else { panic!("an update over the read limit was written") };
    let e = format!("{e:#}");
    assert!(e.starts_with(&format!("{}: the new activity is ", d.activity_json().display())), "{}", &e[..200.min(e.len())]);
    assert!(e.ends_with(" bytes, more than the 1048576 bytes that every reader accepts; not written"), "{e}");
    assert_eq!(fs::read(d.activity_json()).unwrap(), before);
    let a = activity::read(&d).unwrap();
    assert!(a.busy && a.tools_running == 1 && a.running.is_empty());
    // A result of exactly the limit is still written; one byte more is not. `updated_at` has 10 digits until 2286.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let probe = Activity { busy: true, tools_running: 1, updated_at: now, running: BTreeMap::from([("a".into(), "t".into())]), ..Activity::default() };
    let n = (1 << 20) - serde_json::to_vec(&probe).unwrap().len() + 1;
    let over = "y".repeat(n + 1);
    assert!(activity::update(&d, S10, &mut |a| a.subagent_start(&over, "t")).is_err());
    assert_eq!(fs::read(d.activity_json()).unwrap(), before);
    let at_limit = "y".repeat(n);
    activity::update(&d, S10, &mut |a| a.subagent_start(&at_limit, "t")).unwrap();
    assert_eq!(fs::metadata(d.activity_json()).unwrap().len(), 1 << 20);
    assert!(activity::read(&d).unwrap().running.contains_key(&at_limit));
}

// H1 attempt-2 review round 3, S2 and S4; round 4, T3: the persistent conditions of the session folder's files give an
// error of the kind `FileCondition` (they repeat on every update): an `activity.json` that cannot be read, does not
// parse, is over the read limit or is of a newer version; an `activity.lock` that cannot be opened (a link, not a
// regular file); a failed write. The lock timeout and a refused over-limit result are of one call and do not.
#[cfg(unix)]
#[test]
fn update_errors_of_the_files_are_file_conditions_and_the_others_are_not() {
    let (t, d) = dir();
    let condition = |d: &SessionDir| {
        let e = activity::update(d, S10, &mut |a| a.busy = true).unwrap_err();
        (e.is::<activity::FileCondition>(), format!("{e:#}"))
    };
    let act = d.activity_json().display().to_string();
    let lock = d.activity_lock().display().to_string();
    two_mib_file(&d);
    assert_eq!(condition(&d), (true, format!("{act}: activity.json larger than 1 MiB")));
    fs::write(d.activity_json(), "{not json").unwrap();
    assert!(condition(&d).0);
    fs::remove_file(d.activity_json()).unwrap();
    fs::create_dir(d.activity_json()).unwrap();
    assert!(condition(&d).0);
    fs::remove_dir(d.activity_json()).unwrap();
    fs::write(d.activity_json(), r#"{"version":2}"#).unwrap();
    assert_eq!(condition(&d), (true, format!("{act}: written by a newer zygo (version 2)")));
    fs::remove_file(d.activity_json()).unwrap();
    // `activity.lock` as a link: not followed, the target is not changed.
    let outside = t.path().join("outside.lock");
    fs::write(&outside, "x").unwrap();
    fs::remove_file(d.activity_lock()).unwrap();
    std::os::unix::fs::symlink(&outside, d.activity_lock()).unwrap();
    assert_eq!(condition(&d), (true, format!("{lock}: open lock file: {lock}: is a link; it is not followed")));
    assert_eq!(fs::read_to_string(&outside).unwrap(), "x");
    assert!(!d.activity_json().exists());
    fs::remove_file(d.activity_lock()).unwrap();
    // `activity.lock` as a folder: the open for writing fails (EISDIR); the error names the path (FX-28 review
    // round 2 F2).
    fs::create_dir(d.activity_lock()).unwrap();
    assert_eq!(condition(&d), (true, format!("{lock}: open lock file: {lock}: is not a regular file")));
    fs::remove_dir(d.activity_lock()).unwrap();
    // One-off: a refused over-limit result and the lock timeout.
    let id = "x".repeat(2 << 20);
    let e = activity::update(&d, S10, &mut |a| a.subagent_start(&id, "t")).unwrap_err();
    assert!(!e.is::<activity::FileCondition>(), "{e:#}");
    let held =
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(d.activity_lock()).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    let e = activity::update(&d, Duration::from_millis(50), &mut |a| a.busy = true).unwrap_err();
    assert!(!e.is::<activity::FileCondition>(), "{e:#}");
}

/// Restores the mode of a folder when the test ends, also when it fails, so the temporary folder can be removed.
/// Unix only: a mode is a Unix permission (`PermissionsExt`).
#[cfg(unix)]
struct Mode(PathBuf, u32);

#[cfg(unix)]
impl Drop for Mode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1));
    }
}

// Round 4, T3: in a read-only session folder the write fails (an existing `activity.lock`) or the lock file cannot be
// made (no `activity.lock`); both are of the kind `FileCondition`, and the stored file stays as it is.
#[cfg(unix)]
#[test]
fn update_in_a_read_only_folder_is_a_file_condition() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: the test runs as root, which writes in a read-only folder");
        return;
    }
    let (_t, d) = dir();
    activity::update(&d, S10, &mut |a| a.busy = true).unwrap();
    let before = fs::read(d.activity_json()).unwrap();
    let _mode = Mode(d.root.clone(), 0o700);
    fs::set_permissions(&d.root, fs::Permissions::from_mode(0o500)).unwrap();
    let e = activity::update(&d, S10, &mut |a| a.busy = false).unwrap_err();
    assert!(e.is::<activity::FileCondition>(), "{e:#}");
    assert!(format!("{e:#}").starts_with(&format!("{}: ", d.activity_json().display())), "{e:#}");
    assert_eq!(fs::read(d.activity_json()).unwrap(), before);
    fs::set_permissions(&d.root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_file(d.activity_lock()).unwrap();
    fs::set_permissions(&d.root, fs::Permissions::from_mode(0o500)).unwrap();
    let e = activity::update(&d, S10, &mut |a| a.busy = false).unwrap_err();
    assert!(e.is::<activity::FileCondition>(), "{e:#}");
    assert!(format!("{e:#}").starts_with(&format!("{}: open lock file: ", d.activity_lock().display())), "{e:#}");
    assert_eq!(fs::read(d.activity_json()).unwrap(), before);
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
    let until = Instant::now() + LONG;
    while !root.join("start").exists() {
        assert!(Instant::now() < until, "no start file");
        std::thread::sleep(Duration::from_millis(1));
    }
    let d = SessionDir::new(root);
    let ok = (0..100)
        .filter(|i| activity::update(&d, Duration::MAX, &mut |a| a.subagent_start(&format!("{prefix}{i}"), "pw-sub")).is_ok())
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

#[cfg(unix)]
fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

// FIX-22: W-C1 (C6), AU26, AU56. No reader waits on a FIFO or writes through a link.
#[cfg(unix)]
#[test]
fn activity_fifo_and_link_are_errors_without_a_wait() {
    use std::os::unix::fs::FileTypeExt;
    // Both calls in one thread; the answer must come within 5 s.
    let both = |d: &SessionDir| {
        let d = d.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = activity::read(&d).map_err(|e| format!("{e:#}"));
            let u = activity::update(&d, LOCK_BOUND, &mut |a| a.busy = true)
                .map_err(|e| (e.is::<activity::FileCondition>(), format!("{e:#}")));
            let _ = tx.send((r, u));
        });
        rx.recv_timeout(Duration::from_secs(5)).expect("read or update waited")
    };
    let (t, d) = dir();
    let act = d.activity_json().display().to_string();
    let not_file = format!("{act}: is not a regular file");
    let link = format!("{act}: is a link");
    // A FIFO: an error at once; the FIFO stays.
    mkfifo(&d.activity_json());
    let (r, u) = both(&d);
    assert_eq!(r.unwrap_err(), not_file);
    assert_eq!(u.unwrap_err(), (true, not_file.clone()));
    assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_fifo());
    fs::remove_file(d.activity_json()).unwrap();
    // A link to a regular file: an error; the target and the link are not changed.
    let outside = t.path().join("outside.json");
    let body = br#"{"version":1,"busy":false,"running":{},"updated_at":1}"#;
    fs::write(&outside, body).unwrap();
    std::os::unix::fs::symlink(&outside, d.activity_json()).unwrap();
    let (r, u) = both(&d);
    assert_eq!(r.unwrap_err(), link);
    assert_eq!(u.unwrap_err(), (true, link.clone()));
    assert_eq!(fs::read(&outside).unwrap(), body.to_vec());
    assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_symlink());
    fs::remove_file(d.activity_json()).unwrap();
    // A dangling link: an error, not the default; no target is made and the link stays.
    let missing = t.path().join("missing.json");
    std::os::unix::fs::symlink(&missing, d.activity_json()).unwrap();
    let (r, u) = both(&d);
    assert_eq!(r.unwrap_err(), link);
    assert_eq!(u.unwrap_err(), (true, link.clone()));
    assert!(!missing.exists());
    assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_symlink());
    fs::remove_file(d.activity_json()).unwrap();
    // A folder: not a regular file.
    fs::create_dir(d.activity_json()).unwrap();
    let (r, u) = both(&d);
    assert_eq!(r.unwrap_err(), not_file);
    assert_eq!(u.unwrap_err(), (true, not_file));
}

// FIX-22 review round 1, F1: a socket fails the open (ENXIO) before the handle check; it is still "not a regular file".
#[cfg(unix)]
#[test]
fn activity_socket_is_not_a_regular_file() {
    use std::os::unix::fs::FileTypeExt;
    // Under /tmp, not TMPDIR: a long TMPDIR makes the socket path longer than `sun_path` (review round 2).
    let t = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let _sock = std::os::unix::net::UnixListener::bind(d.activity_json()).unwrap();
    let not_file = format!("{}: is not a regular file", d.activity_json().display());
    assert_eq!(format!("{:#}", activity::read(&d).unwrap_err()), not_file);
    let e = activity::update(&d, LOCK_BOUND, &mut |a| a.busy = true).unwrap_err();
    assert!(e.is::<activity::FileCondition>(), "{e:#}");
    assert_eq!(format!("{e:#}"), not_file);
    assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_socket());
}

// FIX-22 review round 1, F4: a link, a FIFO or a folder at activity.json is refused before `update` takes or waits for
// activity.lock, with the error of the load and well under the bound.
#[cfg(unix)]
#[test]
#[ignore = "timing: asserts each refusal takes under 500 ms"]
fn update_refuses_a_link_or_fifo_before_the_lock() {
    let (t, d) = dir();
    let held =
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(d.activity_lock()).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    let act = d.activity_json().display().to_string();
    let outside = t.path().join("outside.json");
    fs::write(&outside, b"{}").unwrap();
    for case in ["fifo", "link", "dangling link", "folder"] {
        match case {
            "fifo" => mkfifo(&d.activity_json()),
            "link" => std::os::unix::fs::symlink(&outside, d.activity_json()).unwrap(),
            "dangling link" => std::os::unix::fs::symlink(t.path().join("missing.json"), d.activity_json()).unwrap(),
            _ => fs::create_dir(d.activity_json()).unwrap(),
        }
        let d2 = d.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let start = Instant::now();
            let e = activity::update(&d2, Duration::from_secs(3), &mut |a| a.busy = true).unwrap_err();
            let _ = tx.send((start.elapsed(), e.is::<activity::FileCondition>(), format!("{e:#}")));
        });
        let (took, cond, text) = rx.recv_timeout(S10).expect("update waited");
        let why = if case.ends_with("link") { "is a link" } else { "is not a regular file" };
        assert_eq!((cond, text), (true, format!("{act}: {why}")), "{case}");
        assert!(took < Duration::from_millis(500), "{case}: {took:?}");
        if case == "folder" {
            fs::remove_dir(d.activity_json()).unwrap();
        } else {
            fs::remove_file(d.activity_json()).unwrap();
        }
    }
    assert_eq!(fs::read(&outside).unwrap(), b"{}".to_vec());
    assert!(!t.path().join("missing.json").exists());
}

/// The number of this process's open handles on `path`, read from `/proc/self/fd`.
#[cfg(target_os = "linux")]
fn handles_on(path: &Path) -> usize {
    fs::read_dir("/proc/self/fd").unwrap().flatten().filter(|e| fs::read_link(e.path()).is_ok_and(|p| p == path)).count()
}

// FIX-22 review round 2, finding 1: an entry that becomes a FIFO or a link while `update` waits for activity.lock
// passed the check before the lock; the check of the opened handle under the lock refuses it, writes nothing and
// does not call `f`. Linux only: the test knows that `update` waits for the lock when a second handle on
// activity.lock is open in this process (`/proc/self/fd`), which no other system gives.
#[cfg(target_os = "linux")]
#[test]
fn update_refuses_a_fifo_or_link_that_appears_while_it_waits_for_the_lock() {
    use std::os::unix::fs::FileTypeExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let (t, d) = dir();
    let act = d.activity_json().display().to_string();
    let body = br#"{"version":1,"busy":false,"running":{},"updated_at":1}"#;
    let outside = t.path().join("outside.json");
    fs::write(&outside, body).unwrap();
    let lock = fs::canonicalize(t.path()).unwrap().join(d.activity_lock().file_name().unwrap());
    for case in ["fifo", "link"] {
        // A regular file at activity.json, so the check before the lock passes.
        fs::write(d.activity_json(), body).unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(d.activity_lock())
            .unwrap();
        fs4::FileExt::try_lock(&held).unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let (d2, called2) = (d.clone(), called.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let e = activity::update(&d2, Duration::MAX, &mut |a| {
                called2.store(true, Ordering::SeqCst);
                a.busy = true;
            });
            let _ = tx.send(e.map_err(|e| (e.is::<activity::FileCondition>(), format!("{e:#}"))));
        });
        // `update` opens activity.lock only after the check before the lock: wait for its handle.
        let start = Instant::now();
        while handles_on(&lock) < 2 {
            assert!(start.elapsed() < LONG, "{case}: update did not open activity.lock");
            std::thread::sleep(Duration::from_millis(5));
        }
        fs::remove_file(d.activity_json()).unwrap();
        match case {
            "fifo" => mkfifo(&d.activity_json()),
            _ => std::os::unix::fs::symlink(&outside, d.activity_json()).unwrap(),
        }
        assert!(rx.try_recv().is_err(), "{case}: update returned before the lock was released");
        drop(held);
        let r = rx.recv_timeout(LONG).expect("update waited after the lock was released");
        let why = if case == "link" { "is a link" } else { "is not a regular file" };
        assert_eq!(r, Err((true, format!("{act}: {why}"))), "{case}");
        assert!(!called.load(Ordering::SeqCst), "{case}: f was called");
        let kind = fs::symlink_metadata(d.activity_json()).unwrap().file_type();
        assert!(if case == "link" { kind.is_symlink() } else { kind.is_fifo() }, "{case}: the entry was replaced");
        assert_eq!(fs::read(&outside).unwrap(), body.to_vec(), "{case}: the link target changed");
        fs::remove_file(d.activity_json()).unwrap();
    }
}
