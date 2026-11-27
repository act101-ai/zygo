#![cfg(unix)]
//! MS-19: the MCP mail tools with addresses (mail sync design S§7.2, S§8.1 row 1, S§8.6 "At send", S§10.4; S§19 #4
//! tool send part, #21 tool part, #22 send part, #27 mail_list part). The spawn seam counts the children, the clock
//! is fixed and the secret pass has an hour of budget, so no test reads the wall clock. No agent runs.
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::{Features, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_core::Agent;
use zygo_mail_sync::env::Spawner;
use zygo_mail_sync::outbox::pending;
use zygo_mail_sync::registry::{write_cache, MemberCache};
use zygo_mail_sync::state::{read_last_start, save, write_last_start, PublicCache, State, SyncPaths};
use zygo_mcp::tools::mail;
use zygo_mcp::{McpCtx, Tool, ToolError};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::mail::{mail_dir, Message};
use zygo_workspace::state::{MemberRecord, Phase, Store, Workspace};

const ID: &str = "0123456789abcdef";
const LEAD: &str = "klead2222";
const ELENA: &str = "kelena222";
const ANON: &str = "kanon2222";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const SUBJECT: &str = "subj-mark-7q";
const BODY: &str = "body-mark-7q";
const SYNC_ON: &str = "[mail]\nhost = \"a\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n";
const NO_NAME: &str = "zygo: remote mail needs a sender name; give this member a name in the loadout or with --name";
const SYNC_OFF: &str = "zygo: mail sync is off for this workspace";
const PUBLIC: &str = "zygo: the remote can be read without a login; mail sync refused";
const RESERVED: &str = "zygo: all, cli and zygo have no address; send to <name> or <name>@<host>";
const BAD_FORM: &str = "zygo: not an address; send to a member name, a pane key, all, or <name>@<host>";
const NO_CONFIG: &str = "zygo: mail sync cannot run: the config does not load";
const SEND_TEXT: &str = "Send mail to members of this workspace. to is a member name, a pane key, or all (every other member). To a member on another machine, use `<name>@<host>`. Returns the message ids. Do not poll; zygo wakes you when mail arrives.";

fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
/// A built-in AWS access key id pattern match, split so the source holds no key.
fn aws() -> String { ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat() }
fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}
/// Workspace `app`, phase Up, main worktree `root`; members LEAD "lead", ELENA "elena", ANON no name.
fn ws(root: &Path) -> Workspace {
    let mut ws = Workspace::new(ID);
    ws.name = "app".into(); ws.common_dir = root.join(".git"); ws.main_root = root.to_path_buf();
    ws.tmux_session = "zygo-app".into(); ws.loadout = "solo".into(); ws.phase = Phase::Up;
    for (k, n) in [(LEAD, Some("lead")), (ELENA, Some("elena")), (ANON, None)] {
        let mut m = MemberRecord::new(Agent::ClaudeCode);
        m.name = n.map(String::from);
        ws.members.insert(k.into(), m);
    }
    ws
}

/// The spawn seam: each start records the number of outbox files at that moment; `fail` makes the start an error.
struct Spawns { sp: SyncPaths, seen: Arc<Mutex<Vec<usize>>>, fail: bool }
impl Spawner for Spawns {
    fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()> {
        assert_eq!(ws_id, ID);
        self.seen.lock().unwrap().push(pending(&self.sp).unwrap().len());
        if self.fail { Err(std::io::Error::other("seam: no child")) } else { Ok(()) }
    }
}

struct X { _d: TempDir, p: Paths, sd: SessionDir, sp: SyncPaths, seen: Arc<Mutex<Vec<usize>>>, woke: Arc<Mutex<Vec<Vec<String>>>> }

