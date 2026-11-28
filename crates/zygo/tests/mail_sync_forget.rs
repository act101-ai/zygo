//! MS-21: zygo ws forget with mail sync: the outbox question and the removal of the sync worktree (mail sync design
//! S§6.3, S§7.6, S§14, S§19 #20 forget part, #26). Machines are scratch homes on one `file://` bare repository, as
//! `tests/mail_sync_cli.rs`; no agent runs, no clock is read.
#![cfg(unix)]
mod common;

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{mpsc, Mutex};
use std::time::{Duration, SystemTime};

use ulid::Ulid;
use zygo::cmd::ws::forget_cmd;
use zygo_config::{builtin_patterns, RedactionConfig, SyncSettings};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::outbox::{count, send_remote_with, SendError};
use zygo_mail_sync::registry::{write_cache, MemberCache};
use zygo_mail_sync::state::SyncPaths;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::state::{MemberRecord, Phase};
use zygo_workspace::up::testing::hook;

use common::write_stdin;
use common::workspace::{ready_exe, Harness, WS_ID};

const ALICE: &str = "kaaaaaaaa";
const BOB: &str = "kbbbbbbbb";
const PROMPT: &str = "zygo: 2 unsent messages and 0 refused messages will be deleted. Forget the workspace? [y/N]";
/// The `sent_at` of each test message: fixed, so no test reads a clock (review focus 75).
const SENT: u64 = 1_790_000_000;

/// `git -c user.name=t -c user.email=t@t.invalid -c commit.gpgSign=false <args>` in `dir`, hermetic (no global or
/// system config, no templates); panics on a non-zero exit; the stdout without its trailing newline.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TEMPLATE_DIR", "")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgSign=false"])
        .args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", err(&o));
    out(&o).trim_end().to_string()
}

