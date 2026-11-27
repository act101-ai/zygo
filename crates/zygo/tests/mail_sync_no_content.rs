//! MS-22: no mail content in logs, state, usage rows, commits or refs (mail sync design S-C2, S§6.5, S§19 #24 as
//! rewritten 2026-11-26). Two machines are two scratch homes on one `file://` bare repository; no agent runs; no wall
//! clock is read for a result.
#![cfg(unix)]
mod common;

use std::collections::BTreeSet;
use std::ffi::OsStr;
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
use zygo_workspace::mail::{mail_dir, store};
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
const HOST_A2: &str = "delta-q7x"; // A's host after #9
const SUBJ: &str = "subj-q7x"; // every subject starts with it
const BODY: &str = "body-q7x"; // every body holds it
const MARKERS: [&str; 8] = [SUBJ, BODY, A_NAME, B_NAME, HOST_A, HOST_B, HOST_B2, HOST_A2];
/// The user-config head of `crates/zygo/tests/workspace_definition.rs:142`: no marketplace is fetched.
const NO_MARKETPLACE: &str = "[[marketplace]]\nname = \"act101\"\nref = \"\"\n";
/// S§6.5: the message of every commit that zygo writes.
const MESSAGE: &str = "zygo mail [skip ci]\n";
/// S§6.5: the author and the committer of every commit that zygo writes.
const ZYGO_ID: &str = "zygo <zygo@zygo.invalid>";
/// The #4 pattern text.
const SHIP: &str = "zz-forbidden-zz";
/// The line of `crates/zygo/tests/mail_sync_cli.rs:29`, byte for byte.
const COLLISION: &str = "zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.";

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

/// Author's call 3: the in-process sync starts git through `git_command`, which reads the global config of `HOME`.
/// The first call builds the stub agent with the real `HOME` (its cargo home), then sets the process `HOME` and
/// `XDG_CONFIG_HOME` to one empty folder and removes the git variables of [`CLEARED`].
fn hermetic() {
    common::stub_agent();
    static HOME: OnceLock<TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let t = tempfile::Builder::new().prefix("home").tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let h = t.path().canonicalize().unwrap();
        std::fs::create_dir_all(h.join(".config")).unwrap();
        // SAFETY: every test calls `hermetic()` first, under `SERIAL`, before it starts a process or a thread, and
        // `OnceLock` blocks the other callers until the variables are set.
        unsafe {
            std::env::set_var("HOME", &h);
            std::env::set_var("XDG_CONFIG_HOME", h.join(".config"));
            for k in CLEARED {
                std::env::remove_var(k);
            }
        }
        t
    });
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

