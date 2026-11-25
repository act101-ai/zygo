//! MS-2: the hardened git builder of the mail sync (S-C4, S§8.7) and the test seams (S-C9). No test waits on
//! wall-clock time.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use zygo_mail_sync::env::{Clock, Crash, Env, NoSeam, Probe, Seam, Spawner, SystemClock, Verdict};
use zygo_mail_sync::git::{has_text, run, stream, sync_git};
use zygo_work::repo::{Flow, RunError, Streamed};

/// A deadline one hour away: a test not about time never times out under machine load.
fn far() -> Instant { Instant::now() + Duration::from_secs(3600) }

/// A new empty repository, made through `run`.
fn init() -> TempDir {
    let t = TempDir::new().unwrap();
    let w = run(t.path(), &["init", "-q"], far());
    assert!(matches!(&w, Ok(w) if w.ok), "{w:?}");
    t
}

/// Program, arguments, environment entries and directory of a command.
fn parts(c: &Command) -> (OsString, Vec<OsString>, BTreeMap<OsString, Option<OsString>>, Option<std::path::PathBuf>) {
    (c.get_program().to_owned(),
     c.get_args().map(OsStr::to_owned).collect(),
     c.get_envs().map(|(k, v)| (k.to_owned(), v.map(OsStr::to_owned))).collect(),
     c.get_current_dir().map(|d| d.to_owned()))
}

#[test]
fn sync_git_is_git_command_plus_the_two_maintenance_overrides() {
    let t = TempDir::new().unwrap();
    let (p0, mut a0, e0, d0) = parts(&zygo_plugins::sync::git_command(t.path()));
    let (p1, a1, e1, d1) = parts(&sync_git(t.path()));
    a0.extend(["-c", "gc.auto=0", "-c", "maintenance.auto=false"].map(OsString::from));
    assert_eq!((p1, a1, e1, d1), (p0, a0, e0, d0));
}

#[test]
fn run_applies_the_maintenance_overrides() {
    let t = init();
    let w = run(t.path(), &["config", "--get", "gc.auto"], far()).unwrap();
    assert!(w.ok);
    assert_eq!(w.out, b"0\n");
    let w = run(t.path(), &["config", "--get", "maintenance.auto"], far()).unwrap();
    assert_eq!(w.out, b"false\n");
}

#[test]
fn run_non_zero_exit_gives_stderr() {
    let t = TempDir::new().unwrap();
    let w = run(t.path(), &["rev-parse", "--verify", "no-such-ref"], far()).unwrap();
    assert!(!w.ok);
    assert!(!w.err.is_empty());
}

#[test]
fn run_past_deadline_times_out() {
    let t = TempDir::new().unwrap();
    assert_eq!(run(t.path(), &["--version"], Instant::now()), Err(RunError::TimedOut));
}

#[test]
fn stream_passes_each_nul_ended_entry() {
    let t = init();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let r = stream(t.path(), &["config", "-z", "--get-regexp", r"^(gc|maintenance)\.auto$"], far(),
        &mut |e| { seen.push(e.to_vec()); Flow::Go });
    assert_eq!(r, Ok(Streamed::Done { ok: true, err: Vec::new() }));
    // The command-line entries come last; a user's global config may add earlier ones.
    assert!(seen.ends_with(&[b"gc.auto\n0".to_vec(), b"maintenance.auto\nfalse".to_vec()]), "{seen:?}");
}

#[test]
fn stream_stop_returns_stopped_after_one_entry() {
    let t = init();
    let mut n = 0;
    let r = stream(t.path(), &["config", "-z", "--list"], far(), &mut |_| { n += 1; Flow::Stop });
    assert_eq!(r, Ok(Streamed::Stopped));
    assert_eq!(n, 1);
}

#[test]
fn has_text_matches_without_case() {
    assert!(has_text(b"ERROR: Repository Not Found.", &["repository not found"]));
    assert!(has_text(b"fatal: could not read Username", &["no such thing", "Could Not Read"]));
    assert!(has_text(b"\xff\xfeREPOSITORY NOT FOUND", &["repository not found"]));
    assert!(!has_text(b"everything up-to-date", &["repository not found"]));
    assert!(!has_text(b"anything", &[]));
    assert!(!has_text(b"anything", &[""]));
    assert!(!has_text(b"", &["a"]));
}

struct Fixed(SystemTime);
impl Clock for Fixed { fn now(&self) -> SystemTime { self.0 } }
struct Says(Verdict);
impl Probe for Says { fn probe(&self, _: &str) -> Verdict { self.0.clone() } }
struct CrashAt(&'static str);
impl Seam for CrashAt {
    fn at(&self, p: &'static str) -> Result<(), Crash> { if p == self.0 { Err(Crash(p)) } else { Ok(()) } }
}
struct Count(std::rc::Rc<std::cell::Cell<u32>>);
impl Spawner for Count {
    fn spawn_sync(&self, _: &str) -> std::io::Result<()> { self.0.set(self.0.get() + 1); Ok(()) }
}

#[test]
fn env_carries_the_injected_seams() {
    let n = std::rc::Rc::new(std::cell::Cell::new(0));
    let at = UNIX_EPOCH + Duration::from_secs(100);
    let env = Env { paths: zygo_core::paths::Paths::from_env(), clock: Box::new(Fixed(at)),
        probe: Box::new(Says(Verdict::Unknown("x".into()))), seam: Box::new(CrashAt("after_push")),
        spawner: Box::new(Count(n.clone())) };
    assert_eq!(env.clock.now(), at);
    assert_eq!(env.probe.probe("https://h/p"), Verdict::Unknown("x".into()));
    assert_eq!(env.seam.at("before_push"), Ok(()));
    assert_eq!(env.seam.at("after_push"), Err(Crash("after_push")));
    env.spawner.spawn_sync("ws").unwrap();
    assert_eq!(n.get(), 1);
}

#[test]
fn production_seams_are_inert() {
    assert_eq!(NoSeam.at("after_push"), Ok(()));
    let _: SystemTime = SystemClock.now(); // no assertion on its value (S-C9: no wall-clock test)
    assert_ne!(Verdict::Public, Verdict::Private);
    assert_ne!(Verdict::Local, Verdict::Unknown(String::new()));
}