/// A bare repository `bare.git` with one commit on `main`, and its `file://` URL.
struct Remote { _t: tempfile::TempDir, url: String }
fn remote() -> Remote {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    // `main` named here: the hermetic git reads no `init.defaultBranch`, and a clone needs a HEAD that exists.
    git(&p, &["init", "-q", "--bare", "--initial-branch=main", "bare.git"]);
    let url = format!("file://{}", p.join("bare.git").display());
    git(&p, &["clone", "-q", &url, "seed"]);
    std::fs::write(p.join("seed/README"), "x\n").unwrap();
    git(&p.join("seed"), &["add", "README"]);
    git(&p.join("seed"), &["commit", "-q", "-m", "seed"]);
    git(&p.join("seed"), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    Remote { _t: t, url }
}

/// The workspace `app` (`WS_ID`) of `h`, down, with a socket whose folder does not exist (so `forget` makes no tmux
/// call, W§12) and the members `(pane key, name)`.
fn seed_down(h: &Harness, members: &[(&str, &str)]) {
    let sock = h.root().join("no-socket/s");
    h.seed(WS_ID, &mut |w| {
        w.phase = Phase::Down;
        w.tmux_socket = Some(sock.clone());
        for (k, n) in members {
            w.members.insert(k.to_string(), MemberRecord { name: Some(n.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
        }
    });
}

/// One machine: a scratch home whose `repo` is a clone of `r`, `git` linked into `bin`, `[mail] host` and
/// `[mail.workspace.app] sync`, and the workspace of [`seed_down`].
fn machine(r: &Remote, host: &str, members: &[(&str, &str)]) -> Harness {
    let h = Harness::no_tmux();
    git(h.root(), &["clone", "-q", &r.url, "repo"]);
    std::os::unix::fs::symlink(which::which("git").unwrap(), h.bin.join("git")).unwrap();
    h.config(&format!("[mail]\nhost = \"{host}\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n"));
    seed_down(&h, members);
    h
}

/// `h.zygo(args, …)`; `input` fed to stdin (none: the null device, the end of input at once).
fn zygo(h: &Harness, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut c = h.zygo(args, &[("ZYGO_WORKSPACE", OsStr::new(WS_ID))]);
    let Some(bytes) = input else { return c.output().unwrap() };
    c.stdin(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    write_stdin(child.stdin.take().unwrap(), bytes);
    child.wait_with_output().unwrap()
}
fn out(o: &Output) -> String { String::from_utf8_lossy(&o.stdout).into_owned() }
fn err(o: &Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }
fn code_out_err(o: &Output) -> (Option<i32>, String, String) { (o.status.code(), out(o), err(o)) }

/// `zygo mail sync`, which must exit 0.
fn sync(h: &Harness) {
    let o = zygo(h, &["mail", "sync"], None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
}

fn sp(h: &Harness) -> SyncPaths { SyncPaths::new(&h.paths, WS_ID).unwrap() }

/// The state folder as the binary names it (`HOME = root`: `~/.local/state/zygo`, a link to the test's folder).
fn state(h: &Harness) -> PathBuf { h.root().join(".local/state/zygo/workspaces").join(WS_ID) }

/// One message from `from@own` to `to` in the outbox of `sp` (as `tests/mail_sync_cli.rs:105-111`, time fixed), with
/// `repo` as the main root; the result of the send.
fn send_at(sp: &SyncPaths, repo: &Path, own: &str, from: &str, to: (&str, &str)) -> Result<Ulid, SendError> {
    let s = SyncSettings { branch: "zygo/mail".into(), remote: "origin".into(), host: own.into(), sync_gap: Duration::from_secs(60) };
    let policy = DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() };
    let red = RedactionConfig { pattern: builtin_patterns(), ..Default::default() };
    let to = Address { name: to.0.into(), host: to.1.into() };
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(SENT);
    send_remote_with(sp, Ok(&s), Some(from), &to, "s", "b", &red, repo, now, &policy)
}

/// One message from `from@own` to `to` in the outbox of `h`, which must be sent.
fn send(h: &Harness, own: &str, from: &str, to: (&str, &str)) {
    send_at(&sp(h), &h.repo(), own, from, to).unwrap();
}

/// A workspace that never synced: the member cache knows `bob@b`, and the repository has a `.git` folder. There is no
/// sync worktree, so the in-process forget runs no git; it runs no tmux, as the socket folder of [`seed_down`] does
/// not exist.
fn unsynced(h: &Harness) {
    write_cache(&sp(h), &MemberCache { hosts: [("b".to_string(), vec!["bob".to_string()])].into() }).unwrap();
    std::fs::create_dir_all(h.repo().join(".git")).unwrap();
}

/// `git worktree list --porcelain` in `repo`, one item per entry.
fn worktrees(repo: &Path) -> Vec<String> {
    git(repo, &["worktree", "list", "--porcelain"]).split("\n\n").map(String::from).collect()
}

/// Whether `entry` is the sync worktree of `h`.
fn is_sync(entry: &str, h: &Harness) -> bool { entry.starts_with(&format!("worktree {}\n", sp(h).worktree.display())) }

#[test] // S§7.6 steps 1-4, S§19 #20 (forget part), #26, S§6.3 (never prune): 2 outbox files: count and question; n and
        // the end of input keep everything (exit 0); --yes forgets; the sync worktree leaves git's list; the other
        // worktrees stay, a prunable one included
fn forget_asks_for_two_unsent_messages_then_yes_removes_the_sync_worktree() {
    let r = remote();
    let (a, b) = (machine(&r, "a", &[(ALICE, "alice")]), machine(&r, "b", &[(BOB, "bob")]));
    sync(&a);
    sync(&b);
    sync(&a);
    for _ in 0..2 {
        send(&a, "a", "alice", ("bob", "b"));
    }
    let repo = a.repo();
    let (other, gone) = (a.root().join("other"), a.root().join("gone"));
    git(&repo, &["worktree", "add", "-q", "-b", "other", other.to_str().unwrap()]);
    git(&repo, &["worktree", "add", "-q", "--detach", gone.to_str().unwrap()]);
    std::fs::remove_dir_all(&gone).unwrap();
    let before = worktrees(&repo);
    assert_eq!((before.len(), before.iter().filter(|e| is_sync(e, &a)).count()), (4, 1), "{before:?}");
    let prunable = |list: &[String]| list.iter().any(|e| e.starts_with(&format!("worktree {}\n", gone.display())) && e.contains("\nprunable"));
    assert!(prunable(&before), "{before:?}");
    for input in [Some(&b"n\n"[..]), None] {
        let o = zygo(&a, &["ws", "forget", "app"], input);
        assert_eq!(code_out_err(&o), (Some(0), "zygo: app not forgotten\n".to_string(), format!("{PROMPT} ")));
        assert_eq!(count(&sp(&a)).unwrap(), 2);
        assert!(state(&a).join("workspace.json").exists());
        assert_eq!(worktrees(&repo), before);
    }
    let o = zygo(&a, &["ws", "forget", "app", "--yes"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), format!("{PROMPT} y (--yes)\n")));
    let after = worktrees(&repo);
    let want: Vec<String> = before.into_iter().filter(|e| !is_sync(e, &a)).collect();
    assert_eq!((after.len(), &after), (3, &want));
    assert!(prunable(&after), "{after:?}");
    assert!(std::fs::symlink_metadata(state(&a)).is_err());
    assert!(std::fs::symlink_metadata(a.paths.workspace_cache_dir().join(WS_ID)).is_err());
}

#[test] // S§7.6 step 2, #26: an empty outbox asks nothing; forget reads no [mail] config; the sync worktree goes
fn forget_with_an_empty_outbox_asks_nothing_and_needs_no_mail_config() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    assert_eq!(worktrees(&a.repo()).iter().filter(|e| is_sync(e, &a)).count(), 1);
    a.config("");
    let o = zygo(&a, &["ws", "forget", "app"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    let after = worktrees(&a.repo());
    assert_eq!((after.len(), after.iter().filter(|e| is_sync(e, &a)).count()), (1, 0), "{after:?}");
}

#[test] // S§6.3 Remove: no worktree folder, no git call: forget succeeds with no git on PATH
fn forget_of_a_workspace_that_never_synced_runs_no_git() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    assert!(std::fs::symlink_metadata(h.bin.join("git")).is_err());
    let o = zygo(&h, &["ws", "forget", "app", "--yes"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    assert!(std::fs::symlink_metadata(state(&h)).is_err());
}

#[test] // S§6.3 Remove: a moved repository gets no git call; forget succeeds; the moved repository lists the old path as prunable
fn forget_after_the_repository_moved_runs_no_git() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    let moved = a.root().join("moved");
    std::fs::rename(a.repo(), &moved).unwrap();
    let o = zygo(&a, &["ws", "forget", "app"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    assert!(std::fs::symlink_metadata(sp(&a).worktree).is_err());
    let left = worktrees(&moved);
    assert!(left.iter().any(|e| is_sync(e, &a) && e.contains("\nprunable")), "{left:?}");
}

#[test] // S§6.3 Remove (MS-21 review F2): a link at `<cache>/<id>/mail-sync` or at `<cache>/<id>` is removed as a link
        // and no git runs, so another worktree of the repository that the link names keeps its files and its entry
fn forget_removes_a_worktree_link_and_never_the_worktree_it_names() {
    let r = remote();
    for link_at_id in [false, true] {
        let a = machine(&r, "a", &[(ALICE, "alice")]);
        let repo = a.repo();
        let other = a.root().join("x/mail-sync");
        git(&repo, &["worktree", "add", "-q", "-b", "other", other.to_str().unwrap()]);
        let wt = sp(&a).worktree;
        let id_dir = wt.parent().unwrap().to_path_buf();
        let link = if link_at_id {
            std::fs::create_dir_all(id_dir.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(other.parent().unwrap(), &id_dir).unwrap();
            id_dir.clone()
        } else {
            std::fs::create_dir_all(&id_dir).unwrap();
            std::os::unix::fs::symlink(&other, &wt).unwrap();
            wt.clone()
        };
        let o = zygo(&a, &["ws", "forget", "app", "--yes"], None);
        assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()), "link at id: {link_at_id}");
        assert!(std::fs::symlink_metadata(&link).is_err(), "link at id: {link_at_id}");
        assert!(other.join("README").is_file() && other.join(".git").is_file(), "link at id: {link_at_id}");
        let listed = worktrees(&repo);
        let entry = listed.iter().find(|e| e.starts_with(&format!("worktree {}\n", other.display())));
        assert!(entry.is_some_and(|e| !e.contains("\nprunable")), "link at id: {link_at_id}: {listed:?}");
    }
}

#[test] // S§6.3 Remove, S§7.6 step 3 (MS-21 review F4): git that cannot remove the worktree is not a refusal: the folder
        // goes by the filesystem, the forget ends, and git keeps a prunable entry (no prune runs)
fn forget_removes_the_worktree_folder_when_git_cannot() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    let g = a.bin.join("git");
    std::fs::remove_file(&g).unwrap();
    std::fs::write(&g, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o755)).unwrap();
    ready_exe(&g);
    let o = zygo(&a, &["ws", "forget", "app", "--yes"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    assert!(std::fs::symlink_metadata(state(&a)).is_err());
    assert!(std::fs::symlink_metadata(sp(&a).worktree).is_err());
    let left = worktrees(&a.repo());
    assert!(left.iter().any(|e| is_sync(e, &a) && e.contains("\nprunable")), "{left:?}");
}

#[test] // S§6.3 Remove, S§7.6 step 3 (MS-21 review F4): a regular file at `mail-sync` is removed by the filesystem
fn forget_removes_a_regular_file_at_the_worktree_path() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    let wt = sp(&a).worktree;
    std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
    std::fs::write(&wt, b"x").unwrap();
    let o = zygo(&a, &["ws", "forget", "app", "--yes"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    assert!(std::fs::symlink_metadata(&wt).is_err() && std::fs::symlink_metadata(state(&a)).is_err());
    assert_eq!(worktrees(&a.repo()).len(), 1);
}

#[test] // S§7.6 steps 1-2 (MS-21 review F3): `mail` as a link or as a file is not followed: no mail, no question, no lock;
        // forget goes on and makes or changes nothing under the link's target
fn forget_does_not_follow_a_mail_link_or_file() {
    for as_link in [true, false] {
        let h = Harness::no_tmux();
        seed_down(&h, &[(ALICE, "alice")]);
        let elsewhere = h.root().join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("sync")).unwrap();
        std::fs::create_dir_all(elsewhere.join("outbox")).unwrap();
        std::fs::write(elsewhere.join("outbox/01ARZ3NDEKTSV4RRFFQ69G5FAV.json"), b"{}").unwrap();
        if as_link {
            std::os::unix::fs::symlink(&elsewhere, state(&h).join("mail")).unwrap();
        } else {
            std::fs::write(state(&h).join("mail"), b"x").unwrap();
        }
        let o = zygo(&h, &["ws", "forget", "app"], None);
        assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()), "link: {as_link}");
        assert!(std::fs::symlink_metadata(state(&h)).is_err(), "link: {as_link}");
        let names = |d: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
            v.sort();
            v
        };
        assert_eq!(names(&elsewhere), vec!["outbox".to_string(), "sync".to_string()]);
        assert!(names(&elsewhere.join("sync")).is_empty());
        assert_eq!(names(&elsewhere.join("outbox")), vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV.json".to_string()]);
    }
}

#[test] // S§7.6 step 2, S§19 #20: an outbox that cannot be read refuses; unsent mail is never taken for none
fn forget_refuses_an_outbox_that_cannot_be_read() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    std::fs::create_dir_all(state(&h).join("mail")).unwrap();
    std::fs::write(state(&h).join("mail/outbox"), "").unwrap();
    let o = zygo(&h, &["ws", "forget", "app", "--yes"], None);
    let line = format!("zygo: cannot forget app: the outbox cannot be read: {} is not a folder\n", state(&h).join("mail/outbox").display());
    assert_eq!(code_out_err(&o), (Some(1), String::new(), line));
    assert!(state(&h).join("workspace.json").exists());
}

/// The question of `zygo ws forget --yes` for `n` unsent messages and no refused message (S§7.6 step 2).
fn asked(n: usize) -> String {
    format!("zygo: {n} unsent messages and 0 refused messages will be deleted. Forget the workspace? [y/N] y (--yes)\n")
}

/// The refusal of the forget of `app` at the count again under `mail/lock` (S§7.6 step 4, FX-28), with `n` unsent
/// messages and no refused message.
fn changed(n: usize) -> String {
    format!("zygo: cannot forget app: the outbox changed after the count: {n} unsent messages and 0 refused messages now; try again\n")
}

/// What the threads of a test report to the test thread.
#[derive(Debug)]
enum Step {
    /// The send stopped at a hook point of `send_remote_with` (`remote_send_before_link` or `remote_send_after_link`).
    Paused(&'static str),
    /// The forget is at `forget_before_mail_lock`: right before it takes `mail/lock` exclusive.
    AtLock,
    /// The send returned.
    Sent(Result<Ulid, SendError>),
    /// The forget returned: (exit code, stdout, stderr, question).
    Forgot(i32, String, String, String),
    /// The named thread panicked before it reported; its panic comes out at its join.
    Panicked(&'static str),
}

/// The fail-safe bound of each wait of a test. The order of a test comes from its hooks and channels, never from this
/// bound: the bound only turns a lost report into a failure that names the step, in place of a hang.
const STEP_BOUND: Duration = Duration::from_secs(60);

/// The body of a hook that holds its thread at `point` until the test sends the go on `rx`, within [`STEP_BOUND`].
/// An error (no go in time, or the test ended) is the hook's error at `point`.
fn wait_go(rx: &Mutex<mpsc::Receiver<()>>, point: &str) -> Result<(), String> {
    let rx = rx.lock().map_err(|_| format!("{point}: the go channel is poisoned"))?;
    rx.recv_timeout(STEP_BOUND).map_err(|e| format!("{point}: no go from the test within {STEP_BOUND:?} ({e})"))
}

/// A hook at `point` that reports `Step::Paused(point)` (or `Step::AtLock` for `forget_before_mail_lock`) on `tx`
/// and then waits for the go on the returned sender.
fn pause_at(dir: &Path, point: &'static str, tx: &mpsc::Sender<Step>) -> mpsc::Sender<()> {
    let (go, gone) = mpsc::channel::<()>();
    let (gone, report) = (Mutex::new(gone), tx.clone());
    hook(dir, point, Some(Box::new(move || {
        let _ = report.send(if point == "forget_before_mail_lock" { Step::AtLock } else { Step::Paused(point) });
        wait_go(&gone, point)
    })));
    go
}

/// Sends `Step::Panicked(name)` when it is dropped while its thread panics, so the test thread stops its wait at once.
struct PanicReport {
    tx: mpsc::Sender<Step>,
    name: &'static str,
}

impl Drop for PanicReport {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let _ = self.tx.send(Step::Panicked(self.name));
        }
    }
}

/// Clears the hooks of `points` for the state folder `dir` when dropped, also when the test panics: no hook, and no
/// sender that a hook holds, outlives the test in the static hook table of this test binary.
struct ClearHooks<'a> {
    dir: &'a Path,
    points: &'static [&'static str],
}

impl Drop for ClearHooks<'_> {
    fn drop(&mut self) {
        for point in self.points {
            hook(self.dir, point, None);
        }
    }
}