/// Sync on with host `a`, a member cache that holds `bob@b`, and the session view of `pane`.
fn setup(pane: &str) -> X {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path());
    let root = d.path().join("repo");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(&p.config_file, SYNC_ON).unwrap();
    Store::open(&p, ID).update(Duration::from_secs(1), &mut |w| { *w = ws(&root); Ok(()) }).unwrap();
    let sd = SessionDir::new(d.path().join("session"));
    sd.create().unwrap();
    SessionView {
        mail: true,
        workspace: Some(ViewWorkspace { member: Member { ws_id: ID.into(), pane: pane.into(), ..Default::default() }, ..Default::default() }),
        ..Default::default()
    }.save(&sd).unwrap();
    let sp = SyncPaths::new(&p, ID).unwrap();
    cache(&sp, &[("b", &["bob"])]);
    X { _d: d, p, sd, sp, seen: Arc::new(Mutex::new(Vec::new())), woke: Arc::new(Mutex::new(Vec::new())) }
}
fn cache(sp: &SyncPaths, hosts: &[(&str, &[&str])]) {
    let mut c = MemberCache::default();
    for (h, ns) in hosts {
        c.hosts.insert((*h).into(), ns.iter().map(|n| (*n).to_string()).collect());
    }
    write_cache(sp, &c).unwrap();
}
fn public(x: &X) {
    save(&x.sp, &State { public: Some(PublicCache { url_sha256: vec!["0".repeat(64)], verdict: "public".into(), at: T0 }), ..Default::default() }).unwrap();
}
fn tools_p(x: &X, fail: bool, policy: DeadlinePolicy) -> Vec<Box<dyn Tool>> { tools_c(x, fail, policy, Arc::new(|| at(T0))) }
/// `tools_p` with the clock of the send given: `mail_send` reads it after its state read and right before the outbox
/// send, so a test clock can change the state between the two.
fn tools_c(x: &X, fail: bool, policy: DeadlinePolicy, clock: mail::Clock) -> Vec<Box<dyn Tool>> {
    let ctx = McpCtx { agent: Agent::ClaudeCode, features: Features { mail: true, ..Default::default() }, session_dir: Some(x.sd.clone()) };
    let woke = Arc::clone(&x.woke);
    mail::tools_with_sync(&ctx, x.p.clone(), MailConfig::default(), clock,
        Arc::new(move |_: &Paths, _: &Workspace, k: &[String], _: &MailConfig| woke.lock().unwrap().push(k.to_vec())),
        Box::new(Spawns { sp: x.sp.clone(), seen: Arc::clone(&x.seen), fail }), policy)
}
fn tools(x: &X) -> Vec<Box<dyn Tool>> { tools_p(x, false, far()) }
fn tool<'a>(t: &'a [Box<dyn Tool>], name: &str) -> &'a dyn Tool {
    t.iter().find(|t| t.def().name == name).map(|b| b.as_ref()).unwrap_or_else(|| panic!("no tool {name}"))
}
fn send(t: &[Box<dyn Tool>], to: &str, body: &str) -> Result<Value, ToolError> {
    tool(t, "mail_send").call(json!({"to": to, "subject": SUBJECT, "body": body}))
}
fn spawns(x: &X) -> Vec<usize> { x.seen.lock().unwrap().clone() }
fn woke(x: &X) -> Vec<Vec<String>> { x.woke.lock().unwrap().clone() }
fn new_ids(p: &Paths, key: &str) -> Vec<String> {
    let Ok(rd) = fs::read_dir(mail_dir(p, ID).join(key).join("new")) else { return Vec::new() };
    let mut v: Vec<String> = rd.map(|e| e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}
fn errors(x: &X) -> String { fs::read_to_string(Store::open(&x.p, ID).dir().join("errors.log")).unwrap_or_default() }
/// Nothing in `outbox/` or `outbox/tmp/`, and no child started.
fn nothing_sent(x: &X) {
    assert!(pending(&x.sp).unwrap().is_empty());
    assert!(!x.sp.outbox_tmp.exists() || fs::read_dir(&x.sp.outbox_tmp).unwrap().next().is_none());
    assert!(spawns(x).is_empty());
}

#[test] // R1: S§8.6 "At send", S§19 #4 tool send part
fn remote_send_with_an_aws_key_is_refused_with_the_pattern_id_and_writes_nothing() {
    let x = setup(LEAD);
    let e = send(&tools(&x), "bob@b", &format!("{BODY} {}", aws())).unwrap_err();
    assert_eq!(e, ToolError::Failed("zygo: message not sent: it holds a secret (aws_access_key_id)".into()));
    assert!(!e.to_string().contains(&aws()));
    nothing_sent(&x);
}

#[test] // R2: S§8.6 "At send": the check could not run (the existing test hook, `secret.rs:18-19`)
fn remote_send_when_the_secret_check_cannot_run_is_refused_and_writes_nothing() {
    let x = setup(LEAD);
    let t = tools_p(&x, false, DeadlinePolicy { expire_after_scans: Some(0), ..far() });
    assert_eq!(send(&t, "bob@b", BODY),
        Err(ToolError::Failed("zygo: message not sent: the secret check could not run (secret_timeout); send it again".into())));
    nothing_sent(&x);
}

#[test] // R3: S§7.2 remote address, S§8.1 row 1, S§19 #22 send part
fn clear_remote_send_writes_one_outbox_file_then_starts_one_child_with_no_gap() {
    let x = setup(LEAD);
    write_last_start(&x.sp, T0).unwrap(); // the hook gap (60 s) would skip a start now
    let v = send(&tools(&x), "bob@b", BODY).unwrap();
    let files = pending(&x.sp).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(v, json!({"sent": [{"id": files[0].0.to_string(), "to": "bob@b"}], "refused": []}));
    let m: Message = serde_json::from_slice(&fs::read(&files[0].1).unwrap()).unwrap();
    assert_eq!((m.from.as_str(), m.to.as_str(), m.subject.as_str(), m.body.as_str(), m.sent_at.as_str()),
        ("lead@a", "bob@b", SUBJECT, BODY, "2023-11-14T22:13:20Z"));
    assert_eq!(spawns(&x), vec![1]); // one child, and the outbox file was there when it started
    assert_eq!(read_last_start(&x.sp), Some(T0)); // a send writes no last_start
    assert!(woke(&x).is_empty());
    assert!(new_ids(&x.p, ELENA).is_empty());
}

#[test] // R4: S§8.1 (a child that does not start), S-C1, S-C2
fn a_child_that_does_not_start_keeps_the_send_and_writes_one_errors_log_line() {
    let x = setup(LEAD);
    let v = send(&tools_p(&x, true, far()), "bob@b", BODY).unwrap();
    assert_eq!(v["sent"][0]["to"], "bob@b");
    assert_eq!(pending(&x.sp).unwrap().len(), 1);
    assert_eq!(spawns(&x), vec![1]);
    let log = errors(&x);
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("mail_send: cannot start the mail sync child: seam: no child"), "{log}");
    for s in [SUBJECT, BODY, "bob", "lead", "app"] {
        assert!(!log.contains(s), "{s} in {log}");
    }
}

