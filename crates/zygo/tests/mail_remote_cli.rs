#![cfg(unix)]
//! MS-18: zygo mail send|read|ls|watch with addresses, the sync line, the sync child of the watch and the doctor twin
//! line (mail sync design S§7.1, S§7.2, S§7.5, S§8.1 rows 1 and 3, S§8.6 "At send", S§10.3, S§10.5; S§19 #4 CLI send
//! part, #21 CLI part, #28). The send and watch tests run in process with a fixed or stepped clock, a counting spawn
//! seam, a recording wake and an hour of secret-pass budget; the binary runs the stub agents of `Harness` only. No
//! agent runs. The in-process sync (`sync_in`) runs git through the production `zygo_plugins::sync::git_command`, which reads the global
//! git config by design; only the git that a test starts itself (`git_cmd`) is kept from it.
mod common;

use std::cell::Cell;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zygo::cmd::mail::{send_cli, watch_sync_step, watch_with};
use zygo_config::{Config, LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_mail_sync::env::{Clock, Env, NoSeam, Probe, Spawner, Verdict, Wake};
use zygo_mail_sync::outbox::pending;
use zygo_mail_sync::registry::{write_cache, MemberCache};
use zygo_mail_sync::state::{read_last_start, save, write_last_start, PublicCache, State, SyncPaths};
use zygo_mail_sync::sync::{run_with, Mode, Opts, Report};
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::{mail_dir, Message};
use zygo_workspace::state::{MemberRecord, Workspace};

use common::workspace::{Harness, WS_ID};

const ALICE: &str = "kaaaaaaaa";
const ELENA: &str = "keeeeeeee";
const ANON: &str = "knnnnnnnn";
const BOB: &str = "kbbbbbbbb";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const SUBJECT: &str = "subj-mark-8r";
const BODY: &str = "body-mark-8r";
const SYNC_ON: &str = "[mail]\nhost = \"a\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n";
const NO_NAME: &str = "zygo: remote mail needs a sender name; give this member a name in the loadout or with --name";
const SYNC_OFF: &str = "zygo: mail sync is off for this workspace";
const PUBLIC: &str = "zygo: the remote can be read without a login; mail sync refused";
const RESERVED: &str = "zygo: all, cli and zygo have no address; send to <name> or <name>@<host>";
const BAD_FORM: &str = "zygo: not an address; send to a member name, a pane key, all, or <name>@<host>";
const NO_HOST: &str = "zygo: set [mail] host to turn on mail sync; zygo does not publish the system host name";
const BAD_BRANCH: &str = "zygo: mail sync is off: sync in [mail.workspace.app] is not zygo/ and 1 to 32 of a-z, 0-9 and -";
const TWIN: &str = "zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host";

fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
/// A built-in AWS access key id pattern match, split so the source holds no key.
fn aws() -> String { ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat() }
fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }
/// S§7.2 (rewritten): the read, ls and watch line of a value with `@` that is not local.
fn not_local(v: &str) -> String {
    format!("zygo: {v} is not a local address; zygo mail read, ls and watch take a member of this machine\n")
}

struct Fixed(SystemTime);
impl Clock for Fixed { fn now(&self) -> SystemTime { self.0 } }
/// No public check runs: a send runs none, and the sync of a `file://` remote maps it to a local path (S§8.2).
struct NoProbe;
impl Probe for NoProbe { fn probe(&self, _: &str) -> Verdict { panic!("no public check runs in these tests") } }
/// The spawn seam: each start records the number of outbox files at that moment; `fail` makes the start an error.
struct Spawns { sp: SyncPaths, seen: Arc<Mutex<Vec<usize>>>, fail: bool }
impl Spawner for Spawns {
    fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()> {
        assert_eq!(ws_id, WS_ID);
        self.seen.lock().unwrap().push(pending(&self.sp).unwrap().len());
        if self.fail { Err(std::io::Error::other("seam: no child")) } else { Ok(()) }
    }
}
/// The wake seam: records the keys of each wake; the time is the fixed clock.
struct Woke(Arc<Mutex<Vec<Vec<String>>>>);
impl Wake for Woke {
    fn wake(&self, _: &Paths, _: &Workspace, keys: &[String], _: &MailConfig, now: SystemTime) {
        assert_eq!(now, at(T0));
        self.0.lock().unwrap().push(keys.to_vec());
    }
}