/// The named threads of a test. A thread's panic is the test's panic, with the thread's own payload.
#[derive(Default)]
struct Threads(Vec<(&'static str, std::thread::JoinHandle<()>)>);

impl Threads {
    /// Starts `f` as the thread `name`; a panic in it sends `Step::Panicked(name)` on `tx`.
    fn spawn(&mut self, name: &'static str, tx: &mpsc::Sender<Step>, f: impl FnOnce() + Send + 'static) {
        let report = PanicReport { tx: tx.clone(), name };
        self.0.push((name, std::thread::spawn(move || {
            let _report = report;
            f();
        })));
    }

    /// The next report on `rx`. `Step::Panicked` joins that thread and resumes its panic; no report within
    /// [`STEP_BOUND`] is a panic that names `step`.
    fn next(&mut self, rx: &mpsc::Receiver<Step>, step: &str) -> Step {
        match rx.recv_timeout(STEP_BOUND) {
            Ok(Step::Panicked(name)) => {
                self.join(name);
                panic!("{step}: the {name} thread reported a panic, but its join gave none");
            }
            Ok(s) => s,
            Err(e) => panic!("{step}: no report within {STEP_BOUND:?} ({e})"),
        }
    }

    /// Joins the thread `name`; its panic is resumed here.
    fn join(&mut self, name: &str) {
        let i = self.0.iter().position(|(n, _)| *n == name).unwrap_or_else(|| panic!("no thread {name}"));
        let (_, t) = self.0.remove(i);
        if let Err(payload) = t.join() {
            std::panic::resume_unwind(payload);
        }
    }

    /// Joins every thread, in start order; the first panic is resumed here.
    fn join_all(&mut self) {
        while let Some(&(name, _)) = self.0.first() {
            self.join(name);
        }
    }

    /// Starts the send of one message from `alice@a` to `bob@b` in the outbox of `h` as the thread `send`; it reports
    /// `Step::Sent`.
    fn send(&mut self, h: &Harness, tx: &mpsc::Sender<Step>) {
        let (st, ssp, srepo) = (tx.clone(), sp(h), h.repo());
        self.spawn("send", tx, move || {
            let _ = st.send(Step::Sent(send_at(&ssp, &srepo, "a", "alice", ("bob", "b"))));
        });
    }

    /// Starts `zygo ws forget --yes app` of `h` in process (`forget_cmd`) as the thread `forget`; it reports
    /// `Step::Forgot`.
    fn forget(&mut self, h: &Harness, tx: &mpsc::Sender<Step>) {
        let (ft, fpaths, fnone) = (tx.clone(), h.paths.clone(), h.root().join("none"));
        self.spawn("forget", tx, move || {
            let _ = ft.send(forgot(&fpaths, &fnone));
        });
    }
}

/// `zygo ws forget --yes app` in process with no tmux (`tmux` names no program), as `Step::Forgot`.
fn forgot(paths: &Paths, tmux: &Path) -> Step {
    let (mut ask, mut out, mut err) = (Vec::new(), Vec::new(), Vec::new());
    let code = forget_cmd(paths, tmux, None, "app", true, &mut std::io::empty(), &mut ask, &mut out, &mut err);
    let text = |b: Vec<u8>| String::from_utf8(b).unwrap();
    Step::Forgot(code, text(out), text(err), text(ask))
}

#[test] // S§7.6 steps 2 and 4 (FX-28): a remote send after the count and before the rename is in a count, or the forget
        // deletes nothing; the order comes from hooks and channels, each wait has a fail-safe bound
fn remote_send_during_forget_is_never_lost_uncounted() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    unsynced(&h);
    send(&h, "a", "alice", ("bob", "b"));
    let dir = h.paths.workspaces_dir().join(WS_ID);
    let _clear = ClearHooks { dir: &dir, points: &["forget_before_mail_lock"] };
    let (tx, rx) = mpsc::channel::<Step>();
    let mut threads = Threads::default();
    // The forget has counted 1 message and asked (step 2); it stops right before it takes mail/lock (step 4).
    let go = pause_at(&dir, "forget_before_mail_lock", &tx);
    threads.forget(&h, &tx);
    match threads.next(&rx, "the forget comes to forget_before_mail_lock") {
        Step::AtLock => {}
        other => panic!("the forget did not come to forget_before_mail_lock: {other:?}"),
    }
    // A remote send after the count and before the rename.
    threads.send(&h, &tx);
    let id = match threads.next(&rx, "the send returns") {
        Step::Sent(r) => r.expect("the send linked its file"),
        other => panic!("the send did not return while the forget waited: {other:?}"),
    };
    go.send(()).expect("the forget waits at forget_before_mail_lock");
    let (code, out, err, ask) = match threads.next(&rx, "the forget returns") {
        Step::Forgot(code, out, err, ask) => (code, out, err, ask),
        other => panic!("the forget did not return: {other:?}"),
    };
    threads.join_all();
    assert_eq!((code, out.as_str(), err.as_str()), (1, "", changed(2).as_str()));
    assert_eq!(ask, asked(1));
    assert!(sp(&h).outbox.join(format!("{id}.json")).is_file());
    assert_eq!(count(&sp(&h)).unwrap(), 2);
    assert!(dir.join("workspace.json").is_file());
    // A second forget, with no hook at its lock, asks with the new count and forgets.
    hook(&dir, "forget_before_mail_lock", None);
    match forgot(&h.paths, &h.root().join("none")) {
        Step::Forgot(code, out, err, ask) => {
            assert_eq!((code, out.as_str(), err.as_str(), ask), (0, "zygo: forgot app\n", "", asked(2)));
        }
        other => panic!("{other:?}"),
    }
    assert!(std::fs::symlink_metadata(&dir).is_err());
}

/// Whether an exclusive taker of `mail/lock` of `dir` is refused now (`WouldBlock`): a shared holder has it. The
/// test's own handle is closed before this returns.
fn mail_lock_is_held(dir: &Path) -> bool {
    let lock = std::fs::OpenOptions::new().read(true).write(true).open(dir.join("mail/lock")).unwrap();
    matches!(fs4::FileExt::try_lock(&lock), Err(fs4::TryLockError::WouldBlock))
}

/// Whether an exclusive taker of `mail/lock` of `dir` gets it within [`STEP_BOUND`]. A blocking lock in a helper
/// thread, not one `try_lock`: a child process that another test of this binary starts in the same moment holds a
/// copy of the send's lock handle from its fork until its exec (the handle is close-on-exec), so the lock can stay
/// held for that short time after the send released it. A lock that the send never releases fails at the bound.
fn mail_lock_frees(dir: &Path) -> bool {
    let lock = std::fs::OpenOptions::new().read(true).write(true).open(dir.join("mail/lock")).unwrap();
    let (tx, rx) = mpsc::channel::<std::io::Result<()>>();
    std::thread::spawn(move || {
        let _ = tx.send(fs4::FileExt::lock(&lock));
    });
    matches!(rx.recv_timeout(STEP_BOUND), Ok(Ok(())))
}

#[test] // S§7.6 step 4, S§8.1 (FX-28, re-plan R3-F1): the remote send holds mail/lock shared on both sides of its link:
        // at remote_send_before_link and at remote_send_after_link an exclusive taker is refused; no timing
fn remote_send_holds_mail_lock_across_its_link() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    unsynced(&h);
    let dir = h.paths.workspaces_dir().join(WS_ID);
    let _clear = ClearHooks { dir: &dir, points: &["remote_send_before_link", "remote_send_after_link"] };
    let (tx, rx) = mpsc::channel::<Step>();
    let mut threads = Threads::default();
    let before = pause_at(&dir, "remote_send_before_link", &tx);
    let after = pause_at(&dir, "remote_send_after_link", &tx);
    threads.send(&h, &tx);
    match threads.next(&rx, "the send stops at remote_send_before_link") {
        Step::Paused("remote_send_before_link") => {}
        other => panic!("the send did not stop at remote_send_before_link: {other:?}"),
    }
    assert_eq!(count(&sp(&h)).unwrap(), 0);
    assert!(mail_lock_is_held(&dir), "an exclusive taker got mail/lock while the send was before its link");
    before.send(()).expect("the send waits at remote_send_before_link");
    match threads.next(&rx, "the send stops at remote_send_after_link") {
        Step::Paused("remote_send_after_link") => {}
        other => panic!("the send did not stop at remote_send_after_link: {other:?}"),
    }
    assert_eq!(count(&sp(&h)).unwrap(), 1);
    assert!(mail_lock_is_held(&dir), "an exclusive taker got mail/lock while the send was after its link");
    after.send(()).expect("the send waits at remote_send_after_link");
    let id = match threads.next(&rx, "the send returns") {
        Step::Sent(r) => r.expect("the send linked its file"),
        other => panic!("the send did not return: {other:?}"),
    };
    threads.join_all();
    assert!(sp(&h).outbox.join(format!("{id}.json")).is_file());
    assert!(mail_lock_frees(&dir), "the send left mail/lock held");
}

