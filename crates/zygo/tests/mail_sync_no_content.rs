//! MS-22: no mail content in logs, state, usage rows, commits or refs (mail sync design S-C2, S§6.5, S§19 #24 as
//! rewritten 2026-11-26). Three machines (A, B, and C, which runs a copy of A's state folder for #11) are three
//! scratch homes on one `file://` bare repository; no agent runs; no network host is asked; no wall clock is read for a
//! result.
#![cfg(unix)]
mod common;

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo::cmd::mail::send_cli;
use zygo_config::{Config, Features, LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_core::Agent;
use zygo_mail_sync::env::{Clock, Crash, Env, NoSeam, Probe, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::outbox;
use zygo_mail_sync::state::SyncPaths;
use zygo_mail_sync::sync::{run_with, Mode, Opts, Report, SyncError};
use zygo_mcp::tools::mail::tools_with_sync;
use zygo_mcp::{McpCtx, Tool, ToolError};
use zygo_redact::act_lite_store;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::wake::{after_send_with, Probe as IdleProbe};
use zygo_workspace::mail::{mail_dir, store};
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_workspace::state::{MemberRecord, Phase, Workspace};

use common::feed_stdin;
use common::workspace::{Harness, WS_ID};

const ALICE: &str = "kaaaaaaaa"; // A's member key
const BOB: &str = "kbbbbbbbb"; // B's member key
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
fn out_of(o: &Output) -> String { String::from_utf8_lossy(&o.stdout).into_owned() }
fn err_of(o: &Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }

fn sp(h: &Harness) -> SyncPaths { SyncPaths::new(&h.paths, WS_ID).unwrap() }
fn errors(h: &Harness) -> String { std::fs::read_to_string(h.store(WS_ID).dir().join("errors.log")).unwrap_or_default() }

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

/// #23: the M§7.3 wake of each key with a `wake_gap` that does not parse, an idle member and a pane that takes no
/// text ([`FailMux`]).
struct FailWake;
impl Wake for FailWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime) {
        let cfg = MailConfig { wake_gap: "soon".into(), ..cfg.clone() };
        for k in keys {
            after_send_with(paths, ws, std::slice::from_ref(k), &cfg, &FailMux(k.clone()), &Idle(now), now);
        }
    }
}
/// The idle rule of a wake: idle, at the clock of the sync.
struct Idle(SystemTime);
impl IdleProbe for Idle {
    fn idle(&self, _: &Path) -> bool { true }
    fn now(&self) -> SystemTime { self.0 }
}
/// The error of every [`FailMux`] send.
fn mux_error() -> MuxError { MuxError::Failed { message: "no server running".into() } }
/// A session `@zygo-ws` = [`WS_ID`] with one pane `%3` that holds the key (the line of
/// `crates/zygo-mail-sync/tests/sync_pass.rs:120`); a send fails with [`mux_error`].
struct FailMux(String);
impl Multiplexer for FailMux {
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
        Ok(vec![format!("0|w|1|l|80|24|1|0|%3|0|1|{}|/srv/app", self.0)])
    }
    fn show_pane_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> { Ok(Some(self.0.clone())) }
    fn set_pane_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn display(&self, _: &str, _: &str) -> Result<String, MuxError> { unreachable!() }
    fn kill_session(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn attach(&self, _: &str) -> Result<i32, MuxError> { unreachable!() }
    fn switch_client(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn send_keys_literal(&self, _: &str, _: &str) -> Result<(), MuxError> { Err(mux_error()) }
    fn send_enter(&self, _: &str) -> Result<(), MuxError> { Err(mux_error()) }
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
    Env { paths, clock: Box::new(Fixed(t0())), probe, seam, spawner, wake, secret_policy: policy }
}
fn env_in(h: &Harness, seam: Box<dyn Seam>, policy: DeadlinePolicy) -> Env { env_paths(h.paths.clone(), seam, policy) }
/// An hour of secret-pass budget: no clock decides the test.
fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }

/// Author's call 2: a sync in process, with an hour of secret-pass budget.
fn sync_in(h: &Harness) -> Report {
    run_with(&env_in(h, Box::new(NoSeam), far()), WS_ID, Mode::Cli, &Opts::default()).unwrap()
}

