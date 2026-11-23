//! M3: hand-started members (W§8.4, AU9, AU34, AU58, AU59, AU79, AU94, AU98, AU102, W-C9; W§18 #19, #32, #54, #55, #56). Every case runs a workspace path, so the file is cfg(unix) (OP5).
#![cfg(unix)]
mod common;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use common::workspace::{ready_exe, Guarded, Harness, WS_ID};
use serde_json::{json, Value};
use zygo_core::Agent;
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind, Phase, Workspace};

const KEY: &str = "kaaaaaaaa";
const KEY2: &str = "kbbbbbbbb";
const OTHER_ID: &str = "fedcba9876543210";
const OLD_ZS: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
const LIVE_ZS: &str = "01JBBBBBBBBBBBBBBBBBBBBBBB";
const MERGE: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force";
const NO_FILE: &str = "zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used";
const AU98: &str = "zygo: --server holds user info; this agent is not saved by the workspace";
const NOT_SAVED: &str = "zygo: this agent is not saved by the workspace: ";
const NO_CHECK: (&str, &str) = ("ZYGO_NO_UPDATE_CHECK", "1");
/// The `[server]` table of `workspace_run.rs` (serve mode reads its token from `[server] token`,
/// `serve_client.rs:309-313`).
const TEAM_SERVER: &str = "[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n";