/// The user config of `h`: no marketplace, `[mail]` with `host` and `mail_keys`, `[mail.workspace.app]` sync on,
/// then `tables`.
fn config(h: &Harness, host: &str, mail_keys: &str, tables: &str) {
    let text = format!("{NO_MARKETPLACE}[mail]\nhost = \"{host}\"\n{mail_keys}[mail.workspace.app]\nsync = \"zygo/mail\"\n{tables}");
    std::fs::write(h.root().join("config.toml"), text).unwrap();
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

/// `h.zygo(args, …)` with `ZYGO_WORKSPACE = WS_ID`; `input` fed to stdin (none: the null device).
fn zygo(h: &Harness, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut c = h.zygo(args, &[("ZYGO_WORKSPACE", OsStr::new(WS_ID))]);
    let Some(bytes) = input else { return c.output().unwrap() };
    c.stdin(Stdio::piped());
    let mut child = c.spawn().unwrap();
    feed_stdin(child.stdin.take().unwrap(), bytes);
    child.wait_with_output().unwrap()
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
/// The wake seam does nothing.
struct NoWake;
impl Wake for NoWake { fn wake(&self, _: &Paths, _: &Workspace, _: &[String], _: &MailConfig, _: SystemTime) {} }
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
    Env {
        paths,
        clock: Box::new(Fixed(t0())),
        probe: Box::new(NoProbe),
        seam,
        spawner: Box::new(NoSpawn),
        wake: Box::new(NoWake),
        secret_policy: policy,
    }
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

/// `send_cli` from `key` with subject `{SUBJ}-{n}` and `body`: (exit code, stdout, stderr).
fn send_with(h: &Harness, key: &str, to: &str, n: u32, body: &str) -> (i32, String, String) {
    let ws = h.store(WS_ID).read().unwrap().unwrap();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = send_cli(&env_in(h, Box::new(NoSeam), far()), &ws, Some(key), to, &format!("{SUBJ}-{n}"), body, &cfg(h), &mut o, &mut e);
    (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
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
fn tools(h: &Harness, key: &str) -> Vec<Box<dyn Tool>> {
    let sd = SessionDir::new(h.root().join(format!("sess-{key}")));
    sd.create().unwrap();
    SessionView {
        mail: true,
        workspace: Some(ViewWorkspace { member: Member { ws_id: WS_ID.into(), pane: key.into(), ..Default::default() }, ..Default::default() }),
        ..Default::default()
    }.save(&sd).unwrap();
    let ctx = McpCtx { agent: Agent::ClaudeCode, features: Features { mail: true, ..Default::default() }, session_dir: Some(sd) };
    tools_with_sync(&ctx, h.paths.clone(), cfg(h).mail, Arc::new(t0),
        Arc::new(|_: &Paths, _: &Workspace, _: &[String], _: &MailConfig| {}), Box::new(NoSpawn), far())
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
    let folder = format!("{B_NAME}@{host}");
    let mut lines = git(bare, &["ls-tree", &format!("refs/heads/zygo/mail:{folder}")]);
    for ((mode, kind, id), name) in entries.iter().zip(&ids) {
        lines.push_str(&format!("\n{mode} {kind} {id}\t{name}.json"));
    }
    let sub = git_in(bare, &["mktree", "--missing"], format!("{}\n", lines.trim_start()).as_bytes());
    let top: Vec<String> = git(bare, &["ls-tree", "refs/heads/zygo/mail"]).lines()
        .map(|l| if l.ends_with(&format!("\t{folder}")) { format!("040000 tree {sub}\t{folder}") } else { l.to_string() })
        .collect();
    let tree = git_in(bare, &["mktree", "--missing"], format!("{}\n", top.join("\n")).as_bytes());
    (ids, test_commit(bare, &tree))
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
/// author and committer [`ZYGO_ID`] (S§6.5).
fn commit_findings(repo: &Path, test_made: &BTreeSet<String>) -> Vec<String> {
    let all = git(repo, &["cat-file", "--batch-all-objects", "--batch-check=%(objecttype) %(objectname)"]);
    let mut found = Vec::new();
    for id in all.lines().filter_map(|l| l.strip_prefix("commit ")) {
        let o = git_cmd().args(["cat-file", "commit", id]).current_dir(repo).output().unwrap();
        assert!(o.status.success(), "cat-file commit {id}: {}", err_of(&o));
        let what = format!("{} commit {id}", repo.display());
        found.extend(text_findings(&what, &o.stdout, &MARKERS));
        if test_made.contains(id) {
            continue;
        }
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
    found
}

/// Every ref name of `repo`, scanned for every marker.
fn ref_findings(repo: &Path) -> Vec<String> {
    git(repo, &["for-each-ref", "--format=%(refname)"]).lines()
        .flat_map(|r| text_findings(&format!("ref {r}"), r.as_bytes(), &MARKERS))
        .collect()
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
    hermetic();
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
    let f = commit_findings(&repo, &BTreeSet::new());
    assert!(f.iter().any(|l| l.ends_with(&format!("holds {SUBJ}"))) && f.iter().any(|l| l.contains("message")), "{f:?}");
    assert_eq!(commit_findings(&repo, &BTreeSet::from([c.clone()])).len(), 1); // the marker stays a finding
    assert_eq!(ref_findings(&repo), vec![format!("ref refs/heads/zygo/{B_NAME} holds {B_NAME}")]);
    let db = d.path().join("usage.db");
    assert_eq!(usage_findings(&db), (0, vec![]));
    zygo_usage::Store::open(&db).unwrap().insert_trim(&zygo_usage::TrimRow { session: BODY.into(), ..Default::default() }).unwrap();
    let (rows, f) = usage_findings(&db);
    assert!(rows >= 1 && f.iter().any(|l| l.ends_with(&format!("holds {BODY}"))), "{rows} {f:?}");
}

#[test] // S-C2, S§6.5, S§19 #24 (as rewritten): after #1, #3, #4, #5, #12, #15, #19, #9 and #20 on two machines through
        // the CLI and the MCP tool, and one usage row, no marker is in a log, a state file, the usage store, a commit
        // or a ref name, and every zygo commit has the S§6.5 message and identity
fn no_mail_content_after_the_scenarios() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    hermetic();

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
    let o = sync_bin(&b, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let log = errors(&b);
    for (id, check) in hs.iter().zip(["mode", "mode", "size", "id", "to", "json"]) {
        assert!(log.lines().any(|l| l.ends_with(&format!("mail sync: import {id}: {check}"))), "{id} {check}: {log}");
    }
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

    // Usage row (author's call 5), on B.
    let sd = SessionDir::new(b.root().join("sess-u"));
    sd.create().unwrap();
    SessionView { session: "u".into(), agent: Agent::ClaudeCode, results: true, trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }.save(&sd).unwrap();
    drop(zygo_usage::Store::open(&b.paths.usage_db).unwrap()); // the database as the launcher leaves it (hook_trim.rs:30)
    // Each line numbered, so the shaper counts no repeated lines and the size trim stores the output (implementer's
    // note, step 6).
    let stdout: String = (0..400).map(|i| format!("{i:03} {SUBJ}-u {BODY}-u {A_NAME}@{HOST_A} {B_NAME}@{HOST_B} {HOST_B2} {HOST_A2}\n")).collect();
    let event = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": format!("zygo mail read --name {B_NAME}")},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}});
    let mut c = b.zygo(&["hook", "trim"], &[("ZYGO_SESSION_DIR", sd.root.as_os_str())]);
    c.stdin(Stdio::piped());
    let mut child = c.spawn().unwrap();
    feed_stdin(child.stdin.take().unwrap(), &serde_json::to_vec(&event).unwrap());
    let o = child.wait_with_output().unwrap();
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
    for (h, own) in [(&a, &[HOST_A, HOST_A2][..]), (&b, &[HOST_B, HOST_B2, HOST_A][..])] {
        let st: Value = serde_json::from_slice(&std::fs::read(sp(h).state).unwrap()).unwrap();
        assert!(!st["pushed"].as_object().unwrap().is_empty());
        found.extend(state_findings(&sp(h).state, own));
    }
    for repo in [r.bare.clone(), a.repo(), b.repo()] {
        found.extend(commit_findings(&repo, &made));
        found.extend(ref_findings(&repo));
    }
    let (rows, f) = usage_findings(&b.paths.usage_db);
    assert!(rows >= 1, "the trim hook wrote no usage row");
    found.extend(f);
    found.extend(usage_findings(&a.paths.usage_db).1);
    assert_eq!(found, Vec::<String>::new());
}