#[test] // S§7.6 step 4, S§8.1 (FX-28, re-plan R3-F2): a send under mail/lock and a forget that comes to its lock meanwhile:
        // the test holds the forget at forget_before_mail_lock until the send returned (this order is the test's, not
        // the lock's: the forget that meets the held lock is forget_cannot_take_mail_lock_while_a_send_holds_it); the
        // forget then counts the linked file again under mail/lock and refuses; each wait has a fail-safe bound
fn remote_send_racing_forget_makes_the_forget_refuse_at_the_count_again() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    unsynced(&h);
    send(&h, "a", "alice", ("bob", "b"));
    let dir = h.paths.workspaces_dir().join(WS_ID);
    let _clear = ClearHooks { dir: &dir, points: &["remote_send_before_link", "forget_before_mail_lock"] };
    let (tx, rx) = mpsc::channel::<Step>();
    let mut threads = Threads::default();
    // The send stops under mail/lock, right before its link.
    let resume = pause_at(&dir, "remote_send_before_link", &tx);
    threads.send(&h, &tx);
    match threads.next(&rx, "the send stops at remote_send_before_link") {
        Step::Paused("remote_send_before_link") => {}
        other => panic!("the send did not stop at remote_send_before_link: {other:?}"),
    }
    // The forget counts 1 message, asks, and stops right before it takes mail/lock exclusive.
    let go = pause_at(&dir, "forget_before_mail_lock", &tx);
    threads.forget(&h, &tx);
    match threads.next(&rx, "the forget comes to forget_before_mail_lock") {
        Step::AtLock => {}
        other => panic!("the forget did not come to forget_before_mail_lock: {other:?}"),
    }
    resume.send(()).expect("the send waits at remote_send_before_link");
    let id = match threads.next(&rx, "the send returns") {
        Step::Sent(r) => r.expect("the send linked its file"),
        other => panic!("the send did not return while the forget waited: {other:?}"),
    };
    go.send(()).expect("the forget waits at forget_before_mail_lock");
    let (code, out, err, ask) = match threads.next(&rx, "the forget returns") {
        Step::Forgot(code, out, err, ask) => (code, out, err, ask),
        other => panic!("the forget did not return: {other:?}"),
    };
    threads.join_all();
    assert_eq!((code, out.as_str(), err.as_str()), (1, "", changed(2).as_str()));
    assert_eq!(ask, asked(1));
    assert!(sp(&h).outbox.join(format!("{id}.json")).is_file());
    assert_eq!(count(&sp(&h)).unwrap(), 2);
    assert!(dir.join("workspace.json").is_file());
}