/// The user config of `h`, as the binary loads it.
fn cfg(h: &Harness) -> Config {
    zygo_config::load(&LoadContext { paths: h.paths.clone(), repo: None, team: None }).unwrap().config
}

/// `send_cli` in `env` from `key` with subject `{SUBJ}-{n}` and `body`: (exit code, stdout, stderr).
fn send_env(env: &Env, h: &Harness, key: &str, to: &str, n: u32, body: &str) -> (i32, String, String) {
    let ws = h.store(WS_ID).read().unwrap().unwrap();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = send_cli(env, &ws, Some(key), to, &format!("{SUBJ}-{n}"), body, &cfg(h), &mut o, &mut e);
    (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
}
/// [`send_env`] with no seam.
fn send_with(h: &Harness, key: &str, to: &str, n: u32, body: &str) -> (i32, String, String) {
    send_env(&env_in(h, Box::new(NoSeam), far()), h, key, to, n, body)
}
fn send(h: &Harness, key: &str, to: &str, n: u32) -> (i32, String, String) { send_with(h, key, to, n, &format!("{BODY}-{n}")) }

/// A send that must pass: exit 0, no stderr, stdout `sent <id> to <to>`; returns `<id>`.
fn send_body(h: &Harness, key: &str, to: &str, n: u32, body: &str) -> String {
    let (code, out, err) = send_with(h, key, to, n, body);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let id = out.strip_prefix("sent ").and_then(|s| s.strip_suffix(&format!(" to {to}\n"))).unwrap_or_else(|| panic!("{out}"));
    id.to_string()
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
    let mut v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
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
    let mut v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
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

/// Each file of `paths` that exists, and each file under each folder of `paths` (links not followed): its path below
/// the scanned path, then its bytes, scanned for `markers`. Returns the count of files read and the findings.
fn files_findings(paths: &[PathBuf], markers: &[&str]) -> (usize, Vec<String>) {
    let (mut n, mut found) = (0, Vec::new());
    for root in paths {
        for e in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(Result::ok) {
            if !e.file_type().is_file() {
                continue;
            }
            let mut bytes = e.path().strip_prefix(root).unwrap().as_os_str().as_encoded_bytes().to_vec();
            bytes.push(b'\n');
            bytes.extend(std::fs::read(e.path()).unwrap());
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
    let Ok(rd) = std::fs::read_dir(gd.join("worktrees")) else { return (read, found) };
    for w in rd.map(|e| e.unwrap().path()) {
        read.admins += 1;
        let index = w.join("index");
        if index.is_file() {
            read.indexes += 1;
            found.extend(text_findings(&index.display().to_string(), &std::fs::read(&index).unwrap(), &CONTENT));
        }
        let rest: Vec<PathBuf> = walkdir::WalkDir::new(&w).follow_links(false).into_iter().filter_map(Result::ok)
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
/// table and of the raw bytes of `db` and of `<db>-wal` when it exists. An absent file is `(0, [])`.
fn usage_findings(db: &Path) -> (usize, Vec<String>) {
    if !db.exists() {
        return (0, vec![]);
    }
    let what = db.display().to_string();
    let mut found = Vec::new();
    let mut rows = 0;
    {
        let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let tables: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        for t in tables {
            let mut st = c.prepare(&format!("SELECT * FROM \"{}\"", t.replace('"', "\"\""))).unwrap();
            let n = st.column_count();
            let mut q = st.query([]).unwrap();
            while let Some(r) = q.next().unwrap() {
                rows += 1;
                for i in 0..n {
                    let bytes = match r.get_ref(i).unwrap() {
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
    found.extend(text_findings(&what, &std::fs::read(db).unwrap(), &MARKERS));
    let wal = PathBuf::from(format!("{what}-wal"));
    if wal.exists() {
        found.extend(text_findings(&wal.display().to_string(), &std::fs::read(&wal).unwrap(), &MARKERS));
    }
    (rows, found)
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
        assert_eq!(Some(got), tip_blob(&r, &path));
    }
    let ack: Value = serde_json::from_slice(&tip_blob(&r, &format!("ack/{HOST_B}.json")).unwrap()).unwrap();
    for id in [&id1, &id2] {
        assert!(ack["imported"].as_array().unwrap().iter().any(|u| u == id.as_str()), "{ack}");
    }
    assert_eq!(call(&tools(&b, BOB), "mail_list", json!({})).unwrap()["remote"], json!([format!("{A_NAME}@{HOST_A}")]));

    // #3 (push race, then five moves).
    let ia = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 3);
    let ib = send_ok(&b, BOB, &format!("{A_NAME}@{HOST_A}"), 4);
    let (b_paths, mut first) = (b.paths.clone(), true);
    let race = Hook::new(move |p| {
        if p == "before_push" && std::mem::take(&mut first) {
            run_with(&env_paths(b_paths.clone(), Box::new(NoSeam), far()), WS_ID, Mode::Child, &Opts::default()).unwrap();
        }
        Ok(())
    });
    run_with(&env_in(&a, Box::new(race), far()), WS_ID, Mode::Cli, &Opts::default()).unwrap();
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
    let e = run_with(&env_in(&a, Box::new(moves), far()), WS_ID, Mode::Cli, &Opts::default()).unwrap_err();
    made.extend(moved.lock().unwrap().drain(..));
    assert_eq!(made.len(), 6); // main and the five moves
    assert_eq!(e, SyncError::Stopped { line: "zygo: the side branch moved 5 times during the sync; try again".into(), check: "moved" });
    assert!(sp(&a).outbox.join(format!("{ic}.json")).is_file());
    assert_eq!(sync_in(&a).pushed, 1);

    // #4 (secret at send, CLI and MCP; then a new pattern before the pass).
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
    assert!(std::fs::read_dir(mail_dir(&a.paths, WS_ID).join(ALICE).join("new")).unwrap().count() >= 1); // the notice from zygo
    config(&a, HOST_A, "", "");

    // #5 (the secret check cannot run: expiry, then an act-lite store that does not parse).
    let i5 = send_ok(&a, ALICE, &format!("{B_NAME}@{HOST_B}"), 9);
    let expire = DeadlinePolicy { expire_after_scans: Some(0), base: Duration::from_secs(3600), ..Default::default() };
    run_with(&env_in(&a, Box::new(NoSeam), expire), WS_ID, Mode::Cli, &Opts::default()).unwrap();
    assert!(sp(&a).outbox.join(format!("{i5}.json")).is_file());
    assert!(errors(&a).contains(&format!("mail sync: outbox {i5}: secret check could not run: secret_timeout")), "{}", errors(&a));
    let store = act_lite_store::store_path(&a.repo(), &act_lite_store::Lookup::system());
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(&store, "not an assignment\n").unwrap();
    sync_in(&a);
    assert!(errors(&a).contains(&format!("mail sync: outbox {i5}: secret check could not run: act_lite_store")), "{}", errors(&a));
    std::fs::remove_file(&store).unwrap();
    assert_eq!(sync_in(&a).pushed, 1);

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
    call(&tools(&b, BOB), "mail_read", json!({})).unwrap();
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(new_count(&b, BOB), 1); // the second one now
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
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let log = errors(&b);
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
    config(&b, HOST_A, "", "");
    let o = sync_bin(&b, &[]);
    assert_eq!((o.status.code(), err_of(&o)), (Some(1), format!("{COLLISION}\n")));
    let o = sync_bin(&b, &["--take-host"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
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
    mid.extend(state_findings(&sp(&a).state, &[HOST_A2]));
    config(&a, HOST_A3, "", "");
    let o = sync_bin(&a, &["--new-identity"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert!(tip_paths(&r).contains(&format!("registry/{HOST_A3}.json")));

    // #6 (public check, the `public` verdict of the injected probe: no network host is asked): the sync stops before
    // any fetch, the bare repository's refs do not change, and a remote send is refused.
    git(&a.repo(), &["remote", "set-url", "--push", "origin", &format!("https://example.invalid/{B_NAME}/r.git")]);
    let refs0 = git(&r.bare, &["for-each-ref"]);
    let public = env_full(a.paths.clone(), Box::new(NoSeam), far(), Box::new(Says(Verdict::Public)), Box::new(NoSpawn), Box::new(NoWake));
    let e = run_with(&public, WS_ID, Mode::Cli, &Opts::default()).unwrap_err();
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
    assert_eq!(run_with(&woke, WS_ID, Mode::Cli, &Opts::default()).unwrap().delivered, 2);
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
    let prompt = "zygo: 2 unsent messages and 0 refused messages will be deleted. Forget the workspace? [y/N]";
    let o = zygo(&c, &["ws", "forget", "app"], Some(b"y\n"));
    assert_eq!((o.status.code(), out_of(&o), err_of(&o)), (Some(0), "zygo: forgot app\n".into(), format!("{prompt} ")));
    assert!(std::fs::symlink_metadata(c.store(WS_ID).dir()).is_err());

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
    let (ea, eb) = (errors(&a), errors(&b));
    assert!(!ea.is_empty() && !eb.is_empty());
    let mut found = Vec::new();
    found.extend(text_findings("A errors.log", ea.as_bytes(), &MARKERS));
    found.extend(text_findings("B errors.log", eb.as_bytes(), &MARKERS));
    found.extend(workspace_findings(&a.store(WS_ID).dir().join("workspace.json"), &[A_NAME]));
    found.extend(workspace_findings(&b.store(WS_ID).dir().join("workspace.json"), &[B_NAME]));
    // S§7.5 own hosts at the end of the walk (MS-22 review F1): A has its current host only; B has its current host
    // and each old host whose ack still holds its identity.
    let (own_a, own_b) = (own_hosts_at_tip(&r, &a, HOST_A3), own_hosts_at_tip(&r, &b, HOST_A));
    assert_eq!(own_a, vec![HOST_A3.to_string()]);
    assert_eq!(own_b[0], HOST_A);
    assert!(!own_b.iter().any(|h| h == HOST_B), "{own_b:?}");
    for (h, own) in [(&a, &own_a), (&b, &own_b)] {
        let st: Value = serde_json::from_slice(&std::fs::read(sp(h).state).unwrap()).unwrap();
        assert!(!st["pushed"].as_object().unwrap().is_empty());
        let own: Vec<&str> = own.iter().map(String::as_str).collect();
        found.extend(state_findings(&sp(h).state, &own));
    }
    for repo in [r.bare.clone(), a.repo(), b.repo(), c.repo()] {
        // MS-22 review F4: each scan read something in each repository.
        let (checked, f) = commit_findings(&repo, &made);
        assert!(checked >= 1, "{}: no zygo commit checked", repo.display());
        found.extend(f);
        let (refs, f) = ref_findings(&repo);
        assert!(refs >= 1, "{}: no ref read", repo.display());
        found.extend(f);
        // MS-22 review F3: the reflogs (the sync worktree's among them) and the linked worktrees' admin files.
        let (read, f) = git_meta_findings(&repo);
        assert!(repo == r.bare || read.logs >= 1, "{}: no reflog", repo.display());
        // MS-22 review R2 F-B: A and B have a sync worktree (C's went with its forget), so the scan read its admin
        // folder and its index.
        if repo == a.repo() || repo == b.repo() {
            assert!(read.admins >= 1 && read.indexes >= 1, "{}: {read:?}", repo.display());
        }
        found.extend(f);
    }
    for h in [&a, &b] {
        let (refs, f) = ref_findings(&sp(h).worktree);
        assert!(refs >= 1, "{}: no ref read", sp(h).worktree.display());
        found.extend(f);
        let (n, f) = files_findings(&sync_files(h), &MARKERS);
        assert!(n >= 2, "{}: imported and identity", h.root().display());
        found.extend(f);
    }
    found.extend(mid);
    let (rows, f) = usage_findings(&b.paths.usage_db);
    assert!(rows >= 1, "the trim hook wrote no usage row");
    found.extend(f);
    assert_eq!(found, Vec::<String>::new());
}
