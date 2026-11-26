#![cfg(unix)]
//! ML-11: the MCP tools mail_send, mail_read, mail_list (M§8, M§8a item 2, M§9, M§10; AU58, AU86).
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
use zygo_mcp::tools::{build, build_with, mail};
use zygo_mcp::{McpCtx, Tool, ToolDef, ToolError};
use zygo_workspace::mail::{mail_dir, Message};
use zygo_workspace::state::{MemberRecord, Phase, Store, Workspace};

const ID: &str = "0123456789abcdef";
const LEAD: &str = "klead2222";
const ELENA: &str = "kelena222";
const ANON: &str = "kanon2222";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;

fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}
/// Identity fields as zygo-workspace tests/mail_names.rs, phase Up; members LEAD "lead", ELENA "elena", ANON no name.
fn ws() -> Workspace {
    let mut ws = Workspace::new(ID);
    ws.name = "app".into(); ws.common_dir = "/srv/app/.git".into(); ws.main_root = "/srv/app".into();
    ws.tmux_session = "zygo-app".into(); ws.loadout = "solo".into(); ws.phase = Phase::Up;
    for (k, n) in [(LEAD, Some("lead")), (ELENA, Some("elena")), (ANON, None)] {
        let mut m = MemberRecord::new(Agent::ClaudeCode);
        m.name = n.map(String::from);
        ws.members.insert(k.into(), m);
    }
    ws
}
fn state(p: &Paths, f: impl Fn(&mut Workspace)) {
    Store::open(p, ID).update(Duration::from_secs(1), &mut |w| { *w = ws(); f(w); Ok(()) }).unwrap();
}
fn member_view(mail: bool, pane: &str) -> SessionView {
    SessionView {
        mail,
        workspace: Some(ViewWorkspace {
            member: Member { ws_id: ID.into(), pane: pane.into(), ..Default::default() },
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn ctx(sd: Option<&SessionDir>, mail: bool) -> McpCtx {
    McpCtx { agent: Agent::ClaudeCode, features: Features { mail, ..Default::default() }, session_dir: sd.cloned() }
}
struct X { _d: TempDir, p: Paths, sd: SessionDir, woke: Arc<Mutex<Vec<Vec<String>>>> }
fn setup() -> X {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path());
    let sd = SessionDir::new(d.path().join("session"));
    sd.create().unwrap();
    state(&p, |_| {});
    member_view(true, LEAD).save(&sd).unwrap();
    X { _d: d, p, sd, woke: Arc::new(Mutex::new(Vec::new())) }
}
fn tools(x: &X, cfg: MailConfig) -> Vec<Box<dyn Tool>> {
    let woke = Arc::clone(&x.woke);
    mail::tools_with(&ctx(Some(&x.sd), true), x.p.clone(), cfg, Arc::new(|| at(T0)),
        Arc::new(move |_: &Paths, _: &Workspace, k: &[String], _: &MailConfig| woke.lock().unwrap().push(k.to_vec())))
}
fn tool<'a>(t: &'a [Box<dyn Tool>], name: &str) -> &'a dyn Tool {
    t.iter().find(|t| t.def().name == name).map(|b| b.as_ref()).unwrap_or_else(|| panic!("no tool {name}"))
}
fn names(t: &[Box<dyn Tool>]) -> Vec<String> { t.iter().map(|t| t.def().name).collect() }
fn ids_in(p: &Paths, key: &str, sub: &str) -> Vec<String> {
    let dir = mail_dir(p, ID).join(key).join(sub);
    let Ok(rd) = fs::read_dir(&dir) else { return Vec::new() };
    let mut v: Vec<String> = rd.map(|e| e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}
fn new_ids(p: &Paths, key: &str) -> Vec<String> { ids_in(p, key, "new") }
fn cur_ids(p: &Paths, key: &str) -> Vec<String> { ids_in(p, key, "cur") }
fn seed(p: &Paths, n: u64) {
    let ws = ws();
    for i in 0..n {
        let r = zygo_workspace::mail::send(p, &ws, Some(LEAD), "elena", &format!("s{i}"), "b", &MailConfig::default(), at(T0 + i)).unwrap();
        assert_eq!(r.sent.len(), 1);
    }
}
fn woke(x: &X) -> Vec<Vec<String>> { x.woke.lock().unwrap().clone() }
fn send_args(to: &str) -> Value { json!({"to": to, "subject": "hi", "body": "b1"}) }

#[test]
fn listed_only_for_a_member_view_with_mail_on() {
    let x = setup();
    let list = |c: &McpCtx| names(&mail::tools_with(c, x.p.clone(), MailConfig::default(), Arc::new(|| at(T0)), Arc::new(|_: &Paths, _: &Workspace, _: &[String], _: &MailConfig| {})));
    assert_eq!(list(&ctx(Some(&x.sd), true)), ["mail_send", "mail_read", "mail_list"]);
    assert!(list(&ctx(Some(&x.sd), false)).is_empty());
    member_view(false, LEAD).save(&x.sd).unwrap();
    assert!(list(&ctx(Some(&x.sd), true)).is_empty());
    SessionView::default().save(&x.sd).unwrap();
    assert!(list(&ctx(Some(&x.sd), true)).is_empty());
    member_view(true, LEAD).save(&x.sd).unwrap();
    assert!(list(&ctx(None, true)).is_empty());
    fs::write(x.sd.view_json(), b"{").unwrap();
    assert!(list(&ctx(Some(&x.sd), true)).is_empty());
    fs::remove_file(x.sd.view_json()).unwrap();
    assert!(list(&ctx(Some(&x.sd), true)).is_empty());
}

#[test]
fn descriptions_end_with_no_poll() {
    assert_eq!(mail::NO_POLL, "Do not poll; zygo wakes you when mail arrives.");
    let x = setup();
    let t = tools(&x, MailConfig::default());
    let texts = [
        ("mail_send", "Send mail to members of this workspace. to is a member name, a pane key, or all (every other member). To a member on another machine, use `<name>@<host>`. Returns the message ids. "),
        ("mail_read", "Read your new mail, oldest first: at most max messages (default 10). The messages returned are marked read. "),
        ("mail_list", "Show the count and the subjects of your unread mail, and the current members of this workspace, without marking anything read. "),
    ];
    for (name, text) in texts {
        let d: ToolDef = tool(&t, name).def();
        assert!(d.description.ends_with(mail::NO_POLL), "{name}");
        assert_eq!(d.description, format!("{text}{}", mail::NO_POLL));
        let l = d.description.to_lowercase();
        for w in ["window", "token", "context"] {
            assert!(!l.contains(w), "{name} names {w}");
        }
    }
    assert_eq!(tool(&t, "mail_send").def().input_schema, json!({"type":"object","properties":{"to":{"type":"string"},"subject":{"type":"string"},"body":{"type":"string"}},"required":["to","subject","body"],"additionalProperties":false}));
    assert_eq!(tool(&t, "mail_read").def().input_schema, json!({"type":"object","properties":{"max":{"type":"integer","minimum":1}},"additionalProperties":false}));
    assert_eq!(tool(&t, "mail_list").def().input_schema, json!({"type":"object","additionalProperties":false}));
}

#[test]
fn send_by_name_returns_the_id_and_wakes_the_target() {
    let x = setup();
    let t = tools(&x, MailConfig::default());
    let v = tool(&t, "mail_send").call(json!({"to":"elena","subject":"hi","body":"b1"})).unwrap();
    let ids = new_ids(&x.p, ELENA);
    assert_eq!(ids.len(), 1);
    assert_eq!(v, json!({"sent":[{"id":ids[0],"to":"elena"}],"refused":[]}));
    let f = mail_dir(&x.p, ID).join(ELENA).join("new").join(format!("{}.json", ids[0]));
    let m: Message = serde_json::from_slice(&fs::read(f).unwrap()).unwrap();
    assert_eq!((m.from.as_str(), m.to.as_str(), m.sent_at.as_str()), ("lead", "elena", "2023-11-14T22:13:20Z"));
    assert_eq!(woke(&x), vec![vec![ELENA.to_string()]]);
}

#[test]
fn send_reaches_a_member_with_no_name_by_key() {
    let x = setup();
    let t = tools(&x, MailConfig::default());
    let v = tool(&t, "mail_send").call(send_args(ANON)).unwrap();
    let ids = new_ids(&x.p, ANON);
    assert_eq!(ids.len(), 1);
    assert_eq!(v["sent"], json!([{"id":ids[0],"to":ANON}]));
    assert_eq!(woke(&x), vec![vec![ANON.to_string()]]);
}

#[test]
fn send_to_all_skips_the_sender() {
    let x = setup();
    let t = tools(&x, MailConfig::default());
    let v = tool(&t, "mail_send").call(send_args("all")).unwrap();
    let (a, e) = (new_ids(&x.p, ANON), new_ids(&x.p, ELENA));
    assert_eq!(v, json!({"sent":[{"id":a[0],"to":ANON},{"id":e[0],"to":"elena"}],"refused":[]}));
    assert!(new_ids(&x.p, LEAD).is_empty());
    assert_eq!(woke(&x), vec![vec![ANON.to_string(), ELENA.to_string()]]);
}

#[test]
fn send_unknown_target_names_the_known_names() {
    let x = setup();
    let t = tools(&x, MailConfig::default());
    assert_eq!(tool(&t, "mail_send").call(send_args("bob")),
        Err(ToolError::Failed("zygo: unknown target; known names: elena, lead".into())));
    assert!(!mail_dir(&x.p, ID).exists() || fs::read_dir(mail_dir(&x.p, ID)).unwrap().next().is_none());
    assert!(woke(&x).is_empty());
}

#[test]
fn send_in_stopping_is_refused() {
    let x = setup();
    state(&x.p, |w| w.phase = Phase::Stopping);
    // The `down` stand-in: it holds `op.lock` while phase is stopping (AU83).
    let _op = Store::open(&x.p, ID).op_lock().unwrap();
    let t = tools(&x, MailConfig::default());
    assert_eq!(tool(&t, "mail_send").call(send_args("elena")), Err(ToolError::Failed("zygo: workspace is stopping".into())));
    assert!(new_ids(&x.p, ELENA).is_empty());
    assert!(woke(&x).is_empty());
}

#[test]
fn send_in_stopping_without_down_is_delivered() {
    let x = setup();
    state(&x.p, |w| w.phase = Phase::Stopping);
    assert!(Store::open(&x.p, ID).op_lock_free());
    let t = tools(&x, MailConfig::default());
    let v = tool(&t, "mail_send").call(send_args("elena")).unwrap();
    let ids = new_ids(&x.p, ELENA);
    assert_eq!(ids.len(), 1);
    assert_eq!(v, json!({"sent":[{"id":ids[0],"to":"elena"}],"refused":[]}));
    assert_eq!(woke(&x), vec![vec![ELENA.to_string()]]);
}

#[test]
fn send_refused_target_alone_is_an_error() {
    let x = setup();
    seed(&x.p, 1);
    let t = tools(&x, MailConfig { max_unread: 1, ..Default::default() });
    assert_eq!(tool(&t, "mail_send").call(send_args("elena")), Err(ToolError::Failed("zygo: elena has 1 unread messages".into())));
    assert!(woke(&x).is_empty());
}

#[test]
fn send_with_one_refused_target_reports_both() {
    let x = setup();
    seed(&x.p, 1);
    let before = new_ids(&x.p, ELENA);
    let t = tools(&x, MailConfig { max_unread: 1, ..Default::default() });
    let v = tool(&t, "mail_send").call(send_args("all")).unwrap();
    let a = new_ids(&x.p, ANON);
    assert_eq!(a.len(), 1);
    assert_eq!(v, json!({"sent":[{"id":a[0],"to":ANON}],"refused":["zygo: elena has 1 unread messages"]}));
    assert_eq!(new_ids(&x.p, ELENA), before);
    assert_eq!(woke(&x), vec![vec![ANON.to_string()]]);
}

#[test]
fn send_bad_arguments_are_invalid_params() {
    let x = setup();
    let t = tools(&x, MailConfig::default());
    for a in [json!([]), json!({"subject":"s","body":"b"}), json!({"to":1,"subject":"s","body":"b"}), json!({"to":"elena","subject":"s","body":"b","cc":"x"})] {
        assert!(matches!(tool(&t, "mail_send").call(a.clone()), Err(ToolError::InvalidParams(_))), "{a}");
    }
    assert!(!mail_dir(&x.p, ID).exists() || fs::read_dir(mail_dir(&x.p, ID)).unwrap().next().is_none());
    assert!(woke(&x).is_empty());
}

fn subjects(v: &Value) -> Vec<String> {
    v["messages"].as_array().unwrap().iter().map(|m| m["subject"].as_str().unwrap().to_string()).collect()
}

#[test]
fn read_returns_at_most_max_oldest_first_and_moves_them() {
    let x = setup();
    seed(&x.p, 12);
    member_view(true, ELENA).save(&x.sd).unwrap();
    let t = tools(&x, MailConfig::default());
    let r = tool(&t, "mail_read");
    let v = r.call(json!({})).unwrap();
    assert_eq!(subjects(&v), (0..10).map(|i| format!("s{i}")).collect::<Vec<_>>());
    assert_eq!(new_ids(&x.p, ELENA).len(), 2);
    assert_eq!(cur_ids(&x.p, ELENA).len(), 10);
    assert_eq!(subjects(&r.call(json!({"max":5})).unwrap()), ["s10", "s11"]);
    assert_eq!(r.call(json!({})).unwrap(), json!({"messages":[]}));
}

#[test]
fn read_bad_max_is_invalid_params() {
    let x = setup();
    seed(&x.p, 2);
    member_view(true, ELENA).save(&x.sd).unwrap();
    let t = tools(&x, MailConfig::default());
    for a in [json!({"max":0}), json!({"max":-1}), json!({"max":"3"}), json!({"max":1.5}), json!({"x":1})] {
        assert!(matches!(tool(&t, "mail_read").call(a.clone()), Err(ToolError::InvalidParams(_))), "{a}");
    }
    assert_eq!(new_ids(&x.p, ELENA).len(), 2);
}

#[test]
fn list_returns_count_subjects_and_current_names_without_a_move() {
    let x = setup();
    seed(&x.p, 2);
    member_view(true, ELENA).save(&x.sd).unwrap();
    let t = tools(&x, MailConfig::default());
    let l = tool(&t, "mail_list");
    let v = l.call(json!({})).unwrap();
    let ids = new_ids(&x.p, ELENA);
    assert_eq!(v["unread"], 2);
    assert_eq!(v["messages"], json!([{"id":ids[0],"from":"lead","subject":"s0"},{"id":ids[1],"from":"lead","subject":"s1"}]));
    assert_eq!(v["members"], json!([{"key":ANON,"name":null},{"key":ELENA,"name":"elena"},{"key":LEAD,"name":"lead"}]));
    assert_eq!(new_ids(&x.p, ELENA).len(), 2);
    assert!(cur_ids(&x.p, ELENA).is_empty());
    state(&x.p, |w| w.members.get_mut(ANON).unwrap().name = Some("ann".into()));
    assert_eq!(l.call(json!({})).unwrap()["members"][0], json!({"key":ANON,"name":"ann"}));
    assert!(matches!(l.call(json!({"x":1})), Err(ToolError::InvalidParams(_))));
}

#[test]
fn no_workspace_state_is_an_error() {
    let x = setup();
    let t = tools(&x, MailConfig::default());
    fs::remove_file(Store::open(&x.p, ID).dir().join("workspace.json")).unwrap();
    let want = Err(ToolError::Failed(format!("zygo: workspace {ID} has no state")));
    assert_eq!(tool(&t, "mail_send").call(send_args("elena")), want);
    assert_eq!(tool(&t, "mail_read").call(json!({})), want);
    assert_eq!(tool(&t, "mail_list").call(json!({})), want);
}

// ---- build: the mail tools in the server's tool list (child process: `tools` reads HOME and ZYGO_CONFIG) ----

fn def(name: &str) -> ToolDef {
    ToolDef { name: name.into(), description: "d".into(), input_schema: json!({"type":"object"}) }
}
fn fake(name: &'static str) -> impl FnOnce(&McpCtx) -> Vec<Box<dyn Tool>> {
    move |_| {
        struct F(&'static str);
        impl Tool for F {
            fn def(&self) -> ToolDef { def(self.0) }
            fn call(&self, _: Value) -> Result<Value, ToolError> { Ok(json!("ok")) }
        }
        vec![Box::new(F(name))]
    }
}

/// Child half: runs only when the parent sets `ZYGO_MCP_MAIL_CASE` to the session folder.
#[test]
fn child_build() {
    let Ok(dir) = std::env::var("ZYGO_MCP_MAIL_CASE") else { return };
    let sd = SessionDir::new(dir.into());
    member_view(true, LEAD).save(&sd).unwrap();
    let on = ctx(Some(&sd), true);
    let n = names(&build(&on).unwrap());
    for m in ["mail_send", "mail_read", "mail_list"] {
        assert!(n.iter().any(|x| x == m), "{m} missing from {n:?}");
    }
    let e = build_with(&on, fake("mail_send"), |_| vec![]).err().expect("duplicate refused");
    assert!(e.to_string().contains("mail_send"), "{e}");
    let n = names(&build(&ctx(Some(&sd), false)).unwrap());
    assert!(!n.iter().any(|x| x.starts_with("mail_")), "{n:?}");
}

#[test]
fn build_lists_mail_tools_and_refuses_duplicates() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("config.toml");
    fs::write(&cfg, "").unwrap();
    let sd = SessionDir::new(d.path().join("session"));
    sd.create().unwrap();
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_build", "--nocapture", "--test-threads=1"])
        .env("HOME", d.path())
        .env("ZYGO_CONFIG", &cfg)
        .env("ZYGO_MCP_MAIL_CASE", &sd.root)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}