/// One machine with no remote: `Harness::no_tmux`, config `SYNC_ON` (host `a`), workspace `app` with ALICE `alice`,
/// ELENA `elena`, ANON (no name), and a member cache that holds `bob@b`.
struct X { h: Harness, w: Workspace, sp: SyncPaths, seen: Arc<Mutex<Vec<usize>>>, woke: Arc<Mutex<Vec<Vec<String>>>> }
fn setup() -> X {
    let h = Harness::no_tmux();
    h.config(SYNC_ON);
    let w = h.seed(WS_ID, &mut |w| {
        for (k, n) in [(ALICE, Some("alice")), (ELENA, Some("elena")), (ANON, None)] {
            w.members.insert(k.to_string(), MemberRecord { name: n.map(String::from), ..MemberRecord::new(Agent::ClaudeCode) });
        }
    });
    let sp = SyncPaths::new(&h.paths, WS_ID).unwrap();
    cache(&sp, &[("b", &["bob"])]);
    X { h, w, sp, seen: Arc::default(), woke: Arc::default() }
}
fn config(x: &X, text: &str) { x.h.config(text); }
fn cache(sp: &SyncPaths, hosts: &[(&str, &[&str])]) {
    let mut c = MemberCache::default();
    for (h, ns) in hosts {
        c.hosts.insert((*h).into(), ns.iter().map(|n| (*n).to_string()).collect());
    }
    write_cache(sp, &c).unwrap();
}
/// The user config of `p`, as the binary loads it (`repo: None`).
fn cfg(p: &Paths) -> Config { zygo_config::load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap().config }
fn env_of(x: &X, fail: bool, policy: DeadlinePolicy) -> Env {
    Env {
        paths: x.h.paths.clone(),
        clock: Box::new(Fixed(at(T0))),
        probe: Box::new(NoProbe),
        seam: Box::new(NoSeam),
        spawner: Box::new(Spawns { sp: x.sp.clone(), seen: Arc::clone(&x.seen), fail }),
        wake: Box::new(Woke(Arc::clone(&x.woke))),
        secret_policy: policy,
    }
}
/// `send_cli` from `pane` with SUBJECT and `body`: (exit code, stdout, stderr).
fn send_p(x: &X, pane: Option<&str>, to: &str, body: &str, fail: bool, policy: DeadlinePolicy) -> (i32, String, String) {
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = send_cli(&env_of(x, fail, policy), &x.w, pane, to, SUBJECT, body, &cfg(&x.h.paths), &mut o, &mut e);
    (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
}
fn send(x: &X, pane: Option<&str>, to: &str, body: &str) -> (i32, String, String) { send_p(x, pane, to, body, false, far()) }
/// A refusal: exit 1, no stdout, `line` on stderr.
fn refused(r: (i32, String, String), line: &str) { assert_eq!(r, (1, String::new(), format!("{line}\n"))); }
fn spawns(x: &X) -> Vec<usize> { x.seen.lock().unwrap().clone() }
fn woke(x: &X) -> Vec<Vec<String>> { x.woke.lock().unwrap().clone() }
fn new_ids(x: &X, key: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(mail_dir(&x.h.paths, WS_ID).join(key).join("new")) else { return Vec::new() };
    let mut v: Vec<String> = rd.map(|e| e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}
fn errors(x: &X) -> String { std::fs::read_to_string(x.h.store(WS_ID).dir().join("errors.log")).unwrap_or_default() }
/// Nothing in `outbox/` or `outbox/tmp/`, and no child started.
fn nothing_sent(x: &X) {
    assert!(pending(&x.sp).unwrap().is_empty());
    assert!(!x.sp.outbox_tmp.exists() || std::fs::read_dir(&x.sp.outbox_tmp).unwrap().next().is_none());
    assert!(spawns(x).is_empty());
}
/// `zygo <args>` with `ZYGO_WORKSPACE = WS_ID` and `extra`.
fn zygo(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)]) -> Output {
    let mut env = vec![("ZYGO_WORKSPACE", OsStr::new(WS_ID))];
    env.extend_from_slice(extra);
    h.zygo(args, &env).output().unwrap()
}
fn out(o: &Output) -> String { String::from_utf8_lossy(&o.stdout).into_owned() }
fn err(o: &Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }

#[test] // C1: S§8.6 "At send", S§19 #4 CLI send part
fn remote_send_with_an_aws_key_exits_1_with_the_pattern_id_and_writes_nothing() {
    let x = setup();
    let r = send(&x, Some(ALICE), "bob@b", &format!("{BODY} {}", aws()));
    assert!(!r.2.contains(&aws()));
    refused(r, "zygo: message not sent: it holds a secret (aws_access_key_id)");
    nothing_sent(&x);
}

#[test] // C2: S§8.6 "At send": the check could not run (the test hook `expire_after_scans`, `secret.rs:18-19`)
fn remote_send_when_the_secret_check_cannot_run_exits_1_and_writes_nothing() {
    let x = setup();
    let r = send_p(&x, Some(ALICE), "bob@b", BODY, false, DeadlinePolicy { expire_after_scans: Some(0), ..far() });
    refused(r, "zygo: message not sent: the secret check could not run (secret_timeout); send it again");
    nothing_sent(&x);
}