#[test] // R5: S§7.2 refusal 1, S§19 #21 tool part
fn member_with_no_name_gets_the_sender_name_line() {
    let x = setup(ANON);
    assert_eq!(send(&tools(&x), "bob@b", BODY), Err(ToolError::Failed(NO_NAME.into())));
    nothing_sent(&x);
}

#[test] // R6: S§7.2 (rewritten), S§19 #21 tool part
fn reserved_and_bad_addresses_are_refused() {
    let x = setup(LEAD);
    let t = tools(&x);
    for to in ["cli@b", "zygo@b", "all@b", "cli@a"] {
        assert_eq!(send(&t, to, BODY), Err(ToolError::Failed(RESERVED.into())), "{to}");
    }
    for to in ["Bob@b", "bob@B", "bob@", "@b", "bob@b@c", "kabcdefgh@b"] {
        assert_eq!(send(&t, to, BODY), Err(ToolError::Failed(BAD_FORM.into())), "{to}");
    }
    nothing_sent(&x);
    assert!(new_ids(&x.p, ELENA).is_empty() && new_ids(&x.p, LEAD).is_empty());
}

#[test] // R7: S§7.2 local host, acceptance `alice@<own host>`
fn own_host_address_is_a_local_send() {
    let x = setup(LEAD);
    let v = send(&tools(&x), "elena@a", BODY).unwrap();
    let ids = new_ids(&x.p, ELENA);
    assert_eq!(ids.len(), 1);
    assert_eq!(v, json!({"sent": [{"id": ids[0], "to": "elena"}], "refused": []}));
    let f = mail_dir(&x.p, ID).join(ELENA).join("new").join(format!("{}.json", ids[0]));
    let m: Message = serde_json::from_slice(&fs::read(f).unwrap()).unwrap();
    assert_eq!((m.from.as_str(), m.to.as_str()), ("lead", "elena"));
    assert_eq!(woke(&x), vec![vec![ELENA.to_string()]]);
    nothing_sent(&x);
}

