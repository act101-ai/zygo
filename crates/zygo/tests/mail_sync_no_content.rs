//! MS-22: no mail content in logs, state, usage rows, commits or refs (mail sync design S-C2, S§6.5, S§19 #24 as
//! rewritten 2026-11-26). Three machines (A, B, and C, which runs a copy of A's state folder for #11) are three
//! scratch homes on one `file://` bare repository; no agent runs; no network host is asked; no wall clock is read for a
//! result.
#![cfg(unix)]
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo::cmd::mail::{send_cli, watch_sync_step, watch_with};
use zygo_config::{Config, Features, LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_core::Agent;
use zygo_hooks::mail::{other_machines, stop_block};
use zygo_hooks::workspace::{run_with_sync, session_start_with_sync};
use zygo_mail_sync::env::{Clock, Crash, Env, NoSeam, Probe, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::outbox;
use zygo_mail_sync::public::{map_url, probe_command, Mapped};
use zygo_mail_sync::registry::{read_cache, write_cache, MemberCache};
use zygo_mail_sync::state::{mark_again, read_last_start, SyncPaths};
use zygo_mail_sync::sync::{run_with_deadline, Mode, Opts, Report, SyncError};
use zygo_mcp::tools::mail::tools_with_sync;
use zygo_mcp::{McpCtx, Tool, ToolError};
use zygo_redact::act_lite_store;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::wake::{after_send_with, Probe as IdleProbe, WAKE_TEXT};
use zygo_workspace::mail::{mail_dir, store};
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_workspace::state::{MemberRecord, Phase, Workspace};

use common::feed_stdin;
use common::workspace::{Harness, WS_ID};

const ALICE: &str = "kaaaaaaaa"; // A's member key
const BOB: &str = "kbbbbbbbb"; // B's member key
const B_ALICE: &str = "kdddddddd"; // #9: B's second member key, named `A_NAME` during the #9 block
const A_NAME: &str = "alice-q7x";
const B_NAME: &str = "bob-q7x";
const HOST_A: &str = "apple-q7x"; // A's host
const HOST_B: &str = "birch-q7x"; // B's host
const HOST_B2: &str = "cedar-q7x"; // B's host after #19
const HOST_A2: &str = "delta-q7x"; // A's host after #9, and C's (the copy of A's state) in #11
const HOST_A3: &str = "elm-q7x"; // A's host after #11
const HOST_R1: &str = "fir-q7x"; // the file name of the #12 registry with 257 names
const HOST_R2: &str = "gum-q7x"; // the file name of the #12 registry whose `host` is not its name
const HOST_R3: &str = "hazel-q7x"; // the `host` of that registry
const SUBJ: &str = "subj-q7x"; // every subject starts with it
const BODY: &str = "body-q7x"; // every body holds it
const MARKERS: [&str; 12] = [SUBJ, BODY, A_NAME, B_NAME, HOST_A, HOST_B, HOST_B2, HOST_A2, HOST_A3, HOST_R1, HOST_R2, HOST_R3];
/// S§6.5: the message of every commit that zygo writes.
const MESSAGE: &str = "zygo mail [skip ci]\n";
/// S§6.5: the author and the committer of every commit that zygo writes.
const ZYGO_ID: &str = "zygo <zygo@zygo.invalid>";
/// The #4 pattern text.
const SHIP: &str = "zz-forbidden-zz";
/// #27: the end of session text item 8 (`crates/zygo-hooks/tests/mail_sync_text.rs:8`), byte for byte.
const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";
/// #21: a member key with no name.
const ANON: &str = "kcccccccc";
/// #10: the other workspace state on the machine.
const OTHER_WS: &str = "fedcba9876543210";
/// #18: the injected clock a year later.
const YEAR: Duration = Duration::from_secs(365 * 86_400);
/// The S§7.2 line of a remote send from a member with no name (`crates/zygo/tests/mail_remote_cli.rs:42`).
const NO_NAME: &str = "zygo: remote mail needs a sender name; give this member a name in the loadout or with --name";
/// The S§7.2 line of a send to a reserved name (`crates/zygo/tests/mail_remote_cli.rs:45`).
const RESERVED: &str = "zygo: all, cli and zygo have no address; send to <name> or <name>@<host>";
/// The S§8.3 step 7 line of a side branch without `ZYGO-MAIL`.
const MARKER_LINE: &str = "zygo: the side branch is not a zygo mail branch";
/// The S§11 line of a `sync` branch that is not of the `zygo/` form.
const BAD_BRANCH: &str = "zygo: mail sync is off: sync in [mail.workspace.app] is not zygo/ and 1 to 32 of a-z, 0-9 and -";
/// The S§7.1 line of no `[mail] host`.
const NO_HOST: &str = "zygo: mail sync is off: no mail host; set host in [mail] of the user config";
/// #25: the line of `[mail]` in `.zygo.toml` (`crates/zygo-config/tests/mail_sync_config.rs:19`).
const REPO_LINE: &str = ".zygo.toml: [mail] ignored; the repository file may set only [guards], [navigation], [limits], [work]";
/// #25: the line of `[mail]` in team config (`crates/zygo-config/tests/mail_sync_config.rs:18`).
const TEAM_LINE: &str = "config: [mail] in team config ignored; only user config may set them";
/// The line of `crates/zygo/tests/mail_sync_cli.rs:29`, byte for byte.
const COLLISION: &str = "zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.";
/// The S§7.5 twin line (`crates/zygo-mail-sync/src/identity.rs:22`), byte for byte.
const TWIN: &str = "zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host";
/// The S§8.2 `public` line (`crates/zygo-mail-sync/src/sync.rs:85`, `outbox.rs:43`), byte for byte.
const PUBLIC: &str = "zygo: the remote can be read without a login; mail sync refused";

/// Author's call 7: both tests hold it, so no in-process lock meets a child that another test thread forks.
static SERIAL: Mutex<()> = Mutex::new(());

/// The git variables that would make a child git read another repository, another config or another author
/// (`crates/zygo-mail-sync/tests/common/mod.rs:27-32`).
const CLEARED: &[&str] = &[
    "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_CONFIG", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS", "GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL", "ACT_CONFIG_DIR",
];

/// The hermetic environment of one test ([`hermetic`]). Its drop gives the process its variables back, then removes
/// the home folder (MS-22 review F5), so no run leaves a folder.
struct Hermetic {
    saved: Vec<(&'static str, Option<OsString>)>,
    _home: TempDir,
}
impl Drop for Hermetic {
    fn drop(&mut self) {
        // SAFETY: the test drops the guard under `SERIAL`, after every process and thread it started has ended.
        unsafe {
            for (k, v) in &self.saved {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

/// Author's call 3: the in-process sync starts git through `git_command`, which reads the global config of `HOME`.
/// It builds the stub agent with the real `HOME` (its cargo home, once per process), then sets the process `HOME`
/// and `XDG_CONFIG_HOME` to a new empty folder in the temporary folder and removes the git variables of [`CLEARED`],
/// until the returned guard drops.
fn hermetic() -> Hermetic {
    common::stub_agent();
    let t = tempfile::Builder::new().prefix("home").tempdir().unwrap();
    let h = t.path().canonicalize().unwrap();
    std::fs::create_dir_all(h.join(".config")).unwrap();
    let saved = ["HOME", "XDG_CONFIG_HOME"].iter().chain(CLEARED).map(|k| (*k, std::env::var_os(k))).collect();
    // SAFETY: every test calls `hermetic()` first, under `SERIAL`, before it starts a process or a thread.
    unsafe {
        std::env::set_var("HOME", &h);
        std::env::set_var("XDG_CONFIG_HOME", h.join(".config"));
        for k in CLEARED {
            std::env::remove_var(k);
        }
    }
    Hermetic { saved, _home: t }
}

/// `git` with no global or system config and no template folder, so the hooks, signing, templates and `insteadOf`
/// rules of the user who runs the tests cannot change a result. Every git that the test starts is made here.
fn git_cmd() -> Command {
    let mut c = Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TEMPLATE_DIR", "");
    c
}

/// `git -c user.name=t -c user.email=t@t.invalid -c commit.gpgSign=false <args>` in `dir`; panics on a non-zero
/// exit; the stdout without its trailing newline.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd()
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgSign=false"])
        .args(args).current_dir(dir).stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", err_of(&o));
    out_of(&o).trim_end().to_string()
}

/// [`git`] with `input` on stdin.
fn git_in(dir: &Path, args: &[&str], input: &[u8]) -> String {
    let mut ch = git_cmd().args(args).current_dir(dir)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    feed_stdin(ch.stdin.take().unwrap(), input);
    let o = ch.wait_with_output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", err_of(&o));
    out_of(&o).trim_end().to_string()
}

/// A bare repository `bare.git` with one commit on `main` (pushed from a seed clone), and its `file://` URL.
struct Remote { _t: TempDir, bare: PathBuf, url: String }
fn remote() -> Remote {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    // `main` named here: the hermetic git reads no `init.defaultBranch`, and a clone needs a HEAD that exists.
    git(&p, &["init", "-q", "--bare", "--initial-branch=main", "bare.git"]);
    let bare = p.join("bare.git");
    let url = format!("file://{}", bare.display());
    git(&p, &["clone", "-q", &url, "seed"]);
    std::fs::write(p.join("seed/README"), "x\n").unwrap();
    git(&p.join("seed"), &["add", "README"]);
    git(&p.join("seed"), &["commit", "-q", "-m", "seed"]);
    git(&p.join("seed"), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    Remote { _t: t, bare, url }
}

/// The user config of `h`: `[mail]` with `host` and `mail_keys`, `[mail.workspace.app]` sync on, then `tables`; then
/// the shared `common::NO_MARKETPLACE` (`Harness::config`, FX-23), so no marketplace is fetched.
fn config(h: &Harness, host: &str, mail_keys: &str, tables: &str) {
    h.config(&format!("[mail]\nhost = \"{host}\"\n{mail_keys}[mail.workspace.app]\nsync = \"zygo/mail\"\n{tables}"));
}

/// One machine: a scratch home whose `repo` is a clone of `r`, `git` linked into `bin`, the config of `host`, and the
/// workspace `app` (`WS_ID`), down, with a socket whose folder does not exist (so `forget` makes no tmux call) and
/// the one member `key` named `name`.
fn machine(r: &Remote, host: &str, key: &str, name: &str) -> Harness {
    let h = Harness::no_tmux();
    git(h.root(), &["clone", "-q", &r.url, "repo"]);
    std::os::unix::fs::symlink(which::which("git").unwrap(), h.bin.join("git")).unwrap();
    config(&h, host, "", "");
    let sock = h.root().join("no-socket/s");
    h.seed(WS_ID, &mut |w| {
        w.phase = Phase::Down;
        w.tmux_socket = Some(sock.clone());
        w.members.insert(key.to_string(), MemberRecord { name: Some(name.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
    });
    h
}

/// `h.zygo(args, …)` with `ZYGO_WORKSPACE = WS_ID`; `input` fed to stdin (none: the null device). After the run, no
/// marketplace sync ran (`common::assert_no_marketplace_sync`, FX-23).
fn zygo(h: &Harness, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut c = h.zygo(args, &[("ZYGO_WORKSPACE", OsStr::new(WS_ID))]);
    let o = match input {
        None => c.output().unwrap(),
        Some(bytes) => {
            c.stdin(Stdio::piped());
            let mut child = c.spawn().unwrap();
            feed_stdin(child.stdin.take().unwrap(), bytes);
            child.wait_with_output().unwrap()
        }
    };
    common::assert_no_marketplace_sync(h.root());
    o
}
/// `h.zygo(args, …)` in the folder `cwd` with `ZYGO_WORKSPACE = WS_ID` and then `extra`, stdin the null device. After
/// the run, no marketplace sync ran.
fn zygo_at(h: &Harness, cwd: &Path, args: &[&str], extra: &[(&str, &OsStr)]) -> Output {
    let mut vars = vec![("ZYGO_WORKSPACE", OsStr::new(WS_ID))];
    vars.extend_from_slice(extra);
    let o = h.zygo(args, &vars).current_dir(cwd).output().unwrap();
    common::assert_no_marketplace_sync(h.root());
    o
}
fn out_of(o: &Output) -> String { String::from_utf8_lossy(&o.stdout).into_owned() }
fn err_of(o: &Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }

fn sp(h: &Harness) -> SyncPaths { SyncPaths::new(&h.paths, WS_ID).unwrap() }
/// The `errors.log` of `h`; empty when the file does not exist (`scan` asserts that it exists where the walk expects
/// one). FX-26 re-plan review round 2 F4: a file that cannot be read, or that is not UTF-8, fails with its path.
fn errors(h: &Harness) -> String {
    let p = h.store(WS_ID).dir().join("errors.log");
    if std::fs::symlink_metadata(&p).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return String::new();
    }
    String::from_utf8(read_file(&p, "scan")).unwrap_or_else(|e| panic!("{}: the scan cannot read it ({e})", p.display()))
}

/// Author's call 2: `zygo mail sync <args>` as the binary, with an empty outbox only (its secret pass has the
/// 200 ms default budget). The caller asserts the exit code.
fn sync_bin(h: &Harness, args: &[&str]) -> Output {
    assert_eq!(outbox::count(&sp(h)).unwrap(), 0, "the binary syncs only an empty outbox");
    let mut a = vec!["mail", "sync"];
    a.extend_from_slice(args);
    zygo(h, &a, None)
}

/// The one clock of every in-process call: the time of its first call.
fn t0() -> SystemTime {
    static T0: OnceLock<SystemTime> = OnceLock::new();
    *T0.get_or_init(SystemTime::now)
}

struct Fixed(SystemTime);
impl Clock for Fixed { fn now(&self) -> SystemTime { self.0 } }
/// No public check runs: the sync of a `file://` remote maps it to a local path (S§8.2).
struct NoProbe;
impl Probe for NoProbe { fn probe(&self, _: &str) -> Verdict { panic!("no public check runs in these tests") } }
/// Author's call 1: the spawn seam starts nothing.
struct NoSpawn;
impl Spawner for NoSpawn { fn spawn_sync(&self, _: &str) -> std::io::Result<()> { Ok(()) } }
/// #6: the injected public probe gives this verdict; no network host is asked.
struct Says(Verdict);
impl Probe for Says { fn probe(&self, _: &str) -> Verdict { self.0.clone() } }
/// #22: the sync child does not start.
struct FailSpawn;
impl Spawner for FailSpawn {
    fn spawn_sync(&self, _: &str) -> std::io::Result<()> { Err(std::io::Error::from(std::io::ErrorKind::NotFound)) }
}
/// The wake seam does nothing.
struct NoWake;
impl Wake for NoWake { fn wake(&self, _: &Paths, _: &Workspace, _: &[String], _: &MailConfig, _: SystemTime) {} }

/// #22: the spawn seam counts each start and starts nothing.
struct Count(Arc<AtomicUsize>);
impl Spawner for Count {
    fn spawn_sync(&self, _: &str) -> std::io::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
/// #6: the injected public probe counts each call and gives its verdict; no network host is asked.
struct CountProbe(Arc<AtomicUsize>, Verdict);
impl Probe for CountProbe {
    fn probe(&self, _: &str) -> Verdict {
        self.0.fetch_add(1, Ordering::SeqCst);
        self.1.clone()
    }
}

/// #23: the M§7.3 wake of each key with a `wake_gap` that does not parse, an idle member and a pane that takes no
/// text ([`StubMux`] with no record).
struct FailWake;
impl Wake for FailWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime) {
        let cfg = MailConfig { wake_gap: "soon".into(), ..cfg.clone() };
        for k in keys {
            let mux = StubMux { key: k.clone(), typed: None };
            after_send_with(paths, ws, std::slice::from_ref(k), &cfg, &mux, &Idle { idle: true, now }, now);
        }
    }
}
/// #23: the M§7.3 wake of each key through the multiplexer stub, which records each typed text, with a fixed idle rule
/// (as `crates/zygo-mail-sync/tests/sync_pass.rs:136-150`).
struct StubWake { idle: bool, typed: Typed }
impl Wake for StubWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime) {
        for k in keys {
            let mux = StubMux { key: k.clone(), typed: Some(self.typed.clone()) };
            after_send_with(paths, ws, std::slice::from_ref(k), cfg, &mux, &Idle { idle: self.idle, now }, now);
        }
    }
}
/// The idle rule of a wake: fixed, at the clock of the sync.
struct Idle { idle: bool, now: SystemTime }
impl IdleProbe for Idle {
    fn idle(&self, _: &Path) -> bool { self.idle }
    fn now(&self) -> SystemTime { self.now }
}
/// The error of every [`StubMux`] send with no record.
fn mux_error() -> MuxError { MuxError::Failed { message: "no server running".into() } }
/// Each `(pane, text)` that a [`StubMux`] typed.
type Typed = Arc<Mutex<Vec<(String, String)>>>;
/// A session `@zygo-ws` = [`WS_ID`] with one pane `%3` that holds the key (the line of
/// `crates/zygo-mail-sync/tests/sync_pass.rs:120`). With `typed`, a send is recorded and passes; with none, a send
/// fails with [`mux_error`].
struct StubMux { key: String, typed: Option<Typed> }
impl Multiplexer for StubMux {
    fn socket(&self) -> Option<&Path> { None }
    fn version(&self) -> Result<(u32, u32), MuxError> { unreachable!() }
    fn socket_path(&self, _: &str) -> Result<PathBuf, MuxError> { unreachable!() }
    fn session_exists(&self, _: &str) -> SessionAnswer { unreachable!() }
    fn new_session(&self, _: &NewSession<'_>) -> Result<u32, MuxError> { unreachable!() }
    fn resize_window(&self, _: &str, _: u32, _: u32, _: u32) -> Result<(), MuxError> { unreachable!() }
    fn unset_window_option(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn set_window_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn set_env(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn hide_env(&self, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn set_session_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn show_session_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> { Ok(Some(WS_ID.into())) }
    fn set_hook(&self, _: &str, _: HookScope, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn new_window(&self, _: &NewWindow<'_>) -> Result<u32, MuxError> { unreachable!() }
    fn rename_window(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn split_window(&self, _: &Split<'_>) -> Result<String, MuxError> { unreachable!() }
    fn select_layout(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn select_window(&self, _: &str, _: u32) -> Result<(), MuxError> { unreachable!() }
    fn select_pane(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn zoom(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn list_panes(&self, _: &str) -> Result<Vec<String>, MuxError> {
        Ok(vec![format!("0|w|1|l|80|24|1|0|%3|0|1|{}|/srv/app", self.key)])
    }
    fn show_pane_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> { Ok(Some(self.key.clone())) }
    fn set_pane_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn display(&self, _: &str, _: &str) -> Result<String, MuxError> { unreachable!() }
    fn kill_session(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn attach(&self, _: &str) -> Result<i32, MuxError> { unreachable!() }
    fn switch_client(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError> {
        let Some(t) = &self.typed else { return Err(mux_error()) };
        t.lock().unwrap().push((pane.to_string(), text.to_string()));
        Ok(())
    }
    fn send_enter(&self, _: &str) -> Result<(), MuxError> {
        if self.typed.is_some() { Ok(()) } else { Err(mux_error()) }
    }
}

/// #23: a live session `<sessions_dir>/<id>` (`token` and a held lock on `pid`) while the file lives
/// (`crates/zygo-mail-sync/tests/common/mod.rs:285-292`).
fn live(paths: &Paths, id: &str) -> std::fs::File {
    let dir = paths.sessions_dir.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token"), "t").unwrap();
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("pid")).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}
/// A seam that runs a closure at each named point.
type HookFn = Box<dyn FnMut(&'static str) -> Result<(), Crash> + Send>;
struct Hook(Mutex<HookFn>);
impl Hook {
    fn new(f: impl FnMut(&'static str) -> Result<(), Crash> + Send + 'static) -> Hook { Hook(Mutex::new(Box::new(f))) }
}
impl Seam for Hook {
    fn at(&self, point: &'static str) -> Result<(), Crash> { (self.0.lock().unwrap())(point) }
}

fn env_paths(paths: Paths, seam: Box<dyn Seam>, policy: DeadlinePolicy) -> Env {
    env_full(paths, seam, policy, Box::new(NoProbe), Box::new(NoSpawn), Box::new(NoWake))
}
/// The env of `paths` at [`t0`] with every seam given.
fn env_full(paths: Paths, seam: Box<dyn Seam>, policy: DeadlinePolicy, probe: Box<dyn Probe>, spawner: Box<dyn Spawner>,
    wake: Box<dyn Wake>) -> Env {
    Env { secret_policy: policy, ..env_now(paths, seam, probe, spawner, wake, t0()) }
}
/// Author's call 3: the env of `paths` at the fixed time `now` with every seam given and an hour of secret-pass budget.
fn env_now(paths: Paths, seam: Box<dyn Seam>, probe: Box<dyn Probe>, spawner: Box<dyn Spawner>, wake: Box<dyn Wake>,
    now: SystemTime) -> Env {
    Env { paths, clock: Box::new(Fixed(now)), probe, seam, spawner, wake, secret_policy: far() }
}
fn env_in(h: &Harness, seam: Box<dyn Seam>, policy: DeadlinePolicy) -> Env { env_paths(h.paths.clone(), seam, policy) }
/// An hour of secret-pass budget: no clock decides the test.
fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }

/// FX-26 review round 2 F-3: an hour of pass deadline (S§8.3 step 4) in place of the production 120 s
/// (`sync::PASS_DEADLINE`), so a result that counts the work of one pass does not depend on how fast git runs.
/// FX-26 R3-11: every pass that a walk runs in process takes it, through `sync::run_with_deadline` (the S-C9 seam);
/// only the real binary runs (`zygo mail sync` and the other [`Harness::zygo`] runs) keep the production 120 s.
const FAR_PASS: Duration = Duration::from_secs(3600);

/// Author's call 2: a sync in process, with an hour of secret-pass budget and the pass deadline [`FAR_PASS`].
fn sync_in(h: &Harness) -> Report {
    run_with_deadline(&env_in(h, Box::new(NoSeam), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap()
}

/// The user config of `h`, as the binary loads it.
fn cfg(h: &Harness) -> Config { cfg_of(&h.paths) }
/// The user config of `paths`.
fn cfg_of(paths: &Paths) -> Config {
    zygo_config::load(&LoadContext { paths: paths.clone(), repo: None, team: None }).unwrap().config
}

/// `send_cli` in `env` from `key` with subject `{SUBJ}-{n}` and `body`: (exit code, stdout, stderr).
fn send_env(env: &Env, h: &Harness, key: &str, to: &str, n: u32, body: &str) -> (i32, String, String) {
    send_subject(env, &h.paths, key, to, &format!("{SUBJ}-{n}"), body)
}
/// `send_cli` in `env` on the machine of `paths` from `key` with `subject` and `body`: (exit code, stdout, stderr).
fn send_subject(env: &Env, paths: &Paths, key: &str, to: &str, subject: &str, body: &str) -> (i32, String, String) {
    let ws = zygo_workspace::state::Store::open(paths, WS_ID).read().unwrap().unwrap();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = send_cli(env, &ws, Some(key), to, subject, body, &cfg_of(paths), &mut o, &mut e);
    (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
}
/// A send on the machine of `paths` that must pass ([`send_body`]), with no seam; returns `<id>`. A seam closure, which
/// holds no [`Harness`], sends with it (#16).
fn send_paths(paths: &Paths, key: &str, to: &str, n: u32) -> String {
    let env = env_paths(paths.clone(), Box::new(NoSeam), far());
    let (code, out, err) = send_subject(&env, paths, key, to, &format!("{SUBJ}-{n}"), &format!("{BODY}-{n}"));
    sent_id(code, &out, &err, to)
}
/// The `<id>` of a send that passed: exit 0, no stderr, stdout `sent <id> to <to>`.
fn sent_id(code: i32, out: &str, err: &str, to: &str) -> String {
    assert_eq!((code, err), (0, ""), "{out}");
    let id = out.strip_prefix("sent ").and_then(|s| s.strip_suffix(&format!(" to {to}\n"))).unwrap_or_else(|| panic!("{out}"));
    id.to_string()
}
/// [`send_env`] with no seam.
fn send_with(h: &Harness, key: &str, to: &str, n: u32, body: &str) -> (i32, String, String) {
    send_env(&env_in(h, Box::new(NoSeam), far()), h, key, to, n, body)
}
fn send(h: &Harness, key: &str, to: &str, n: u32) -> (i32, String, String) { send_with(h, key, to, n, &format!("{BODY}-{n}")) }

/// A send that must pass: exit 0, no stderr, stdout `sent <id> to <to>`; returns `<id>`.
fn send_body(h: &Harness, key: &str, to: &str, n: u32, body: &str) -> String {
    let (code, out, err) = send_with(h, key, to, n, body);
    sent_id(code, &out, &err, to)
}
fn send_ok(h: &Harness, key: &str, to: &str, n: u32) -> String { send_body(h, key, to, n, &format!("{BODY}-{n}")) }

/// The MCP mail tools of member `key` of `h` (author's call 1: the spawn seam starts nothing).
fn tools(h: &Harness, key: &str) -> Vec<Box<dyn Tool>> { tools_spawn(h, key, Box::new(NoSpawn)) }
/// [`tools`] with the spawn seam `spawner`.
fn tools_spawn(h: &Harness, key: &str, spawner: Box<dyn Spawner>) -> Vec<Box<dyn Tool>> {
    let sd = SessionDir::new(h.root().join(format!("sess-{key}")));
    sd.create().unwrap();
    SessionView {
        mail: true,
        workspace: Some(ViewWorkspace { member: Member { ws_id: WS_ID.into(), pane: key.into(), ..Default::default() }, ..Default::default() }),
        ..Default::default()
    }.save(&sd).unwrap();
    let ctx = McpCtx { agent: Agent::ClaudeCode, features: Features { mail: true, ..Default::default() }, session_dir: Some(sd) };
    tools_with_sync(&ctx, h.paths.clone(), cfg(h).mail, Arc::new(t0),
        Arc::new(|_: &Paths, _: &Workspace, _: &[String], _: &MailConfig| {}), spawner, far())
}
fn call(t: &[Box<dyn Tool>], name: &str, args: Value) -> Result<Value, ToolError> {
    t.iter().find(|t| t.def().name == name).unwrap_or_else(|| panic!("no tool {name}")).call(args)
}

/// The tip of `refs/heads/zygo/mail` of the bare repository.
fn tip(r: &Remote) -> String { git(&r.bare, &["rev-parse", "refs/heads/zygo/mail"]) }
/// The paths at the tip; empty when the branch does not exist.
fn tip_paths(r: &Remote) -> Vec<String> {
    let o = git_cmd().args(["ls-tree", "-r", "--name-only", "refs/heads/zygo/mail"]).current_dir(&r.bare).output().unwrap();
    if !o.status.success() {
        return Vec::new();
    }
    out_of(&o).lines().map(String::from).collect()
}
/// The bytes of `<tip>:<path>`; `None` when the path is absent at the tip.
fn tip_blob(r: &Remote, path: &str) -> Option<Vec<u8>> {
    let o = git_cmd().args(["cat-file", "blob", &format!("{}:{path}", tip(r))]).current_dir(&r.bare).output().unwrap();
    o.status.success().then_some(o.stdout)
}

/// A commit of `tree` on the tip of `refs/heads/zygo/mail` of `bare`, made the tip; returns its id (the caller puts
/// it in `made`).
fn test_commit(bare: &Path, tree: &str) -> String {
    let tip = git(bare, &["rev-parse", "refs/heads/zygo/mail"]);
    let c = git(bare, &["commit-tree", tree, "-p", &tip, "-m", "test commit"]);
    git(bare, &["update-ref", "refs/heads/zygo/mail", &c]);
    c
}

/// A move of the branch: a test commit with the tip's tree (as `crates/zygo-mail-sync/tests/common/mod.rs:294-301`).
fn advance(bare: &Path) -> String {
    let tree = git(bare, &["rev-parse", "refs/heads/zygo/mail^{tree}"]);
    test_commit(bare, &tree)
}

/// #17: `n` new empty commits on `refs/heads/zygo/mail` of `bare` with one `git fast-import` (one process), each with
/// the tree of the one before and that one as parent, the first on the tip (as `crates/zygo-mail-sync/tests/prune.rs`
/// `grow`). Returns the new commits (the caller puts them in `made`).
fn grow(bare: &Path, n: u64) -> Vec<String> {
    let old = git(bare, &["rev-parse", "refs/heads/zygo/mail"]);
    let mut s = String::new();
    for i in 0..n {
        s.push_str("commit refs/heads/zygo/mail\ncommitter t <t@t.invalid> 1700000000 +0000\ndata 0\n");
        if i == 0 {
            s.push_str(&format!("from {old}\n"));
        }
        s.push('\n');
    }
    git_in(bare, &["fast-import", "--quiet"], s.as_bytes());
    let new = git(bare, &["rev-parse", "refs/heads/zygo/mail"]);
    git(bare, &["rev-list", &new, &format!("^{old}")]).lines().map(String::from).collect()
}

/// The commit count of `refs/heads/zygo/mail` of the bare repository.
fn count(r: &Remote) -> u64 { git(&r.bare, &["rev-list", "--count", "refs/heads/zygo/mail"]).parse().unwrap() }

/// A test commit on the tip with the new top-level folder `folder` (not at the tip) of the `git mktree` lines `lines`;
/// returns its id.
fn add_folder(bare: &Path, folder: &str, lines: &[String]) -> String {
    let sub = git_in(bare, &["mktree", "--missing"], format!("{}\n", lines.join("\n")).as_bytes());
    let mut top: Vec<String> = git(bare, &["ls-tree", "refs/heads/zygo/mail"]).lines().map(String::from).collect();
    assert!(!top.iter().any(|l| l.ends_with(&format!("\t{folder}"))), "{folder} is at the tip");
    top.push(format!("040000 tree {sub}\t{folder}"));
    let tree = git_in(bare, &["mktree", "--missing"], format!("{}\n", top.join("\n")).as_bytes());
    test_commit(bare, &tree)
}

/// The M§5 version 1 JSON of a message `id` from `from` to `to` with `subject` and the body `{BODY}-<id>` (as the
/// messages of [`hostile`]).
fn message(id: &str, from: &str, to: &str, subject: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"version": 1, "id": id, "from": from, "to": to, "sent_at": "2026-11-25T10:00:00Z",
        "subject": subject, "body": format!("{BODY}-{id}")})).unwrap()
}

/// The sorted `<ULID>` names of the files in `mail/<key>/new/` of `h`; empty when the folder is missing.
fn new_ids(h: &Harness, key: &str) -> Vec<String> {
    let Ok(d) = std::fs::read_dir(mail_dir(&h.paths, WS_ID).join(key).join("new")) else { return Vec::new() };
    let mut ids: Vec<String> = d.map(|e| e.unwrap().file_name().into_string().unwrap())
        .map(|n| n.strip_suffix(".json").map(String::from).unwrap_or(n)).collect();
    ids.sort();
    ids
}

/// Author's call 5: `zygo hook workspace --event <event>` in process for the session folder `sd` of `h`, with the sync
/// seams of `env` and a `zygo` program that does not exist.
fn hook(h: &Harness, sd: &SessionDir, event: &str, payload: &Value, env: &Env) -> Option<String> {
    let bytes = serde_json::to_vec(payload).unwrap();
    let root = sd.root.clone().into_os_string();
    run_with_sync(event, &bytes, &h.paths, &|k| (k == "ZYGO_SESSION_DIR").then(|| root.clone()), &h.root().join("missing"), None, env)
        .unwrap()
}

/// The entries of `git worktree list --porcelain` in `repo` (as `crates/zygo/tests/mail_sync_forget.rs`).
fn worktrees(repo: &Path) -> Vec<String> {
    git(repo, &["worktree", "list", "--porcelain"]).split("\n\n").map(String::from).collect()
}

/// #2: the parts of the user repository that the sync must not change (S-G2; as
/// `crates/zygo-mail-sync/tests/sync_pass.rs` `snap`).
#[derive(Debug, PartialEq)]
struct Snap {
    head: String,
    branches: String,
    status: String,
    index: Vec<u8>,
    fetch_head: Option<Vec<u8>>,
    packs: BTreeSet<String>,
    worktrees: String,
}
fn snap(root: &Path) -> Snap {
    let packs = std::fs::read_dir(root.join(".git/objects/pack"))
        .map(|d| d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect())
        .unwrap_or_default();
    Snap {
        head: git(root, &["rev-parse", "HEAD"]),
        branches: git(root, &["for-each-ref", "refs/heads"]),
        status: git(root, &["status", "--porcelain"]),
        index: std::fs::read(root.join(".git/index")).unwrap(),
        fetch_head: std::fs::read(root.join(".git/FETCH_HEAD")).ok(),
        packs,
        worktrees: git(root, &["worktree", "list", "--porcelain"]),
    }
}

/// S§19 #12: six hostile entries in `bob-q7x@<host>/` on top of the tip: a link, a gitlink, a 9 MiB blob, a message
/// whose id is not its file's, a message to another name, and a file that is not JSON. Returns the six ids and the
/// commit id.
fn hostile(bare: &Path, host: &str) -> ([String; 6], String) {
    let ids: [String; 6] = std::array::from_fn(|i| format!("01KQ7X0000000000000000000{}", i + 1));
    let msg = |id: &str, to: &str| {
        serde_json::to_vec(&json!({"version": 1, "id": id, "from": format!("{A_NAME}@{HOST_A}"), "to": to,
            "sent_at": "2026-11-25T10:00:00Z", "subject": format!("{SUBJ}-h"), "body": format!("{BODY}-h")})).unwrap()
    };
    let blob = |b: &[u8]| git_in(bare, &["hash-object", "-w", "--stdin"], b);
    let entries = [
        ("120000", "blob", blob(b"/etc/passwd")),
        ("160000", "commit", "0123456789abcdef0123456789abcdef01234567".to_string()),
        ("100644", "blob", blob(&vec![b' '; 9 * 1024 * 1024])),
        ("100644", "blob", blob(&msg("01KQ7X00000000000000000099", &format!("{B_NAME}@{host}")))),
        ("100644", "blob", blob(&msg(&ids[4], &format!("eve@{host}")))),
        ("100644", "blob", blob(b"{not json")),
    ];
    let lines: Vec<String> = entries.iter().zip(&ids).map(|((mode, kind, id), name)| format!("{mode} {kind} {id}\t{name}.json")).collect();
    (ids.clone(), add_entries(bare, &format!("{B_NAME}@{host}"), &lines))
}

/// A test commit on the tip whose top-level folder `folder` (at the tip) also holds the `git mktree` lines `lines`;
/// returns its id.
fn add_entries(bare: &Path, folder: &str, lines: &[String]) -> String {
    let mut all = git(bare, &["ls-tree", &format!("refs/heads/zygo/mail:{folder}")]);
    for l in lines {
        all.push_str(&format!("\n{l}"));
    }
    let sub = git_in(bare, &["mktree", "--missing"], format!("{}\n", all.trim_start()).as_bytes());
    let top: Vec<String> = git(bare, &["ls-tree", "refs/heads/zygo/mail"]).lines()
        .map(|l| if l.ends_with(&format!("\t{folder}")) { format!("040000 tree {sub}\t{folder}") } else { l.to_string() })
        .collect();
    let tree = git_in(bare, &["mktree", "--missing"], format!("{}\n", top.join("\n")).as_bytes());
    test_commit(bare, &tree)
}

/// FX-26 re-plan 2 review round 2 F1: each entry under `root` (`root` left out, links not followed, git object stores
/// left out as in [`marked_files`]) with its kind and its content: the bytes of a file, the target of a link, nothing
/// for a folder. A read error fails with the path.
fn tree_state(root: &Path) -> BTreeMap<PathBuf, (char, Vec<u8>)> {
    let mut all = BTreeMap::new();
    for e in walk(root, "snapshot") {
        let rel = e.path().strip_prefix(root).unwrap();
        if e.depth() == 0 || in_object_store(rel) {
            continue;
        }
        let t = e.file_type();
        let v = if t.is_file() {
            ('f', read_file(e.path(), "snapshot"))
        } else if t.is_symlink() {
            let to = std::fs::read_link(e.path()).unwrap_or_else(|err| panic!("{}: the snapshot cannot read it ({err})", e.path().display()));
            ('l', to.into_os_string().into_encoded_bytes())
        } else {
            ('d', Vec::new())
        };
        all.insert(e.path().to_path_buf(), v);
    }
    all
}

/// F1: each path that is new, changed (kind or content) or gone in `after` against `before`, sorted.
fn changed(before: &BTreeMap<PathBuf, (char, Vec<u8>)>, after: &BTreeMap<PathBuf, (char, Vec<u8>)>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = after.iter().filter(|(p, v)| before.get(*p) != Some(*v)).map(|(p, _)| p.clone()).collect();
    out.extend(before.keys().filter(|p| !after.contains_key(*p)).cloned());
    out.sort();
    out
}

/// S§19 #12, registry half: `registry/<HOST_R1>.json` with 257 names and `registry/<HOST_R2>.json` whose `host` is
/// `HOST_R3`, each of a valid form otherwise. Returns their blob ids and the commit id.
fn bad_registries(bare: &Path) -> (String, String, String) {
    let reg = |host: &str, members: Vec<String>| {
        serde_json::to_vec(&json!({"version": 1, "host": host, "identity": "0".repeat(32), "members": members})).unwrap()
    };
    let many = git_in(bare, &["hash-object", "-w", "--stdin"], &reg(HOST_R1, (0..257).map(|i| format!("{A_NAME}-{i:03}")).collect()));
    let other = git_in(bare, &["hash-object", "-w", "--stdin"], &reg(HOST_R3, vec![B_NAME.into()]));
    let c = add_entries(bare, "registry", &[format!("100644 blob {many}\t{HOST_R1}.json"), format!("100644 blob {other}\t{HOST_R2}.json")]);
    (many, other, c)
}

/// S§7.5 Own hosts of `h` at the tip of `r`: `host` (its current host), then each other host whose ack at the tip
/// holds the mail identity of `h`, in path order.
fn own_hosts_at_tip(r: &Remote, h: &Harness, host: &str) -> Vec<String> {
    let id = zygo_mail_sync::state::identity(&sp(h)).unwrap().unwrap();
    let mut own = vec![host.to_string()];
    for p in tip_paths(r) {
        let Some(k) = p.strip_prefix("ack/").and_then(|x| x.strip_suffix(".json")) else { continue };
        let ack: Value = serde_json::from_slice(&tip_blob(r, &p).unwrap()).unwrap();
        if k != host && ack["identity"] == id.as_str() {
            own.push(k.to_string());
        }
    }
    own
}

/// The unread count of member `key` of `h`.
/// FX-26 R3-1: the maildir folder `mail/<key>/<sub>/` of `h` (`new` or `cur`), for [`Scanned::folders`].
fn maildir(h: &Harness, key: &str, sub: &str) -> PathBuf { mail_dir(&h.paths, WS_ID).join(key).join(sub) }
fn new_count(h: &Harness, key: &str) -> usize { store::unread(&mail_dir(&h.paths, WS_ID), key).unwrap() }

/// One finding `"<what> holds <marker>"` per marker of `markers` that occurs in `bytes` (not per occurrence), in
/// `markers` order; empty when clean.
fn text_findings(what: &str, bytes: &[u8], markers: &[&str]) -> Vec<String> {
    markers.iter()
        .filter(|m| bytes.windows(m.len()).any(|w| w == m.as_bytes()))
        .map(|m| format!("{what} holds {m}"))
        .collect()
}

/// `workspace.json` at `path`: each `members.*.name` and `panes[].name` whose value is in `own` is the record's own
/// member name (M§6) and is removed; the rest is scanned for every marker.
fn workspace_findings(path: &Path, own: &[&str]) -> Vec<String> {
    // FX-26 re-plan review round 2 F4: a read error fails the scan with the path.
    let mut v: Value = serde_json::from_slice(&read_file(path, "scan")).unwrap();
    let own_name = |x: &Value| x.as_str().is_some_and(|s| own.contains(&s));
    if let Some(members) = v.get_mut("members").and_then(Value::as_object_mut) {
        for m in members.values_mut().filter_map(Value::as_object_mut) {
            if m.get("name").is_some_and(own_name) {
                m.remove("name");
            }
        }
    }
    if let Some(panes) = v.get_mut("panes").and_then(Value::as_array_mut) {
        for p in panes.iter_mut().filter_map(Value::as_object_mut) {
            if p.get("name").is_some_and(own_name) {
                p.remove("name");
            }
        }
    }
    text_findings(&path.display().to_string(), &serde_json::to_vec(&v).unwrap(), &MARKERS)
}

/// `mail/sync/state.json` at `path`: each key of `pushed` and `pending` that is not `registry/<h>.json` or
/// `ack/<h>.json` with `h` in `own_hosts` is a finding (S§7.5); the rest is scanned for every marker.
fn state_findings(path: &Path, own_hosts: &[&str]) -> Vec<String> {
    // FX-26 re-plan review round 2 F4: a read error fails the scan with the path.
    let mut v: Value = serde_json::from_slice(&read_file(path, "scan")).unwrap();
    let what = path.display().to_string();
    let own = |k: &str| own_hosts.iter().any(|h| k == format!("registry/{h}.json") || k == format!("ack/{h}.json"));
    let mut found = Vec::new();
    let obj = v.as_object_mut().unwrap();
    for field in ["pushed", "pending"] {
        if let Some(Value::Object(m)) = obj.remove(field) {
            found.extend(m.keys().filter(|k| !own(k)).map(|k| format!("{what} key {k} of {field} is not an own file")));
        }
    }
    found.extend(text_findings(&what, &serde_json::to_vec(&v).unwrap(), &MARKERS));
    found
}

/// Every commit object of `repo` (unreachable ones too, so a pruned history counts): the whole object (header and
/// message) is scanned for every marker; a commit not in `test_made` must have the message [`MESSAGE`] and the
/// author and committer [`ZYGO_ID`] (S§6.5). Returns the count of commits checked against S§6.5 (those not in
/// `test_made`) and the findings.
fn commit_findings(repo: &Path, test_made: &BTreeSet<String>) -> (usize, Vec<String>) {
    let all = git(repo, &["cat-file", "--batch-all-objects", "--batch-check=%(objecttype) %(objectname)"]);
    let (mut checked, mut found) = (0, Vec::new());
    // MS-22 review F3: a tag object (zygo writes none) is scanned whole for every marker.
    for id in all.lines().filter_map(|l| l.strip_prefix("tag ")) {
        let o = git_cmd().args(["cat-file", "tag", id]).current_dir(repo).output().unwrap();
        assert!(o.status.success(), "cat-file tag {id}: {}", err_of(&o));
        found.extend(text_findings(&format!("{} tag {id}", repo.display()), &o.stdout, &MARKERS));
    }
    for id in all.lines().filter_map(|l| l.strip_prefix("commit ")) {
        let o = git_cmd().args(["cat-file", "commit", id]).current_dir(repo).output().unwrap();
        assert!(o.status.success(), "cat-file commit {id}: {}", err_of(&o));
        let what = format!("{} commit {id}", repo.display());
        found.extend(text_findings(&what, &o.stdout, &MARKERS));
        if test_made.contains(id) {
            continue;
        }
        checked += 1;
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let (header, message) = text.split_once("\n\n").unwrap_or((&text, ""));
        if message != MESSAGE {
            found.push(format!("{what} has the message {message:?}, not {MESSAGE:?}"));
        }
        for role in ["author", "committer"] {
            let ok = header.lines().any(|l| l.starts_with(&format!("{role} {ZYGO_ID} ")));
            if !ok {
                found.push(format!("{what} has no {role} {ZYGO_ID}"));
            }
        }
    }
    (checked, found)
}

/// `mail/sync/imported`, `identity`, `last_start` and `incoming/` of `h` (MS-22 review F3).
fn sync_files(h: &Harness) -> Vec<PathBuf> {
    let s = sp(h);
    vec![s.imported, s.identity, s.last_start, s.incoming]
}

/// The content markers: the text of a message. S-C2: the sync worktree and its index hold paths with host and member
/// names by design (they are the synced tree), so its index is scanned for these only.
const CONTENT: [&str; 2] = [SUBJ, BODY];

/// The git folder of `repo`: `repo/.git` for a checkout, `repo` for a bare repository.
fn git_dir(repo: &Path) -> PathBuf {
    let g = repo.join(".git");
    if g.is_dir() { g } else { repo.to_path_buf() }
}

/// FX-26 re-plan review round 1 F6: each entry under `root` (`root` among them, links not followed). No walk error is
/// dropped: an entry or a folder that cannot be read fails the `what` (`scan` or `sweep`) with its path. A `root`
/// that does not exist gives no entry.
fn walk(root: &Path, what: &str) -> Vec<walkdir::DirEntry> {
    match std::fs::symlink_metadata(root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => panic!("{}: the {what} cannot read it ({e})", root.display()),
        Ok(_) => {}
    }
    walkdir::WalkDir::new(root).follow_links(false).into_iter()
        .map(|e| e.unwrap_or_else(|err| panic!("{}: the {what} cannot read it ({err})", err.path().unwrap_or(root).display())))
        .collect()
}

/// F6: the bytes of the file `path`; a read error fails the `what` with the path.
fn read_file(path: &Path, what: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: the {what} cannot read it ({e})", path.display()))
}

/// Each file of `paths` that exists, and each file under each folder of `paths` (links not followed): its path below
/// the scanned path, then its bytes, scanned for `markers`. Returns the count of files read and the findings. A path
/// that does not exist is skipped; any other read or walk error fails the scan with its path (F6).
fn files_findings(paths: &[PathBuf], markers: &[&str]) -> (usize, Vec<String>) {
    let (mut n, mut found) = (0, Vec::new());
    for root in paths {
        for e in walk(root, "scan") {
            if !e.file_type().is_file() {
                continue;
            }
            let mut bytes = e.path().strip_prefix(root).unwrap().as_os_str().as_encoded_bytes().to_vec();
            bytes.push(b'\n');
            bytes.extend(read_file(e.path(), "scan"));
            n += 1;
            found.extend(text_findings(&e.path().display().to_string(), &bytes, markers));
        }
    }
    (n, found)
}

/// S-C2 (MS-22 review F3): the reflogs of `repo` (`logs/**`) and the admin files of each of its linked worktrees
/// (`worktrees/<name>/**`, its reflogs among them), for every marker, but the index of a linked worktree for the
/// [`CONTENT`] markers only. Returns what it read (MS-22 review R2 F-B) and the findings.
fn git_meta_findings(repo: &Path) -> (GitMeta, Vec<String>) {
    let gd = git_dir(repo);
    let (logs, mut found) = files_findings(&[gd.join("logs")], &MARKERS);
    let mut read = GitMeta { logs, admins: 0, indexes: 0 };
    let admins = gd.join("worktrees");
    let rd = match std::fs::read_dir(&admins) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (read, found),
        r => r.unwrap_or_else(|e| panic!("{}: the scan cannot read it ({e})", admins.display())),
    };
    for w in rd.map(|e| e.unwrap_or_else(|e| panic!("{}: the scan cannot read it ({e})", admins.display())).path()) {
        read.admins += 1;
        let index = w.join("index");
        if index.is_file() {
            read.indexes += 1;
            found.extend(text_findings(&index.display().to_string(), &read_file(&index, "scan"), &CONTENT));
        }
        // F6: no walk error of the admin folder is dropped.
        let rest: Vec<PathBuf> = walk(&w, "scan").into_iter()
            .filter(|e| e.file_type().is_file() && e.path() != index)
            .map(|e| e.into_path())
            .collect();
        found.extend(files_findings(&rest, &MARKERS).1);
    }
    (read, found)
}

/// What [`git_meta_findings`] read: the files under `logs/`, the linked-worktree admin folders, and their index files.
#[derive(Debug, PartialEq)]
struct GitMeta { logs: usize, admins: usize, indexes: usize }

/// Every ref name that `git for-each-ref` lists from the worktree (or bare repository) `dir`, its per-worktree refs
/// among them, and the ref that its `HEAD` names (MS-22 review F3), scanned for every marker. A detached `HEAD` names
/// none. Returns the count of refs that `for-each-ref` listed and the findings.
fn ref_findings(dir: &Path) -> (usize, Vec<String>) {
    let refs = git(dir, &["for-each-ref", "--format=%(refname)"]);
    let mut found: Vec<String> = refs.lines()
        .flat_map(|r| text_findings(&format!("ref {r}"), r.as_bytes(), &MARKERS))
        .collect();
    let o = git_cmd().args(["symbolic-ref", "-q", "HEAD"]).current_dir(dir).output().unwrap();
    let head = out_of(&o);
    found.extend(text_findings(&format!("HEAD {}", head.trim_end()), head.as_bytes(), &MARKERS));
    (refs.lines().count(), found)
}

/// The usage store at `db`: the row count over all tables, and the findings of every text or blob value of every
/// table and of the raw bytes of `db` and of `<db>-wal` when it exists. An absent file is `(0, [])`. FX-26 re-plan
/// review round 2 F4: any other read error, of the files or of the database, fails the scan with the path.
fn usage_findings(db: &Path) -> (usize, Vec<String>) {
    let absent = |p: &Path| std::fs::symlink_metadata(p).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
    if absent(db) {
        return (0, vec![]);
    }
    let what = db.display().to_string();
    let raw = read_file(db, "scan");
    let cannot = |e: rusqlite::Error| -> ! { panic!("{what}: the scan cannot read it ({e})") };
    let mut found = Vec::new();
    let mut rows = 0;
    {
        let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap_or_else(|e| cannot(e));
        let tables: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").unwrap_or_else(|e| cannot(e))
            .query_map([], |r| r.get(0)).unwrap_or_else(|e| cannot(e)).map(|r| r.unwrap_or_else(|e| cannot(e))).collect();
        for t in tables {
            let mut st = c.prepare(&format!("SELECT * FROM \"{}\"", t.replace('"', "\"\""))).unwrap_or_else(|e| cannot(e));
            let n = st.column_count();
            let mut q = st.query([]).unwrap_or_else(|e| cannot(e));
            while let Some(r) = q.next().unwrap_or_else(|e| cannot(e)) {
                rows += 1;
                for i in 0..n {
                    let bytes = match r.get_ref(i).unwrap_or_else(|e| cannot(e)) {
                        rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => b,
                        _ => continue,
                    };
                    for f in text_findings(&format!("{what} table {t}"), bytes, &MARKERS) {
                        if !found.contains(&f) {
                            found.push(f);
                        }
                    }
                }
            }
        }
    }
    found.extend(text_findings(&what, &raw, &MARKERS));
    let wal = PathBuf::from(format!("{what}-wal"));
    if !absent(&wal) {
        found.extend(text_findings(&wal.display().to_string(), &read_file(&wal, "scan"), &MARKERS));
    }
    (rows, found)
}

/// One machine that [`scan`] reads: its label in a finding, its own member names (M§6, `workspace.json`), its own hosts
/// (S§7.5, the keys of `state.json`), whether it synced (then each of its stores must be there), whether the walk
/// expects its `errors.log` (FX-26 review F5: a walk that sets `false` names why in a comment), and whether its
/// repository holds zygo commits (then the scan checks at least one against S§6.5, whatever its refs). FX-26 R3-1:
/// `folders` names each folder kind that the walk knows holds a file on this machine (the maildir folder of each
/// member that the walk delivered to, the outbox or `refused/` when a message waits there, each `sess-*` folder that
/// the walk made, and `results/` when the walk wrote a result); the sweep must read a file in each.
struct Scanned<'a> {
    label: &'a str,
    h: &'a Harness,
    own_names: Vec<&'a str>,
    own_hosts: Vec<String>,
    synced: bool,
    errors_log: bool,
    zygo_commits: bool,
    folders: Vec<PathBuf>,
}

/// The tip of the zygo branch ref of `repo` (`refs/heads/zygo/mail` in a bare repository,
/// `refs/remotes/origin/zygo/mail` in a checkout); `None` when the ref does not exist.
fn zygo_ref(repo: &Path, bare: bool) -> Option<String> {
    let name = if bare { "refs/heads/zygo/mail" } else { "refs/remotes/origin/zygo/mail" };
    let o = git_cmd().args(["rev-parse", "--verify", "-q", name]).current_dir(repo).output().unwrap();
    o.status.success().then(|| out_of(&o).trim_end().to_string())
}

/// The session logs `<root>/sess-*/session.log` of `h` that exist. FX-26 re-plan review round 2 F4: a home that cannot
/// be listed, or a `session.log` path that cannot be looked up (other than a missing one), fails the scan with its path.
fn session_logs(h: &Harness) -> Vec<PathBuf> {
    let root = h.root();
    let cannot = |p: &Path, e: std::io::Error| -> ! { panic!("{}: the scan cannot read it ({e})", p.display()) };
    let mut logs = Vec::new();
    for e in std::fs::read_dir(root).unwrap_or_else(|e| cannot(root, e)) {
        let e = e.unwrap_or_else(|e| cannot(root, e));
        if !e.file_name().to_string_lossy().starts_with("sess-") || e.file_type().unwrap_or_else(|err| cannot(&e.path(), err)).is_file() {
            continue;
        }
        let log = e.path().join("session.log");
        match std::fs::metadata(&log) {
            Ok(m) if m.is_file() => logs.push(log),
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => cannot(&log, err),
        }
    }
    logs.sort();
    logs
}

/// Step 2: the scans of S§19 #24 after a walk. For each machine of `ms`: `errors.log`, `workspace.json`, `state.json`
/// (when it exists), the refs of the sync worktree, the sync files, each session log and the usage store. For each bare
/// repository of `remotes` and each machine repository: every commit and tag ([`commit_findings`]), every ref name
/// ([`ref_findings`]), and the reflogs and linked-worktree admin files ([`git_meta_findings`]). A synced machine must
/// have each store; a repository whose zygo branch ref is a commit not in `made` must have a zygo commit checked.
/// Returns the findings.
fn scan(remotes: &[&Remote], made: &BTreeSet<String>, ms: &[Scanned]) -> Vec<String> {
    let mut found = Vec::new();
    for m in ms {
        let (h, s) = (m.h, sp(m.h));
        // FX-26 review F5: no scan of an expected `errors.log` reads nothing, and a machine that the walk records as
        // writing none has none.
        let log = h.store(WS_ID).dir().join("errors.log");
        assert_eq!(log.is_file(), m.errors_log, "{}: errors.log", m.label);
        found.extend(text_findings(&format!("{} errors.log", m.label), errors(h).as_bytes(), &MARKERS));
        let ws = h.store(WS_ID).dir().join("workspace.json");
        assert!(!m.synced || ws.is_file(), "{}: no workspace.json", m.label);
        if ws.is_file() {
            found.extend(workspace_findings(&ws, &m.own_names));
        }
        assert!(!m.synced || s.state.is_file(), "{}: no state.json", m.label);
        if s.state.is_file() {
            if m.synced {
                // FX-26 re-plan 2 review round 3 F5: a read error fails the scan with the path.
                let st: Value = serde_json::from_slice(&read_file(&s.state, "scan")).unwrap();
                assert!(!st["pushed"].as_object().unwrap().is_empty(), "{}: {st}", m.label);
            }
            let own: Vec<&str> = m.own_hosts.iter().map(String::as_str).collect();
            found.extend(state_findings(&s.state, &own));
        }
        assert!(!m.synced || s.worktree.is_dir(), "{}: no sync worktree", m.label);
        if s.worktree.is_dir() {
            let (refs, f) = ref_findings(&s.worktree);
            assert!(refs >= 1, "{}: no ref read", s.worktree.display());
            found.extend(f);
        }
        let (n, f) = files_findings(&sync_files(h), &MARKERS);
        assert!(!m.synced || n >= 2, "{}: imported and identity", m.label);
        found.extend(f);
        found.extend(files_findings(&session_logs(h), &MARKERS).1);
        found.extend(usage_findings(&h.paths.usage_db).1);
        found.extend(sweep_machine(m));
    }
    for r in remotes {
        found.extend(sweep_remote(r));
    }
    let repos = remotes.iter().map(|r| (r.bare.clone(), true, false, false))
        .chain(ms.iter().map(|m| (m.h.repo(), false, m.synced, m.zygo_commits)));
    for (repo, bare, synced, zygo_commits) in repos {
        // MS-22 review F4: each scan read something in each repository.
        let (checked, f) = commit_findings(&repo, made);
        let tip = zygo_ref(&repo, bare);
        assert!(!synced || tip.is_some(), "{}: no zygo branch fetched", repo.display());
        // FX-26 review F5: a machine repository that holds zygo commits has one checked, with or without its ref; a
        // bare repository whose branch tip is a zygo commit too.
        if zygo_commits || tip.is_some_and(|t| !made.contains(&t)) {
            assert!(checked >= 1, "{}: no zygo commit checked", repo.display());
        }
        found.extend(f);
        let (refs, f) = ref_findings(&repo);
        assert!(refs >= 1, "{}: no ref read", repo.display());
        found.extend(f);
        // MS-22 review F3: the reflogs (the sync worktree's among them) and the linked worktrees' admin files.
        let (read, f) = git_meta_findings(&repo);
        assert!(bare || read.logs >= 1, "{}: no reflog", repo.display());
        // MS-22 review R2 F-B: a synced machine has a sync worktree, so the scan read its admin folder and its index.
        if synced {
            assert!(read.admins >= 1 && read.indexes >= 1, "{}: {read:?}", repo.display());
        }
        found.extend(f);
    }
    found
}

/// The markers that occur in `bytes`.
fn markers_in(bytes: &[u8]) -> Vec<&'static str> {
    MARKERS.iter().copied().filter(|m| bytes.windows(m.len()).any(|w| w == m.as_bytes())).collect()
}

/// The markers that occur in the file `path`. FX-26 re-plan review round 1 F6: a file that cannot be read fails the
/// sweep, and the failure names the file.
fn file_markers(path: &Path) -> Vec<&'static str> { markers_in(&read_file(path, "sweep")) }

/// F7: the markers in the name of the entry `e` and, for a link, in its target.
fn name_markers(e: &walkdir::DirEntry) -> Vec<&'static str> {
    let mut bytes = e.file_name().as_encoded_bytes().to_vec();
    if e.path_is_symlink() {
        let target = std::fs::read_link(e.path())
            .unwrap_or_else(|err| panic!("{}: the sweep cannot read it ({err})", e.path().display()));
        bytes.push(b'\n');
        bytes.extend(target.as_os_str().as_encoded_bytes());
    }
    markers_in(&bytes)
}

/// What [`marked_files`] found: each file whose bytes hold a marker, and each entry (file, folder or link) whose name
/// or link target holds one.
#[derive(Debug)]
struct Marked {
    files: Vec<(PathBuf, Vec<&'static str>)>,
    names: Vec<(PathBuf, Vec<&'static str>)>,
}

/// FX-26 review round 2 F-4: `rel` (a path below the swept root) is inside a git object store: under a folder
/// `objects` whose parent folder is `.git` (a checkout) or `<name>.git` (a bare repository). A folder `objects`
/// anywhere else is not left out.
fn in_object_store(rel: &Path) -> bool {
    let c: Vec<&OsStr> = rel.iter().collect();
    c.windows(3).any(|w| w[1] == "objects" && w[0].as_encoded_bytes().ends_with(b".git"))
}

/// Each file under `root` (links not followed; git object stores left out, as [`commit_findings`] reads every commit
/// and tag) that holds a marker: its path and the markers. FX-26 review round 2 F-4: the sweep read each file of
/// `known` (the files that the walk knows exist under `root`, at least one), so a sweep that reads nothing fails.
/// FX-26 R3-1: an entry or a folder that the walk cannot read fails the sweep with its path (no error is dropped), and
/// the sweep read at least one file in each folder of `dirs` (the folders that the walk knows hold a file).
/// FX-26 re-plan review round 1: a file that cannot be read fails the sweep with its path (F6); the name of each entry
/// below `root` (object stores among them) and the target of each link are checked for markers too (F7).
fn marked_files(root: &Path, known: &[PathBuf], dirs: &[PathBuf]) -> Marked {
    assert!(!known.is_empty(), "{}: no known file", root.display());
    let mut read = BTreeSet::new();
    let mut marked = Marked { files: Vec::new(), names: Vec::new() };
    for e in walk(root, "sweep") {
        if e.depth() > 0 {
            let hits = name_markers(&e);
            if !hits.is_empty() {
                marked.names.push((e.path().to_path_buf(), hits));
            }
        }
        if !e.file_type().is_file() || in_object_store(e.path().strip_prefix(root).unwrap()) {
            continue;
        }
        let hits = file_markers(e.path());
        read.insert(e.path().to_path_buf());
        if !hits.is_empty() {
            marked.files.push((e.into_path(), hits));
        }
    }
    for k in known {
        assert!(read.contains(k), "{}: the sweep of {} did not read it ({} files read)", k.display(), root.display(), read.len());
    }
    for d in dirs {
        assert!(read.iter().any(|p: &PathBuf| p.starts_with(d)), "{}: the sweep of {} read no file in it", d.display(), root.display());
    }
    marked
}

/// FX-26 review F5: the whole scratch home of `m`. Each file that holds a marker must be in the S-C2 allowed set: the
/// index of the sync worktree, a maildir file (`mail/<key>/{new,cur,tmp}/`), the outbox with `refused/`, `incoming/`,
/// `members.json`, the config inputs (`config.toml`, `team/team.toml`), `workspace.json` with its own member names
/// only, `state.json` with its own hosts only, and the results store `sess-*/results/`. Any other file is a finding.
/// FX-26 re-plan review round 1 F7: so is any file, folder or link whose name or link target holds a marker. FX-26
/// re-plan 2 a (author's call): there is no exception, also not inside the sync worktree folder. S§6.3 makes that
/// worktree with `--no-checkout` and zygo never checks a file out, so production writes no entry there.
fn sweep_machine(m: &Scanned) -> Vec<String> {
    let (h, s) = (m.h, sp(m.h));
    let maildir = mail_dir(&h.paths, WS_ID);
    let ws_json = h.store(WS_ID).dir().join("workspace.json");
    // The admin folder of the sync worktree: the `gitdir:` line of its `.git` file.
    let sync_index = std::fs::read_to_string(s.worktree.join(".git")).ok()
        .and_then(|g| g.strip_prefix("gitdir: ").map(|d| PathBuf::from(d.trim_end()).join("index")));
    // FX-26 review round 2 F-4: the files that the walk knows exist in this home (`imported` is not among them: a
    // synced machine that never received mail has none).
    let mut known = vec![h.paths.config_file.clone()];
    if m.errors_log {
        known.push(h.store(WS_ID).dir().join("errors.log"));
    }
    if m.synced {
        known.extend([ws_json.clone(), s.state.clone(), s.identity.clone(), h.repo().join(".git/HEAD")]);
    }
    let mut found = Vec::new();
    let marked = marked_files(h.root(), &known, &m.folders);
    // F7: a name or a link target that holds a marker is a finding, anywhere in the home. FX-26 re-plan 2 a (author's
    // call): the sync worktree folder has no exception, as S§6.3 keeps it unchecked-out (`--no-checkout`).
    for (p, hits) in marked.names {
        found.push(format!("{} {} has a name or link target that holds {hits:?}", m.label, p.display()));
    }
    for (p, hits) in marked.files {
        // FX-26 re-plan review round 2 F2: `mail/outbox/` is allowed whole below (`s.outbox`), so this rule names
        // only `sync`.
        let in_maildir = p.strip_prefix(&maildir).ok().is_some_and(|rel| {
            let c: Vec<&OsStr> = rel.iter().collect();
            c.len() == 3 && c[0] != "sync" && ["new", "cur", "tmp"].iter().any(|d| c[1] == *d)
        });
        let in_results = p.strip_prefix(h.root()).ok().is_some_and(|rel| {
            let c: Vec<&OsStr> = rel.iter().collect();
            c.len() == 3 && c[0].to_string_lossy().starts_with("sess-") && c[1] == "results"
        });
        let allowed = in_maildir || in_results
            || sync_index.as_ref() == Some(&p)
            || p.starts_with(&s.outbox) || p.starts_with(&s.refused) || p.starts_with(&s.incoming)
            || p == s.members || p == h.paths.config_file || p == h.root().join("team/team.toml")
            || (p == ws_json && hits.iter().all(|x| m.own_names.contains(x)))
            || (p == s.state && hits.iter().all(|x| m.own_hosts.iter().any(|o| o == x)));
        if !allowed {
            found.push(format!("{} {} holds {hits:?} and is not in the S-C2 allowed set", m.label, p.display()));
        }
    }
    found
}

/// FX-26 review F5: the folder of the bare repository `r` with the seed clone that [`remote`] makes. Only a test input
/// may hold a marker there. FX-26 review round 2 F-5: that is the one file `seed/.zygo.toml`, which #25 commits in
/// the seed clone; no file of `bare.git/` (its object store left out) and no other file.
fn sweep_remote(r: &Remote) -> Vec<String> {
    let root = r.bare.parent().unwrap();
    let input = root.join("seed/.zygo.toml");
    let known = [r.bare.join("HEAD"), r.bare.join("config"), root.join("seed/README"), root.join("seed/.git/HEAD")];
    let marked = marked_files(root, &known, &[]);
    marked.names.into_iter()
        .map(|(p, hits)| format!("{} has a name or link target that holds {hits:?}", p.display()))
        .chain(marked.files.into_iter()
            .filter(|(p, _)| *p != input)
            .map(|(p, hits)| format!("{} holds {hits:?} and is not a test input", p.display())))
        .collect()
}

/// FX-26 re-plan review round 2 F7: whether `p` (a file or a folder that a planted case just made unreadable) can still
/// be read. As root it can; then that case's assertion is skipped, and one line says why. CI
/// (`.github/workflows/ci.yml`) runs on GitHub-hosted runners (`ubuntu-latest`, `macos-latest`), whose user is not root
/// (the workflow calls `sudo apt-get`), so there every case runs. FX-26 re-plan 2 review round 1 F7: when the variable
/// `CI` is set and every case was skipped, `scanners_find_a_planted_marker` fails ([`PERMISSION_CASES_RUN`]), so a CI
/// that runs as root cannot pass it without this coverage.
fn still_readable(p: &Path) -> bool {
    let read = match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).is_ok(),
        _ => std::fs::File::open(p).is_ok(),
    };
    if read {
        println!("{}: still readable after its mode change (the tests run as root), so this case's assertion is skipped", p.display());
    }
    read
}

/// The permission cases ([`failure_at_mode`]) whose assertion ran, as the file or folder could not be read.
static PERMISSION_CASES_RUN: AtomicUsize = AtomicUsize::new(0);

/// F7: sets the mode of `p` to `mode`, runs `f` unless `p` stays readable ([`still_readable`]), sets the mode back to
/// `back`, and gives the panic message of `f`; `None` when `f` did not run. A run of `f` that does not fail panics with
/// `what`.
fn failure_at_mode<T>(p: &Path, mode: u32, back: u32, what: &str, f: impl FnOnce() -> T) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    let r = (!still_readable(p)).then(|| {
        PERMISSION_CASES_RUN.fetch_add(1, Ordering::SeqCst);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
    });
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(back)).unwrap();
    r.map(|r| match r {
        Ok(_) => panic!("{what}"),
        Err(e) => e.downcast::<String>().map(|s| *s).unwrap_or_default(),
    })
}

#[test] // author's call 6: each scanner reports a planted marker and passes the allowed form
fn scanners_find_a_planted_marker() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // MS-22 review F5: the hermetic home goes with its guard.
    let home = {
        let _h = hermetic();
        PathBuf::from(std::env::var_os("HOME").unwrap())
    };
    assert!(!home.exists(), "{}: the hermetic home stays after its guard", home.display());
    let _h = hermetic();
    let d = tempfile::tempdir().unwrap();
    assert_eq!(text_findings("errors.log", b"x mail sync: outbox 01J: refused: ship_word\n", &MARKERS), Vec::<String>::new());
    assert_eq!(text_findings("errors.log", format!("x {HOST_B}\n").as_bytes(), &MARKERS), vec![format!("errors.log holds {HOST_B}")]);
    let ws = d.path().join("workspace.json");
    std::fs::write(&ws, serde_json::to_vec(&json!({"members": {ALICE: {"name": A_NAME}}, "panes": [{"name": A_NAME}]})).unwrap()).unwrap();
    assert!(workspace_findings(&ws, &[A_NAME]).is_empty());
    assert_eq!(workspace_findings(&ws, &[]).len(), 1); // one finding per marker, not per occurrence
    std::fs::write(&ws, serde_json::to_vec(&json!({"members": {ALICE: {"name": A_NAME}}, "x": B_NAME})).unwrap()).unwrap();
    assert_eq!(workspace_findings(&ws, &[A_NAME]), vec![format!("{} holds {B_NAME}", ws.display())]);
    let st = d.path().join("state.json");
    std::fs::write(&st, serde_json::to_vec(&json!({"pushed": {format!("registry/{HOST_A}.json"): "0".repeat(40)}, "pending": {}})).unwrap()).unwrap();
    assert!(state_findings(&st, &[HOST_A]).is_empty());
    assert_eq!(state_findings(&st, &[HOST_B]).len(), 1);
    let repo = d.path().join("r");
    git(d.path(), &["init", "-q", "--initial-branch=main", "r"]);
    let tree = git(&repo, &["write-tree"]);
    let c = git(&repo, &["commit-tree", &tree, "-m", &format!("{SUBJ} note")]);
    git(&repo, &["update-ref", &format!("refs/heads/zygo/{B_NAME}"), &c]);
    let (n, f) = commit_findings(&repo, &BTreeSet::new());
    assert_eq!(n, 1); // MS-22 review F4: the count of commits checked against S§6.5
    assert!(f.iter().any(|l| l.ends_with(&format!("holds {SUBJ}"))) && f.iter().any(|l| l.contains("message")), "{f:?}");
    let (n, f) = commit_findings(&repo, &BTreeSet::from([c.clone()]));
    assert_eq!((n, f.len()), (0, 1)); // a test commit is not checked; the marker stays a finding
    // MS-22 review F4: the count of refs read.
    assert_eq!(ref_findings(&repo), (1, vec![format!("ref refs/heads/zygo/{B_NAME} holds {B_NAME}")]));
    let db = d.path().join("usage.db");
    assert_eq!(usage_findings(&db), (0, vec![]));
    zygo_usage::Store::open(&db).unwrap().insert_trim(&zygo_usage::TrimRow { session: BODY.into(), ..Default::default() }).unwrap();
    let (rows, f) = usage_findings(&db);
    assert!(rows >= 1 && f.iter().any(|l| l.ends_with(&format!("holds {BODY}"))), "{rows} {f:?}");

    // MS-22 review F3: a reflog message, a tag object, a linked worktree whose index holds a path with a content
    // marker and a path with names, and a per-worktree ref.
    let r2 = d.path().join("r2");
    git(d.path(), &["init", "-q", "--initial-branch=main", "r2"]);
    std::fs::write(r2.join("f"), "x").unwrap();
    git(&r2, &["add", "f"]);
    git(&r2, &["commit", "-q", "-m", "zygo mail [skip ci]"]);
    let head = git(&r2, &["rev-parse", "HEAD"]);
    git(&r2, &["update-ref", "-m", &format!("{BODY} moved"), "refs/heads/main", &head]);
    git(&r2, &["tag", "-a", "-m", &format!("{SUBJ} tag"), "t1", &head]);
    let wt = d.path().join("wt");
    git(&r2, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()]);
    for name in [format!("{B_NAME}@{HOST_B}"), format!("{SUBJ}.json")] {
        std::fs::write(wt.join(&name), "x").unwrap();
        git(&wt, &["add", &name]);
    }
    git(&wt, &["update-ref", &format!("refs/worktree/{HOST_A}"), &head]);
    let (read, f) = git_meta_findings(&r2);
    assert!(read.logs >= 1, "{f:?}");
    assert_eq!((read.admins, read.indexes), (1, 1), "{f:?}"); // MS-22 review R2 F-B: the one linked worktree
    assert!(f.iter().any(|l| l.contains("/logs/") && l.ends_with(&format!("holds {BODY}"))), "{f:?}");
    assert!(f.iter().any(|l| l.ends_with(&format!("/index holds {SUBJ}"))), "{f:?}");
    assert!(!f.iter().any(|l| l.contains("/index holds ") && !l.ends_with(&format!("holds {SUBJ}"))), "{f:?}");
    let f = commit_findings(&r2, &BTreeSet::new()).1;
    assert!(f.iter().any(|l| l.contains(" tag ") && l.ends_with(&format!("holds {SUBJ}"))), "{f:?}");
    assert!(ref_findings(&wt).1.contains(&format!("ref refs/worktree/{HOST_A} holds {HOST_A}")));
    git(&r2, &["symbolic-ref", "HEAD", &format!("refs/heads/zygo/{HOST_B2}")]); // unborn: no ref lists it
    assert!(ref_findings(&r2).1.contains(&format!("HEAD refs/heads/zygo/{HOST_B2} holds {HOST_B2}")), "{:?}", ref_findings(&r2));
    // The sync files: each file of a folder is read, with its name; a path that is not there is skipped.
    std::fs::write(d.path().join("identity"), format!("x {B_NAME}\n")).unwrap();
    std::fs::create_dir_all(d.path().join("incoming")).unwrap();
    std::fs::write(d.path().join("incoming").join(format!("{HOST_B2}.json")), "x").unwrap();
    std::fs::write(d.path().join("imported"), format!("{SUBJ}\n")).unwrap();
    let (n, f) = files_findings(&[d.path().join("identity"), d.path().join("incoming"), d.path().join("imported"), d.path().join("absent")], &MARKERS);
    assert_eq!(n, 3);
    for m in [B_NAME, HOST_B2, SUBJ] {
        assert!(f.iter().any(|l| l.ends_with(&format!("holds {m}"))), "{m}: {f:?}");
    }

    // FX-26 review F5: the full-folder sweep passes the S-C2 set and reports any other file that holds a marker.
    let h = Harness::no_tmux();
    config(&h, HOST_A, "", "");
    let s = sp(&h);
    std::fs::create_dir_all(s.sync.join("incoming")).unwrap();
    std::fs::create_dir_all(mail_dir(&h.paths, WS_ID).join(ALICE).join("new")).unwrap();
    std::fs::write(mail_dir(&h.paths, WS_ID).join(ALICE).join("new").join("m.json"), SUBJ).unwrap();
    std::fs::write(s.incoming.join("m.json"), BODY).unwrap();
    std::fs::write(&s.state, format!("registry/{HOST_A}.json")).unwrap();
    // FX-26 review round 2 F-6: the allowed form at each boundary. `workspace.json` with its own member name only; the
    // index of the sync worktree (the `gitdir:` of its `.git` file); a file of a git object store, of a checkout and
    // of a bare repository; a file at the top of a results store.
    let put = |p: &Path, text: &str| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    let ws_json = h.store(WS_ID).dir().join("workspace.json");
    put(&ws_json, A_NAME);
    let git_dir = h.root().join("repo/.git");
    let admin = git_dir.join("worktrees/mail-sync");
    put(&s.worktree.join(".git"), &format!("gitdir: {}\n", admin.display()));
    put(&admin.join("index"), B_NAME);
    put(&git_dir.join("objects/ab/cdef"), HOST_B);
    put(&h.root().join("old.git/objects/pack/p.pack"), HOST_B2);
    let results = h.root().join("sess-x/results");
    put(&results.join("r.json"), SUBJ);
    // FX-26 re-plan 2 review round 3 F6: an allowed-side plant for each other allowance, so that a sweep without it
    // reports the plant: a maildir file in `cur/` and in `tmp/`, an outbox file, a `refused/` file, `members.json`,
    // `config.toml` (`config` writes its host, a marker) and `team/team.toml`.
    let alice_dir = mail_dir(&h.paths, WS_ID).join(ALICE);
    put(&alice_dir.join("cur/c.json"), BODY);
    put(&alice_dir.join("tmp/t.json"), HOST_B);
    put(&s.outbox.join("o.json"), B_NAME);
    put(&s.refused.join("r.json"), SUBJ);
    put(&s.members, HOST_B2);
    assert_eq!(file_markers(&h.paths.config_file), vec![HOST_A]);
    put(&h.root().join("team/team.toml"), HOST_R3);
    assert!(s.worktree.starts_with(h.root()), "{}", s.worktree.display());
    let m = Scanned { label: "X", h: &h, own_names: vec![A_NAME], own_hosts: vec![HOST_A.into()], synced: false,
        errors_log: false, zygo_commits: false,
        folders: vec![mail_dir(&h.paths, WS_ID).join(ALICE).join("new"), s.incoming.clone(), h.root().join("sess-x"), results.clone()] };
    assert_eq!(sweep_machine(&m), Vec::<String>::new());
    // Outside the set, each is reported: the review F5 plants, then the FX-26 review round 2 F-6 plants at each
    // boundary (a foreign member name in the swept `workspace.json`, a file of `.git/` that is not the sync index, a
    // file deeper in `results/`, a file under folders named `objects` that are not a git object store).
    let planted = [
        (s.state.clone(), HOST_B), // not an own host
        (h.root().join("stray"), B_NAME),
        (mail_dir(&h.paths, WS_ID).join(ALICE).join("wake.json"), A_NAME), // not a maildir file
        (ws_json.clone(), B_NAME),
        (git_dir.join("COMMIT_EDITMSG"), SUBJ),
        (admin.join("HEAD"), HOST_A2),
        (results.join("deep/r.json"), BODY),
        (h.root().join("objects/o"), HOST_A3),
        (h.root().join("cache/objects/ab/cdef"), HOST_R1),
        (h.root().join("repo/objects/ab/cdef"), HOST_R2), // `objects` in a checkout's work tree, not in `.git/`
        // FX-26 re-plan review round 1 F7: a maildir file one level deeper; a file in the `new/` of `mail/sync/` (the
        // `c[0] != "sync"` rule); a sibling of `team/team.toml`.
        (mail_dir(&h.paths, WS_ID).join(ALICE).join("new/x/f"), HOST_B),
        (s.sync.join("new/f"), HOST_A2),
        (h.root().join("team/other.toml"), HOST_R3),
        // FX-26 re-plan review round 2 F2: one plant past each boundary of the maildir and results rules: a folder of
        // `mail/<key>/` that is not `new`, `cur` or `tmp`; a folder of `sess-x/` that is not `results`; a `results/`
        // whose parent is not a `sess-*` folder.
        (mail_dir(&h.paths, WS_ID).join(ALICE).join("other/f"), HOST_B),
        (h.root().join("sess-x/other/f"), SUBJ),
        (h.root().join("x/results/f"), BODY),
        // FX-26 re-plan 2 review round 3 F6: a marked file beside `config.toml`, in the config folder.
        (h.paths.config_file.with_file_name("other.toml"), HOST_R2),
    ];
    for (p, hit) in &planted {
        put(p, hit);
    }
    // FX-26 re-plan review round 1 F7: a marker in a file name, in a folder name and in a link target, each with no
    // marker in its bytes.
    let named_file = h.root().join(format!("note-{HOST_B}"));
    put(&named_file, "x");
    let named_dir = h.root().join(format!("dir-{HOST_A2}"));
    put(&named_dir.join("f"), "x");
    let link = h.root().join("ln");
    std::os::unix::fs::symlink(format!("/nonexistent/{HOST_A3}"), &link).unwrap();
    let named = [(named_file, HOST_B), (named_dir, HOST_A2), (link, HOST_A3)];
    // FX-26 re-plan 2 a (author's call): a marked name is a finding anywhere, also inside the sync worktree folder,
    // which S§6.3 keeps unchecked-out (`--no-checkout`), so production writes no entry there. Outside it: a
    // `<name>@<host>` folder. Inside it: the S§6.1 and S§7 path forms (`<name>@<host>/<ULID>.json`, where the finding
    // is the `<name>@<host>` folder, as a ULID holds no marker; `registry/<host>.json`; `ack/<host>.json`), a top folder
    // with `@` and a host that is not in the host form, each name of a deeper path, a name that is not of a synced form,
    // a name that holds a subject, and a registry file one folder deeper.
    let outside = h.root().join(format!("{B_NAME}@{HOST_B}"));
    put(&outside.join("f"), "x");
    let tree_dir = s.worktree.join(format!("{B_NAME}@{HOST_B}"));
    put(&tree_dir.join("01JQ0000000000000000000000.json"), "x");
    let registry = s.worktree.join(format!("registry/{HOST_B2}.json"));
    put(&registry, "x");
    let ack = s.worktree.join(format!("ack/{HOST_B2}.json"));
    put(&ack, "x");
    let bad_host = s.worktree.join(format!("x@{HOST_R1}!!"));
    put(&bad_host.join("f"), "x");
    let deep_dir = tree_dir.join(format!("dir-{HOST_A2}"));
    let deep_file = deep_dir.join(format!("note-{HOST_A3}.txt"));
    put(&deep_file, "x");
    let loose = s.worktree.join(format!("note-{HOST_A2}"));
    put(&loose, "x");
    let subject_name = tree_dir.join(format!("{SUBJ}.json"));
    put(&subject_name, "x");
    let deep_registry = s.worktree.join(format!("registry/x/{HOST_A}.json"));
    put(&deep_registry, "x");
    let tree_names = [(outside, vec![B_NAME, HOST_B]), (tree_dir, vec![B_NAME, HOST_B]), (registry, vec![HOST_B2]),
        (ack, vec![HOST_B2]), (bad_host, vec![HOST_R1]), (deep_dir, vec![HOST_A2]), (deep_file, vec![HOST_A3]),
        (loose, vec![HOST_A2]), (subject_name, vec![SUBJ]), (deep_registry, vec![HOST_A])];
    let f = sweep_machine(&m);
    assert_eq!(f.len(), planted.len() + named.len() + tree_names.len(), "{f:?}");
    for (p, hits) in &tree_names {
        assert!(f.contains(&format!("X {} has a name or link target that holds {hits:?}", p.display())), "{p:?}: {f:?}");
    }
    for (p, hit) in &planted {
        assert!(f.contains(&format!("X {} holds [{hit:?}] and is not in the S-C2 allowed set", p.display())), "{p:?}: {f:?}");
    }
    for (p, hit) in &named {
        assert!(f.contains(&format!("X {} has a name or link target that holds [{hit:?}]", p.display())), "{p:?}: {f:?}");
    }
    // FX-26 re-plan 2 review round 3 F6: own and foreign values together in `workspace.json` and in `state.json` are
    // reported (every hit must be own), so a rule that lets one own value pass the file fails here.
    let mixed = [(ws_json.clone(), format!("{A_NAME} {B_NAME}"), vec![A_NAME, B_NAME]),
        (s.state.clone(), format!("registry/{HOST_A}.json {HOST_B}"), vec![HOST_A, HOST_B])];
    for (p, text, _) in &mixed {
        put(p, text);
    }
    let f = sweep_machine(&m);
    assert_eq!(f.len(), planted.len() + named.len() + tree_names.len(), "{f:?}");
    for (p, _, hits) in &mixed {
        assert!(f.contains(&format!("X {} holds {hits:?} and is not in the S-C2 allowed set", p.display())), "{p:?}: {f:?}");
    }
    // FX-26 review round 2 F-4: a sweep that does not read a file the walk knows exists fails.
    let empty = d.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert!(std::panic::catch_unwind(|| marked_files(&empty, &[empty.join("absent")], &[])).is_err());
    // FX-26 R3-1: a sweep that reads no file in a folder that the walk knows holds one fails, and names that folder.
    let quiet = d.path().join("quiet");
    put(&quiet.join("f"), "x");
    std::fs::create_dir(quiet.join("new")).unwrap();
    let r = std::panic::catch_unwind(|| marked_files(&quiet, &[quiet.join("f")], &[quiet.join("new")]));
    let msg = r.expect_err("a known folder with no file read").downcast::<String>().map(|s| *s).unwrap_or_default();
    assert!(msg.contains(&format!("{}: the sweep of {} read no file in it", quiet.join("new").display(), quiet.display())), "{msg}");
    // FX-26 R3-1: a folder that the sweep cannot read fails it, and the failure names that folder (the walk error is
    // not dropped). The mode goes back to 0700 before the temp folder drops. FX-26 re-plan review round 2 F7: each
    // case below runs its assertion only when its file or folder is unreadable at that mode (not as root).
    let locked = h.root().join("locked");
    put(&locked.join("leak"), B_NAME);
    if let Some(msg) = failure_at_mode(&locked, 0o000, 0o700, "the sweep read past a folder it cannot read", || sweep_machine(&m)) {
        assert!(msg.contains(&format!("{}: the sweep cannot read it", locked.display())), "{msg}");
    }
    std::fs::remove_dir_all(&locked).unwrap();
    // FX-26 re-plan review round 1 F6: a regular file that the sweep cannot read fails it, and the failure names that
    // file. The mode goes back to 0600 before the file is removed.
    let sealed = h.root().join("sealed");
    put(&sealed, B_NAME);
    if let Some(msg) = failure_at_mode(&sealed, 0o000, 0o600, "the sweep read past a file it cannot read", || sweep_machine(&m)) {
        assert!(msg.contains(&format!("{}: the sweep cannot read it", sealed.display())), "{msg}");
    }
    std::fs::remove_file(&sealed).unwrap();
    // F6: `files_findings` drops no walk error either: a folder that it cannot read fails it, with its path.
    let closed = d.path().join("closed");
    put(&closed.join("f"), "x");
    if let Some(msg) = failure_at_mode(&closed, 0o000, 0o700, "files_findings read past a folder it cannot read",
        || files_findings(std::slice::from_ref(&closed), &MARKERS)) {
        assert!(msg.contains(&format!("{}: the scan cannot read it", closed.display())), "{msg}");
    }
    // F6: nor does `git_meta_findings`: a folder in a linked-worktree admin folder that it cannot read fails it.
    let shut = r2.join(".git/worktrees/wt/shut");
    put(&shut.join("f"), "x");
    if let Some(msg) = failure_at_mode(&shut, 0o000, 0o700, "git_meta_findings read past a folder it cannot read", || git_meta_findings(&r2)) {
        assert!(msg.contains(&format!("{}: the scan cannot read it", shut.display())), "{msg}");
    }
    std::fs::remove_dir_all(&shut).unwrap();
    // FX-26 re-plan review round 2 F4: every other read of the scan fails with the path too. `errors.log`: a file that
    // cannot be read, and a file that is not UTF-8; a missing one reads as empty (the scan asserts where one must be).
    let log = h.store(WS_ID).dir().join("errors.log");
    assert_eq!(errors(&h), "");
    put(&log, "x\n");
    if let Some(msg) = failure_at_mode(&log, 0o000, 0o600, "errors() read past an errors.log it cannot read", || errors(&h)) {
        assert!(msg.contains(&format!("{}: the scan cannot read it", log.display())), "{msg}");
    }
    std::fs::write(&log, [b'x', 0xff, b'\n']).unwrap();
    let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| errors(&h))).expect_err("errors() read an errors.log that is not UTF-8")
        .downcast::<String>().map(|s| *s).unwrap_or_default();
    assert!(msg.contains(&format!("{}: the scan cannot read it", log.display())), "{msg}");
    std::fs::remove_file(&log).unwrap();
    // `workspace.json`, `state.json` and the usage store that cannot be read.
    for (p, what) in [(&ws, "workspace_findings"), (&st, "state_findings"), (&db, "usage_findings")] {
        let f = || match what {
            "workspace_findings" => workspace_findings(p, &[]).len(),
            "state_findings" => state_findings(p, &[]).len(),
            _ => usage_findings(p).1.len(),
        };
        if let Some(msg) = failure_at_mode(p, 0o000, 0o600, &format!("{what} read past a file it cannot read"), f) {
            assert!(msg.contains(&format!("{}: the scan cannot read it", p.display())), "{what}: {msg}");
        }
    }
    // The session logs: a home whose entries cannot be listed, and a `sess-*` folder that cannot be read.
    let home_mode = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(h.root()).unwrap().permissions().mode() & 0o7777
    };
    if let Some(msg) = failure_at_mode(h.root(), 0o300, home_mode, "session_logs read past a home it cannot list", || session_logs(&h)) {
        assert!(msg.contains(&format!("{}: the scan cannot read it", h.root().display())), "{msg}");
    }
    let sess = h.root().join("sess-shut");
    put(&sess.join("session.log"), "x");
    if let Some(msg) = failure_at_mode(&sess, 0o000, 0o700, "session_logs read past a sess-* folder it cannot read", || session_logs(&h)) {
        assert!(msg.contains(&format!("{}: the scan cannot read it", sess.join("session.log").display())), "{msg}");
    }
    std::fs::remove_dir_all(&sess).unwrap();
    // FX-26 review round 2 F-5: on a remote, only `seed/.zygo.toml` may hold a marker; the object store is left out.
    let r = remote();
    let root = r.bare.parent().unwrap();
    put(&root.join("seed/.zygo.toml"), HOST_R1);
    put(&r.bare.join("objects/zz/f"), HOST_R2);
    assert_eq!(sweep_remote(&r), Vec::<String>::new());
    let planted = [
        (root.join("seed/notes"), HOST_R1),
        (root.join("seed9/.zygo.toml"), HOST_R1), // a `.zygo.toml` of another clone
        (r.bare.join("description"), HOST_R3), // a file of `bare.git/`
        (r.bare.join("hooks/x"), HOST_R3),
    ];
    for (p, hit) in &planted {
        put(p, hit);
    }
    let mut want: Vec<String> = planted.iter().map(|(p, hit)| format!("{} holds [{hit:?}] and is not a test input", p.display())).collect();
    // FX-26 re-plan review round 1 F7: a marker in a file name on a remote.
    let named = root.join(format!("seed/{HOST_R2}.txt"));
    put(&named, "x");
    want.push(format!("{} has a name or link target that holds [{HOST_R2:?}]", named.display()));
    let mut got = sweep_remote(&r);
    want.sort();
    got.sort();
    assert_eq!(got, want);
    // FX-26 re-plan 2 review round 1 F7: in CI (the variable `CI` set; read only) at least one permission case ran.
    if std::env::var_os("CI").is_some() {
        assert!(PERMISSION_CASES_RUN.load(Ordering::SeqCst) > 0,
            "CI is set and every permission case was skipped (the tests run as root), so the unreadable-path cases have no coverage");
    }
}

#[test] // S-C2, S§6.5, S§19 #24 (as rewritten): after #1, #3, #4, #5, #12, #15, #19, #9, #20, #11, #6, #22, #23 and a
        // `ws forget` answered yes, on three machines through the CLI and the MCP tool, and one usage row, no marker is
        // in a log, a state file, the usage store, a commit or a ref name, and every zygo commit has the S§6.5 message
        // and identity
fn no_mail_content_after_the_scenarios() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _h = hermetic(); // dropped after every machine of the walk, before `_g`

    // Setup and #1 (normal; CLI and MCP send).
    let r = remote();
    let a = machine(&r, HOST_A, ALICE, A_NAME);
    let b = machine(&r, HOST_B, BOB, B_NAME);
    let mut made = BTreeSet::from([git(&r.bare, &["rev-parse", "refs/heads/main"])]);
    // #2 (user repository untouched): `gc.auto = 1` in each user checkout, and its state before the first sync.
    for h in [&a, &b] {
        git(&h.repo(), &["config", "gc.auto", "1"]);
    }
    let before = [snap(&a.repo()), snap(&b.repo())];
    for h in [&b, &a, &b] {
        let o = sync_bin(h, &[]);
        assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    }
    let id1 = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 1);
    let v = call(&tools(&a, ALICE), "mail_send", json!({"to": format!("{B_NAME}@{HOST_B}"), "subject": format!("{SUBJ}-2"), "body": format!("{BODY}-2")})).unwrap();
    let id2 = v["sent"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(sync_in(&a).pushed, 2);
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    for id in [&id1, &id2] {
        let path = format!("{B_NAME}@{HOST_B}/{id}.json");
        let got = std::fs::read(mail_dir(&b.paths, WS_ID).join(BOB).join("new").join(format!("{id}.json"))).unwrap();
        // FX-26 re-plan 2 review round 3 F4: the delivered message names its sender and its target with the walk's own
        // names (S§7.3), so an address that `outbox::send_remote_with` builds wrong fails here.
        let msg: Value = serde_json::from_slice(&got).unwrap();
        assert_eq!((&msg["from"], &msg["to"]), (&json!(format!("{A_NAME}@{HOST_A}")), &json!(format!("{B_NAME}@{HOST_B}"))), "{msg}");
        assert_eq!(Some(got), tip_blob(&r, &path));
    }
    let ack: Value = serde_json::from_slice(&tip_blob(&r, &format!("ack/{HOST_B}.json")).unwrap()).unwrap();
    for id in [&id1, &id2] {
        assert!(ack["imported"].as_array().unwrap().iter().any(|u| u == id.as_str()), "{ack}");
    }
    assert_eq!(call(&tools(&b, BOB), "mail_list", json!({})).unwrap()["remote"], json!([format!("{A_NAME}@{HOST_A}")]));
    // #2: after #1, `HEAD`, the branch list, the status, the index, `FETCH_HEAD` and the packs of each user checkout
    // are as before; no `gc.log`; the worktree list is as before plus one detached sync worktree.
    for (h, s0) in [&a, &b].into_iter().zip(before) {
        let s = snap(&h.repo());
        assert_eq!((&s.head, &s.branches, &s.status), (&s0.head, &s0.branches, &s0.status), "{}", h.root().display());
        assert_eq!(s.index, s0.index, "{}", h.root().display());
        assert_eq!(s.fetch_head, s0.fetch_head, "{}", h.root().display());
        assert_eq!(s.packs, s0.packs, "{}", h.root().display());
        assert!(!h.repo().join(".git/gc.log").exists(), "{}", h.root().display());
        let added = s.worktrees.strip_prefix(&s0.worktrees).unwrap();
        let blocks: Vec<&str> = added.split("\n\n").map(str::trim).filter(|b| !b.is_empty()).collect();
        assert_eq!(blocks.len(), 1, "{added}");
        assert!(blocks[0].starts_with(&format!("worktree {}\n", sp(h).worktree.display())), "{added}");
        assert!(blocks[0].lines().any(|l| l == "detached"), "{added}");
    }

    // #3 (push race, then five moves).
    let ia = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 3);
    let ib = send_ok(&b, BOB, &format!("{A_NAME}@{HOST_A}"), 4);
    let (b_paths, mut first) = (b.paths.clone(), true);
    let race = Hook::new(move |p| {
        if p == "before_push" && std::mem::take(&mut first) {
            run_with_deadline(&env_paths(b_paths.clone(), Box::new(NoSeam), far()), WS_ID, Mode::Child, &Opts::default(), FAR_PASS).unwrap();
        }
        Ok(())
    });
    run_with_deadline(&env_in(&a, Box::new(race), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap();
    assert!(tip_paths(&r).contains(&format!("{B_NAME}@{HOST_B}/{ia}.json")));
    assert!(tip_paths(&r).contains(&format!("{A_NAME}@{HOST_A}/{ib}.json")));
    let ic = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 5);
    let (bare, moved) = (r.bare.clone(), Arc::new(Mutex::new(Vec::new())));
    let m2 = Arc::clone(&moved);
    let moves = Hook::new(move |p| {
        if p == "before_push" {
            m2.lock().unwrap().push(advance(&bare));
        }
        Ok(())
    });
    let e = run_with_deadline(&env_in(&a, Box::new(moves), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap_err();
    made.extend(moved.lock().unwrap().drain(..));
    assert_eq!(made.len(), 6); // main and the five moves
    assert_eq!(e, SyncError::Stopped { line: "zygo: the side branch moved 5 times during the sync; try again".into(), check: "moved" });
    assert!(sp(&a).outbox.join(format!("{ic}.json")).is_file());
    assert_eq!(sync_in(&a).pushed, 1);

    // #4 (secret at send, CLI and MCP; then a new pattern before the pass).
    // FX-26 re-plan 2 review round 1 F1: alice's `new/` already holds bob's #3 message, so the files before #4 are
    // recorded, and #4 must add exactly one: the notice.
    let new4 = new_ids(&a, ALICE);
    let aws = ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat();
    let (code, o, e) = send_with(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 6, &format!("{BODY}-6 {aws}"));
    assert_eq!((code, o, e), (1, String::new(), "zygo: message not sent: it holds a secret (aws_access_key_id)\n".into()));
    assert_eq!(call(&tools(&a, ALICE), "mail_send", json!({"to": format!("{B_NAME}@{HOST_B}"), "subject": format!("{SUBJ}-7"), "body": format!("{BODY}-7 {aws}")})),
        Err(ToolError::Failed("zygo: message not sent: it holds a secret (aws_access_key_id)".into())));
    assert_eq!(outbox::count(&sp(&a)).unwrap(), 0);
    let i4 = send_body(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 8, &format!("{BODY}-8 {SHIP}")); // passes at send
    config(&a, HOST_A, "", &format!("[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"{SHIP}\"\naction = \"redact\"\ndescription = \"d\"\n"));
    sync_in(&a);
    assert!(sp(&a).refused.join(format!("{i4}.json")).is_file());
    assert!(!tip_paths(&r).iter().any(|p| p.ends_with(&format!("/{i4}.json"))));
    assert!(errors(&a).lines().any(|l| l.ends_with(&format!("mail sync: outbox {i4}: refused: ship_word"))), "{}", errors(&a));
    // The notice from `zygo` (S§8.6 "At commit"; `outbox::refuse` gives its subject and body, `sync::refuse` sends it
    // from `zygo` to the sender's name).
    let added: Vec<String> = new_ids(&a, ALICE).into_iter().filter(|id| !new4.contains(id)).collect();
    assert_eq!(added.len(), 1, "#4 adds exactly one file to alice's new/: {added:?}");
    let notice: Value = serde_json::from_slice(&std::fs::read(maildir(&a, ALICE, "new").join(format!("{}.json", added[0]))).unwrap()).unwrap();
    assert_eq!((notice["from"].as_str(), notice["to"].as_str()), (Some("zygo"), Some(A_NAME)), "{notice}");
    assert_eq!(notice["subject"].as_str(), Some(format!("not sent: {i4}").as_str()), "{notice}");
    let text = format!("zygo did not send message {i4} to {B_NAME}@{HOST_B}: it holds a secret (ship_word). The message is kept on this machine in mail/outbox/refused/.");
    assert_eq!(notice["body"].as_str(), Some(text.as_str()), "{notice}");
    config(&a, HOST_A, "", "");

    // #5 (the secret check cannot run: expiry, then an act-lite store that does not parse).
    // F2: "no notice is sent": alice's `new/` gets no new file in this block.
    let new5 = new_ids(&a, ALICE);
    let i5 = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 9);
    let expire = DeadlinePolicy { expire_after_scans: Some(0), base: Duration::from_secs(3600), ..Default::default() };
    run_with_deadline(&env_in(&a, Box::new(NoSeam), expire), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap();
    assert!(sp(&a).outbox.join(format!("{i5}.json")).is_file());
    assert!(errors(&a).contains(&format!("mail sync: outbox {i5}: secret check could not run: secret_timeout")), "{}", errors(&a));
    let store = act_lite_store::store_path(&a.repo(), &act_lite_store::Lookup::system());
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(&store, "not an assignment\n").unwrap();
    sync_in(&a);
    assert!(errors(&a).contains(&format!("mail sync: outbox {i5}: secret check could not run: act_lite_store")), "{}", errors(&a));
    std::fs::remove_file(&store).unwrap();
    assert_eq!(sync_in(&a).pushed, 1);
    assert_eq!(new_ids(&a, ALICE), new5, "#5 sends no notice");

    // #15 (inbox full; bob reads through the MCP tool). B first imports what waits for it since #3 (in the plan's
    // order the #12 pass did this).
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    call(&tools(&b, BOB), "mail_read", json!({"max": 1000})).unwrap();
    assert_eq!(new_count(&b, BOB), 0);
    config(&b, HOST_B, "max_unread = 1\n", "");
    let f1 = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 10);
    let f2 = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 11);
    assert_eq!(sync_in(&a).pushed, 2);
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(new_count(&b, BOB), 1);
    let ack: Value = serde_json::from_slice(&tip_blob(&r, &format!("ack/{HOST_B}.json")).unwrap()).unwrap();
    let acked = |id: &str| ack["imported"].as_array().unwrap().iter().any(|u| u == id);
    assert_eq!(acked(&f1) as u8 + acked(&f2) as u8, 1, "{ack}"); // the one not delivered is not acknowledged
    // F2: "a prune keeps it". B's `--prune` pass (the inbox still full) makes a new root: the delivered message, whose
    // ULID B acknowledged, goes, and the one not acknowledged stays at the tip.
    let (done, waiting) = if acked(&f1) { (&f1, &f2) } else { (&f2, &f1) };
    let o = sync_bin(&b, &["--prune"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let paths = tip_paths(&r);
    assert_eq!(count(&r), 1, "the prune made a new root");
    assert!(!paths.contains(&format!("{B_NAME}@{HOST_B}/{done}.json")), "{paths:?}");
    assert!(paths.contains(&format!("{B_NAME}@{HOST_B}/{waiting}.json")), "{paths:?}");
    assert_eq!(new_count(&b, BOB), 1);
    call(&tools(&b, BOB), "mail_read", json!({})).unwrap();
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(new_ids(&b, BOB), vec![waiting.clone()]); // the second one now
    config(&b, HOST_B, "", "");

    // #19 (host rename while mail waits).
    for n in 12..15 {
        send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), n);
    }
    assert_eq!(sync_in(&a).pushed, 3);
    config(&b, HOST_B2, "", "");
    let before = new_count(&b, BOB);
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(new_count(&b, BOB), before + 3);
    let paths = tip_paths(&r);
    assert!(!paths.contains(&format!("registry/{HOST_B}.json")) && paths.contains(&format!("registry/{HOST_B2}.json")), "{paths:?}");
    assert!(paths.contains(&format!("ack/{HOST_B}.json")), "{paths:?}");
    let o = sync_bin(&a, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let (code, out, _) = send(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 15);
    assert_eq!((code, out.as_str(), outbox::count(&sp(&a)).unwrap()), (1, "", 0));
    let o = sync_bin(&b, &["--prune"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(!tip_paths(&r).contains(&format!("ack/{HOST_B}.json")), "{:?} {}", tip_paths(&r), errors(&b));
    let i19 = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B2}"), 16);
    sync_in(&a);
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(mail_dir(&b.paths, WS_ID).join(BOB).join("new").join(format!("{i19}.json")).is_file());

    // #12 (hostile tip, imported by the binary on B), in the folder of B's host after #19 (implementer's note, step 6).
    let (hs, hc) = hostile(&r.bare, HOST_B2);
    made.insert(hc);
    // The registry half: a registry with 257 names, and one whose `host` is not its name (MS-22 review F2).
    let (many, other, rc) = bad_registries(&r.bare);
    made.insert(rc);
    // FX-26 re-plan 2 review round 2 F1: "None is delivered" (bob's `new/` is the same after the pass) and "no file is
    // written outside bob's maildir and mail/sync/" (every path of B's home that the pass adds, changes or removes).
    // The pass also writes, by design, what S§19 #12 names besides: the `errors.log` lines (an append of exactly the
    // eight lines below), and the git files of its fetch, commit and push of `ack/<host>.json`: the remote-tracking
    // ref `refs/remotes/origin/zygo/mail` (S§6.5, S§8.3 step 15) and the sync worktree's own `HEAD` and index (S§6.3,
    // S§15), each with the reflog that git keeps for it. Object stores are left out (`tree_state`).
    let new12 = new_ids(&b, BOB);
    let (log12, home12) = (errors(&b), tree_state(b.root()));
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(new_ids(&b, BOB), new12, "#12: a hostile entry was delivered");
    let (s12, gd) = (sp(&b), b.repo().join(".git"));
    let admin = PathBuf::from(std::fs::read_to_string(s12.worktree.join(".git")).unwrap().strip_prefix("gitdir: ").unwrap().trim_end());
    let ws_log = b.store(WS_ID).dir().join("errors.log");
    let by_design = [ws_log.clone(), gd.join("refs/remotes/origin/zygo/mail"), gd.join("logs/refs/remotes/origin/zygo/mail"),
        admin.join("HEAD"), admin.join("index"), admin.join("logs/HEAD")];
    let outside: Vec<PathBuf> = changed(&home12, &tree_state(b.root())).into_iter()
        .filter(|p| !(p.starts_with(mail_dir(&b.paths, WS_ID).join(BOB)) || p.starts_with(&s12.sync) || by_design.contains(p)))
        .collect();
    assert_eq!(outside, Vec::<PathBuf>::new(), "#12: the pass wrote outside bob's maildir, mail/sync/ and its git files");
    let log = errors(&b);
    let added: Vec<&str> = log.strip_prefix(log12.as_str()).unwrap_or_else(|| panic!("#12: errors.log was not appended to: {log}")).lines().collect();
    assert_eq!(added.len(), 8, "#12: one errors.log line for each hostile entry and each refused registry: {added:?}");
    for (id, check) in hs.iter().zip(["mode", "mode", "size", "id", "to", "json"]) {
        assert!(log.lines().any(|l| l.ends_with(&format!("mail sync: import {id}: {check}"))), "{id} {check}: {log}");
    }
    for (blob, check) in [(&many, "members"), (&other, "host")] {
        assert!(log.lines().any(|l| l.ends_with(&format!("mail sync: registry file refused: check {check}, object {blob}"))), "{check}: {log}");
    }
    let cache = std::fs::read(&sp(&b).members).unwrap();
    assert_eq!(text_findings("B members.json", &cache, &[HOST_R1, HOST_R2, HOST_R3]), Vec::<String>::new());
    let ack: Value = serde_json::from_slice(&tip_blob(&r, &format!("ack/{HOST_B2}.json")).unwrap()).unwrap();
    for id in &hs {
        assert!(ack["imported"].as_array().unwrap().iter().any(|u| u == id.as_str()), "{id}: {ack}");
    }

    // #9 (collision and take-host, on the two machines).
    // F2: "a message that the first machine acknowledged is not delivered on the second". B gets a second member
    // `alice-q7x` (key `B_ALICE`), so a folder `alice-q7x@<HOST_A>/` is a candidate folder of B once B owns `HOST_A`.
    // Bob sends to `alice-q7x@<HOST_A>`; A delivers it and acknowledges it; then B takes `HOST_A` and must not deliver
    // it (S§7.6 `--take-host` keeps the ack's entries).
    b.store(WS_ID).update(Duration::from_secs(5), &mut |w| {
        w.members.insert(B_ALICE.to_string(), MemberRecord { name: Some(A_NAME.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
        Ok(())
    }).unwrap();
    let i9 = send_ok(&b, BOB, &format!("{A_NAME}@{HOST_A}"), 30);
    assert_eq!(sync_in(&b).pushed, 1);
    assert_eq!(sync_in(&a).delivered, 1);
    assert!(new_ids(&a, ALICE).contains(&i9));
    let ack: Value = serde_json::from_slice(&tip_blob(&r, &format!("ack/{HOST_A}.json")).unwrap()).unwrap();
    assert!(ack["imported"].as_array().unwrap().iter().any(|u| u == i9.as_str()), "{ack}");
    config(&b, HOST_A, "", "");
    let o = sync_bin(&b, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{COLLISION}\n")));
    let o = sync_bin(&b, &["--take-host"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(out_of(&o), "zygo: mail sync: 0 sent, 0 delivered\n"); // not counted
    let o = sync_bin(&b, &[]); // the pass after the take-host: still not delivered
    assert_eq!((o.status.code(), out_of(&o)), (Some(0), "zygo: mail sync: 0 sent, 0 delivered\n".into()), "{}", err_of(&o));
    assert!(tip_paths(&r).contains(&format!("{A_NAME}@{HOST_A}/{i9}.json"))); // the candidate is still there
    assert_eq!(new_ids(&b, B_ALICE), Vec::<String>::new());
    assert!(!std::fs::read_to_string(sp(&b).imported).unwrap().lines().any(|l| l == i9));
    b.store(WS_ID).update(Duration::from_secs(5), &mut |w| {
        w.members.remove(B_ALICE);
        Ok(())
    }).unwrap();
    let o = sync_bin(&a, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{COLLISION}\n")));
    config(&a, HOST_A2, "", "");
    let o = sync_bin(&a, &["--new-identity"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(tip_paths(&r).contains(&format!("registry/{HOST_A2}.json")));

    // #20 (forget part with "no", then drop host with "no" and `--yes`).
    let to = format!("{B_NAME}@{HOST_A}"); // B's host since #9
    send_ok(&a, ALICE, &to, 17);
    send_ok(&a, ALICE, &to, 18);
    let refused = outbox::refused_count(&sp(&a)).unwrap();
    let prompt = format!("zygo: 2 unsent messages and {refused} refused messages will be deleted. Forget the workspace? [y/N]");
    let o = zygo(&a, &["ws", "forget", "app"], Some(b"n\n"));
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), "zygo: app not forgotten\n".into(), format!("{prompt} ")));
    send_ok(&a, ALICE, &to, 19);
    assert_eq!(sync_in(&a).pushed, 3);
    let ask = format!("zygo: 3 messages wait for host {HOST_A}; they will be deleted. Delete them? [y/N]");
    let before = tip_paths(&r);
    let o = zygo(&a, &["mail", "sync", "--drop-host", HOST_A], Some(b"n\n"));
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), format!("zygo: host {HOST_A} not dropped\n"), format!("{ask} ")));
    assert_eq!(tip_paths(&r), before);
    let o = zygo(&a, &["mail", "sync", "--drop-host", HOST_A, "--yes"], None);
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), format!("zygo: host {HOST_A} dropped; 3 messages deleted\n"), format!("{ask} y (--yes)\n")));
    let after = tip_paths(&r);
    assert!(after.iter().all(|p| !p.starts_with(&format!("{B_NAME}@{HOST_A}/")) && *p != format!("registry/{HOST_A}.json") && *p != format!("ack/{HOST_A}.json")), "{after:?}");

    // The findings of the state files at a stop (`last_error`, the public cache), which a later pass clears.
    let mut mid = Vec::new();

    // #11 (twin: machine C runs a copy of A's state folder and writes the ack; A's next pass stops with the twin line;
    // --new-identity with a new host then syncs). B writes its files again after #20 and reads A's host first.
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let it = send_ok(&b, BOB, &format!("{A_NAME}@{HOST_A2}"), 20);
    assert_eq!(sync_in(&b).pushed, 1);
    let c = machine(&r, HOST_A2, ALICE, A_NAME);
    for d in [sp(&c).mail, sp(&c).sync] {
        zygo_workspace::mail::ensure_mail_root(&d).unwrap();
    }
    std::fs::copy(sp(&a).identity, sp(&c).identity).unwrap();
    std::fs::copy(sp(&a).state, sp(&c).state).unwrap();
    let o = sync_bin(&c, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(mail_dir(&c.paths, WS_ID).join(ALICE).join("new").join(format!("{it}.json")).is_file());
    let o = sync_bin(&a, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{TWIN}\n")));
    assert!(errors(&a).lines().any(|l| l.ends_with("mail sync: stopped: twin")), "{}", errors(&a));
    let st: Value = serde_json::from_slice(&std::fs::read(sp(&a).state).unwrap()).unwrap();
    assert_eq!(st["last_error"], "twin");
    // #11: `zygo doctor` on A prints the twin line while it stands.
    let doctor_twin = |h: &Harness| {
        let o = zygo(h, &["doctor"], None);
        assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
        out_of(&o).lines().any(|l| l == TWIN)
    };
    assert!(doctor_twin(&a));
    mid.extend(state_findings(&sp(&a).state, &[HOST_A2]));
    config(&a, HOST_A3, "", "");
    let o = sync_bin(&a, &["--new-identity"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(tip_paths(&r).contains(&format!("registry/{HOST_A3}.json")));
    assert!(!doctor_twin(&a));

    // #6 (public check, the `public` verdict of the injected probe: no network host is asked): the sync stops before
    // any fetch, the bare repository's refs do not change, and a remote send is refused.
    git(&a.repo(), &["remote", "set-url", "--push", "origin", &format!("https://example.invalid/{B_NAME}/r.git")]);
    let refs0 = git(&r.bare, &["for-each-ref"]);
    let public = env_full(a.paths.clone(), Box::new(NoSeam), far(), Box::new(Says(Verdict::Public)), Box::new(NoSpawn), Box::new(NoWake));
    let e = run_with_deadline(&public, WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: PUBLIC.into(), check: "public" });
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    let (code, out, err) = send(&a, ALICE, &format!("{B_NAME}@{HOST_A}"), 21);
    assert_eq!((code, out.as_str(), err), (1, "", format!("{PUBLIC}\n")));
    assert!(errors(&a).lines().any(|l| l.ends_with("mail sync: stopped: public")), "{}", errors(&a));
    let st: Value = serde_json::from_slice(&std::fs::read(sp(&a).state).unwrap()).unwrap();
    assert_eq!((&st["last_error"], &st["public"]["verdict"]), (&json!("public"), &json!("public")));
    mid.extend(state_findings(&sp(&a).state, &[HOST_A3]));
    git(&a.repo(), &["config", "--unset", "remote.origin.pushurl"]);
    sync_in(&a); // the check runs again on the file URL: `local`

    // #22 (a sync child that does not start): the CLI send, the MCP send and the watch step each log one line.
    let fail = env_full(a.paths.clone(), Box::new(NoSeam), far(), Box::new(NoProbe), Box::new(FailSpawn), Box::new(NoWake));
    let (code, _, err) = send_env(&fail, &a, ALICE, &format!("{B_NAME}@{HOST_A}"), 22, &format!("{BODY}-22"));
    assert_eq!((code, err.as_str()), (0, ""));
    call(&tools_spawn(&a, ALICE, Box::new(FailSpawn)), "mail_send", json!({"to": format!("{B_NAME}@{HOST_A}"), "subject": format!("{SUBJ}-23"), "body": format!("{BODY}-23")})).unwrap();
    let ws = a.store(WS_ID).read().unwrap().unwrap();
    zygo::cmd::mail::watch_sync_step(&fail, &ws)();
    let not_started = std::io::Error::from(std::io::ErrorKind::NotFound);
    for who in ["mail send", "mail_send", "mail watch"] {
        let line = format!("{who}: cannot start the mail sync child: {not_started}");
        assert!(errors(&a).lines().any(|l| l.ends_with(&line)), "{line}: {}", errors(&a));
    }
    assert_eq!(sync_in(&a).pushed, 2);

    // #23 (a wake that fails, after B's import): a `wake_gap` that does not parse, a `wake.json` that cannot be
    // written, and a pane that takes no text each log one line.
    b.store(WS_ID).update(Duration::from_secs(5), &mut |w| {
        w.members.get_mut(BOB).unwrap().zygo_session = Some("sbob".into());
        Ok(())
    }).unwrap();
    let held = live(&b.paths, "sbob");
    let wake_json = mail_dir(&b.paths, WS_ID).join(BOB).join("wake.json");
    std::fs::create_dir(&wake_json).unwrap();
    let woke = env_full(b.paths.clone(), Box::new(NoSeam), far(), Box::new(NoProbe), Box::new(NoSpawn), Box::new(FailWake));
    assert_eq!(run_with_deadline(&woke, WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap().delivered, 2);
    let log = errors(&b);
    let gap_line = "mail wake: [mail] wake_gap \"soon\" is not <n>s, <n>m, <n>h or <n>d; using 30s".to_string();
    assert!(log.lines().any(|l| l.ends_with(&gap_line)), "{log}");
    assert!(log.lines().any(|l| l.contains(&format!("mail wake {BOB}: wake.json not written: "))), "{log}");
    assert!(log.lines().any(|l| l.ends_with(&format!("mail wake {BOB}: {}", mux_error()))), "{log}");
    std::fs::remove_dir(&wake_json).unwrap();
    drop(held);

    // `zygo ws forget` answered yes, on C. Its stores are scanned first, as the forget removes them.
    let ec = errors(&c);
    assert!(!ec.is_empty());
    mid.extend(text_findings("C errors.log", ec.as_bytes(), &MARKERS));
    mid.extend(workspace_findings(&c.store(WS_ID).dir().join("workspace.json"), &[A_NAME]));
    let own_c = own_hosts_at_tip(&r, &c, HOST_A2);
    assert_eq!(own_c, vec![HOST_A2.to_string()]);
    mid.extend(state_findings(&sp(&c).state, &[HOST_A2]));
    mid.extend(ref_findings(&sp(&c).worktree).1);
    mid.extend(files_findings(&sync_files(&c), &MARKERS).1);
    for n in [24, 25] {
        send_ok(&c, ALICE, &format!("{B_NAME}@{HOST_A}"), n);
    }
    // #26: another worktree of C's checkout, beside its own and the sync worktree.
    git(&c.repo(), &["worktree", "add", "-q", "-b", "other", c.root().join("other").to_str().unwrap()]);
    let wt_before = worktrees(&c.repo());
    let is_sync = |e: &String| e.starts_with(&format!("worktree {}\n", sp(&c).worktree.display()));
    assert_eq!((wt_before.len(), wt_before.iter().filter(|e| is_sync(e)).count()), (3, 1), "{wt_before:?}");
    let prompt = "zygo: 2 unsent messages and 0 refused messages will be deleted. Forget the workspace? [y/N]";
    let o = zygo(&c, &["ws", "forget", "app"], Some(b"y\n"));
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), "zygo: forgot app\n".into(), format!("{prompt} ")));
    assert!(std::fs::symlink_metadata(c.store(WS_ID).dir()).is_err());
    // #26: the sync worktree left git's list; the other worktrees are unchanged.
    let want: Vec<String> = wt_before.iter().filter(|e| !is_sync(e)).cloned().collect();
    assert_eq!(worktrees(&c.repo()), want);
    assert_eq!(want.len(), 2);

    // Usage row (author's call 5), on B.
    let sd = SessionDir::new(b.root().join("sess-u"));
    sd.create().unwrap();
    SessionView { session: "u".into(), agent: Agent::ClaudeCode, results: true, trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }.save(&sd).unwrap();
    drop(zygo_usage::Store::open(&b.paths.usage_db).unwrap()); // the database as the launcher leaves it (hook_trim.rs:30)
    // Each line numbered, so the shaper counts no repeated lines and the size trim stores the output (implementer's
    // note, step 6).
    let stdout: String = (0..400).map(|i| format!("{i:03} {SUBJ}-u {BODY}-u {A_NAME}@{HOST_A} {B_NAME}@{HOST_B} {}\n", MARKERS[6..].join(" "))).collect();
    let event = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": format!("zygo mail read --name {B_NAME}")},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}});
    let mut trim = b.zygo(&["hook", "trim"], &[("ZYGO_SESSION_DIR", sd.root.as_os_str())]);
    trim.stdin(Stdio::piped());
    let mut child = trim.spawn().unwrap();
    feed_stdin(child.stdin.take().unwrap(), &serde_json::to_vec(&event).unwrap());
    let o = child.wait_with_output().unwrap();
    common::assert_no_marketplace_sync(b.root());
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(out_of(&o).contains("[zygo trimmed this output:"), "{} {}", out_of(&o), err_of(&o));

    // The scans (S§19 #24 as rewritten; each also proves the file it reads is there).
    assert!(!errors(&a).is_empty() && !errors(&b).is_empty());
    // S§7.5 own hosts at the end of the walk (MS-22 review F1): A has its current host only; B has its current host
    // and each old host whose ack still holds its identity.
    let (own_a, own_b) = (own_hosts_at_tip(&r, &a, HOST_A3), own_hosts_at_tip(&r, &b, HOST_A));
    assert_eq!(own_a, vec![HOST_A3.to_string()]);
    assert_eq!(own_b[0], HOST_A);
    assert!(!own_b.iter().any(|h| h == HOST_B), "{own_b:?}");
    // C's stores went with its forget (scanned above, in `mid`); its repository stays and is scanned.
    let mut found = scan(&[&r], &made, &[
        // FX-26 R3-1: A got B's mail and keeps the #3 refused message (`refused/`); B read some of its mail (`cur/`),
        // and the usage row wrote `sess-u/results/`.
        Scanned { label: "A", h: &a, own_names: vec![A_NAME], own_hosts: own_a, synced: true, errors_log: true, zygo_commits: true,
            folders: vec![maildir(&a, ALICE, "new"), sp(&a).refused, a.root().join(format!("sess-{ALICE}"))] },
        Scanned { label: "B", h: &b, own_names: vec![B_NAME], own_hosts: own_b, synced: true, errors_log: true, zygo_commits: true,
            folders: vec![maildir(&b, BOB, "new"), maildir(&b, BOB, "cur"), b.root().join(format!("sess-{BOB}")), b.root().join("sess-u"),
                b.root().join("sess-u/results")] },
        // C: the forget removed its state folder with its `errors.log`, which `mid` scanned before (`assert!(!ec.is_empty())`).
        // Its repository keeps the zygo commits that it fetched and pushed.
        // FX-26 R3-1: C has no folder of these kinds after its forget.
        Scanned { label: "C", h: &c, own_names: vec![A_NAME], own_hosts: own_c, synced: false, errors_log: false, zygo_commits: true, folders: vec![] },
    ]);
    found.extend(mid);
    let (rows, _) = usage_findings(&b.paths.usage_db);
    assert!(rows >= 1, "the trim hook wrote no usage row");
    assert_eq!(found, Vec::<String>::new());
}

/// Writes the member cache of `h` with the one host `host` and its one member `name`; the mail folders are made first
/// (as `crates/zygo-mail-sync/tests/sync_pass.rs` `knows`).
fn knows(h: &Harness, host: &str, name: &str) {
    let s = sp(h);
    for d in [&s.mail, &s.sync] {
        zygo_workspace::mail::ensure_mail_root(d).unwrap();
    }
    write_cache(&s, &MemberCache { hosts: [(host.to_string(), vec![name.to_string()])].into() }).unwrap();
}

#[test] // S-C2, S§6.5, S§19 #24 (FX-26 walk 2): after #8 (two bootstraps between one fetch and one push), #10 (a moved
        // repository) and #7 (own branch only), no marker is in a log, a state file, the usage store, a commit or a ref
        // name, and every zygo commit has the S§6.5 message and identity
fn no_mail_content_after_the_branch_scenarios() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _h = hermetic(); // dropped after every machine of the walk, before `_g`

    // 2.1 #8 (bootstrap race): no branch on the remote; B bootstraps between A's fetch and A's push.
    let r2 = remote();
    let mut made = BTreeSet::from([git(&r2.bare, &["rev-parse", "refs/heads/main"])]);
    let a = machine(&r2, HOST_A, ALICE, A_NAME);
    let b = machine(&r2, HOST_B, BOB, B_NAME);
    knows(&a, HOST_B, B_NAME);
    knows(&b, HOST_A, A_NAME);
    let (to_a, to_b) = (format!("{A_NAME}@{HOST_A}"), format!("{B_NAME}@{HOST_B}"));
    let ida = send_ok(&a, ALICE, &to_b, 1);
    let idb = send_ok(&b, BOB, &to_a, 2);
    let (b_paths, mut first, inner) = (b.paths.clone(), true, Arc::new(Mutex::new(None)));
    let inner2 = Arc::clone(&inner);
    let race = Hook::new(move |p| {
        if p == "before_push" && std::mem::take(&mut first) {
            let env = env_paths(b_paths.clone(), Box::new(NoSeam), far());
            *inner2.lock().unwrap() = Some(run_with_deadline(&env, WS_ID, Mode::Child, &Opts::default(), FAR_PASS).is_ok());
        }
        Ok(())
    });
    run_with_deadline(&env_in(&a, Box::new(race), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap();
    assert_eq!(*inner.lock().unwrap(), Some(true), "B's bootstrap at A's first push");
    let t = tip(&r2);
    assert_eq!(git(&r2.bare, &["rev-list", "--max-parents=0", &t]).lines().count(), 1);
    let paths = tip_paths(&r2);
    assert!(paths.contains(&format!("{to_b}/{ida}.json")) && paths.contains(&format!("{to_a}/{idb}.json")), "{paths:?}");
    assert_eq!(tip_blob(&r2, "ZYGO-MAIL").unwrap(), b"1\n");
    assert_eq!(git(&r2.bare, &["for-each-ref", "--format=%(refname)", "refs/heads/zygo/"]), "refs/heads/zygo/mail");
    assert_eq!(sync_in(&b).delivered, 1);
    // A's pass fetched again after the rejected push and imported B's message then (S§8.3): each message is in its
    // target's `new/` once, and A's next pass delivers nothing.
    assert_eq!(sync_in(&a).delivered, 0);
    assert_eq!((new_ids(&a, ALICE), new_ids(&b, BOB)), (vec![idb.clone()], vec![ida.clone()]));

    // 2.2 #10 (moved repository): a new home E with A's host, whose other workspace state holds A's identity.
    let e = machine(&r2, HOST_A, ALICE, A_NAME);
    let other = SyncPaths::new(&e.paths, OTHER_WS).unwrap();
    std::fs::create_dir_all(other.mail.parent().unwrap()).unwrap();
    for d in [&other.mail, &other.sync] {
        zygo_workspace::mail::ensure_mail_root(d).unwrap();
    }
    std::fs::copy(sp(&a).identity, &other.identity).unwrap();
    let refs0 = git(&r2.bare, &["for-each-ref"]);
    let o = sync_bin(&e, &[]);
    let moved = format!("{COLLISION} The workspace state {OTHER_WS} on this machine also holds that identity.\n");
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), moved));
    assert!(errors(&e).lines().any(|l| l.ends_with("mail sync: stopped: collision")), "{}", errors(&e));
    assert_eq!(git(&r2.bare, &["for-each-ref"]), refs0);

    // 2.3 #7 (own branch only): `zygo/mail` without `ZYGO-MAIL`, then `sync = "main"`, then no host.
    let r3 = remote();
    made.insert(git(&r3.bare, &["rev-parse", "refs/heads/main"]));
    let readme = git_in(&r3.bare, &["hash-object", "-w", "--stdin"], b"x\n");
    let tree = git_in(&r3.bare, &["mktree"], format!("100644 blob {readme}\tREADME\n").as_bytes());
    let root = git(&r3.bare, &["commit-tree", &tree, "-m", "test root"]);
    git(&r3.bare, &["update-ref", "refs/heads/zygo/mail", &root]);
    made.insert(root.clone());
    // FX-26 review F3: every ref of `r3` (`refs/heads/main` among them), compared after each run of D, as in 2.2.
    let refs3 = git(&r3.bare, &["for-each-ref"]);
    assert!(refs3.lines().any(|l| l.ends_with("\trefs/heads/main")) && refs3.lines().any(|l| l.ends_with("\trefs/heads/zygo/mail")), "{refs3}");
    let d = machine(&r3, HOST_B2, BOB, B_NAME);
    let o = sync_bin(&d, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{MARKER_LINE}\n")));
    assert_eq!(tip(&r3), root);
    assert_eq!(git(&r3.bare, &["for-each-ref"]), refs3);
    assert!(errors(&d).lines().any(|l| l.ends_with("mail sync: stopped: marker")), "{}", errors(&d));
    let st: Value = serde_json::from_slice(&std::fs::read(sp(&d).state).unwrap()).unwrap();
    assert_eq!(st["last_error"], "marker");
    d.config(&format!("[mail]\nhost = \"{HOST_B2}\"\n[mail.workspace.app]\nsync = \"main\"\n"));
    let o = sync_bin(&d, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{BAD_BRANCH}\n")));
    assert_eq!(git(&r3.bare, &["for-each-ref"]), refs3);
    let o = zygo_at(&d, &d.repo(), &["config", "check"], &[]);
    let line = "error: mail.workspace.app.sync: must match ^zygo/[a-z0-9][a-z0-9-]{0,31}$";
    assert_eq!(o.status.code(), Some(11), "{} {}", out_of(&o), err_of(&o));
    assert!(out_of(&o).lines().any(|l| l == line), "{}", out_of(&o));
    d.config("[mail]\n[mail.workspace.app]\nsync = \"zygo/mail\"\n");
    let o = sync_bin(&d, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{NO_HOST}\n")));
    assert_eq!(git(&r3.bare, &["for-each-ref"]), refs3);
    let o = zygo_at(&d, &d.repo(), &["config", "check"], &[]);
    let line = "error: mail.host: set [mail] host to turn on mail sync; zygo does not publish the system host name";
    assert_eq!(o.status.code(), Some(11), "{} {}", out_of(&o), err_of(&o));
    assert!(out_of(&o).lines().any(|l| l == line), "{}", out_of(&o));
    assert_eq!(tip(&r3), root);
    assert_eq!(git(&r3.bare, &["for-each-ref"]), refs3);

    // The scans (S§19 #24). A and B write no `errors.log` in this walk: each of their runs ends without a stop and
    // refuses no file (the #8 race is a rejected push that the pass fetches again, not a stop). E (collision) and D
    // (marker) stop and each write one.
    let (own_a, own_b) = (own_hosts_at_tip(&r2, &a, HOST_A), own_hosts_at_tip(&r2, &b, HOST_B));
    let found = scan(&[&r2, &r3], &made, &[
        // FX-26 R3-1: the #8 race delivered one message to each of A and B; E and D got none and made no session.
        Scanned { label: "A", h: &a, own_names: vec![A_NAME], own_hosts: own_a, synced: true, errors_log: false, zygo_commits: true,
            folders: vec![maildir(&a, ALICE, "new")] },
        Scanned { label: "B", h: &b, own_names: vec![B_NAME], own_hosts: own_b, synced: true, errors_log: false, zygo_commits: true,
            folders: vec![maildir(&b, BOB, "new")] },
        Scanned { label: "E", h: &e, own_names: vec![A_NAME], own_hosts: vec![HOST_A.into()], synced: false, errors_log: true, zygo_commits: true, folders: vec![] },
        Scanned { label: "D", h: &d, own_names: vec![B_NAME], own_hosts: vec![HOST_B2.into()], synced: false, errors_log: true, zygo_commits: false, folders: vec![] },
    ]);
    assert_eq!(found, Vec::<String>::new());
}

/// #14: a seam that stops the run at the first `point` only.
fn crash_once(point: &'static str) -> Hook {
    let mut first = true;
    Hook::new(move |p| if p == point && std::mem::take(&mut first) { Err(Crash(p)) } else { Ok(()) })
}

/// The ULIDs that `ack/<host>.json` at the tip of `r` lists.
fn acked(r: &Remote, host: &str) -> Vec<String> {
    let ack: Value = serde_json::from_slice(&tip_blob(r, &format!("ack/{host}.json")).unwrap()).unwrap();
    ack["imported"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect()
}

/// The ULIDs that the ack blob `oid` lists, read in the sync worktree of `h` (its objects are the repository's).
fn ack_blob(h: &Harness, oid: &str) -> Vec<String> {
    let ack: Value = serde_json::from_str(&git(&sp(h).worktree, &["cat-file", "blob", oid])).unwrap();
    ack["imported"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect()
}

/// FX-26 re-plan review round 1 F8: the ULIDs that `ack/<host>.json` lists in the index of the sync worktree of `h`
/// (what the next `branch::commit` starts from), and in each blob that `state.json` records for that path (`pushed`
/// and `pending`, S§7.5). Each of the index entry and the `pushed` record must exist.
fn local_acks(h: &Harness, host: &str) -> (Vec<String>, Vec<String>) {
    let path = format!("ack/{host}.json");
    let line = git(&sp(h).worktree, &["ls-files", "-s", "--", &path]);
    let oid = line.split_whitespace().nth(1).unwrap_or_else(|| panic!("no {path} in the sync index: {line:?}"));
    let st: Value = serde_json::from_slice(&std::fs::read(sp(h).state).unwrap()).unwrap();
    assert!(st["pushed"][&path].is_string(), "no pushed {path}: {st}");
    let recorded: Vec<String> = ["pushed", "pending"].iter()
        .filter_map(|m| st[m][&path].as_str())
        .flat_map(|o| ack_blob(h, o))
        .collect();
    (ack_blob(h, oid), recorded)
}

/// How often the imported log of `h` lists `id`.
fn imported_times(h: &Harness, id: &str) -> usize {
    std::fs::read_to_string(sp(h).imported).unwrap().lines().filter(|l| *l == id).count()
}

/// The message of the tip commit of `r`.
fn tip_message(r: &Remote) -> String {
    let c = git(&r.bare, &["cat-file", "commit", &tip(r)]);
    c.split_once("\n\n").map(|(_, m)| format!("{m}\n")).unwrap_or_default()
}

#[test] // S-C2, S§6.5, S§19 #24 (FX-26 walk 3): after #13 (the import cap), #14 (no second delivery), #17 (prune after
        // push, the lease) and #18 (prune keeps the unacknowledged, the clock a year later), no marker is in a log, a
        // state file, the usage store, a commit or a ref name, and every zygo commit has the S§6.5 message and identity
fn no_mail_content_after_the_import_and_prune_scenarios() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _h = hermetic(); // dropped after every machine of the walk, before `_g`
    let r = remote();
    let mut made = BTreeSet::from([git(&r.bare, &["rev-parse", "refs/heads/main"])]);
    let a = machine(&r, HOST_A, ALICE, A_NAME);
    let b = machine(&r, HOST_B, BOB, B_NAME);
    config(&a, HOST_A, "keep = \"1d\"\n", "");
    config(&b, HOST_B, "max_unread = 5000\nkeep = \"90d\"\n", "");
    for h in [&b, &a, &b] {
        let o = sync_bin(h, &[]);
        assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    }
    let (to_a, to_b) = (format!("{A_NAME}@{HOST_A}"), format!("{B_NAME}@{HOST_B}"));

    // 3.1 #13 (import cap): 1200 messages for bob-q7x@birch-q7x, one test commit.
    let ids: Vec<String> = (1..=1200).map(|n: u32| format!("01KQ7Y{n:020}")).collect();
    let files = tempfile::tempdir().unwrap();
    let mut list = String::new();
    for id in &ids {
        let p = files.path().join(format!("{id}.json"));
        std::fs::write(&p, message(id, &to_a, &to_b, &format!("{SUBJ}-c"))).unwrap();
        list.push_str(&format!("{}\n", p.display()));
    }
    let blobs = git_in(&r.bare, &["hash-object", "-w", "--stdin-paths"], list.as_bytes());
    let lines: Vec<String> = blobs.lines().zip(&ids).map(|(blob, id)| format!("100644 blob {blob}\t{id}.json")).collect();
    assert_eq!(lines.len(), 1200);
    made.insert(add_folder(&r.bare, &to_b, &lines));
    // FX-26 review round 2 F-3: each pass of 500 deliveries gets an hour, so the 500/500/200 result is the cap's
    // (`import::CANDIDATE_CAP`), never the 120 s pass deadline of a slow machine.
    let got: Vec<usize> = (0..3).map(|_| sync_in(&b).delivered).collect();
    assert_eq!(got, vec![500, 500, 200]);
    assert_eq!(new_ids(&b, BOB), ids);
    let log = std::fs::read_to_string(sp(&b).imported).unwrap();
    let logged: Vec<&str> = log.lines().collect();
    assert_eq!(logged.len(), 1200);
    assert_eq!(logged.iter().copied().collect::<BTreeSet<&str>>(), ids.iter().map(String::as_str).collect());
    let ack: BTreeSet<String> = acked(&r, HOST_B).into_iter().collect();
    assert_eq!(ack, ids.iter().cloned().collect());
    for _ in 0..2 {
        call(&tools(&b, BOB), "mail_read", json!({"max": 1000})).unwrap();
    }
    assert_eq!(new_count(&b, BOB), 0);

    // 3.2 #14 (no second delivery): a crash after A's push and before its outbox delete, then a prune, then A's next
    // pass; then a crash after each delivery step.
    let u = send_ok(&a, ALICE, &to_b, 30);
    let e = run_with_deadline(&env_in(&a, Box::new(crash_once("before_outbox_delete")), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: stopped at seam before_outbox_delete".into(), check: "seam" });
    assert!(sp(&a).outbox.join(format!("{u}.json")).is_file());
    assert_eq!(sync_in(&b).delivered, 1);
    run_with_deadline(&env_in(&b, Box::new(NoSeam), far()), WS_ID, Mode::Cli, &Opts { prune: true }, FAR_PASS).unwrap();
    let at_tip = |id: &str| tip_paths(&r).contains(&format!("{to_b}/{id}.json"));
    assert!(!at_tip(&u));
    sync_in(&b);
    assert!(!acked(&r, HOST_B).contains(&u));
    assert_eq!(sync_in(&a).pushed, 1); // the path is not at the tip: A adds it again
    assert!(at_tip(&u));
    assert_eq!(outbox::count(&sp(&a)).unwrap(), 0);
    assert_eq!(sync_in(&b).delivered, 0);
    assert_eq!((new_ids(&b, BOB), imported_times(&b, &u)), (vec![u.clone()], 1));
    assert!(acked(&r, HOST_B).contains(&u)); // acked again
    for (n, point) in (31..).zip(["after_incoming", "after_log", "after_new", "after_ack"]) {
        let v = send_ok(&a, ALICE, &to_b, n);
        assert_eq!(sync_in(&a).pushed, 1);
        let acks0 = local_acks(&b, HOST_B);
        let e = run_with_deadline(&env_in(&b, Box::new(crash_once(point)), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap_err();
        assert_eq!(e, SyncError::Stopped { line: format!("zygo: stopped at seam {point}"), check: "seam" });
        // FX-26 R3-13: the state that each crash point leaves, in the step order of `import::run` (`import.rs`): the
        // staged file in `incoming/`, then `after_incoming`; the ULID appended to the imported log, then `after_log`;
        // the move into `new/`, then `after_new`; the ULID added to the acked list in memory, then `after_ack`. The own
        // ack reaches the branch only with the pass's commit, which no crash point reaches, so at each point the tip's
        // `ack/<host>.json` does not list the ULID.
        // FX-26 re-plan review round 1 F8: `after_new` and `after_ack` are equal on disk. Between them `import::run`
        // only pushes to `out.delivered`, sets `out.read` and pushes to `out.acked` (`import.rs:386-391`), all in
        // memory, and the crash drops them; so the same state is asserted for both. The ack is written nowhere: the
        // sync worktree's index and each blob that `state.json` records for `ack/<host>.json` stay as they were
        // before the crashed run and do not list the ULID, and neither does the remote tip.
        let staged = sp(&b).incoming.join(format!("{BOB}.{v}.json"));
        let in_new = new_ids(&b, BOB).iter().filter(|i| **i == v).count();
        let want = match point { "after_incoming" => (true, 0, 0), "after_log" => (true, 0, 1), _ => (false, 1, 1) };
        assert_eq!((staged.is_file(), in_new, imported_times(&b, &v)), want, "{point}");
        assert!(!acked(&r, HOST_B).contains(&v), "{point}");
        let acks = local_acks(&b, HOST_B);
        assert_eq!(acks, acks0, "{point}");
        assert!(!acks.0.contains(&v) && !acks.1.contains(&v), "{point}: {acks:?}");
        // FX-26 review round 2 F-2: the recovery runs through the pass, as in production (S§8.3 step 5, then step 9).
        // Its `delivered` and its wake are not asserted here: the pass drops the count of `import::finish_incoming`,
        // and a message that reached `new/` in the crashed run is only acknowledged, so the recovered message gets no
        // count and no wake. That is the defect FX-30 (`docs/work/phase3/nodes/FX-30.md`), which asserts both.
        sync_in(&b);
        assert_eq!(new_ids(&b, BOB).iter().filter(|i| **i == v).count(), 1, "{point}");
        assert_eq!(imported_times(&b, &v), 1, "{point}");
        assert_eq!(std::fs::read_dir(sp(&b).incoming).map_or(0, Iterator::count), 0, "{point}");
        // F8: the reads above can see an ack: after the recovery pass the ULID is in the tip's ack, in the sync index
        // and in the recorded blob.
        let acks = local_acks(&b, HOST_B);
        assert!(acked(&r, HOST_B).contains(&v) && acks.0.contains(&v) && acks.1.contains(&v), "{point}: {acks:?}");
        assert_eq!(sync_in(&b).delivered, 0, "{point}");
    }

    // 3.3 #17 (prune after push): 999 commits; the pass whose push makes 1000 prunes, and the new root holds the
    // pushed message; then a push of another machine between the push and the prune push loses the lease.
    made.extend(grow(&r.bare, 999 - count(&r)));
    assert_eq!(sync_in(&a).pushed, 0);
    assert_eq!(count(&r), 999);
    let w = send_ok(&a, ALICE, &to_b, 60);
    assert_eq!(sync_in(&a).pushed, 1);
    assert_eq!(count(&r), 1);
    assert!(at_tip(&w));
    assert_eq!(tip_blob(&r, "ZYGO-MAIL").unwrap(), b"1\n");
    let paths = tip_paths(&r);
    for h in [HOST_A, HOST_B] {
        for p in [format!("registry/{h}.json"), format!("ack/{h}.json")] {
            assert!(paths.contains(&p), "{p}: {paths:?}");
        }
    }
    assert_eq!(tip_message(&r), MESSAGE);
    assert_eq!(sync_in(&b).delivered, 1);
    made.extend(grow(&r.bare, 999 - count(&r)));
    let x = send_ok(&a, ALICE, &to_b, 61);
    let (bare, moved) = (r.bare.clone(), Arc::new(Mutex::new(Vec::new())));
    let m2 = Arc::clone(&moved);
    let lease = Hook::new(move |p| {
        if p == "before_prune_push" {
            m2.lock().unwrap().push(advance(&bare));
        }
        Ok(())
    });
    assert_eq!(run_with_deadline(&env_in(&a, Box::new(lease), far()), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap().pushed, 1);
    let moved: Vec<String> = moved.lock().unwrap().drain(..).collect();
    assert_eq!(moved.len(), 1, "the prune was due once");
    made.extend(moved.iter().cloned());
    assert_eq!(tip(&r), moved[0]);
    assert_eq!(count(&r), 1001);
    assert!(at_tip(&x));
    sync_in(&a); // the next pass prunes again
    assert_eq!(count(&r), 1);
    assert!(at_tip(&x));

    // 3.4 #18 (prune keeps the unacknowledged), last: the clock moves a year. B is idle after its push of u0.
    let u0 = send_ok(&b, BOB, &to_a, 40);
    assert_eq!(sync_in(&b).pushed, 1);
    let u1 = send_ok(&a, ALICE, &to_b, 41);
    let u2 = send_ok(&a, ALICE, &to_b, 42);
    let rep = sync_in(&a);
    assert_eq!((rep.pushed, rep.delivered), (2, 1));
    assert!(acked(&r, HOST_A).contains(&u0));
    let year = t0() + YEAR;
    let at = |h: &Harness| env_now(h.paths.clone(), Box::new(NoSeam), Box::new(NoProbe), Box::new(NoSpawn), Box::new(NoWake), year);
    run_with_deadline(&at(&a), WS_ID, Mode::Cli, &Opts { prune: true }, FAR_PASS).unwrap();
    let paths = tip_paths(&r);
    assert!(!paths.contains(&format!("{to_a}/{u0}.json")), "{paths:?}");
    for id in [&u1, &u2] {
        assert!(paths.contains(&format!("{to_b}/{id}.json")), "{id}: {paths:?}");
    }
    for h in [HOST_A, HOST_B] {
        for p in [format!("registry/{h}.json"), format!("ack/{h}.json")] {
            assert!(paths.contains(&p), "{p}: {paths:?}");
        }
    }
    let got: Vec<usize> = (0..2).map(|_| run_with_deadline(&at(&b), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap().delivered).collect();
    assert_eq!(got, vec![2, 0]);
    let new_b = new_ids(&b, BOB);
    for id in [&u1, &u2] {
        assert_eq!(new_b.iter().filter(|i| *i == id).count(), 1, "{id}");
        assert!(acked(&r, HOST_B).contains(id), "{id}");
    }
    run_with_deadline(&at(&a), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap();
    assert!(!acked(&r, HOST_A).contains(&u0)); // S§7.4: the owner drops the pruned ULID

    // The scans (S§19 #24). A and B write no `errors.log` in this walk: its only stops are the #14 seam crashes, and
    // a crash writes no line (`sync.rs` `Fail::Crash` → `Stopped { check: "seam" }`, no `log_error`); every other run
    // ends without a stop or a refusal.
    let (own_a, own_b) = (own_hosts_at_tip(&r, &a, HOST_A), own_hosts_at_tip(&r, &b, HOST_B));
    // FX-26 R3-1: A got u0; B read the 1200 #13 messages (`cur/`) through its MCP session and holds the later ones.
    let found = scan(&[&r], &made, &[
        Scanned { label: "A", h: &a, own_names: vec![A_NAME], own_hosts: own_a, synced: true, errors_log: false, zygo_commits: true,
            folders: vec![maildir(&a, ALICE, "new")] },
        Scanned { label: "B", h: &b, own_names: vec![B_NAME], own_hosts: own_b, synced: true, errors_log: false, zygo_commits: true,
            folders: vec![maildir(&b, BOB, "new"), maildir(&b, BOB, "cur"), b.root().join(format!("sess-{BOB}"))] },
    ]);
    assert_eq!(found, Vec::<String>::new());
}

/// The env of `h` at `now` with the counting spawn seam `c` and no other effect.
fn env_count(h: &Harness, c: &Arc<AtomicUsize>, now: SystemTime) -> Env {
    env_now(h.paths.clone(), Box::new(NoSeam), Box::new(NoProbe), Box::new(Count(Arc::clone(c))), Box::new(NoWake), now)
}
/// The whole seconds of `t` since the Unix epoch.
fn unix(t: SystemTime) -> u64 { t.duration_since(UNIX_EPOCH).unwrap().as_secs() }
/// `n` seconds.
fn secs(n: u64) -> Duration { Duration::from_secs(n) }

/// #16: a run of A in `Mode::Child` whose seam, at the first `point`, sends one message to `to` and then runs `then`
/// with A's paths. Returns the outer report's pass count, what `then` gave, and the id of the message.
fn starter<T: Send + 'static>(a: &Harness, point: &'static str, to: &str, n: u32,
    then: impl FnOnce(&Paths) -> T + Send + 'static) -> (u32, T, String) {
    let (paths, to) = (a.paths.clone(), to.to_string());
    let got = Arc::new(Mutex::new(None));
    let got2 = Arc::clone(&got);
    let mut then = Some(then);
    let h = Hook::new(move |p| {
        if p == point && let Some(f) = then.take() {
            let id = send_paths(&paths, ALICE, &to, n);
            *got2.lock().unwrap() = Some((f(&paths), id));
        }
        Ok(())
    });
    let rep = run_with_deadline(&env_in(a, Box::new(h), far()), WS_ID, Mode::Child, &Opts::default(), FAR_PASS).unwrap();
    let (t, id) = got.lock().unwrap().take().unwrap_or_else(|| panic!("the seam {point} ran"));
    (rep.passes, t, id)
}

#[test] // S-C2, S§6.5, S§19 #24 (FX-26 walk 4): after #21 (remote send rules), #28 (terminal escape), #16 (lost wakeup),
        // #23 (a wake through the multiplexer stub, the `Stop` block), #22 (the rate limit through the hook path, a send
        // with no gap, a child that does not start), a session-log line, #27 (agent text), #25 (config layers) and #6
        // (`unknown`, the URL forms, the cache, the probe command, and the production probe through a `PATH` stub), no
        // marker is in a log, a state file, a session log, the usage store, a commit or a ref name, and every zygo commit
        // has the S§6.5 message and identity
fn no_mail_content_after_the_hook_wake_and_text_scenarios() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _h = hermetic(); // dropped after every machine of the walk, before `_g`
    let r = remote();
    let mut made = BTreeSet::from([git(&r.bare, &["rev-parse", "refs/heads/main"])]);
    let a = machine(&r, HOST_A, ALICE, A_NAME);
    let b = machine(&r, HOST_B, BOB, B_NAME);
    a.store(WS_ID).update(Duration::from_secs(5), &mut |w| {
        w.members.insert(ANON.into(), MemberRecord { name: None, ..MemberRecord::new(Agent::ClaudeCode) });
        Ok(())
    }).unwrap();
    b.store(WS_ID).update(Duration::from_secs(5), &mut |w| {
        w.members.get_mut(BOB).unwrap().zygo_session = Some("sbob".into());
        Ok(())
    }).unwrap();
    let held = live(&b.paths, "sbob");
    for h in [&b, &a, &b] {
        let o = sync_bin(h, &[]);
        assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    }
    let (to_a, to_b) = (format!("{A_NAME}@{HOST_A}"), format!("{B_NAME}@{HOST_B}"));
    let mcp_send = |key: &str, to: &str, n: u32| {
        call(&tools(&a, key), "mail_send", json!({"to": to, "subject": format!("{SUBJ}-{n}"), "body": format!("{BODY}-{n}")}))
    };

    // 4.1 #21 (remote send rules): a member with no name, then a send to `cli@<host>`, through the CLI and MCP.
    let (code, out, err) = send(&a, ANON, &to_b, 50);
    assert_eq!((code, out.as_str(), err), (1, "", format!("{NO_NAME}\n")));
    assert_eq!(mcp_send(ANON, &to_b, 50), Err(ToolError::Failed(NO_NAME.into())));
    let cli = format!("cli@{HOST_B}");
    let (code, out, err) = send(&a, ALICE, &cli, 51);
    assert_eq!((code, out.as_str(), err), (1, "", format!("{RESERVED}\n")));
    assert_eq!(mcp_send(ALICE, &cli, 51), Err(ToolError::Failed(RESERVED.into())));
    assert_eq!(outbox::count(&sp(&a)).unwrap(), 0);

    // 4.2 #28 (terminal escape): `zygo mail watch` prints a remote subject that holds `\x1b[2J` as escaped text.
    let (code, out, err) = send_subject(&env_in(&a, Box::new(NoSeam), far()), &a.paths, ALICE, &to_b,
        &format!("{SUBJ}-52\x1b[2J"), &format!("{BODY}-52"));
    let e52 = sent_id(code, &out, &err, &to_b);
    assert_eq!(sync_in(&a).pushed, 1);
    assert_eq!(sync_in(&b).delivered, 1);
    let watched = Arc::new(AtomicUsize::new(0));
    let env = env_count(&b, &watched, t0());
    let ws_b = b.store(WS_ID).read().unwrap().unwrap();
    let (i, mut out) = (Instant::now(), Vec::new());
    let code = watch_with(&mail_dir(&b.paths, WS_ID), BOB, Duration::ZERO, Duration::ZERO, &|| i, &mut out,
        &mut watch_sync_step(&env, &ws_b));
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8(out.clone()).unwrap(), format!("{e52} {to_a} {SUBJ}-52\\u{{1b}}[2J\n"));
    assert!(!out.contains(&0x1b));
    assert_eq!(watched.load(Ordering::SeqCst), 1);
    call(&tools(&b, BOB), "mail_read", json!({"max": 1000})).unwrap();
    assert_eq!(new_count(&b, BOB), 0);

    // 4.3 #16 (lost wakeup), on A in `Mode::Child`.
    let at_tip = |id: &str| tip_paths(&r).contains(&format!("{to_b}/{id}.json"));
    // (a) A second sender and a second child between the end of the holder's pass and its release: the child fails
    // the lock and marks; the holder runs a second pass.
    let (passes, inner, l1) = starter(&a, "before_release", &to_b, 53, |p: &Paths| {
        run_with_deadline(&env_paths(p.clone(), Box::new(NoSeam), far()), WS_ID, Mode::Child, &Opts::default(), FAR_PASS).map(|r| r.passes)
    });
    assert_eq!((inner, passes), (Err(SyncError::Busy), 2));
    assert!(at_tip(&l1));
    assert!(!sp(&a).again.exists());
    // (b) A starter between the holder's release and its look takes the free lock and runs the pass itself.
    let (passes, inner, l2) = starter(&a, "after_release", &to_b, 54, |p: &Paths| {
        run_with_deadline(&env_paths(p.clone(), Box::new(NoSeam), far()), WS_ID, Mode::Child, &Opts::default(), FAR_PASS).map(|r| r.passes)
    });
    assert_eq!((inner, passes), (Ok(1), 1));
    assert!(at_tip(&l2));
    // (c) A starter that has written its mark there but not reached the lock: the holder's look sees the mark.
    let (passes, (), l3) = starter(&a, "after_release", &to_b, 55, |p: &Paths| {
        mark_again(&SyncPaths::new(p, WS_ID).unwrap()).unwrap();
    });
    assert_eq!(passes, 2);
    assert!(at_tip(&l3));
    assert!(!sp(&a).again.exists());
    // B imports and reads the three, so 4.4 counts from an empty inbox.
    assert_eq!(sync_in(&b).delivered, 3);
    call(&tools(&b, BOB), "mail_read", json!({"max": 1000})).unwrap();
    assert_eq!(new_count(&b, BOB), 0);

    // 4.4 #23 (wake): an idle bob with a live launcher is woken through the multiplexer stub once within `wake_gap`;
    // a busy bob is not, and its `Stop` hook blocks with the M§7.1 line.
    let typed = Typed::default();
    let woke = |idle: bool, typed: &Typed, now: SystemTime| {
        let wake = StubWake { idle, typed: Arc::clone(typed) };
        let env = env_now(b.paths.clone(), Box::new(NoSeam), Box::new(NoProbe), Box::new(NoSpawn), Box::new(wake), now);
        run_with_deadline(&env, WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap().delivered
    };
    send_ok(&a, ALICE, &to_b, 56);
    sync_in(&a);
    assert_eq!(woke(true, &typed, t0()), 1);
    send_ok(&a, ALICE, &to_b, 57);
    sync_in(&a);
    assert_eq!(woke(true, &typed, t0() + secs(10)), 1); // within wake_gap (30 s, M§11 default)
    assert_eq!(*typed.lock().unwrap(), vec![("%3".to_string(), WAKE_TEXT.to_string())]);
    let busy = Typed::default();
    send_ok(&a, ALICE, &to_b, 58);
    sync_in(&a);
    assert_eq!(woke(false, &busy, t0() + secs(100)), 1);
    assert!(busy.lock().unwrap().is_empty());
    assert_eq!(new_count(&b, BOB), 3);
    let sess_b = SessionDir::new(b.root().join(format!("sess-{BOB}")));
    let none = Arc::new(AtomicUsize::new(0));
    let stop = |active: bool| json!({"hook_event_name": "Stop", "stop_hook_active": active});
    let block = hook(&b, &sess_b, "stop", &stop(false), &env_count(&b, &none, t0() + secs(100)));
    assert_eq!(block, Some(stop_block(3)));
    assert_eq!(stop_block(3), r#"{"decision":"block","reason":"zygo: 3 new messages; read them with mail_read"}"#);
    assert_eq!(hook(&b, &sess_b, "stop", &stop(true), &env_count(&b, &none, t0() + secs(100))), None);

    // 4.5 #22 (rate limit) and `session.log`: with the clock fixed, ten `tool-end` events start one child; after the
    // clock moves 60 s the next event starts a second; a send starts a child with no gap.
    let tt = t0() + secs(600);
    let c = Arc::new(AtomicUsize::new(0));
    for _ in 0..10 {
        assert_eq!(hook(&b, &sess_b, "tool-end", &json!({}), &env_count(&b, &c, tt)), None);
    }
    assert_eq!((c.load(Ordering::SeqCst), read_last_start(&sp(&b))), (1, Some(unix(tt))));
    hook(&b, &sess_b, "tool-end", &json!({}), &env_count(&b, &c, tt + secs(59)));
    assert_eq!(c.load(Ordering::SeqCst), 1);
    hook(&b, &sess_b, "tool-end", &json!({}), &env_count(&b, &c, tt + secs(60)));
    assert_eq!((c.load(Ordering::SeqCst), read_last_start(&sp(&b))), (2, Some(unix(tt) + 60)));
    let (c2, a_last) = (Arc::new(AtomicUsize::new(0)), read_last_start(&sp(&a)));
    for n in 59..62 {
        let (code, out, err) = send_env(&env_count(&a, &c2, tt), &a, ALICE, &to_b, n, &format!("{BODY}-{n}"));
        sent_id(code, &out, &err, &to_b);
    }
    assert_eq!((c2.load(Ordering::SeqCst), read_last_start(&sp(&a))), (3, a_last));
    let fail = env_now(b.paths.clone(), Box::new(NoSeam), Box::new(NoProbe), Box::new(FailSpawn), Box::new(NoWake), tt + secs(120));
    hook(&b, &sess_b, "tool-end", &json!({}), &fail);
    let line = format!("workspace hook tool-end: cannot start the mail sync child: {}", std::io::Error::from(std::io::ErrorKind::NotFound));
    assert!(errors(&b).lines().any(|l| l.ends_with(&line)), "{line}: {}", errors(&b));
    // An `activity.json` that is a folder: the `prompt` hook writes its line to the session log.
    let activity = sess_b.root.join("activity.json");
    if activity.is_file() {
        std::fs::remove_file(&activity).unwrap();
    }
    std::fs::create_dir(&activity).unwrap();
    hook(&b, &sess_b, "prompt", &json!({"hook_event_name": "UserPromptSubmit"}), &env_count(&b, &c, tt + secs(200)));
    let slog = std::fs::read_to_string(sess_b.log()).unwrap();
    assert!(slog.lines().any(|l| l.starts_with("workspace hook prompt: ")), "{slog}");
    std::fs::remove_dir(&activity).unwrap();

    // 4.6 #27 (agent text): after a sync on A that sees B's registry, the session text of A's member lists
    // bob-q7x@birch-q7x in the item 8 clause, and `mail_list` returns it.
    assert_eq!(sync_in(&a).pushed, 3);
    assert_eq!(read_cache(&sp(&a)).hosts, [(HOST_B.to_string(), vec![B_NAME.to_string()])].into());
    let ta = tools(&a, ALICE);
    let sess_a = SessionDir::new(a.root().join(format!("sess-{ALICE}")));
    let view = SessionView::load(&sess_a).unwrap();
    let mut out = Vec::new();
    // FX-26 review F4: the SessionStart payload and the assertion of `crates/zygo-hooks/tests/mail_sync_text.rs`
    // `item_8_names_bob_at_b_after_the_member_list` (`json!({})`, `text == "TEXT\n\n" + item(Some(clause))`).
    let got = session_start_with_sync(&json!({}), &sess_a, &view, &a.paths, &a.root().join("missing"), &mut out, "TEXT",
        &env_count(&a, &c2, tt + secs(200))).unwrap();
    assert_eq!(got, None);
    let v: Value = serde_json::from_slice(&out).unwrap();
    let text = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    let clause = other_machines(std::slice::from_ref(&to_b)).unwrap();
    assert_eq!(clause, format!("Other machines: {to_b}; mail them by the full address."));
    let members = format!("Workspace mail: you are `{A_NAME}` in workspace `app`. Members: `{A_NAME}`.");
    assert_eq!(text, format!("TEXT\n\n{members} {clause} {MAIL_TAIL}"));
    let (at_members, at_clause) = (text.find(&members).unwrap(), text.find(&clause).unwrap());
    assert!(at_members + members.len() < at_clause, "the clause comes after the member list: {text}");
    assert_eq!(call(&ta, "mail_list", json!({})).unwrap()["remote"], json!([to_b]));

    // 4.7 #25 (config layers): `[mail]` in `.zygo.toml` on the default branch is ignored with the existing line; in
    // team config it is dropped with one warning.
    // FX-26 review round 2 F-5: in the seed clone of `remote()` (no other clone pushes `main`); its `.zygo.toml` is the
    // one test input that `sweep_remote` allows.
    let seed = r.bare.parent().unwrap().join("seed");
    std::fs::write(seed.join(".zygo.toml"), format!("[mail]\nhost = \"{HOST_R1}\"\n")).unwrap();
    git(&seed, &["add", ".zygo.toml"]);
    git(&seed, &["commit", "-q", "-m", "repo config"]);
    made.insert(git(&seed, &["rev-parse", "HEAD"]));
    git(&seed, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&a.repo(), &["fetch", "-q", "origin"]);
    let team = a.root().join("team");
    std::fs::create_dir(&team).unwrap();
    // A team layer makes the server's catalog authoritative (`crates/zygo-config/src/layers.rs:495-501`), so the team
    // file carries the built-in catalog, as a server's does, then its `[mail]` table.
    let catalog = include_str!("../../zygo-config/data/catalog.toml");
    std::fs::write(team.join("team.toml"), format!("{catalog}\n[mail]\nhost = \"{HOST_R2}\"\n")).unwrap();
    let o = zygo_at(&a, &a.repo(), &["config", "check"], &[("ZYGO_SESSION_DIR", team.as_os_str())]);
    assert_eq!((o.status.code(), out_of(&o).as_str()), (Some(0), "config ok\n"), "{}", err_of(&o));
    let mut lines: Vec<String> = err_of(&o).lines().map(String::from).collect();
    lines.sort();
    let mut want = vec![REPO_LINE.to_string(), TEAM_LINE.to_string()];
    want.sort();
    assert_eq!(lines, want);
    let both: Vec<u8> = o.stdout.iter().chain(&o.stderr).copied().collect();
    assert_eq!(text_findings("config check", &both, &MARKERS), Vec::<String>::new());
    sync_in(&a);
    let paths = tip_paths(&r);
    assert!(paths.contains(&format!("registry/{HOST_A}.json")), "{paths:?}");
    for h in [HOST_R1, HOST_R2] {
        assert!(!paths.contains(&format!("registry/{h}.json")), "{paths:?}");
    }

    // 4.8 #6 (public check), last: the clock moves. The URL table, then the probe command (not run), then `unknown`
    // with an `https` push URL, the cache at 59 minutes and a new check at 61.
    assert_eq!(map_url(&r.url), Mapped::Local);
    let https = || Mapped::Https("https://example.invalid/p".into());
    assert_eq!(map_url("https://u:p@example.invalid/p"), https());
    assert_eq!(map_url("git@example.invalid:p"), https());
    assert_eq!(map_url("ssh://user@example.invalid:22/p"), https());
    assert_eq!(map_url("http://example.invalid/p"), Mapped::Unknown);
    assert_eq!(map_url("ext::sh -c x"), Mapped::Unknown);
    let home = tempfile::tempdir().unwrap();
    let url = "https://example.invalid/r.git";
    let cmd = probe_command(url, home.path());
    let args: Vec<&OsStr> = cmd.get_args().collect();
    let tail: Vec<&OsStr> = ["-c", "credential.helper=", "-c", "core.askPass=", "ls-remote", "--heads", url].map(OsStr::new).to_vec();
    assert!(args.ends_with(&tail), "{args:?}");
    let envs: std::collections::BTreeMap<&OsStr, Option<&OsStr>> = cmd.get_envs().collect();
    for (k, v) in [("GIT_ASKPASS", None), ("SSH_ASKPASS", None), ("GIT_TERMINAL_PROMPT", Some(OsStr::new("0"))),
        ("GIT_CONFIG_NOSYSTEM", Some(OsStr::new("1"))), ("HOME", Some(home.path().as_os_str()))] {
        assert_eq!(envs.get(OsStr::new(k)), Some(&v), "{k}: {envs:?}");
    }
    git(&a.repo(), &["remote", "set-url", "--push", "origin", &format!("https://example.invalid/{B_NAME}/u.git")]);
    let refs0 = git(&r.bare, &["for-each-ref"]);
    let t6 = t0() + secs(7200);
    let probes = Arc::new(AtomicUsize::new(0));
    let unknown = |now: SystemTime| {
        let probe = CountProbe(Arc::clone(&probes), Verdict::Unknown("probe says no".into()));
        env_now(a.paths.clone(), Box::new(NoSeam), Box::new(probe), Box::new(NoSpawn), Box::new(NoWake), now)
    };
    let note = vec!["zygo: cannot check if the remote is public: probe says no".to_string()];
    let rep = run_with_deadline(&unknown(t6), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap();
    assert_eq!((&rep.notes, rep.passes, rep.pushed), (&note, 1, 0));
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    let cached = env_now(a.paths.clone(), Box::new(NoSeam), Box::new(NoProbe), Box::new(NoSpawn), Box::new(NoWake), t6 + secs(59 * 60));
    assert_eq!(run_with_deadline(&cached, WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap().notes,
        vec!["zygo: cannot check if the remote is public: the last check, under 1 h ago, could not tell".to_string()]);
    let rep = run_with_deadline(&unknown(t6 + secs(61 * 60)), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS).unwrap();
    assert_eq!((probes.load(Ordering::SeqCst), &rep.notes), (2, &note));
    git(&a.repo(), &["config", "--unset", "remote.origin.pushurl"]);

    // 4.9 #6 (the production probe, FX-26 review F1): `zygo mail sync` as the binary runs `public::LsRemote`, whose
    // `probe_command` → `sync_git` → `git_command` starts `git` from `PATH`, and the binary's `PATH` is `a.bin`. A stub
    // `a.bin/git` answers `ls-remote` and runs the real git for every other command, so no network host is asked. The
    // binary's clock is the production clock: the 4.8 cache is at a later injected time (`c.at <= now` fails) and for
    // other URLs, so each run probes. Each URL is new, so no cached verdict decides a run.
    let real_git = which::which("git").unwrap();
    let calls = a.root().join("ls-remote-calls");
    let tmp = a.root().join("tmp"); // the binary's TMPDIR: `LsRemote` makes its empty home here
    std::fs::create_dir(&tmp).unwrap();
    // FX-26 review round 2 F-7, R3-12: each proxy variable is recorded too; S§8.2 step 3 keeps the proxy variables.
    // FX-26 re-plan review round 1 F5: the lower-case forms too (git and curl read `http_proxy`, `https_proxy`,
    // `all_proxy` and `no_proxy`), each with its own value.
    const PROXIES: [(&str, &str); 8] = [("HTTPS_PROXY", "http://proxy.invalid:3128"), ("HTTP_PROXY", "http://proxy-http.invalid:3128"),
        ("ALL_PROXY", "socks5://proxy-all.invalid:1080"), ("NO_PROXY", "a.invalid,b.invalid,.c.invalid"),
        ("https_proxy", "http://lc-proxy.invalid:3128"), ("http_proxy", "http://lc-proxy-http.invalid:3128"),
        ("all_proxy", "socks5://lc-proxy-all.invalid:1080"), ("no_proxy", "d.invalid,e.invalid,.f.invalid")];
    let mut probe_env = vec!["GIT_ASKPASS", "SSH_ASKPASS", "GIT_TERMINAL_PROMPT", "GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM"];
    probe_env.extend(PROXIES.map(|(k, _)| k));
    // FX-26 review round 2 F-8: the stub refuses a `push` or a `fetch` whose remote URL (the argument after `--`, or
    // the URL that `git remote get-url [--push]` gives for a remote name) or argument list names `example.invalid`.
    // It logs the call to `refused` and exits 128 without running the real git, so a later change that makes this
    // pass commit something still asks no network host.
    // FX-26 re-plan review round 2 F1: the answer runs in a subshell, and the stub then logs `done <exit code>`, so the
    // record of a probe that the 10 s probe limit killed (its process group) has no `done` line.
    let refused = a.root().join("refused-calls");
    // FX-26 re-plan 2 c: the stub also logs to `subs` the subcommand of each `push`, `fetch` or `ls-remote` call, before
    // it answers, refuses or runs the real git, so a run's network calls can be asserted whole.
    let subs = a.root().join("network-calls");
    const STUB: &str = r#"#!/bin/sh
sub=
for x in "$@"; do
  case "$x" in push|fetch|ls-remote) sub=$x; break ;; esac
done
if [ -n "$sub" ]; then printf '%s\n' "$sub" >> '@SUBS@'; fi
if [ "$sub" = push ] || [ "$sub" = fetch ]; then
  remote= after=
  for x in "$@"; do
    if [ -n "$after" ]; then remote=$x; break; fi
    if [ "$x" = -- ]; then after=1; fi
  done
  url=$remote
  case "$remote" in
    ''|*:*) ;;
    *) if [ "$sub" = push ]; then url=$('@REAL@' remote get-url --push -- "$remote" 2>/dev/null)
       else url=$('@REAL@' remote get-url -- "$remote" 2>/dev/null); fi ;;
  esac
  case "$url $*" in
    *example.invalid*) printf '%s %s\n' "$sub" "$url" >> '@REFUSED@'; exit 128 ;;
  esac
fi
if [ "$sub" = ls-remote ]; then
  {
    printf 'argv\n'; for y in "$@"; do printf '%s\n' "$y"; done
@ENV@    printf 'HOME=%s\n' "$HOME"
    e=empty
    for f in "$HOME"/* "$HOME"/.[!.]* "$HOME"/..?*; do [ -e "$f" ] && e=not-empty; done
    printf 'HOME is %s\n' "$e"
  } >> '@CALLS@'
  ( @ANSWER@ )
  rc=$?
  printf 'done %s\n' "$rc" >> '@CALLS@'
  exit "$rc"
fi
exec '@REAL@' "$@"
"#;
    // Author's call 2: the script is written under `SERIAL` (this test holds it), so no fork of another test thread
    // holds it open for writing when the binary runs it (ETXTBSY).
    let stub = |answer: &str| {
        let env_lines: String = probe_env.iter().map(|k| format!("    printf '{k}=%s\\n' \"${{{k}-(unset)}}\"\n")).collect();
        let body = STUB.replace("@REAL@", &real_git.display().to_string())
            .replace("@REFUSED@", &refused.display().to_string())
            .replace("@CALLS@", &calls.display().to_string())
            .replace("@SUBS@", &subs.display().to_string())
            .replace("@ENV@", &env_lines)
            .replace("@ANSWER@", answer);
        std::fs::remove_file(a.bin.join("git")).unwrap();
        std::fs::write(a.bin.join("git"), body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(a.bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    // The binary gets ask-pass programs and a prompt on: the probe must remove and turn them off. It also gets each
    // proxy variable, which the probe must keep (F-7, R3-12).
    let askpass = a.root().join("no-askpass");
    let probe_run = || {
        assert_eq!(outbox::count(&sp(&a)).unwrap(), 0, "the binary syncs only an empty outbox");
        let mut extra = vec![("GIT_ASKPASS", askpass.as_os_str()), ("SSH_ASKPASS", askpass.as_os_str()),
            ("GIT_TERMINAL_PROMPT", OsStr::new("1")), ("TMPDIR", tmp.as_os_str())];
        extra.extend(PROXIES.map(|(k, v)| (k, OsStr::new(v))));
        zygo_at(&a, a.root(), &["mail", "sync"], &extra)
    };
    let refs0 = git(&r.bare, &["for-each-ref"]);
    // `public`: a ref line and exit 0. The sync stops at `sync::public_check`, before `branch::fetch`: its one network
    // call is the probe (FX-26 re-plan 2 c: `subs` holds `ls-remote` only, no `fetch`).
    git(&a.repo(), &["remote", "set-url", "--push", "origin", "https://u:p@example.invalid/p.git"]);
    stub("printf 'deadbeef\\trefs/heads/main\\n'; exit 0");
    let o = probe_run();
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(1), String::new(), format!("{PUBLIC}\n")));
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    let public_log = std::fs::read_to_string(&calls).unwrap();
    let public_subs = std::fs::read_to_string(&subs).unwrap();
    assert_eq!(public_subs, "ls-remote\n");
    // `private`: the S§8.2 step 4 text and exit 128. The sync goes on. FX-26 re-plan review round 2 F1: an `Unknown`
    // verdict also lets the sync go on (`sync::public_check` only adds a note), so the run's whole result is asserted:
    // exit 0, the summary line as the only stdout, and one probe that ran to its end. FX-26 re-plan 2 c: stderr is
    // exactly empty. `sync_cli` (`cmd/mail.rs`) gives an `Ok` report to `print_report`, which writes each of
    // `Report.notes` on stderr and nothing else there; the `Private` verdict adds no note (`sync::public_check`), and
    // a pass with nothing to send or prune adds none either. After the probe the sync fetches (`branch::fetch`).
    git(&a.repo(), &["remote", "set-url", "--push", "origin", "https://example.invalid/q.git"]);
    stub("echo 'remote: Repository not found.' >&2; exit 128");
    let o = probe_run();
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), "zygo: mail sync: 0 sent, 0 delivered\n".to_string(), String::new()));
    let all_subs = std::fs::read_to_string(&subs).unwrap();
    let private_subs = all_subs.strip_prefix(&public_subs).unwrap_or_else(|| panic!("{all_subs}"));
    assert_eq!(private_subs, "ls-remote\nfetch\n");
    let private_log = std::fs::read_to_string(&calls).unwrap().strip_prefix(&public_log).unwrap_or_else(|| panic!("{public_log}")).to_string();
    // FX-26 re-plan 2 review round 3 F1: `unknown`, a failure text that is not in `public::PRIVATE_TEXT`, and exit 128.
    // `public::classify` gives `Unknown("git ls-remote failed: <first stderr line>")`, `sync::public_check` adds the
    // note `zygo: cannot check if the remote is public: <reason>` and the sync goes on (it fetches), and `print_report`
    // (`cmd/mail.rs`) writes that note, and only it, on stderr, then the summary on stdout. Each URL is new, so no
    // cached verdict decides the run.
    git(&a.repo(), &["remote", "set-url", "--push", "origin", "https://example.invalid/s.git"]);
    stub("echo 'remote: no idea' >&2; exit 128");
    let o = probe_run();
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), "zygo: mail sync: 0 sent, 0 delivered\n".to_string(),
        "zygo: cannot check if the remote is public: git ls-remote failed: remote: no idea\n".to_string()));
    let all_subs2 = std::fs::read_to_string(&subs).unwrap();
    let unknown_subs = all_subs2.strip_prefix(&all_subs).unwrap_or_else(|| panic!("{all_subs2}"));
    assert_eq!(unknown_subs, "ls-remote\nfetch\n");
    // Each run asked once, and that probe ran to its end (its `done` line), with the arguments that `probe_command`
    // builds for the `https` form (no user info) and the S§8.2 step 3 environment: no ask-pass, no prompt, no global or
    // system config, `HOME` an empty folder.
    let log = std::fs::read_to_string(&calls).unwrap();
    let unknown_log = log.strip_prefix(&format!("{public_log}{private_log}")).unwrap_or_else(|| panic!("{log}"));
    for (run, rc) in [(&public_log[..], 0), (&private_log[..], 128), (unknown_log, 128)] {
        let recs: Vec<&str> = run.split("argv\n").filter(|r| !r.is_empty()).collect();
        assert_eq!(recs.len(), 1, "{run}");
        assert!(recs[0].ends_with(&format!("\ndone {rc}\n")), "{run}");
    }
    let records: Vec<&str> = log.split("argv\n").filter(|r| !r.is_empty()).collect();
    assert_eq!(records.len(), 3, "{log}");
    for ((rec, url), rc) in records.iter()
        .zip(["https://example.invalid/p.git", "https://example.invalid/q.git", "https://example.invalid/s.git"]).zip([0, 128, 128]) {
        let lines: Vec<&str> = rec.lines().collect();
        let k = lines.iter().position(|l| l.starts_with("GIT_ASKPASS=")).unwrap_or_else(|| panic!("{rec}"));
        let want: Vec<String> = probe_command(url, Path::new("/unused")).get_args().map(|x| x.to_string_lossy().into_owned()).collect();
        assert_eq!(lines[..k], want, "{rec}");
        let mut env_want: Vec<String> = ["GIT_ASKPASS=(unset)", "SSH_ASKPASS=(unset)", "GIT_TERMINAL_PROMPT=0", "GIT_CONFIG_NOSYSTEM=1",
            "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null"].map(String::from).to_vec();
        env_want.extend(PROXIES.iter().map(|(k, v)| format!("{k}={v}")));
        let n = env_want.len();
        assert_eq!(lines[k..k + n], env_want, "{rec}");
        let home = Path::new(lines[k + n].strip_prefix("HOME=").unwrap_or_else(|| panic!("{rec}")));
        assert!(home.starts_with(&tmp) && home != tmp, "{rec}");
        assert_eq!(lines[k + n + 1..], ["HOME is empty".to_string(), format!("done {rc}")], "{rec}");
    }
    // F-8: neither run that must stay local tried a push or a fetch to an `example.invalid` URL.
    assert!(!refused.exists(), "{}", std::fs::read_to_string(&refused).unwrap_or_default());
    // F-8: the guard itself. A push to `origin` (its push URL is the `https` form) and a fetch of an `example.invalid`
    // URL each exit 128, run no real git, and are logged; the bare repository's refs do not change.
    let guarded = |args: &[&str]| {
        Command::new(a.bin.join("git")).args(args).current_dir(a.repo())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").stdin(Stdio::null()).output().unwrap()
    };
    assert_eq!(guarded(&["push", "--porcelain", "--", "origin", "HEAD:refs/heads/x"]).status.code(), Some(128));
    assert_eq!(guarded(&["fetch", "--no-tags", "--", "https://example.invalid/r.git", "+refs/heads/x:refs/x"]).status.code(), Some(128));
    assert_eq!(std::fs::read_to_string(&refused).unwrap(),
        "push https://example.invalid/s.git\nfetch https://example.invalid/r.git\n");
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    // The real git again, for the scans.
    std::fs::remove_file(a.bin.join("git")).unwrap();
    std::os::unix::fs::symlink(&real_git, a.bin.join("git")).unwrap();
    git(&a.repo(), &["config", "--unset", "remote.origin.pushurl"]);
    drop(held);

    // The scans (S§19 #24); B's session log is among the files read.
    let b_log = sess_b.log();
    assert!(session_logs(&b).contains(&b_log));
    assert_eq!(files_findings(&[b_log], &MARKERS), (1, vec![]));
    let (own_a, own_b) = (own_hosts_at_tip(&r, &a, HOST_A), own_hosts_at_tip(&r, &b, HOST_B));
    let found = scan(&[&r], &made, &[
        // FX-26 R3-1: A got no mail in this walk and made the MCP sessions of alice-q7x and of the member with no
        // name (#21); B read mail (`cur/`) and holds the #16 and #23 messages.
        Scanned { label: "A", h: &a, own_names: vec![A_NAME], own_hosts: own_a, synced: true, errors_log: true, zygo_commits: true,
            folders: vec![a.root().join(format!("sess-{ALICE}")), a.root().join(format!("sess-{ANON}"))] },
        Scanned { label: "B", h: &b, own_names: vec![B_NAME], own_hosts: own_b, synced: true, errors_log: true, zygo_commits: true,
            folders: vec![maildir(&b, BOB, "new"), maildir(&b, BOB, "cur"), b.root().join(format!("sess-{BOB}"))] },
    ]);
    assert_eq!(found, Vec::<String>::new());
}