#[test] // C3: S§7.2 remote address, S§8.1 row 1, S§19 #22 send part
fn clear_remote_send_writes_one_outbox_file_then_starts_one_child_with_no_gap() {
    let x = setup();
    write_last_start(&x.sp, T0).unwrap(); // the hook gap (60 s) would skip a start now
    let (code, o, e) = send(&x, Some(ALICE), "bob@b", BODY);
    assert_eq!((code, e.as_str()), (0, ""));
    let files = pending(&x.sp).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(o, format!("sent {} to bob@b\n", files[0].0));
    let m: Message = serde_json::from_slice(&std::fs::read(&files[0].1).unwrap()).unwrap();
    assert_eq!((m.from.as_str(), m.to.as_str(), m.subject.as_str(), m.body.as_str(), m.sent_at.as_str()),
        ("alice@a", "bob@b", SUBJECT, BODY, "2023-11-14T22:13:20Z"));
    assert_eq!(spawns(&x), vec![1]); // one child, after the outbox link
    assert_eq!(read_last_start(&x.sp), Some(T0)); // a send writes no last_start
    assert!(woke(&x).is_empty());
}

#[test] // C4: S§8.1 (a child that does not start), S-C1, S-C2
fn a_child_that_does_not_start_keeps_the_send_and_writes_one_errors_log_line() {
    let x = setup();
    let (code, o, e) = send_p(&x, Some(ALICE), "bob@b", BODY, true, far());
    assert_eq!((code, e.as_str()), (0, ""));
    assert!(o.ends_with(" to bob@b\n"), "{o}");
    assert_eq!(pending(&x.sp).unwrap().len(), 1);
    assert_eq!(spawns(&x), vec![1]);
    let log = errors(&x);
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("mail send: cannot start the mail sync child: seam: no child"), "{log}");
    for s in [SUBJECT, BODY, "bob", "alice", "@"] {
        assert!(!log.contains(s), "{s} in {log}");
    }
}

#[test] // C5: S§7.2 refusal 1, S§19 #21 CLI part
fn cli_with_no_zygo_pane_and_a_member_with_no_name_get_the_sender_name_line() {
    let x = setup();
    refused(send(&x, None, "bob@b", BODY), NO_NAME);
    refused(send(&x, Some(ANON), "bob@b", BODY), NO_NAME);
    nothing_sent(&x);
    let o = zygo(&x.h, &["mail", "send", "bob@b", "--subject", SUBJECT], &[]); // the binary: `cli`, no ZYGO_PANE
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(1), String::new(), format!("{NO_NAME}\n")));
    assert!(pending(&x.sp).unwrap().is_empty());
}

#[test] // C6: S§7.2 (MS-19 lines), S§19 #21 CLI part
fn reserved_and_bad_addresses_are_refused() {
    let x = setup();
    for to in ["cli@b", "zygo@b", "all@b", "cli@a"] {
        refused(send(&x, Some(ALICE), to, BODY), RESERVED);
    }
    for to in ["Bob@b", "bob@B", "bob@", "@b", "bob@b@c", "kabcdefgh@b"] {
        refused(send(&x, Some(ALICE), to, BODY), BAD_FORM);
    }
    nothing_sent(&x);
    assert!(new_ids(&x, ELENA).is_empty() && new_ids(&x, ALICE).is_empty());
    let o = zygo(&x.h, &["mail", "send", "cli@b"], &[("ZYGO_PANE", OsStr::new(ALICE))]);
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(1), String::new(), format!("{RESERVED}\n")));
}

#[test] // C7: S§7.2 local host
fn own_host_address_is_a_local_send() {
    let x = setup();
    let (code, o, e) = send(&x, Some(ALICE), "elena@a", BODY);
    assert_eq!((code, e.as_str()), (0, ""));
    let ids = new_ids(&x, ELENA);
    assert_eq!(ids.len(), 1);
    assert_eq!(o, format!("sent {} to elena\n", ids[0]));
    let f = mail_dir(&x.h.paths, WS_ID).join(ELENA).join("new").join(format!("{}.json", ids[0]));
    let m: Message = serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap();
    assert_eq!((m.from.as_str(), m.to.as_str()), ("alice", "elena"));
    assert_eq!(woke(&x), vec![vec![ELENA.to_string()]]);
    nothing_sent(&x);
}