#[test] // R8: S§7.2 (rewritten): an older own host is a remote address at send
fn an_older_own_host_is_refused_by_the_member_cache_rule() {
    let x = setup(LEAD);
    fs::write(&x.p.config_file, "[mail]\nhost = \"a2\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n").unwrap();
    assert_eq!(send(&tools(&x), "elena@a", BODY),
        Err(ToolError::Failed("zygo: unknown address; known addresses: bob@b".into())));
    assert!(new_ids(&x.p, ELENA).is_empty());
    nothing_sent(&x);
}

#[test] // R9: S§7.2 refusal 2; with sync off there is no local host
fn remote_send_with_sync_off_is_refused() {
    let x = setup(LEAD);
    fs::write(&x.p.config_file, "").unwrap();
    let t = tools(&x);
    assert_eq!(send(&t, "bob@b", BODY), Err(ToolError::Failed(SYNC_OFF.into())));
    assert_eq!(send(&t, "elena@a", BODY), Err(ToolError::Failed(SYNC_OFF.into())));
    nothing_sent(&x);
    assert_eq!(send(&t, "elena", BODY).unwrap()["sent"][0]["to"], "elena");
}

#[test] // R10: S§7.2 refusal 3
fn remote_send_after_a_public_verdict_is_refused() {
    let x = setup(LEAD);
    public(&x);
    assert_eq!(send(&tools(&x), "bob@b", BODY), Err(ToolError::Failed(PUBLIC.into())));
    nothing_sent(&x);
}

#[test] // R11: S§7.2 refusal 4, with the known addresses (as M§10)
fn unknown_address_is_refused_with_the_known_addresses() {
    let x = setup(LEAD);
    let t = tools(&x);
    assert_eq!(send(&t, "carol@b", BODY), Err(ToolError::Failed("zygo: unknown address; known addresses: bob@b".into())));
    cache(&x.sp, &[]);
    assert_eq!(send(&t, "bob@b", BODY), Err(ToolError::Failed("zygo: unknown address; known addresses: none".into())));
    nothing_sent(&x);
}

#[test] // R12: D2: a config that does not load refuses an addressed send; a bare name stays local
fn config_that_does_not_load_refuses_an_addressed_send_only() {
    let x = setup(LEAD);
    fs::write(&x.p.config_file, "[mail\n").unwrap();
    let t = tools(&x);
    assert_eq!(send(&t, "bob@b", BODY), Err(ToolError::Failed(NO_CONFIG.into())));
    assert_eq!(send(&t, "elena@a", BODY), Err(ToolError::Failed(NO_CONFIG.into())));
    nothing_sent(&x);
    assert_eq!(send(&t, "elena", BODY).unwrap()["sent"][0]["to"], "elena");
}

#[test] // R17: S§8.1 row 1, S§7.6 step 4 (FX-28 review round 1 F2): mail/lock held exclusive for the whole wait
fn remote_send_with_mail_lock_held_is_busy_and_starts_no_child() {
    let x = setup(LEAD);
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(mail_dir(&x.p, ID).join("lock")).unwrap();
    lock.try_lock().unwrap();
    assert_eq!(send(&tools(&x), "bob@b", BODY), Err(ToolError::Failed("zygo: mail is busy; try again".into())));
    nothing_sent(&x);
    drop(lock);
    assert_eq!(send(&tools(&x), "bob@b", BODY).unwrap()["sent"][0]["to"], "bob@b");
    assert_eq!(spawns(&x), vec![1]);
}