// ---- Helpers (as `workspace_run.rs:19-39`, `:175-178`, `:339-351`, `:827-842`) ----

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn json_file(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

/// Polls `f` every 20 ms until it is true or `bound` is over (a test bound).
fn until(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = std::time::Instant::now();
    loop {
        if f() {
            return true;
        }
        if t0.elapsed() >= bound {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The `(&'static str, OsString)` list of `Harness::stub` as `(&str, &OsStr)` pairs.
fn pairs<'a>(v: &'a [(&'static str, OsString)]) -> Vec<(&'a str, &'a OsStr)> {
    v.iter().map(|(k, v)| (*k, v.as_os_str())).collect()
}

/// `NO_CHECK` as an environment pair.
fn no_check() -> (&'static str, &'static OsStr) {
    (NO_CHECK.0, OsStr::new(NO_CHECK.1))
}

/// The control folder of a stub named `name`.
fn ctl(h: &Harness, name: &str) -> PathBuf {
    h.root().join("ctl").join(name)
}

/// The control stub at `ctl` has started (its `done` file starts with `start `).
fn started(ctl: &Path) -> bool {
    until(Duration::from_secs(20), || std::fs::read_to_string(ctl.join("done")).is_ok_and(|d| d.starts_with("start ")))
}

/// The environment the stub received.
fn stub_env(out: &Path) -> Value {
    json_file(&out.join("env.json"))
}

/// A gate hold (M2 D9): `<gate>/<point>.hold` exists while this value lives; `Drop` removes it.
struct Hold(PathBuf);

impl Hold {
    fn new(gate: &Path, point: &str) -> Hold {
        std::fs::create_dir_all(gate).unwrap();
        let p = gate.join(format!("{point}.hold"));
        std::fs::write(&p, "").unwrap();
        Hold(p)
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn argv(out: &Path) -> Vec<String> {
    serde_json::from_value(json_file(&out.join("argv.json"))).unwrap()
}

/// The `--settings` file the stub received; empty when there is none.
fn settings(out: &Path) -> String {
    std::fs::read_to_string(out.join("settings.json")).unwrap_or_default()
}

/// The `--settings` tokens of an argv.
fn settings_args(a: &[String]) -> usize {
    a.iter().filter(|t| *t == "--settings" || t.starts_with("--settings=")).count()
}

/// The session view the stub received.
fn view(out: &Path) -> Value {
    serde_json::from_str(json_file(&out.join("session.json"))["view.json"].as_str().unwrap()).unwrap()
}

fn member(h: &Harness, key: &str) -> Option<MemberRecord> {
    h.store(WS_ID).read().unwrap().and_then(|w| w.members.get(key).cloned())
}

/// The stored JSON of `members[key]` of WS_ID.
fn raw_member(h: &Harness, key: &str) -> Value {
    json_file(&h.store(WS_ID).dir().join("workspace.json"))["members"][key].clone()
}

/// The bytes of `workspace.json` of WS_ID.
fn state_bytes(h: &Harness) -> Vec<u8> {
    std::fs::read(h.store(WS_ID).dir().join("workspace.json")).unwrap()
}

/// Session `zygo-app` on the private server with the first pane `cat`; `@zygo-pane` of that pane and `@zygo-ws` of
/// the session when `Some`. Returns the first pane id.
fn workspace_session(h: &Harness, first_key: Option<&str>, ws_opt: Option<&str>) -> String {
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "-x", "120", "-y", "40", "cat"]);
    let pane = h.tmux(&["display-message", "-p", "-t", "=zygo-app:", "#{pane_id}"]).trim().to_string();
    if let Some(k) = first_key {
        h.tmux(&["set-option", "-p", "-t", &pane, "@zygo-pane", k]);
    }
    if let Some(id) = ws_opt {
        h.tmux(&["set-option", "-t", "=zygo-app:", "@zygo-ws", id]);
    }
    pane
}

/// A new pane in `session_target` that runs `cmd`; returns its id.
fn split(h: &Harness, session_target: &str, cmd: &str) -> String {
    h.tmux(&["split-window", "-P", "-F", "#{pane_id}", "-t", session_target, cmd]).trim().to_string()
}

/// `@zygo-pane` of `pane`; empty when unset.
fn pane_key(h: &Harness, pane: &str) -> String {
    h.tmux(&["show-options", "-p", "-qv", "-t", pane, "@zygo-pane"]).trim().to_string()
}

/// The state WS_ID: the base fields of `Harness::seed`, `panes[]` = KEY2 (shell) and one pane per member key, the
/// members, then `f`.
fn up_state(h: &Harness, members: &[(&str, MemberRecord)], f: &mut dyn FnMut(&mut Workspace)) {
    let repo = h.repo();
    h.seed(WS_ID, &mut |w| {
        w.panes = vec![Pane { key: KEY2.into(), cwd: repo.clone(), kind: PaneKind::Shell, ..Default::default() }];
        w.members.clear();
        for (i, (k, m)) in members.iter().enumerate() {
            let kind = match m.launch.kind {
                Agent::ClaudeCode => PaneKind::ClaudeCode,
                Agent::OpenCode => PaneKind::OpenCode,
            };
            w.panes.push(Pane { key: (*k).into(), index: i as u32 + 1, cwd: repo.clone(), kind, ..Default::default() });
            w.members.insert((*k).into(), m.clone());
        }
        f(w);
    });
}

/// `$TMUX` of a client of the private server.
fn tmux_env(h: &Harness) -> OsString {
    OsString::from(format!("{},1,0", h.socket().display()))
}

/// `zygo <args>` as a hand start in `pane` of WS_ID: `TMUX`, `TMUX_PANE`, `ZYGO_WORKSPACE`, the update check off, then
/// `extra` (a later name wins).
fn hand(h: &Harness, pane: &str, args: &[&str], extra: &[(&str, &OsStr)]) -> Command {
    let t = tmux_env(h);
    let mut all: Vec<(&str, &OsStr)> =
        vec![("TMUX", t.as_os_str()), ("TMUX_PANE", OsStr::new(pane)), ("ZYGO_WORKSPACE", OsStr::new(WS_ID)), no_check()];
    all.extend_from_slice(extra);
    h.zygo(args, &all)
}

/// `s` as one single-quoted shell word.
fn quote(s: &OsStr) -> String {
    format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"))
}

/// As `Harness::script` (`common/workspace.rs:343-363`): writes `root/<name>.sh` (mode 0644), which waits until
/// `root/<name>.go` exists, then runs `zygo <args>` with only the harness environment, the update check off and
/// `extra`, and with `TMUX` and `TMUX_PANE` of the pane; its stderr goes to `root/<name>.err`. Returns the pane
/// command.
fn hand_script(h: &Harness, name: &str, args: &[&str], extra: &[(&str, &OsStr)]) -> String {
    use std::os::unix::fs::PermissionsExt;
    let go = h.root().join(format!("{name}.go"));
    let mut vars = h.env();
    let mut all = vec![no_check()];
    all.extend_from_slice(extra);
    for (k, v) in all {
        vars.retain(|(n, _)| n != OsStr::new(k));
        vars.push((OsString::from(k), v.to_os_string()));
    }
    let assigns: Vec<String> = vars.iter().map(|(k, v)| format!("{}={}", k.to_string_lossy(), quote(v))).collect();
    let words: Vec<String> = args.iter().map(|a| quote(OsStr::new(a))).collect();
    let body = format!(
        "while [ ! -e {} ]; do sleep 0.05; done\nexec env -i {} TMUX=\"$TMUX\" TMUX_PANE=\"$TMUX_PANE\" {} {} 2>{}\n",
        quote(go.as_os_str()),
        assigns.join(" "),
        quote(OsStr::new(env!("CARGO_BIN_EXE_zygo"))),
        words.join(" "),
        quote(h.root().join(format!("{name}.err")).as_os_str())
    );
    let path = h.root().join(format!("{name}.sh"));
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    format!("/bin/sh {}", quote(path.as_os_str()))
}

/// `h.repo()` as a git repository with one commit (the form of FIX-27's `repo` helper).
fn git_repo(h: &Harness) {
    for args in [&["init", "-q"][..], &["commit", "-q", "--allow-empty", "-m", "init"]] {
        let o = Command::new("git")
            .args(["-c", "user.name=zygo-test", "-c", "user.email=zygo-test@example.invalid", "-c", "init.defaultBranch=main"])
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(h.repo())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o.stderr));
    }
    // `zygo up` finds git on the harness `PATH` (as `tests/update.rs` `git_repo` does).
    let link = h.bin.join("git");
    if !link.exists() {
        std::os::unix::fs::symlink(which::which("git").unwrap(), link).unwrap();
    }
}

/// `root/hold.sh`: the `SHELL` of the CLI workspaces, so a shell pane stays. It answers `--version` (the probe of
/// `ready_exe`) at once. tmux takes it as `default-shell` and runs the command of a new pane with `-c`, so that form
/// runs the command.
fn hold_shell(h: &Harness) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = h.root().join("hold.sh");
    if !p.exists() {
        std::fs::write(&p, "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\n[ \"$1\" = -c ] && exec /bin/sh -c \"$2\"\nPATH=/usr/bin:/bin\nexec sleep 600\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        ready_exe(&p);
    }
    p
}

/// A workspace verb in `h.repo()` on the private server, with the hold shell.
fn cli(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)]) -> Output {
    let t = tmux_env(h);
    let sh = hold_shell(h);
    let mut all: Vec<(&str, &OsStr)> = vec![("TMUX", t.as_os_str()), no_check(), ("SHELL", sh.as_os_str())];
    all.extend_from_slice(extra);
    let mut c = h.zygo(args, &all);
    c.current_dir(h.repo());
    Guarded::spawn(c).output(args[0])
}

/// The one workspace state of the harness.
fn only_state(h: &Harness) -> (String, Workspace) {
    let mut all = zygo_workspace::state::list_all(&h.paths);
    assert_eq!(all.len(), 1, "{:?}", all.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
    let (id, ws) = all.remove(0);
    (id, ws.unwrap())
}

/// A record of kind `kind` in `state` with `agent_session` and `zygo_session`.
fn record(kind: Agent, state: MemberState, agent: Option<&str>, zs: Option<&str>) -> MemberRecord {
    let mut r = MemberRecord::new(kind);
    r.state = state;
    r.agent_session = agent.map(String::from);
    r.zygo_session = zs.map(String::from);
    r
}

/// A port with no listener: the port of a dropped `127.0.0.1:0` listener.
fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The pane of the session `session` whose `@zygo-pane` is `key`.
fn pane_of(h: &Harness, session: &str, key: &str) -> String {
    let rows = h.tmux(&["list-panes", "-s", "-t", &format!("={session}"), "-F", "#{pane_id} #{@zygo-pane}"]);
    rows.lines().find_map(|l| l.strip_suffix(&format!(" {key}")).map(String::from)).unwrap_or_else(|| panic!("{rows}"))
}

// ---- Not a member: a v1 launch (OP9, AU58, W-C6; W§18 #54, #55) ----

#[test]
fn w18_54_outside_tmux_is_a_v1_launch_with_no_tmux_call() { // decision 8
    let h = Harness::no_tmux();
    let t = h.bin.join("tmux");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&t, "#!/bin/sh\ntouch \"$HOME/tmux-called\"\nexit 1\n").unwrap();
        std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    ready_exe(&t);
    let _ = std::fs::remove_file(h.root().join("tmux-called")); // the probe of `ready_exe` ran it once
    for (i, tmux) in [None, Some("")].into_iter().enumerate() {
        let (out, env) = h.stub(Agent::ClaudeCode, &format!("a{i}"));
        let mut extra = pairs(&env);
        extra.extend([("ZYGO_WORKSPACE", OsStr::new(WS_ID)), ("ZYGO_PANE", OsStr::new(KEY)), ("TMUX_PANE", OsStr::new("%0")), no_check()]);
        if let Some(v) = tmux {
            extra.push(("TMUX", OsStr::new(v)));
        }
        let o = Guarded::spawn(h.zygo(&["claude"], &extra)).output("zygo claude");
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        let e = stub_env(&out);
        assert!(e.get("ZYGO_WORKSPACE").is_none() && e.get("ZYGO_PANE").is_none(), "{e}");
        assert!(!settings(&out).contains("workspace --event"));
        assert!(!h.store(WS_ID).dir().exists());
        assert!(!h.root().join("tmux-called").exists());
        assert!(!text(&o.stderr).contains("workspace"), "{}", text(&o.stderr));
    }
}

#[test]
fn w18_54_a_tmux_session_that_is_not_a_workspace_is_a_v1_launch() {
    for case in 1..=3 {
        let Some(h) = Harness::new() else { return };
        let pane = match case {
            1 => {
                h.tmux(&["new-session", "-d", "-s", "plain", "-x", "120", "-y", "40", "cat"]);
                h.tmux(&["display-message", "-p", "-t", "=plain:", "#{pane_id}"]).trim().to_string()
            }
            2 => workspace_session(&h, None, None),
            _ => {
                up_state(&h, &[], &mut |_| {});
                workspace_session(&h, None, Some(OTHER_ID))
            }
        };
        let before = (case == 3).then(|| state_bytes(&h));
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        let mut extra = pairs(&env);
        extra.push(("ZYGO_PANE", OsStr::new(KEY)));
        let o = Guarded::spawn(hand(&h, &pane, &["claude"], &extra)).output("hand start");
        assert_eq!(o.status.code(), Some(0), "case {case}: {}", text(&o.stderr));
        let e = stub_env(&out);
        assert!(e.get("ZYGO_WORKSPACE").is_none() && e.get("ZYGO_PANE").is_none(), "case {case}: {e}");
        assert!(!settings(&out).contains("workspace --event"), "case {case}");
        assert_eq!(pane_key(&h, &pane), "", "case {case}");
        assert!(!text(&o.stderr).contains("workspace"), "case {case}: {}", text(&o.stderr));
        match before {
            None => assert!(!h.store(WS_ID).dir().exists(), "case {case}"),
            Some(b) => assert_eq!(state_bytes(&h), b, "case {case}"),
        }
    }
}

#[test]
fn w18_55_a_launch_from_a_member_tool_does_not_join() {
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let pane = split(&h, "=zygo-app:", "cat");
    let before = state_bytes(&h);
    for (i, zs) in ["01JCCCCCCCCCCCCCCCCCCCCCCC", ""].into_iter().enumerate() {
        let (out, env) = h.stub(Agent::ClaudeCode, &format!("a{i}"));
        let mut extra = pairs(&env);
        extra.push(("ZYGO_SESSION", OsStr::new(zs)));
        let o = Guarded::spawn(hand(&h, &pane, &["claude"], &extra)).output("hand start");
        assert_eq!(o.status.code(), Some(0), "{zs:?}: {}", text(&o.stderr));
        let e = stub_env(&out);
        assert!(e.get("ZYGO_WORKSPACE").is_none() && e.get("ZYGO_PANE").is_none(), "{zs:?}: {e}");
        assert!(!settings(&out).contains("workspace --event"), "{zs:?}");
        assert_eq!(pane_key(&h, &pane), "", "{zs:?}");
        assert_eq!(state_bytes(&h), before, "{zs:?}");
        assert!(!text(&o.stderr).contains("workspace"), "{zs:?}: {}", text(&o.stderr));
    }
}

// ---- The hand join (AU9, AU41, AU79, W§8.3, AU53, decision 3) ----

#[test]
fn au9_au79_a_hand_start_in_a_new_split_gets_a_key_and_one_record_write() {
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let g = h.root().join("gate");
    let hold = Hold::new(&g, "callback");
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    let c = ctl(&h, "a");
    env.extend([("ZYGO_STUB_CONTROL", c.clone().into_os_string()), ("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string())]);
    let mut extra = pairs(&env);
    extra.push(("ZYGO_WORKSPACE", OsStr::new(WS_ID)));
    let pane = split(&h, "=zygo-app:", &hand_script(&h, "hand", &["claude"], &extra));
    std::fs::write(h.root().join("hand.go"), "").unwrap();
    assert!(until(Duration::from_secs(20), || g.join("callback").exists()));
    // At the gate: the start write is done, the agent is not started.
    let key = pane_key(&h, &pane);
    assert!(zygo_workspace::identity::valid_pane_key(&key) && key != KEY2, "{key}");
    let m = member(&h, &key).unwrap();
    let p = m.pending.clone().unwrap();
    assert_eq!(
        (m.state, m.launch.kind, m.zygo_session.clone(), m.agent_session.clone()),
        (MemberState::Running, Agent::ClaudeCode, None, None)
    );
    assert!(p.agent_session.is_some() && !p.zygo_session.is_empty());
    assert_eq!((m.program.clone(), m.launch.args.clone()), (Some(h.bin.join("claude")), Vec::<String>::new()));
    drop(hold);
    assert!(started(&c));
    // The start snapshot runs after the callback gate and before the agent starts (AU53).
    let ws = h.store(WS_ID).read().unwrap().unwrap();
    assert!(ws.panes.iter().any(|q| q.key == key && q.kind == PaneKind::ClaudeCode), "{:?}", ws.panes);
    let e = stub_env(&out);
    let id = json_file(&out.join("session-arg.json"))["id"].as_str().unwrap().to_string();
    assert_eq!(Some(id.clone()), p.agent_session);
    assert!(argv(&out).contains(&format!("--session-id={id}")));
    assert_eq!(
        (e["ZYGO_WORKSPACE"].as_str(), e["ZYGO_PANE"].as_str(), e["ZYGO_SESSION"].as_str()),
        (Some(WS_ID), Some(key.as_str()), Some(p.zygo_session.as_str()))
    );
    assert!(settings(&out).contains("workspace --event prompt"));
    // No workspace line; the launch lines of a v1 launch (for example `plugins: offline`) stay (A5).
    let err = std::fs::read_to_string(h.root().join("hand.err")).unwrap_or_default();
    assert!(!err.contains("workspace") && !err.contains("zygo: "), "{err}");
    common::stub_control(&c, "exit 0").unwrap();
    assert!(until(Duration::from_secs(20), || member(&h, &key)
        .is_some_and(|m| m.state == MemberState::Ended && m.agent_session == Some(id.clone()))));
}

/// A workspace `zygo-app` (WS_ID) with KEY2 on its first pane, the state up, and a new `cat` split; returns the split.
fn split_in_workspace(h: &Harness, f: &mut dyn FnMut(&mut Workspace)) -> String {
    workspace_session(h, Some(KEY2), Some(WS_ID));
    up_state(h, &[], f);
    split(h, "=zygo-app:", "cat")
}

#[test]
fn au9_an_opencode_hand_start_gets_the_member_plugin() {
    let Some(h) = Harness::new() else { return };
    let pane = split_in_workspace(&h, &mut |_| {});
    let (out, env) = h.stub(Agent::OpenCode, "o");
    let o = Guarded::spawn(hand(&h, &pane, &["opencode"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let key = pane_key(&h, &pane);
    assert!(zygo_workspace::identity::valid_pane_key(&key), "{key}");
    assert!(!out.join("session-arg.json").exists());
    let e = stub_env(&out);
    assert_eq!((e["ZYGO_WORKSPACE"].as_str(), e["ZYGO_PANE"].as_str()), (Some(WS_ID), Some(key.as_str())));
    let plugin = json_file(&out.join("ocdir.json"))["plugins/zygo.js"].as_str().unwrap().to_string();
    assert!(plugin.contains("process.env.ZYGO_WORKSPACE"));
    assert_eq!(view(&out)["workspace"]["member"], json!({"ws_id": WS_ID, "pane": key}));
    let m = member(&h, &key).unwrap();
    assert_eq!((m.launch.kind, m.state, m.agent_session.clone()), (Agent::OpenCode, MemberState::Ended, None));
    assert_eq!(m.zygo_session.as_deref(), e["ZYGO_SESSION"].as_str());
}

#[test]
fn au30_an_own_session_argument_is_not_preassigned_or_saved() { // W§8.3, AU30
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    for (i, (flag, own)) in [("--resume", "X1"), ("--session-id", "Y1")].into_iter().enumerate() {
        let pane = split(&h, "=zygo-app:", "cat");
        let (out, env) = h.stub(Agent::ClaudeCode, &format!("a{i}"));
        let o = Guarded::spawn(hand(&h, &pane, &["claude", "--", flag, own], &pairs(&env))).output("hand start");
        assert_eq!(o.status.code(), Some(0), "{flag}: {}", text(&o.stderr));
        // The stub names the session argument it got: the user's, not a pre-assigned one.
        assert_eq!(json_file(&out.join("session-arg.json")), json!({"flag": flag, "id": own}), "{flag}");
        let a = argv(&out);
        assert!(a.iter().any(|t| t == flag) && a.iter().any(|t| t == own), "{a:?}");
        assert!(!a.iter().any(|t| t.starts_with("--session-id=")), "{a:?}");
        let m = member(&h, &pane_key(&h, &pane)).unwrap();
        assert_eq!((m.launch.args.clone(), m.pending.clone()), (Vec::<String>::new(), None), "{flag}");
        assert!(m.zygo_session.is_some(), "{flag}");
    }
}

// ---- AU94, W-C9 ----

#[test]
fn w18_32_a_resumable_key_refuses_with_its_resume_command_and_an_ended_key_joins() {
    let Some(h) = Harness::new() else { return };
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    up_state(&h, &[(KEY, record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS)))], &mut |_| {});
    let before = raw_member(&h, KEY);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &first, &["claude"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        text(&o.stderr),
        format!("zygo: pane {KEY} holds resumable session abc; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume 'abc'; or run zygo ws forget-pane {KEY}\n")
    );
    assert_eq!(raw_member(&h, KEY), before);
    assert!(!out.join("argv.json").exists());
    h.store(WS_ID)
        .update_existing(Duration::from_secs(5), &mut |w| {
            let m = w.members.get_mut(KEY).unwrap();
            m.state = MemberState::Ended;
            m.overrides = Some(json!({"results": false}));
            Ok(())
        })
        .unwrap();
    let (out, env) = h.stub(Agent::ClaudeCode, "b");
    let o = Guarded::spawn(hand(&h, &first, &["claude"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let id = json_file(&out.join("session-arg.json"))["id"].as_str().unwrap().to_string();
    let zs = stub_env(&out)["ZYGO_SESSION"].as_str().unwrap().to_string();
    let m = member(&h, KEY).unwrap();
    assert_eq!((m.zygo_session, m.agent_session, m.state, m.overrides), (Some(zs), Some(id), MemberState::Ended, None)); // replaced
}

#[test]
fn au94_the_resume_commands_that_zygo_prints_are_accepted() { // decision 4
    for case in ["a", "b", "c"] {
        let Some(h) = Harness::new() else { return };
        let first = workspace_session(&h, Some(KEY), Some(WS_ID));
        let (kind, id) = if case == "c" { (Agent::OpenCode, "ses_1") } else { (Agent::ClaudeCode, "abc") };
        let mut r = record(kind, MemberState::Resumable, Some(id), Some(OLD_ZS));
        r.overrides = Some(json!({"results": false}));
        let dir = h.root().join("c c");
        if case == "b" {
            std::fs::create_dir(&dir).unwrap();
            r.config_dir = Some(dir.clone());
        }
        up_state(&h, &[(KEY, r)], &mut |_| {});
        // The texts that `member.rs` pins (`exit_lines_are_the_spec_texts`, `the_resume_command_names_the_config_dir_quoted`).
        let command = match case {
            "a" => "env -u CLAUDE_CONFIG_DIR zygo claude -- --resume 'abc'".to_string(),
            "b" => format!("CLAUDE_CONFIG_DIR='{}' zygo claude -- --resume 'abc'", dir.display()),
            _ => "zygo opencode -- --session='ses_1'".to_string(),
        };
        // The shell finds `zygo` and `env` on the harness `PATH`.
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_zygo"), h.bin.join("zygo")).unwrap();
        let env_tool = ["/usr/bin/env", "/bin/env"].into_iter().map(PathBuf::from).find(|p| p.is_file()).unwrap();
        std::os::unix::fs::symlink(env_tool, h.bin.join("env")).unwrap();
        let (out, env) = h.stub(kind, "a");
        // `hand`'s environment, so the shell unquotes the words as a user's shell does.
        let c = hand(&h, &first, &[], &pairs(&env));
        let mut sh = Command::new("/bin/sh");
        sh.args(["-c", &command]).env_clear().envs(c.get_envs().filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string()))));
        sh.current_dir(h.root()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let o = Guarded::spawn(sh).output("resume command");
        assert_eq!(o.status.code(), Some(0), "{case}: {}", text(&o.stderr));
        let a = argv(&out);
        if case == "c" {
            assert!(a.iter().any(|t| t == "--session=ses_1"), "{a:?}");
        } else {
            assert!(a.iter().any(|t| t == "--resume") && a.iter().any(|t| t == "abc"), "{a:?}");
        }
        let m = member(&h, KEY).unwrap();
        assert_eq!((m.agent_session.as_deref(), m.overrides.clone()), (Some(id), Some(json!({"results": false}))), "{case}");
        assert_eq!(m.zygo_session.as_deref(), stub_env(&out)["ZYGO_SESSION"].as_str(), "{case}");
    }
}

#[test]
fn au94_a_live_launcher_on_the_key_refuses() {
    let Some(h) = Harness::new() else { return };
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    up_state(&h, &[(KEY, record(Agent::ClaudeCode, MemberState::Ended, Some("abc"), Some(LIVE_ZS)))], &mut |_| {});
    let d = h.paths.sessions_dir.join(LIVE_ZS);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("token"), "t").unwrap();
    std::fs::write(d.join("pid"), "1").unwrap();
    let lock = std::fs::File::options().read(true).write(true).open(d.join("pid")).unwrap();
    assert!(fs4::FileExt::try_lock(&lock).is_ok());
    let before = raw_member(&h, KEY);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &first, &["claude"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        text(&o.stderr),
        "zygo: pane kaaaaaaaa holds session abc, whose launcher is still running; zygo starts no second agent on it\n"
    );
    assert_eq!(raw_member(&h, KEY), before);
    assert!(!out.join("argv.json").exists());
    drop(lock);
}

#[test]
fn w_c9_an_agent_session_of_another_key_refuses() { // decision 5
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[(KEY, record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS)))], &mut |_| {});
    let pane = split(&h, "=zygo-app:", "cat");
    let before = raw_member(&h, KEY);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &pane, &["claude", "--", "--resume", "abc"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        text(&o.stderr),
        "zygo: agent session abc belongs to pane kaaaaaaaa; resume it in that pane, or run zygo ws forget-pane kaaaaaaaa\n"
    );
    assert!(!out.join("argv.json").exists());
    let ws = h.store(WS_ID).read().unwrap().unwrap();
    assert_eq!(ws.members.keys().cloned().collect::<Vec<_>>(), [KEY]);
    assert_eq!(raw_member(&h, KEY), before);
}

#[test]
fn a_launch_that_ends_before_the_callback_leaves_the_record_byte_equal() { // decision 3
    let Some(h) = Harness::new() else { return };
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    let mut r = record(Agent::ClaudeCode, MemberState::Ended, Some("old"), Some(OLD_ZS));
    r.overrides = Some(json!({"results": false}));
    up_state(&h, &[(KEY, r)], &mut |_| {});
    let bin2 = h.root().join("bin2");
    std::fs::create_dir(&bin2).unwrap();
    std::os::unix::fs::symlink(h.bin.join("tmux"), bin2.join("tmux")).unwrap();
    let before = raw_member(&h, KEY);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    extra.push(("PATH", bin2.as_os_str()));
    let o = Guarded::spawn(hand(&h, &first, &["claude"], &extra)).output("hand start");
    assert_ne!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(raw_member(&h, KEY), before);
    assert!(!out.join("argv.json").exists());
}

// ---- The not-saved line (decision 6) and the callback re-check (AU34 `NotMember`) ----

/// A hand start of Claude Code in a new split of `zygo-app` (WS_ID) after `setup`; checks exit 0, no
/// `ZYGO_WORKSPACE`, and that stderr holds the line that starts with `NOT_SAVED` + `reason` once; returns the pane.
fn not_saved(h: &Harness, reason: &str) -> String {
    workspace_session(h, Some(KEY2), Some(WS_ID));
    let pane = split(h, "=zygo-app:", "cat");
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(h, &pane, &["claude"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
    let err = text(&o.stderr);
    let want = format!("{NOT_SAVED}{reason}");
    assert_eq!(err.lines().filter(|l| l.starts_with(&want)).count(), 1, "{want}\n{err}");
    pane
}

#[test]
fn the_not_saved_line_names_each_reason() {
    let state_dir = |h: &Harness| h.root().join(".local/state/zygo/workspaces").join(WS_ID);
    // The state is missing.
    if let Some(h) = Harness::new() {
        not_saved(&h, &format!("no workspace state for {WS_ID}"));
    }
    // The state does not parse (`state.rs:440-443`).
    if let Some(h) = Harness::new() {
        std::fs::create_dir_all(h.store(WS_ID).dir()).unwrap();
        std::fs::write(h.store(WS_ID).dir().join("workspace.json"), "{").unwrap();
        not_saved(&h, &format!("{}: ", state_dir(&h).join("workspace.json").display()));
    }
    // The state folder is a link.
    if let Some(h) = Harness::new() {
        let elsewhere = h.root().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::create_dir_all(h.paths.workspaces_dir()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, h.store(WS_ID).dir()).unwrap();
        not_saved(&h, &format!("{}: state folder is a link", state_dir(&h).display()));
    }
    // Another tmux session.
    if let Some(h) = Harness::new() {
        up_state(&h, &[], &mut |w| {
            w.name = "other".into();
            w.tmux_session = "zygo-other".into();
        });
        not_saved(&h, "the workspace state names tmux session zygo-other, not zygo-app");
    }
    // Another tmux server.
    if let Some(h) = Harness::new() {
        let other = h.root().join("other");
        up_state(&h, &[], &mut |w| w.tmux_socket = Some(other.clone()));
        not_saved(&h, &format!("the workspace state names tmux server {}, not {}", other.display(), h.socket().display()));
    }
    // A phase that is not up or draining.
    for (ph, word) in [(Phase::Down, "down"), (Phase::Building, "building"), (Phase::Stopping, "stopping")] {
        let Some(h) = Harness::new() else { return };
        up_state(&h, &[], &mut |w| w.phase = ph);
        workspace_session(&h, Some(KEY2), Some(WS_ID));
        let pane = split(&h, "=zygo-app:", "cat");
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        let o = Guarded::spawn(hand(&h, &pane, &["claude"], &pairs(&env))).output("hand start");
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
        let want = format!("{NOT_SAVED}cannot assign a key to pane {pane}: a member starts only in phase up or draining, not {word}\n");
        assert_eq!(text(&o.stderr).matches(&want).count(), 1, "{want}\n{}", text(&o.stderr));
    }
    // The record write fails.
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let pane = split(&h, "=zygo-app:", "cat");
    let g = h.root().join("gate");
    let hold = Hold::new(&g, "launch");
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    let child = Guarded::spawn(hand(&h, &pane, &["claude"], &pairs(&env)));
    assert!(until(Duration::from_secs(20), || g.join("launch").exists()));
    use std::os::unix::fs::PermissionsExt;
    let dir = h.store(WS_ID).dir().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    drop(hold);
    let o = child.output("hand start");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let want = format!("{NOT_SAVED}could not write the member record: ");
    assert_eq!(text(&o.stderr).lines().filter(|l| l.starts_with(&want)).count(), 1, "{}", text(&o.stderr));
    assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
    assert_eq!(member(&h, &pane_key(&h, &pane)), None);
}

#[test]
fn the_callback_checks_again_and_the_launch_goes_on_as_v1() { // decision 3, AU34 `NotMember`
    let Some(h) = Harness::new() else { return };
    let pane = split_in_workspace(&h, &mut |_| {});
    let g = h.root().join("gate");
    let hold = Hold::new(&g, "launch");
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    let child = Guarded::spawn(hand(&h, &pane, &["claude"], &pairs(&env)));
    assert!(until(Duration::from_secs(20), || g.join("launch").exists()));
    let key = pane_key(&h, &pane);
    h.store(WS_ID)
        .update_existing(Duration::from_secs(5), &mut |w| {
            w.members.insert(key.clone(), record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS)));
            Ok(())
        })
        .unwrap();
    let written = raw_member(&h, &key);
    drop(hold);
    let o = child.output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let want = format!("{NOT_SAVED}pane {key} holds resumable session abc\n");
    assert_eq!(text(&o.stderr).matches(&want).count(), 1, "{}", text(&o.stderr));
    let e = stub_env(&out);
    assert!(e.get("ZYGO_WORKSPACE").is_none() && e.get("ZYGO_PANE").is_none(), "{e}");
    assert!(!settings(&out).contains("workspace --event"));
    assert!(view(&out).get("workspace").is_none_or(Value::is_null), "{}", view(&out));
    assert_eq!(raw_member(&h, &key), written);
}

#[test]
fn a_join_in_phase_draining_works() {
    let Some(h) = Harness::new() else { return };
    let pane = split_in_workspace(&h, &mut |w| w.phase = Phase::Draining);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &pane, &["claude"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let key = pane_key(&h, &pane);
    let e = stub_env(&out);
    assert_eq!(e["ZYGO_PANE"].as_str(), Some(key.as_str()));
    assert_eq!(member(&h, &key).unwrap().zygo_session.as_deref(), e["ZYGO_SESSION"].as_str());
}

// ---- AU98 ----

#[test]
fn au98_a_hand_launch_with_user_info_is_v1_for_both_agents() {
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let url = format!("https://u:p@127.0.0.1:{}", closed_port());
    let before = state_bytes(&h);
    for agent in ["claude", "opencode"] {
        let pane = split(&h, "=zygo-app:", "cat");
        let kind = if agent == "claude" { Agent::ClaudeCode } else { Agent::OpenCode };
        let (_, env) = h.stub(kind, agent);
        let o = Guarded::spawn(hand(&h, &pane, &[agent, "--server", &url], &pairs(&env))).output("hand start");
        assert_eq!(text(&o.stderr).matches(&format!("{AU98}\n")).count(), 1, "{agent}: {}", text(&o.stderr));
        assert!(h.store(WS_ID).read().unwrap().unwrap().members.is_empty(), "{agent}");
        assert_eq!(pane_key(&h, &pane), "", "{agent}");
        assert_eq!(state_bytes(&h), before, "{agent}");
    }
}

// ---- Through the CLI: AU59 (W§18 #56), loadout values (W§18 #19) ----

#[test]
fn w18_56_a_settings_argument_keeps_a_hand_start_a_member_through_down_and_up() { // AU59; decision 2
    for (case, line) in [("readable", MERGE), ("missing", NO_FILE)] {
        let Some(h) = Harness::new() else { return };
        git_repo(&h);
        std::fs::write(h.root().join("config.toml"), "[loadout.hold]\n[[loadout.hold.window]]\n[[loadout.hold.window.pane]]\nkind = \"shell\"\n")
            .unwrap();
        let o = cli(&h, &["up", "--loadout", "hold", "--no-attach"], &[]);
        assert_eq!(o.status.code(), Some(0), "{case}: {}", text(&o.stderr));
        let (id, ws) = only_state(&h);
        let file = h.root().join("user-settings.json");
        if case == "readable" {
            std::fs::write(&file, r#"{"env":{"W":"1"}}"#).unwrap();
        }
        let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
        let c = ctl(&h, "a");
        env.push(("ZYGO_STUB_CONTROL", c.clone().into_os_string()));
        let mut extra = pairs(&env);
        extra.push(("ZYGO_WORKSPACE", OsStr::new(&id)));
        let script = hand_script(&h, "hand", &["claude", "--", "--settings", file.to_str().unwrap()], &extra);
        let pane = split(&h, &format!("={}:", ws.tmux_session), &script);
        std::fs::write(h.root().join("hand.go"), "").unwrap();
        assert!(started(&c), "{case}: {}", std::fs::read_to_string(h.root().join("hand.err")).unwrap_or_default());
        let key = pane_key(&h, &pane);
        let a = argv(&out);
        assert_eq!(settings_args(&a), 1, "{a:?}");
        assert!(!a.iter().any(|t| t.contains("user-settings.json")), "{a:?}");
        assert!(settings(&out).contains("workspace --event prompt"), "{case}");
        assert_eq!(case == "readable", settings(&out).contains("\"W\""), "{case}");
        let m = h.store(&id).read().unwrap().unwrap().members[&key].clone();
        assert_eq!(m.launch.args, ["--settings", file.to_str().unwrap()]);
        let err = std::fs::read_to_string(h.root().join("hand.err")).unwrap();
        assert!(err.contains(&format!("{line}\n")) && !err.contains("workspace"), "{err}");
        let agent_id = json_file(&out.join("session-arg.json"))["id"].as_str().unwrap().to_string();
        let o = cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
        assert_eq!(o.status.code(), Some(0), "{case}: {}", text(&o.stderr));
        assert!(until(Duration::from_secs(20), || h.store(&id).read().unwrap().unwrap().members[&key].state
            == MemberState::Resumable));
        let (out_b, env_b) = h.stub(Agent::ClaudeCode, "b");
        let o = cli(&h, &["up", &ws.name, "--no-attach"], &pairs(&env_b));
        assert_eq!(o.status.code(), Some(0), "{case}: {}", text(&o.stderr));
        assert!(until(Duration::from_secs(20), || out_b.join("argv.json").exists()), "{case}");
        let b = argv(&out_b);
        assert!(b.contains(&format!("--resume={agent_id}")), "{b:?}");
        assert_eq!(settings_args(&b), 1, "{b:?}");
        assert!(settings(&out_b).contains("workspace --event prompt"), "{case}");
        assert_eq!(case == "readable", settings(&out_b).contains("\"W\""), "{case}"); // AU59: read again at resume
        cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
    }
}

#[test]
fn w18_19_loadout_values_reach_the_launch_and_the_resume() { // decision 2; #19 sentences 1-2
    let Some(h) = Harness::new() else { return };
    git_repo(&h);
    let dir = h.root().join("cc");
    std::fs::create_dir(&dir).unwrap();
    let port = closed_port();
    let config = format!(
        "{TEAM_SERVER}[loadout.l19]\n[[loadout.l19.window]]\n[[loadout.l19.window.pane]]\nkind = \"shell\"\n\
         [[loadout.l19.window.pane]]\nkind = \"claude-code\"\ncompact = \"600k\"\nconfig_dir = \"{}\"\n\
         [[loadout.l19.window.pane]]\nkind = \"claude-code\"\nserver = \"https://127.0.0.1:{port}\"\n",
        dir.display()
    );
    std::fs::write(h.root().join("config.toml"), config).unwrap();
    let token = OsString::from(zygo_proxy::team::new_token().0);
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    let c = ctl(&h, "a");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into_os_string()));
    let mut extra = pairs(&env);
    extra.push(("ZYGO_TEAM_TOKEN", token.as_os_str()));
    let o = cli(&h, &["up", "--loadout", "l19", "--no-attach"], &extra);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(started(&c));
    let (id, ws) = only_state(&h);
    let e = stub_env(&out);
    assert_eq!(e["CLAUDE_CONFIG_DIR"].as_str(), Some(dir.to_str().unwrap()));
    let client = zygo_proxy::client::SessionClient::from_session_dir(&PathBuf::from(e["ZYGO_SESSION_DIR"].as_str().unwrap())).unwrap();
    assert_eq!(client.status().unwrap().compact, Some(zygo_proxy::client::WindowArg::Tokens(600_000)));
    let server_key = ws.members.iter().find(|(_, m)| m.launch.server.is_some()).map(|(k, _)| k.clone()).unwrap();
    let server_pane = pane_of(&h, &ws.tmux_session, &server_key);
    assert!(
        until(Duration::from_secs(20), || {
            let s = h.tmux(&["capture-pane", "-p", "-J", "-t", &server_pane]);
            s.contains("zygo: server unreachable: ") && s.contains("zygo: the claude launch failed before the agent started (exit 12)")
        }),
        "{}",
        h.tmux(&["capture-pane", "-p", "-J", "-t", &server_pane])
    );
    let agent_key = ws.members.iter().find(|(_, m)| m.launch.compact.is_some()).map(|(k, _)| k.clone()).unwrap();
    let agent_id = json_file(&out.join("session-arg.json"))["id"].as_str().unwrap().to_string();
    assert_eq!(cli(&h, &["down", &ws.name, "--now", "--wait"], &[]).status.code(), Some(0));
    assert!(until(Duration::from_secs(20), || h.store(&id).read().unwrap().unwrap().members[&agent_key].state
        == MemberState::Resumable));
    let (out_b, mut env_b) = h.stub(Agent::ClaudeCode, "b");
    let cb = ctl(&h, "b");
    env_b.push(("ZYGO_STUB_CONTROL", cb.clone().into_os_string()));
    let mut extra = pairs(&env_b);
    extra.push(("ZYGO_TEAM_TOKEN", token.as_os_str()));
    let o = cli(&h, &["up", &ws.name, "--no-attach"], &extra);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(started(&cb));
    let b = argv(&out_b);
    assert!(b.contains(&format!("--resume={agent_id}")), "{b:?}");
    let e = stub_env(&out_b);
    assert_eq!(e["CLAUDE_CONFIG_DIR"].as_str(), Some(dir.to_str().unwrap()));
    let client = zygo_proxy::client::SessionClient::from_session_dir(&PathBuf::from(e["ZYGO_SESSION_DIR"].as_str().unwrap())).unwrap();
    assert_eq!(client.status().unwrap().compact, Some(zygo_proxy::client::WindowArg::Tokens(600_000))); // AU32
    common::stub_control(&cb, "exit 0").unwrap();
    cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
}

/// The `hold` loadout and `l19u`, a Claude Code pane whose server holds user info.
const L19U: &str = "[loadout.hold]\n[[loadout.hold.window]]\n[[loadout.hold.window.pane]]\nkind = \"shell\"\n\
                    [loadout.l19u]\n[[loadout.l19u.window]]\n[[loadout.l19u.window.pane]]\nkind = \"claude-code\"\nserver = \"https://u:p@h\"\n";

#[test]
fn w18_19_config_check_and_a_hand_launch_report_user_info() { // #19 sentence 3; FIX-27 D9
    let Some(h) = Harness::new() else { return };
    git_repo(&h);
    std::fs::write(h.root().join("config.toml"), L19U).unwrap();
    let o = Guarded::spawn(h.zygo(&["config", "check"], &[no_check()])).output("config check");
    assert_eq!(o.status.code(), Some(11), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    let line = "error: loadout.l19u.window[0].pane[0].server: holds user info; zygo up starts this pane as a shell and does not save it (AU98)\n";
    assert!(out.contains(line), "{out}");
    assert!(!out.replace(line, "").contains("u:p") && !text(&o.stderr).contains("u:p"), "{out}{}", text(&o.stderr));
    let o = cli(&h, &["up", "--loadout", "hold", "--no-attach"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    let pane = split(&h, &format!("={}:", ws.tmux_session), "cat");
    let url = format!("https://u:p@127.0.0.1:{}", closed_port());
    let (_, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    extra.push(("ZYGO_WORKSPACE", OsStr::new(&id)));
    let o = Guarded::spawn(hand(&h, &pane, &["claude", "--server", &url], &extra)).output("hand start");
    assert_eq!(text(&o.stderr).matches(&format!("{AU98}\n")).count(), 1, "{}", text(&o.stderr));
    // The `after-split-window` hook of a `zygo up` session gives the new pane a key by its snapshot (W§7.4); the hand
    // start writes no record for it (A6).
    let ws = h.store(&id).read().unwrap().unwrap();
    assert!(ws.members.is_empty(), "{:?}", ws.members);
    let key = pane_key(&h, &pane);
    assert!(ws.panes.iter().all(|p| p.key != key || p.kind == PaneKind::Shell), "{:?}", ws.panes);
    cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
}

#[test]
fn w18_19_up_starts_a_user_info_pane_as_a_shell() { // AU98 in a loadout, through the CLI
    let Some(h) = Harness::new() else { return };
    git_repo(&h);
    std::fs::write(h.root().join("config.toml"), L19U).unwrap();
    let o = cli(&h, &["up", "--loadout", "l19u", "--no-attach"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(text(&o.stderr).matches(&format!("{AU98}\n")).count(), 1, "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    assert!(ws.members.is_empty());
    assert!(ws.panes.iter().all(|p| p.kind == PaneKind::Shell), "{:?}", ws.panes);
    let raw = std::fs::read_to_string(h.store(&id).dir().join("workspace.json")).unwrap();
    assert!(!raw.contains("u:p"));
    cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
}