#[test] // C8: S§7.2 refusals 2-4; with sync off there is no local host
fn the_other_s7_2_refusals_write_nothing() {
    let x = setup();
    refused(send(&x, Some(ALICE), "carol@b", BODY), "zygo: unknown address; known addresses: bob@b");
    save(&x.sp, &State { public: Some(PublicCache { url_sha256: vec!["0".repeat(64)], verdict: "public".into(), at: T0 }), ..Default::default() }).unwrap();
    refused(send(&x, Some(ALICE), "bob@b", BODY), PUBLIC);
    config(&x, "");
    refused(send(&x, Some(ALICE), "bob@b", BODY), SYNC_OFF);
    refused(send(&x, Some(ALICE), "elena@a", BODY), SYNC_OFF);
    nothing_sent(&x);
    let (code, o, _) = send(&x, Some(ALICE), "elena", BODY);
    assert_eq!((code, o.ends_with(" to elena\n")), (0, true));
}

#[test] // R1: S§7.2 (rewritten): a value with `@` that is not local is refused in read, ls and watch
fn read_ls_and_watch_refuse_a_value_with_at_that_is_not_local() {
    let x = setup();
    for v in ["bob@b", "cli@a", "Bob@a"] {
        for args in [vec!["mail", "read", "--name", v], vec!["mail", "ls", v], vec!["mail", "watch", v, "--timeout", "0s"]] {
            let o = zygo(&x.h, &args, &[]);
            assert_eq!((o.status.code(), out(&o), err(&o)), (Some(1), String::new(), not_local(v)), "{args:?}");
        }
    }
    config(&x, ""); // sync off: no local host
    let o = zygo(&x.h, &["mail", "ls", "elena@a"], &[]);
    assert_eq!((o.status.code(), err(&o)), (Some(1), not_local("elena@a")));
}

#[test] // R2: S§7.2: `<name>@<own host>` works as `<name>`
fn own_host_address_works_as_the_bare_name_in_ls_read_and_watch() {
    let x = setup();
    let r = zygo_workspace::mail::send(&x.h.paths, &x.w, None, "elena", SUBJECT, BODY, &MailConfig::default(), at(T0)).unwrap();
    let id = r.sent[0].id.clone();
    let line = format!("{id} cli {SUBJECT}\n");
    // Review round 2 F3: the own host needs sync on (S§7.2), so the binary watch is held by the rate limit (S§8.4)
    // and starts no background child.
    let last = recent_start(&x.sp);
    for args in [&["mail", "ls", "elena@a"][..], &["mail", "ls", "elena"][..], &["mail", "watch", "elena@a", "--timeout", "0s"][..]] {
        let o = zygo(&x.h, args, &[]);
        assert_eq!((o.status.code(), out(&o)), (Some(0), line.clone()), "{args:?}: {}", err(&o));
    }
    let o = zygo(&x.h, &["mail", "read", "--name", "elena@a"], &[]);
    assert_eq!((o.status.code(), out(&o)), (Some(0), format!("{id} cli 2023-11-14T22:13:20Z {SUBJECT}\n  {BODY}\n")), "{}", err(&o));
    assert_eq!(read_last_start(&x.sp), Some(last)); // a start writes its own time first
}

/// Review round 2 F3: writes `mail/sync/last_start` 1 s before the system time and returns it, so a binary watch
/// within the 60 s `sync_gap` starts no child (S§8.4); a start would write a later time, so the value shows a start.
fn recent_start(sp: &SyncPaths) -> u64 {
    let last = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - 1;
    write_last_start(sp, last).unwrap();
    last
}