#[test] // R18: S§8.1 row 1 (FX-28 review round 1 F2): no workspace.json, at the tool's read and under mail/lock
fn remote_send_with_no_workspace_state_is_refused_and_starts_no_child() {
    let x = setup(LEAD);
    let state = Store::open(&x.p, ID).dir().join("workspace.json");
    let saved = fs::read(&state).unwrap();
    fs::remove_file(&state).unwrap();
    assert_eq!(send(&tools(&x), "bob@b", BODY), Err(ToolError::Failed(format!("zygo: workspace {ID} has no state"))));
    nothing_sent(&x);
    // The state is gone after the tool's read: the outbox send finds no workspace.json under mail/lock.
    fs::write(&state, &saved).unwrap();
    let gone = state.clone();
    let t = tools_c(&x, false, far(), Arc::new(move || {
        fs::remove_file(&gone).unwrap();
        at(T0)
    }));
    assert_eq!(send(&t, "bob@b", BODY), Err(ToolError::Failed("zygo: the workspace has no state; nothing sent".into())));
    nothing_sent(&x);
    assert!(fs::symlink_metadata(&state).is_err());
}

#[test] // R16: acceptance, S-C2, M§8a: no message text in any tool error
fn no_message_text_in_any_tool_error() {
    let cases: Vec<(&str, &str, &str, String, bool)> = vec![
        (ANON, SYNC_ON, "bob@b", BODY.into(), false),
        (LEAD, "", "bob@b", BODY.into(), false),
        (LEAD, SYNC_ON, "bob@b", BODY.into(), true),
        (LEAD, SYNC_ON, "carol@b", BODY.into(), false),
        (LEAD, SYNC_ON, "bob@b", format!("{BODY} {}", aws()), false),
        (LEAD, SYNC_ON, "cli@b", BODY.into(), false),
        (LEAD, SYNC_ON, "Bob@b", BODY.into(), false),
        (LEAD, "[mail\n", "bob@b", BODY.into(), false),
        (LEAD, SYNC_ON, "nobody", BODY.into(), false),
    ];
    for (pane, config, to, body, is_public) in cases {
        let x = setup(pane);
        fs::write(&x.p.config_file, config).unwrap();
        if is_public {
            public(&x);
        }
        let e = send(&tools(&x), to, &body).unwrap_err().to_string();
        assert!(!e.contains(SUBJECT) && !e.contains(BODY), "{to}: {e}");
    }
}

#[test] // R13: S§10.4 (rewritten), S§19 #27 mail_list part
fn mail_list_returns_the_remote_addresses_of_the_member_cache() {
    let x = setup(LEAD);
    let l = |x: &X| tool(&tools(x), "mail_list").call(json!({})).unwrap();
    assert_eq!(l(&x)["remote"], json!(["bob@b"]));
    cache(&x.sp, &[("c", &["carol"]), ("b", &["bob", "bob", "dan"])]);
    let v = l(&x);
    assert_eq!(v["remote"], json!(["bob@b", "dan@b", "carol@c"]));
    assert_eq!(v["unread"], 0);
    assert_eq!(v["messages"], json!([]));
    assert_eq!(v["members"], json!([{"key": ANON, "name": null}, {"key": ELENA, "name": "elena"}, {"key": LEAD, "name": "lead"}]));
    fs::remove_file(&x.sp.members).unwrap();
    assert_eq!(l(&x)["remote"], json!([]));
}

#[test] // R14: S§10.4 (rewritten): sync off gives no remote address
fn mail_list_remote_is_empty_with_sync_off() {
    let x = setup(LEAD);
    fs::write(&x.p.config_file, "").unwrap();
    assert_eq!(tool(&tools(&x), "mail_list").call(json!({})).unwrap()["remote"], json!([]));
    fs::write(&x.p.config_file, "[mail\n").unwrap(); // R12 list part: a config that does not load
    assert_eq!(tool(&tools(&x), "mail_list").call(json!({})).unwrap()["remote"], json!([]));
}

#[test] // R15: S§10.4: the mail_send description sentence
fn mail_send_description_holds_the_remote_sentence() {
    let x = setup(LEAD);
    let d = tool(&tools(&x), "mail_send").def().description;
    assert_eq!(d, SEND_TEXT);
    assert!(d.contains("To a member on another machine, use `<name>@<host>`."));
    assert!(d.ends_with(mail::NO_POLL));
}
