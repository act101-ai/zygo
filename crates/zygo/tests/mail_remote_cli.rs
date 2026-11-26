#![cfg(unix)]
//! MS-18: zygo mail send|read|ls|watch with addresses, the sync line, the watch sync and the doctor twin line (mail
//! sync design S§7.1, S§7.2, S§7.5, S§8.1 rows 1 and 3, S§8.6 "At send", S§10.3, S§10.5; S§19 #4 CLI send part, #21
//! CLI part, #28). The send tests run `send_cli` with a fixed clock, a counting spawn seam, a recording wake and an
//! hour of secret-pass budget; the binary runs the stub agents of `Harness` only. No agent runs.
mod common;

use std::ffi::OsStr;
use std::process::Output;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zygo::cmd::mail::send_cli;
use zygo_config::{Config, LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_mail_sync::env::{Clock, Env, NoSeam, Probe, Spawner, Verdict, Wake};
use zygo_mail_sync::outbox::pending;
use zygo_mail_sync::registry::{write_cache, MemberCache};
use zygo_mail_sync::state::{read_last_start, save, write_last_start, PublicCache, State, SyncPaths};
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
/// A send runs no public check.
struct NoProbe;
impl Probe for NoProbe { fn probe(&self, _: &str) -> Verdict { panic!("a send runs no public check") } }
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
    std::fs::write(h.root().join("config.toml"), SYNC_ON).unwrap();
    let w = h.seed(WS_ID, &mut |w| {
        for (k, n) in [(ALICE, Some("alice")), (ELENA, Some("elena")), (ANON, None)] {
            w.members.insert(k.to_string(), MemberRecord { name: n.map(String::from), ..MemberRecord::new(Agent::ClaudeCode) });
        }
    });
    let sp = SyncPaths::new(&h.paths, WS_ID).unwrap();
    cache(&sp, &[("b", &["bob"])]);
    X { h, w, sp, seen: Arc::default(), woke: Arc::default() }
}
fn config(x: &X, text: &str) { std::fs::write(x.h.root().join("config.toml"), text).unwrap(); }
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