/// `zygo mail ls` (exit 0): its stdout.
fn ls(x: &X) -> String {
    let o = zygo(&x.h, &["mail", "ls"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    out(&o)
}
const MEMBERS: &str = "kaaaaaaaa alice 0\nkeeeeeeee elena 0\nknnnnnnnn - 0\n";

#[test] // L1: S§10.3 (rewritten): the sync line
fn ls_ends_with_the_sync_line() {
    let x = setup();
    assert_eq!(ls(&x), format!("{MEMBERS}mail sync: last sync never, 0 in the outbox, 0 refused\n"));
    save(&x.sp, &State { last_ok: Some(T0), ..Default::default() }).unwrap();
    std::fs::create_dir_all(&x.sp.refused).unwrap();
    for (d, n) in [(&x.sp.outbox, 1u128), (&x.sp.outbox, 2), (&x.sp.refused, 3)] {
        std::fs::write(d.join(format!("{}.json", ulid::Ulid::from_parts(T0 * 1000, n))), "{}").unwrap();
    }
    assert_eq!(ls(&x), format!("{MEMBERS}mail sync: last sync 2023-11-14T22:13:20Z, 2 in the outbox, 1 refused\n"));
    save(&x.sp, &State { last_ok: Some(T0), last_error: Some("fetch".into()), ..Default::default() }).unwrap();
    assert_eq!(ls(&x), format!("{MEMBERS}mail sync: last sync 2023-11-14T22:13:20Z, 2 in the outbox, 1 refused, last error fetch\n"));
    std::fs::write(&x.sp.state, "[]").unwrap(); // not an object: `load` refuses it
    assert_eq!(ls(&x), format!("{MEMBERS}mail sync: last sync -, 2 in the outbox, 1 refused\n"));
}

#[test] // L2: S§7.1, S§10.3 (rewritten): the off cases
fn ls_sync_line_off_cases() {
    let x = setup();
    config(&x, "[mail.workspace.app]\nsync = \"zygo/mail\"\n");
    assert_eq!(ls(&x), format!("{MEMBERS}{NO_HOST}\n"));
    config(&x, "[mail]\nhost = \"a\"\n[mail.workspace.app]\nsync = \"main\"\n");
    assert_eq!(ls(&x), format!("{MEMBERS}{BAD_BRANCH}\n"));
    config(&x, "[mail]\nhost = \"a\"\n");
    assert_eq!(ls(&x), MEMBERS);
    config(&x, "[mail\n");
    assert_eq!(ls(&x), MEMBERS);
}

/// `git` with no global or system config and no template folder (MS-18 review F4), so the hooks, signing, templates
/// and `insteadOf` rules of the user who runs the tests cannot change a result. Every git that a test starts is made
/// here.
fn git_cmd() -> std::process::Command {
    let mut c = std::process::Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TEMPLATE_DIR", "");
    c
}

/// `git -c user.name=t -c user.email=t@t.invalid -c commit.gpgSign=false <args>` in `dir`; panics on a non-zero
/// exit; the stdout without its trailing newline. (Copied from `tests/mail_sync_cli.rs`.)
fn git(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd()
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgSign=false"])
        .args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", err(&o));
    out(&o).trim_end().to_string()
}

/// A bare repository `bare.git` with one commit on `main` (pushed from a seed clone), and its `file://` URL.
/// (Copied from `tests/mail_sync_cli.rs`.)
struct Remote { _t: tempfile::TempDir, url: String }
fn remote() -> Remote {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    git(&p, &["init", "-q", "--bare", "bare.git"]);
    let url = format!("file://{}", p.join("bare.git").display());
    git(&p, &["clone", "-q", &url, "seed"]);
    std::fs::write(p.join("seed/README"), "x\n").unwrap();
    git(&p.join("seed"), &["add", "README"]);
    git(&p.join("seed"), &["commit", "-q", "-m", "seed"]);
    git(&p.join("seed"), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    Remote { _t: t, url }
}

/// One machine: a scratch home whose `repo` is a clone of `r`, `git` linked into `bin`, the config of `host` with
/// `[mail.workspace.app]` `sync = "zygo/mail"`, and the workspace `app` (`WS_ID`) with the members
/// `(pane key, name)`. (Copied from `tests/mail_sync_cli.rs`, its `config` call inlined.)
fn machine(r: &Remote, host: &str, members: &[(&str, &str)]) -> Harness {
    let h = Harness::no_tmux();
    git(h.root(), &["clone", "-q", &r.url, "repo"]);
    std::os::unix::fs::symlink(which::which("git").unwrap(), h.bin.join("git")).unwrap();
    let text = format!("[mail]\nhost = \"{host}\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n");
    h.config(&text);
    h.seed(WS_ID, &mut |w| {
        for (k, n) in members {
            w.members.insert(k.to_string(), MemberRecord { name: Some(n.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
        }
    });
    h
}

/// No child starts in an in-process sync.
struct NoSpawn;
impl Spawner for NoSpawn { fn spawn_sync(&self, _: &str) -> std::io::Result<()> { Ok(()) } }
/// The wake of an in-process sync does nothing (MS-13 tests the wake).
struct NoWake;
impl Wake for NoWake { fn wake(&self, _: &Paths, _: &Workspace, _: &[String], _: &MailConfig, _: SystemTime) {} }

/// One in-process sync of machine `h` (`run_with`, `Mode::Cli`) with the fixed clock and an hour of secret-pass
/// budget, as the zygo-mail-sync tests run it. The binary sync takes the 200 ms default, which a slow machine can
/// miss at the commit-time secret check (global constraint: no deadline test in CI).
fn sync_in(h: &Harness) -> Report {
    let env = Env {
        paths: h.paths.clone(), clock: Box::new(Fixed(at(T0))), probe: Box::new(NoProbe), seam: Box::new(NoSeam),
        spawner: Box::new(NoSpawn), wake: Box::new(NoWake), secret_policy: far(),
    };
    run_with(&env, WS_ID, Mode::Cli, &Opts::default()).unwrap()
}

/// Machines `b` (`bob`) and `a` (`alice`) on one remote, each synced in process; then `alice` sends `bob@b` the
/// subject `\x1b[2Jhi` through `send_cli` (one child through the counting seam), and `a` pushes it in process.
/// Returns (remote, b, a, the message id).
fn sent_to_bob() -> (Remote, Harness, Harness, String) {
    let r = remote();
    let b = machine(&r, "b", &[(BOB, "bob")]);
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync_in(&b); // makes the branch; registers b
    sync_in(&a); // a's member cache now holds bob@b
    let wa = a.store(WS_ID).read().unwrap().unwrap();
    let spa = SyncPaths::new(&a.paths, WS_ID).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env {
        paths: a.paths.clone(), clock: Box::new(Fixed(at(T0))), probe: Box::new(NoProbe), seam: Box::new(NoSeam),
        spawner: Box::new(Spawns { sp: spa.clone(), seen: Arc::clone(&seen), fail: false }),
        wake: Box::new(Woke(Arc::default())), secret_policy: far(),
    };
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = send_cli(&env, &wa, Some(ALICE), "bob@b", "\u{1b}[2Jhi", BODY, &cfg(&a.paths), &mut o, &mut e);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&e));
    let id = pending(&spa).unwrap()[0].0.to_string();
    assert_eq!(*seen.lock().unwrap(), vec![1]);
    assert_eq!(sync_in(&a).pushed, 1); // pushes the message
    (r, b, a, id)
}

/// The file names in `mail/<key>/new/` of `h`; empty when absent.
fn new_of(h: &Harness, key: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(mail_dir(&h.paths, WS_ID).join(key).join("new")) else { return Vec::new() };
    rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

/// The binary `zygo mail watch bob --timeout 0s` and then `zygo mail read --name bob` on `b`: each prints the
/// subject `\x1b[2Jhi` of message `id` escaped and no ESC byte.
fn watch_and_read_escape(b: &Harness, id: &str) {
    let w = zygo(b, &["mail", "watch", "bob", "--timeout", "0s"], &[]);
    assert_eq!(w.status.code(), Some(0), "{}", err(&w));
    assert_eq!(out(&w), format!("{id} alice@a \\u{{1b}}[2Jhi\n"));
    assert!(!w.stdout.contains(&0x1b));
    let rd = zygo(b, &["mail", "read", "--name", "bob"], &[]);
    assert_eq!(out(&rd), format!("{id} alice@a 2023-11-14T22:13:20Z \\u{{1b}}[2Jhi\n  {BODY}\n"));
    assert!(!rd.stdout.contains(&0x1b));
}

#[test] // W1: S§19 #28, S§10.5 (rewritten): watch and read print a remote subject escaped (an imported message)
fn watch_and_read_print_an_imported_remote_subject_escaped() {
    let (_r, b, _a, id) = sent_to_bob();
    assert_eq!(sync_in(&b).delivered, 1); // imported in process before the binary runs
    let sp = SyncPaths::new(&b.paths, WS_ID).unwrap();
    let last = recent_start(&sp); // review round 2 F3: the binary watch starts no background child
    watch_and_read_escape(&b, &id);
    assert_eq!(read_last_start(&sp), Some(last)); // a start writes its own time first
}

#[test] // W4: S§8.1 row 3 (rewritten, review F1/F3): the binary watch starts the real child; its import shows at a later look
#[ignore = "timing: background sync child"]
fn watch_starts_the_sync_child_and_shows_its_import_at_a_later_look() {
    let (_r, b, _a, id) = sent_to_bob();
    assert!(new_of(&b, BOB).is_empty()); // nothing imported before the watch
    // b's child sync has an empty outbox: no commit-time secret check runs; the 60 s timeout bounds the child's time.
    let w = zygo(&b, &["mail", "watch", "bob", "--timeout", "60s"], &[]);
    assert_eq!((w.status.code(), out(&w)), (Some(0), format!("{id} alice@a \\u{{1b}}[2Jhi\n")), "{}", err(&w));
}

#[test] // W3: S§8.1 row 3 (rewritten): the step runs before each look at new/
fn watch_with_runs_the_sync_step_before_each_look() {
    let x = setup();
    let dir = mail_dir(&x.h.paths, WS_ID);
    let base = Instant::now(); // an origin only: each call of `now` adds 1 s, so no test time is read
    let calls = Cell::new(0u64);
    let now = || { let n = calls.get(); calls.set(n + 1); base + Duration::from_secs(n) };
    let (mut steps, mut o) = (0, Vec::new());
    assert_eq!(watch_with(&dir, ELENA, Duration::from_secs(3), Duration::ZERO, &now, &mut o, &mut || steps += 1), 1);
    assert_eq!((steps, o.len()), (3, 0)); // looks at +1 s, +2 s, +3 s (the timeout)
    zygo_workspace::mail::send(&x.h.paths, &x.w, None, "elena", SUBJECT, BODY, &MailConfig::default(), at(T0)).unwrap();
    let mut steps = 0;
    assert_eq!(watch_with(&dir, ELENA, Duration::from_secs(3), Duration::ZERO, &now, &mut o, &mut || steps += 1), 0);
    assert_eq!(steps, 1);
}

/// A clock that moves 10 s at each read: read `n` (from 0) is `T0 + 10 n`; `reads` counts the reads.
struct Steps(Arc<AtomicU64>);
impl Clock for Steps { fn now(&self) -> SystemTime { at(T0 + 10 * self.0.fetch_add(1, Ordering::SeqCst)) } }
/// The spawn seam of the watch: each start records the clock reads so far and returns at once; `fail` makes the start
/// an error.
struct Starts { reads: Arc<AtomicU64>, seen: Arc<Mutex<Vec<u64>>>, fail: bool }
impl Spawner for Starts {
    fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()> {
        assert_eq!(ws_id, WS_ID);
        self.seen.lock().unwrap().push(self.reads.load(Ordering::SeqCst));
        if self.fail { Err(std::io::Error::other("seam: no child")) } else { Ok(()) }
    }
}
/// The clock reads and the starts of one watch of `x`.
struct Watched { reads: Arc<AtomicU64>, seen: Arc<Mutex<Vec<u64>>>, code: i32, out: String }
/// `watch_with` of ELENA with `watch_sync_step` over the `Steps` clock and the `Starts` seam, a timeout of `looks` s
/// and an `Instant` clock that moves 1 s at each call (no test time is read): `looks` looks when no mail lands.
fn watch_steps(x: &X, looks: u64, fail: bool) -> Watched {
    let (reads, seen) = (Arc::new(AtomicU64::new(0)), Arc::new(Mutex::new(Vec::new())));
    let env = Env {
        paths: x.h.paths.clone(), clock: Box::new(Steps(Arc::clone(&reads))), probe: Box::new(NoProbe),
        seam: Box::new(NoSeam), spawner: Box::new(Starts { reads: Arc::clone(&reads), seen: Arc::clone(&seen), fail }),
        wake: Box::new(NoWake), secret_policy: far(),
    };
    let base = Instant::now();
    let calls = Cell::new(0u64);
    let now = || { let n = calls.get(); calls.set(n + 1); base + Duration::from_secs(n) };
    let mut o = Vec::new();
    let mut step = watch_sync_step(&env, &x.w);
    let code = watch_with(&mail_dir(&x.h.paths, WS_ID), ELENA, Duration::from_secs(looks), Duration::ZERO, &now, &mut o, &mut step);
    Watched { reads, seen, code, out: String::from_utf8(o).unwrap() }
}
fn starts(w: &Watched) -> Vec<u64> { w.seen.lock().unwrap().clone() }

#[test] // W5: S§8.1 row 3, S§8.4 (review round 2 F1): the per-workspace rate limit of `mail/sync/last_start`
fn watch_starts_the_sync_child_under_the_workspace_rate_limit() {
    let x = setup();
    assert_eq!(read_last_start(&x.sp), None);
    let w = watch_steps(&x, 6, false); // looks at T0, T0 + 10 s, .., T0 + 50 s
    assert_eq!((w.code, starts(&w)), (1, vec![1])); // one start at T0, none until T0 + sync_gap (60 s)
    assert_eq!(read_last_start(&x.sp), Some(T0)); // written for the hooks and the other watches
    let w = watch_steps(&x, 6, false); // a watch started again at T0: the start of T0 is recent
    assert_eq!(starts(&w), Vec::<u64>::new());
    assert_eq!(read_last_start(&x.sp), Some(T0));
    let x = setup();
    let w = watch_steps(&x, 7, false); // looks at T0, .., T0 + 60 s
    assert_eq!(starts(&w), vec![1, 7]); // after the reads of T0 and T0 + 60 s
    assert_eq!(read_last_start(&x.sp), Some(T0 + 60));
    assert!(errors(&x).is_empty());
    let x = setup();
    write_last_start(&x.sp, T0).unwrap(); // a hook started a child at T0: it syncs already
    let w = watch_steps(&x, 7, false);
    assert_eq!(starts(&w), vec![7]); // no start at T0; the next at T0 + sync_gap
    assert_eq!(read_last_start(&x.sp), Some(T0 + 60));
    assert!(errors(&x).is_empty());
}

#[test] // W6: S§8.1 row 3 (review F1/F3): the looks go on after each start; the step does not wait for the child
// (the production `ChildSpawner` does not wait: `spawn_reaped`, tested in zygo-mail-sync)
fn watch_looks_go_on_after_each_start() {
    let x = setup();
    let w = watch_steps(&x, 13, false); // the counting seam returns at once
    assert_eq!((w.code, w.reads.load(Ordering::SeqCst)), (1, 13)); // one clock read per look: all 13 looks ran
    assert_eq!(starts(&w), vec![1, 7, 13]);
    let x = setup();
    zygo_workspace::mail::send(&x.h.paths, &x.w, None, "elena", SUBJECT, BODY, &MailConfig::default(), at(T0)).unwrap();
    let w = watch_steps(&x, 7, false);
    assert_eq!((w.code, starts(&w)), (0, vec![1])); // the first look after the start shows the mail
    assert!(w.out.ends_with(&format!(" cli {SUBJECT}\n")), "{}", w.out);
}

#[test] // W7: S§8.1 row 3 (review F1/F3): sync off, or a config that does not load, starts no child
fn watch_with_sync_off_or_a_bad_config_starts_no_child() {
    let x = setup();
    for text in ["", "[mail]\nhost = \"a\"\n", "[mail.workspace.app]\nsync = \"zygo/mail\"\n", "[mail\n"] {
        config(&x, text);
        let w = watch_steps(&x, 3, false);
        assert_eq!((w.code, starts(&w), w.reads.load(Ordering::SeqCst)), (1, vec![], 0), "{text:?}");
    }
    assert_eq!(read_last_start(&x.sp), None);
    assert!(errors(&x).is_empty());
}

#[test] // W8: S§8.1 row 3, S§8.4 (review round 2 F1): a child that does not start gives one errors.log line per start;
// `last_start` is written before the start, as for a hook; the looks go on
fn watch_child_that_does_not_start_writes_one_errors_log_line_per_start() {
    let x = setup();
    let w = watch_steps(&x, 7, true);
    assert_eq!((w.code, w.reads.load(Ordering::SeqCst), starts(&w)), (1, 7, vec![1, 7]));
    assert_eq!(read_last_start(&x.sp), Some(T0 + 60));
    let log = errors(&x);
    assert_eq!(log.matches("mail watch: cannot start the mail sync child: seam: no child").count(), 2, "{log}");
}

#[test] // D1: S§7.5 (rewritten)
fn doctor_prints_the_twin_line_only_while_last_error_is_twin() {
    let x = setup();
    let twins = |ws: &str| {
        let o = x.h.zygo(&["doctor"], &[("ZYGO_WORKSPACE", OsStr::new(ws))]).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        out(&o).lines().filter(|l| *l == TWIN).count()
    };
    assert_eq!(twins(WS_ID), 0); // no state.json
    save(&x.sp, &State { last_error: Some("twin".into()), ..Default::default() }).unwrap();
    assert_eq!(twins(WS_ID), 1);
    assert_eq!(twins("nothex"), 0); // no workspace: no line, exit 0
    save(&x.sp, &State { last_error: Some("collision".into()), ..Default::default() }).unwrap();
    assert_eq!(twins(WS_ID), 0);
    save(&x.sp, &State { last_ok: Some(T0), ..Default::default() }).unwrap();
    assert_eq!(twins(WS_ID), 0);
}

#[test] // D2: S§7.5 (rewritten), review F2: with no ZYGO_WORKSPACE the workspace is the checkout of the current folder
fn doctor_prints_the_twin_line_for_the_checkout_of_the_current_folder() {
    let h = Harness::no_tmux();
    std::os::unix::fs::symlink(which::which("git").unwrap(), h.bin.join("git")).unwrap();
    git(&h.repo(), &["init", "-q"]);
    let id = zygo_workspace::identity::workspace_id(&h.repo().join(".git").canonicalize().unwrap());
    h.seed(&id, &mut |_| {});
    let sp = SyncPaths::new(&h.paths, &id).unwrap();
    let twins = |cwd: &Path| {
        // `Harness::env` sets no ZYGO_WORKSPACE.
        let o = h.zygo(&["doctor"], &[]).current_dir(cwd).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        out(&o).lines().filter(|l| *l == TWIN).count()
    };
    assert_eq!(twins(&h.repo()), 0); // no state.json
    save(&sp, &State { last_error: Some("twin".into()), ..Default::default() }).unwrap();
    assert_eq!(twins(&h.repo()), 1);
    assert_eq!(twins(h.root()), 0); // not a checkout: no workspace, no line, exit 0
    save(&sp, &State { last_ok: Some(T0), ..Default::default() }).unwrap();
    assert_eq!(twins(&h.repo()), 0);
}