#[test] // S§7.6 step 4, S§8.1 (FX-28 review F1): a send that holds mail/lock shared (stopped at remote_send_before_link)
        // and a forget that comes to the lock meanwhile: the forget waits the code's own MAIL_LOCK_WAIT in take_lock
        // and refuses as busy, the state stays, and the send then links and returns Ok; the test itself has no sleep
fn forget_cannot_take_mail_lock_while_a_send_holds_it() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    unsynced(&h);
    let dir = h.paths.workspaces_dir().join(WS_ID);
    let _clear = ClearHooks { dir: &dir, points: &["remote_send_before_link", "forget_before_mail_lock"] };
    let (tx, rx) = mpsc::channel::<Step>();
    let mut threads = Threads::default();
    // The send stops under mail/lock, right before its link.
    let resume = pause_at(&dir, "remote_send_before_link", &tx);
    threads.send(&h, &tx);
    match threads.next(&rx, "the send stops at remote_send_before_link") {
        Step::Paused("remote_send_before_link") => {}
        other => panic!("the send did not stop at remote_send_before_link: {other:?}"),
    }
    // The forget reports that it came to its lock and goes on at once: no go from the test, so the only thing that
    // can stop it is the lock that the send holds.
    let at_lock = tx.clone();
    hook(&dir, "forget_before_mail_lock", Some(Box::new(move || {
        let _ = at_lock.send(Step::AtLock);
        Ok(())
    })));
    threads.forget(&h, &tx);
    match threads.next(&rx, "the forget comes to forget_before_mail_lock") {
        Step::AtLock => {}
        other => panic!("the forget did not come to forget_before_mail_lock: {other:?}"),
    }
    let (code, out, err, ask) = match threads.next(&rx, "the forget returns while the send holds mail/lock") {
        Step::Forgot(code, out, err, ask) => (code, out, err, ask),
        other => panic!("the forget did not return while the send held mail/lock: {other:?}"),
    };
    assert_eq!((code, out.as_str(), err.as_str(), ask.as_str()), (1, "", "zygo: cannot forget app: mail is busy; try again\n", ""));
    assert!(dir.join("workspace.json").is_file(), "the forget removed the state while the send held mail/lock");
    resume.send(()).expect("the send waits at remote_send_before_link");
    let id = match threads.next(&rx, "the send returns") {
        Step::Sent(r) => r.expect("the send linked its file"),
        other => panic!("the send did not return: {other:?}"),
    };
    threads.join_all();
    assert!(sp(&h).outbox.join(format!("{id}.json")).is_file());
    assert!(dir.join("workspace.json").is_file());
}
