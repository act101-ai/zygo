//! M3: hand-started members (W§8.4, AU9, AU34, AU58, AU59, AU79, AU94, AU98, AU102, W-C9; W§18 #19, #32, #54, #55, #56). Every case runs a workspace path, so the file is cfg(unix) (OP5).
#![cfg(unix)]
mod common;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use common::workspace::{ready_exe, Guarded, Harness, WS_ID};
use serde_json::{json, Value};
use zygo_core::Agent;
use zygo_workspace::state::{CompactOverride, MemberRecord, MemberState, Pane, PaneKind, Pending, Phase, Workspace};

const KEY: &str = "kaaaaaaaa";
const KEY2: &str = "kbbbbbbbb";
const OTHER_ID: &str = "fedcba9876543210";
const OLD_ZS: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
const LIVE_ZS: &str = "01JBBBBBBBBBBBBBBBBBBBBBBB";
/// A zygo session with no session folder: its launcher is not live.
const DEAD_ZS: &str = "01JCCCCCCCCCCCCCCCCCCCCCCC";
const MERGE: &str = "zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force";
const NO_FILE: &str = "zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used";
const AU98: &str = "zygo: --server holds user info; this agent is not saved by the workspace";
const NOT_SAVED: &str = "zygo: this agent is not saved by the workspace: ";
const NO_CHECK: (&str, &str) = ("ZYGO_NO_UPDATE_CHECK", "1");
/// The exit line of the member wrapper for a launch that did not start the agent (W§8.2 step 7).
const NOT_STARTED: &str = "launch failed before the agent started";
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

/// FIX-24 P12, AU27 known limit 8; diff review round 2, finding 5: a member saved from `run --attach <url>` keeps only
/// `["--attach"]`; its `ws run` start (the real `record_start` path) runs the TUI, computes `events_off = false` over a
/// stale `true`, and `down` counts the quiet member as idle.
#[test]
fn a_saved_attach_flag_without_run_is_not_events_off() {
    let Some(h) = Harness::new() else { return };
    git_repo(&h);
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    let mut rec = record(Agent::OpenCode, MemberState::Ended, None, None);
    rec.launch.args = vec!["--attach".into()];
    rec.launch.events_off = true; // a stale value: the start write must compute it again
    up_state(&h, &[(KEY, rec)], &mut |_| {});
    let (_out, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into_os_string()));
    let run = h.script("run", &["ws", "run", "--ws-id", WS_ID, "--pane", KEY], &pairs(&env));
    let pane = split(&h, "=zygo-app:", &run);
    h.tmux(&["set-option", "-p", "-t", &pane, "@zygo-pane", KEY]);
    std::fs::write(h.root().join("run.go"), "").unwrap();
    assert!(started(&c), "the ws run start did not reach the agent");
    let m = member(&h, KEY).unwrap();
    assert_eq!((m.launch.args.clone(), m.launch.events_off), (vec!["--attach".to_string()], false));
    // No activity.json and a quiet proxy: idle unless the record says events off.
    let o = cli(&h, &["down", "--timeout", "5s", "--wait", "--", "app"], &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(err.contains(&format!("zygo: app pane {KEY} is idle\n")), "{err}");
    assert!(!err.contains("member events off"), "{err}");
}

#[test] // FIX-24 diff review round 2, finding 4, AU26: CLAUDE_CODE_SIMPLE in the environment of the start is bare mode
fn a_hand_start_with_claude_code_simple_is_saved_with_events_off() {
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let cases: [(Agent, &str, &str, bool); 5] = [
        (Agent::ClaudeCode, "claude", "1", true),
        (Agent::ClaudeCode, "claude", "true", true),
        (Agent::ClaudeCode, "claude", "0", false),
        (Agent::ClaudeCode, "claude", "", false),
        (Agent::OpenCode, "opencode", "1", false), // Claude Code only
    ];
    for (i, (kind, agent, value, want)) in cases.into_iter().enumerate() {
        let pane = split(&h, "=zygo-app:", "cat");
        let (out, env) = h.stub(kind, &format!("s{i}"));
        let mut extra = pairs(&env);
        extra.push(("CLAUDE_CODE_SIMPLE", OsStr::new(value)));
        let o = Guarded::spawn(hand(&h, &pane, &[agent], &extra)).output("hand start");
        assert_eq!(o.status.code(), Some(0), "{agent} {value:?}: {}", text(&o.stderr));
        // The agent really gets the value: the launcher neither removes nor sets it.
        assert_eq!(stub_env(&out)["CLAUDE_CODE_SIMPLE"], json!(value), "{agent} {value:?}");
        let m = member(&h, &pane_key(&h, &pane)).unwrap();
        assert_eq!(m.launch.events_off, want, "{agent} {value:?}");
        assert!(m.launch.args.is_empty(), "{agent} {value:?}: {:?}", m.launch.args);
        // Room for the next split: the window holds a few panes only.
        h.tmux(&["kill-pane", "-t", &pane]);
    }
}

#[test] // FIX-24 D1, AU26: the real hand-start path writes events_off from the typed arguments
fn a_hand_start_whose_agent_sends_no_member_events_is_saved_with_events_off() {
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let cases: [(Agent, &[&str], &[&str]); 3] = [
        (Agent::OpenCode, &["opencode", "--", "attach", "http://127.0.0.1:1"], &[]),
        (Agent::OpenCode, &["opencode", "--", "run", "--attach", "http://127.0.0.1:1"], &["--attach"]),
        (Agent::ClaudeCode, &["claude", "--", "--bare"], &["--bare"]),
    ];
    for (i, (kind, args, want_args)) in cases.into_iter().enumerate() {
        let pane = split(&h, "=zygo-app:", "cat");
        let (_out, env) = h.stub(kind, &format!("e{i}"));
        let o = Guarded::spawn(hand(&h, &pane, args, &pairs(&env))).output("hand start");
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", text(&o.stderr));
        let key = pane_key(&h, &pane);
        let m = member(&h, &key).unwrap();
        let want: Vec<String> = want_args.iter().map(|a| a.to_string()).collect();
        assert_eq!((m.launch.kind, m.launch.events_off, m.launch.args.clone()), (kind, true, want), "{args:?}");
        assert_eq!(raw_member(&h, &key)["launch"]["events_off"], json!(true), "{args:?}");
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
        format!("zygo: pane {KEY} holds resumable session abc; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'; or run zygo ws forget-pane {KEY}\n")
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
fn a_launch_that_ends_before_the_callback_leaves_the_record_byte_equal() { // decision 3; review round 2, findings 4, 9
    let Some(h) = Harness::new() else { return };
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    let mut r = record(Agent::ClaudeCode, MemberState::Ended, Some("old"), Some(OLD_ZS));
    r.overrides = Some(json!({"results": false}));
    up_state(&h, &[(KEY, r)], &mut |_| {});
    let bin2 = h.root().join("bin2");
    std::fs::create_dir(&bin2).unwrap();
    std::os::unix::fs::symlink(h.bin.join("tmux"), bin2.join("tmux")).unwrap();
    let before = raw_member(&h, KEY);
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    // The gate marks `launch` and `returned` of the member wrapper show that the M3 member path ran.
    let g = h.root().join("gate");
    let launch = Hold::new(&g, "launch");
    let returned = Hold::new(&g, "returned");
    env.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    let mut extra = pairs(&env);
    extra.push(("PATH", bin2.as_os_str()));
    let child = Guarded::spawn(hand(&h, &first, &["claude"], &extra));
    assert!(until(Duration::from_secs(20), || g.join("launch").exists()));
    drop(launch);
    assert!(until(Duration::from_secs(20), || g.join("returned").exists()));
    drop(returned);
    let o = child.output("hand start");
    assert_ne!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(raw_member(&h, KEY), before);
    assert!(!out.join("argv.json").exists());
    // No callback ran, so the wrapper prints no exit line (finding 4).
    assert!(!text(&o.stderr).contains(NOT_STARTED), "{}", text(&o.stderr));
}

/// A guard, not evidence for a rule of the member wrapper: a v1 launch never enters `run_wrapped` (`launch_hand`,
/// `HandStart::V1`), so no change of the wrapper can make it fail. It keeps a later change from giving v1 launches a
/// wrapper line.
#[test]
fn guard_a_v1_launch_that_fails_prints_no_wrapper_line() {
    let h = Harness::no_tmux();
    let bin2 = h.root().join("bin2");
    std::fs::create_dir(&bin2).unwrap();
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    extra.extend([("PATH", bin2.as_os_str()), no_check()]);
    let o = Guarded::spawn(h.zygo(&["claude"], &extra)).output("zygo claude");
    assert_ne!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!out.join("argv.json").exists());
    assert!(!text(&o.stderr).contains(NOT_STARTED), "{}", text(&o.stderr));
}

#[test]
fn a_spawn_failure_after_the_callback_puts_the_previous_record_back() { // review round 2, findings 4, 5
    let Some(h) = Harness::new() else { return };
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    let mut r = record(Agent::ClaudeCode, MemberState::Ended, Some("old"), Some(OLD_ZS));
    r.overrides = Some(json!({"results": false}));
    up_state(&h, &[(KEY, r)], &mut |_| {});
    let pane = split(&h, "=zygo-app:", "cat");
    let before = raw_member(&h, KEY);
    // `--agents []` is refused by the adapter after the callback (exit 2): the agent never starts.
    for target in [first.as_str(), pane.as_str()] {
        let (out, env) = h.stub(Agent::ClaudeCode, &format!("a{}", target.trim_start_matches('%')));
        let o = Guarded::spawn(hand(&h, target, &["claude", "--", "--agents", "[]"], &pairs(&env))).output("hand start");
        assert_eq!(o.status.code(), Some(2), "{}", text(&o.stderr));
        assert!(!out.join("argv.json").exists());
        // The callback wrote the record, so the wrapper prints its line (finding 4).
        let line = "zygo: the claude launch failed before the agent started (exit 2)\n";
        assert_eq!(text(&o.stderr).matches(line).count(), 1, "{}", text(&o.stderr));
    }
    assert_eq!(raw_member(&h, KEY), before);
    assert_eq!(member(&h, &pane_key(&h, &pane)), None);
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
    // The socket folder is not private (AU62; review round 2, finding 7).
    if let Some(h) = Harness::new() {
        use std::os::unix::fs::PermissionsExt;
        up_state(&h, &[], &mut |_| {});
        workspace_session(&h, Some(KEY2), Some(WS_ID));
        let pane = split(&h, "=zygo-app:", "cat");
        let folder = h.socket().parent().unwrap().to_path_buf();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        let o = Guarded::spawn(hand(&h, &pane, &["claude"], &pairs(&env))).output("hand start");
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        let want = format!(
            "{NOT_SAVED}tmux socket {} is not private: the socket and its folder must be yours and the folder must have mode 0700\n",
            h.socket().display()
        );
        assert_eq!(text(&o.stderr).matches(&want).count(), 1, "{want}{}", text(&o.stderr));
        assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
        assert_eq!(pane_key(&h, &pane), "");
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
    // Review round 2, finding 3: no session argument that the member pre-assign added.
    assert!(!argv(&out).iter().any(|t| t.starts_with("--session-id")), "{:?}", argv(&out));
    assert!(!out.join("session-arg.json").exists());
    // Review round 2, finding 4: a NotMember launch prints no wrapper line.
    assert!(!text(&o.stderr).contains(NOT_STARTED), "{}", text(&o.stderr));
}

#[test]
fn a_not_member_launch_keeps_the_typed_session_argument_and_prints_no_wrapper_line() { // review round 2, findings 3, 4
    let Some(h) = Harness::new() else { return };
    let pane = split_in_workspace(&h, &mut |_| {});
    let g = h.root().join("gate");
    let hold = Hold::new(&g, "launch");
    let (_, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    let child = Guarded::spawn(hand(&h, &pane, &["claude", "--", "--session-id", "Y1", "--agents", "[]"], &pairs(&env)));
    assert!(until(Duration::from_secs(20), || g.join("launch").exists()));
    h.store(WS_ID)
        .update_existing(Duration::from_secs(5), &mut |w| {
            w.phase = Phase::Down;
            Ok(())
        })
        .unwrap();
    drop(hold);
    let o = child.output("hand start");
    // The adapter refuses `--agents []` after the callback (exit 2): a NotMember launch that never starts.
    assert_eq!(o.status.code(), Some(2), "{}", text(&o.stderr));
    let err = text(&o.stderr);
    assert_eq!(err.matches(&format!("{NOT_SAVED}a member starts only in phase up or draining, not down\n")).count(), 1, "{err}");
    assert!(!err.contains(NOT_STARTED), "{err}");
    let pane2 = split(&h, "=zygo-app:", "cat");
    h.store(WS_ID)
        .update_existing(Duration::from_secs(5), &mut |w| {
            w.phase = Phase::Up;
            Ok(())
        })
        .unwrap();
    // The user's own session argument stays in a NotMember launch.
    let hold = Hold::new(&g, "launch");
    let (out, mut env) = h.stub(Agent::ClaudeCode, "b");
    env.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    let _ = std::fs::remove_file(g.join("launch"));
    let child = Guarded::spawn(hand(&h, &pane2, &["claude", "--", "--session-id", "Y1"], &pairs(&env)));
    assert!(until(Duration::from_secs(20), || g.join("launch").exists()));
    h.store(WS_ID)
        .update_existing(Duration::from_secs(5), &mut |w| {
            w.phase = Phase::Down;
            Ok(())
        })
        .unwrap();
    drop(hold);
    let o = child.output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(json_file(&out.join("session-arg.json")), json!({"flag": "--session-id", "id": "Y1"}));
    assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
}

// ---- Callback refusals through the binary (review round 2, finding 8) ----

/// A hand start of Claude Code with `args` in a new split of the workspace, held at the wrapper gate `launch` (the
/// pane key is set, the callback has not run); `at_gate` changes the state there. Returns the output, the stub output
/// folder and the pane key.
fn refused_in_callback(
    h: &Harness, args: &[&str], at_gate: &mut dyn FnMut(&Harness, &str),
) -> (Output, PathBuf, String) {
    let pane = split_in_workspace(h, &mut |_| {});
    let g = h.root().join("gate");
    let hold = Hold::new(&g, "launch");
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    let child = Guarded::spawn(hand(h, &pane, args, &pairs(&env)));
    assert!(until(Duration::from_secs(20), || g.join("launch").exists()));
    let key = pane_key(h, &pane);
    at_gate(h, &key);
    drop(hold);
    (child.output("hand start"), out, key)
}

/// The launch went on as v1 with the not-saved line `reason` once.
fn v1_with(o: &Output, out: &Path, reason: &str) {
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let want = format!("{NOT_SAVED}{reason}\n");
    assert_eq!(text(&o.stderr).matches(&want).count(), 1, "{want}{}", text(&o.stderr));
    let e = stub_env(out);
    assert!(e.get("ZYGO_WORKSPACE").is_none() && e.get("ZYGO_PANE").is_none(), "{e}");
    assert!(!settings(out).contains("workspace --event"));
}

#[test]
fn the_callback_refuses_a_phase_that_changed_to_down() {
    let Some(h) = Harness::new() else { return };
    let (o, out, _) = refused_in_callback(&h, &["claude"], &mut |h, _| {
        h.store(WS_ID)
            .update_existing(Duration::from_secs(5), &mut |w| {
                w.phase = Phase::Down;
                Ok(())
            })
            .unwrap();
    });
    v1_with(&o, &out, "a member starts only in phase up or draining, not down");
}

#[test]
fn the_callback_refuses_a_launcher_that_became_live() {
    let Some(h) = Harness::new() else { return };
    let mut lock = None;
    let (o, out, key) = refused_in_callback(&h, &["claude"], &mut |h, key| {
        let d = h.paths.sessions_dir.join(LIVE_ZS);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("token"), "t").unwrap();
        std::fs::write(d.join("pid"), "1").unwrap();
        let f = std::fs::File::options().read(true).write(true).open(d.join("pid")).unwrap();
        assert!(fs4::FileExt::try_lock(&f).is_ok());
        lock = Some(f);
        h.store(WS_ID)
            .update_existing(Duration::from_secs(5), &mut |w| {
                w.members.insert(key.to_string(), record(Agent::ClaudeCode, MemberState::Ended, None, Some(LIVE_ZS)));
                Ok(())
            })
            .unwrap();
    });
    v1_with(&o, &out, &format!("pane {key} holds session {LIVE_ZS}, whose launcher is still running"));
    drop(lock);
}

#[test]
fn the_callback_refuses_an_agent_session_of_another_key() {
    let Some(h) = Harness::new() else { return };
    let (o, out, _) = refused_in_callback(&h, &["claude", "--", "--resume", "abc"], &mut |h, _| {
        h.store(WS_ID)
            .update_existing(Duration::from_secs(5), &mut |w| {
                w.members.insert(KEY.into(), record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS)));
                Ok(())
            })
            .unwrap();
    });
    v1_with(&o, &out, &format!("agent session abc belongs to pane {KEY}"));
}

#[test]
fn the_callback_after_ws_forget_writes_no_state() {
    let Some(h) = Harness::new() else { return };
    let (o, out, _) = refused_in_callback(&h, &["claude"], &mut |h, _| {
        std::fs::remove_dir_all(h.store(WS_ID).dir()).unwrap();
    });
    v1_with(&o, &out, &format!("no workspace state for {WS_ID}"));
    assert!(!h.store(WS_ID).dir().exists());
}

// ---- W-C9 with own session arguments, AU94 by kind, the saved launch at a resume (review round 2) ----

#[test]
fn w_c9_two_hand_starts_with_one_own_session_start_one_agent() { // finding 1
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let a = split(&h, "=zygo-app:", "cat");
    let b = split(&h, "=zygo-app:", "cat");
    let g = h.root().join("gate");
    let hold = Hold::new(&g, "callback");
    let (out_a, mut env_a) = h.stub(Agent::ClaudeCode, "a");
    let ca = ctl(&h, "a");
    env_a.push(("ZYGO_TEST_MEMBER_GATE", g.clone().into_os_string()));
    env_a.push(("ZYGO_STUB_CONTROL", ca.clone().into_os_string()));
    let first = Guarded::spawn(hand(&h, &a, &["claude", "--", "--resume", "X1"], &pairs(&env_a)));
    assert!(until(Duration::from_secs(20), || g.join("callback").exists()));
    // The start write holds the own session as `pending.agent_session`; AU30 keeps it out of `launch.args`.
    let key_a = pane_key(&h, &a);
    let m = member(&h, &key_a).unwrap();
    assert_eq!((m.agent_session.clone(), m.pending.as_ref().and_then(|p| p.agent_session.clone())), (None, Some("X1".to_string())));
    assert!(m.launch.args.is_empty(), "{:?}", m.launch.args);
    let (out_b, env_b) = h.stub(Agent::ClaudeCode, "b");
    let o = Guarded::spawn(hand(&h, &b, &["claude", "--", "--resume", "X1"], &pairs(&env_b))).output("second hand start");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert_eq!(
        text(&o.stderr),
        format!("zygo: agent session X1 belongs to pane {key_a}; resume it in that pane, or run zygo ws forget-pane {key_a}\n")
    );
    assert!(!out_b.join("argv.json").exists());
    drop(hold);
    // Round 3, finding 6: the own session commits through the `SessionStart` hook, which the stub calls at start.
    assert!(started(&ca));
    common::stub_control(&ca, "exit 0").unwrap();
    let o = first.output("first hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(out_a.join("argv.json").exists());
    let m = member(&h, &key_a).unwrap();
    assert_eq!((m.agent_session.as_deref(), m.launch.args.clone()), (Some("X1"), Vec::<String>::new()));
}

#[test]
fn au94_a_resume_of_another_agent_kind_refuses() { // finding 2
    let Some(h) = Harness::new() else { return };
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    up_state(&h, &[(KEY, record(Agent::OpenCode, MemberState::Resumable, Some("ses_1"), Some(OLD_ZS)))], &mut |_| {});
    let before = std::fs::read(h.store(WS_ID).dir().join("workspace.json")).unwrap();
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &first, &["claude", "--", "--resume", "ses_1"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert_eq!(
        text(&o.stderr),
        format!("zygo: pane {KEY} holds resumable session ses_1; to resume it: zygo opencode -- --session='ses_1'; or run zygo ws forget-pane {KEY}\n")
    );
    assert_eq!(json_file(&h.store(WS_ID).dir().join("workspace.json"))["members"][KEY], serde_json::from_slice::<Value>(&before).unwrap()["members"][KEY]);
    assert!(!out.join("argv.json").exists());
}

#[test]
fn a_resume_command_names_the_saved_launch_and_a_hand_resume_keeps_it() { // finding 6
    let Some(h) = Harness::new() else { return };
    std::fs::write(h.root().join("config.toml"), "[profile.p1]\nprompt = \"P\"\n").unwrap();
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    let mut r = record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS));
    r.launch.profile = Some("p1".into());
    r.launch.compact = Some("600k".into());
    r.launch.args = vec!["--model".into(), "m".into()];
    up_state(&h, &[(KEY, r.clone())], &mut |_| {});
    let (_, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &first, &["claude"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    let command = "env -u CLAUDE_CONFIG_DIR zygo claude --profile 'p1' --compact '600k' -- --resume='abc' '--model' 'm'";
    assert_eq!(
        text(&o.stderr),
        format!("zygo: pane {KEY} holds resumable session abc; to resume it: {command}; or run zygo ws forget-pane {KEY}\n")
    );
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_zygo"), h.bin.join("zygo")).unwrap();
    let env_tool = ["/usr/bin/env", "/bin/env"].into_iter().map(PathBuf::from).find(|p| p.is_file()).unwrap();
    std::os::unix::fs::symlink(env_tool, h.bin.join("env")).unwrap();
    let (out, env) = h.stub(Agent::ClaudeCode, "b");
    let c = hand(&h, &first, &[], &pairs(&env));
    let mut sh = Command::new("/bin/sh");
    sh.args(["-c", command]).env_clear().envs(c.get_envs().filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string()))));
    sh.current_dir(h.root()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let o = Guarded::spawn(sh).output("resume command");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(argv(&out).iter().any(|t| t == "--resume=abc"), "{:?}", argv(&out));
    let m = member(&h, KEY).unwrap();
    assert_eq!(m.launch, r.launch);
    assert_eq!(m.zygo_session.as_deref(), stub_env(&out)["ZYGO_SESSION"].as_str());
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
        // FIX-24 diff review round 2, finding 6: the stub writes argv.json before settings.json (stub.rs:282, :308), so
        // the test waits for a settings.json that is not empty; argv.json and env.json are complete by then.
        assert!(
            until(Duration::from_secs(20), || std::fs::metadata(out_b.join("settings.json")).is_ok_and(|m| m.len() > 0)),
            "{case}"
        );
        let b = argv(&out_b);
        assert!(b.contains(&format!("--resume={agent_id}")), "{b:?}");
        assert_eq!(settings_args(&b), 1, "{b:?}");
        assert!(settings(&out_b).contains("workspace --event prompt"), "{case}");
        assert_eq!(case == "readable", settings(&out_b).contains("\"W\""), "{case}"); // AU59: read again at resume
        cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
    }
}

/// W§18 #19, sentences 1-2: the loadout part. It runs only `zygo up` and `ws run` (M2, W7 code), no hand start; the
/// hand-launch part of #19 (sentence 3, AU98) is `w18_19_config_check_and_a_hand_launch_report_user_info`.
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

/// W§18 #19, sentence 3: `zygo config check` reports the user-info server, and a hand launch with it in a workspace
/// pane writes no member record (AU98).
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

// ---- Diff review round 3 ----

/// A team server on 127.0.0.1:0 with one token, as `workspace_run.rs` `team_server` (`tests/serve_cli.rs:57-87`)
/// builds it; returns its folder, handle and token.
fn team_server() -> (tempfile::TempDir, zygo_proxy::serve::ServeHandle, String) {
    use std::sync::Arc;
    let d = tempfile::tempdir().unwrap();
    let root = d.path().to_path_buf();
    let (token, sha) = zygo_proxy::team::new_token();
    let config_file = root.join("config.toml");
    std::fs::write(&config_file, format!("[serve]\ntokens = [{{ label = \"m3\", sha256 = \"{sha}\" }}]\n")).unwrap();
    let paths = zygo_core::paths::Paths {
        config_file: config_file.clone(),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"),
        marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"),
        approvals_file: root.join("data/approvals.toml"),
        usage_db: root.join("data/usage.db"),
    };
    let ctx = zygo_config::LoadContext { paths, repo: None, team: None };
    let mut builtin = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    builtin.remove("marketplace");
    let h = zygo_proxy::serve::start(zygo_proxy::serve::ServeOpts {
        listen: "127.0.0.1:0".into(),
        tls: None,
        config_file,
        load: Arc::new(move || Ok(zygo_config::load_with_builtin(&ctx, builtin.clone())?)),
        report: Arc::new(|_: &str| {}),
        pid_file: None,
        signals: false,
        start: zygo_proxy::server::StartOpts { usage_db: Some(root.join("data/usage.db")), ..Default::default() },
    })
    .unwrap();
    (d, h, token)
}

/// What the stub received: argv, environment, cwd (see [`launch_seen`]).
type Seen = (Vec<String>, BTreeMap<String, String>, String);

/// A command made from the environment pairs of one run.
type MakeCommand<'a> = dyn Fn(&[(&str, &OsStr)]) -> Command + 'a;

/// What the stub received: its argv, its environment and its cwd, with the values of this one launch (its session
/// folder, its zygo session, its proxy URL and its session token) written as their names, so two launches of one
/// record compare.
fn launch_seen(out: &Path) -> Seen {
    let env: BTreeMap<String, String> = serde_json::from_value(stub_env(out)).unwrap();
    let mut own: Vec<(String, &str)> =
        ["ZYGO_SESSION_DIR", "ZYGO_SESSION", "ZYGO_URL"].into_iter().filter_map(|k| env.get(k).map(|v| (v.clone(), k))).collect();
    let token = json_file(&out.join("session.json"))["token"].as_str().unwrap().trim().to_string();
    assert!(!token.is_empty());
    own.push((token, "token"));
    let norm = |t: &str| own.iter().fold(t.to_string(), |acc, (v, k)| acc.replace(v.as_str(), &format!("<{k}>")));
    let cwd = std::fs::read_to_string(out.join("cwd.txt")).unwrap();
    (argv(out).iter().map(|a| norm(a)).collect(), env.iter().map(|(k, v)| (k.clone(), norm(v))).collect(), cwd)
}

/// `line` run by `/bin/sh -c` in `h.repo()` with the environment of a hand start in `pane` and `extra`.
fn sh_in_pane(h: &Harness, pane: &str, line: &str, extra: &[(&str, &OsStr)]) -> Command {
    for (name, target) in [("zygo", PathBuf::from(env!("CARGO_BIN_EXE_zygo"))), ("env", which::which("env").unwrap())] {
        let link = h.bin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink(target, link).unwrap();
        }
    }
    let c = hand(h, pane, &[], extra);
    let mut sh = Command::new("/bin/sh");
    sh.args(["-c", line]).env_clear().envs(c.get_envs().filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string()))));
    sh.current_dir(h.repo()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    sh
}

/// Runs `cmd` with a control-mode stub `name` and `extra`; while the agent runs, reads the session's compaction; then
/// ends the agent with exit 0. Gives what the stub received, the compaction and the output.
fn controlled(
    h: &Harness, name: &str, extra: &[(&str, &OsStr)], cmd: &MakeCommand<'_>,
) -> (Seen, Option<zygo_proxy::client::WindowArg>, Output) {
    let (out, mut env) = h.stub(Agent::ClaudeCode, name);
    let c = ctl(h, name);
    env.push(("ZYGO_STUB_CONTROL", c.clone().into_os_string()));
    let mut all = pairs(&env);
    all.extend_from_slice(extra);
    let child = Guarded::spawn(cmd(&all));
    if !started(&c) {
        let o = child.output(name);
        panic!("{name}: the agent did not start: {}", text(&o.stderr));
    }
    let dir = PathBuf::from(stub_env(&out)["ZYGO_SESSION_DIR"].as_str().unwrap());
    let compact = zygo_proxy::client::SessionClient::from_session_dir(&dir).unwrap().status().unwrap().compact;
    common::stub_control(&c, "exit 0").unwrap();
    let o = child.output(name);
    (launch_seen(&out), compact, o)
}

#[test]
fn the_printed_resume_command_starts_the_agent_as_ws_run_does() { // diff review round 3, findings 1-4; AU61
    // The serve mode refuses a passthrough session (`zygo-proxy` `routes.rs:129`, "passthrough is local mode only"), so
    // passthrough and a server are two records: passthrough in local mode with the `cleared` override, and a server
    // with a port with the `set` override.
    for (case, passthrough, serve, compact, flag) in [
        ("passthrough, cleared", true, false, CompactOverride::Cleared, ""),
        ("server, set", false, true, CompactOverride::Set("800k".into()), " --compact '800k'"),
    ] {
        let Some(h) = Harness::new() else { return };
        let (_d, srv, token) = team_server();
        let config = if serve { TEAM_SERVER } else { "" };
        std::fs::write(h.root().join("config.toml"), format!("{config}[profile.p1]\nprompt = \"P\"\n")).unwrap();
        let first = workspace_session(&h, Some(KEY), Some(WS_ID));
        let mut r = record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS));
        r.launch.profile = Some("p1".into());
        r.launch.passthrough = passthrough;
        r.launch.server = serve.then(|| srv.url().to_string());
        r.launch.yolo = true;
        r.launch.compact = Some("600k".into());
        r.launch.args = vec!["--model".into(), "m".into()];
        r.compact = Some(compact);
        up_state(&h, &[(KEY, r.clone())], &mut |_| {});
        let token = OsString::from(token);
        let extra = [("ZYGO_TEAM_TOKEN", token.as_os_str())];
        // The command that zygo prints for the record (AU94 refusal of a hand start with no session argument).
        let (_, env) = h.stub(Agent::ClaudeCode, "refused");
        let mut all = pairs(&env);
        all.extend_from_slice(&extra);
        let o = Guarded::spawn(hand(&h, &first, &["claude"], &all)).output("hand start");
        assert_eq!(o.status.code(), Some(1), "{case}: {}", text(&o.stderr));
        let mid = match (passthrough, serve) {
            (true, _) => " --passthrough".to_string(),
            (_, true) => format!(" --server '{}'", srv.url()),
            _ => String::new(),
        };
        let command = format!("env -u CLAUDE_CONFIG_DIR zygo claude --profile 'p1'{mid}{flag} --yolo -- --resume='abc' '--model' 'm'");
        assert_eq!(
            text(&o.stderr),
            format!("zygo: pane {KEY} holds resumable session abc; to resume it: {command}; or run zygo ws forget-pane {KEY}\n"),
            "{case}"
        );
        // `ws run` of the record, then the printed command on the same record. Both run as `/bin/sh -c 'env …'` with one
        // stub name, so the shell's `_` and the stub's own variables are the same in the two runs.
        let ws_run = format!("env zygo ws run --ws-id {WS_ID} --pane {KEY}");
        let (by_ws_run, compact_ws_run, o) = controlled(&h, "s", &extra, &|all| sh_in_pane(&h, &first, &ws_run, all));
        assert_eq!(o.status.code(), Some(0), "{case}: {}", text(&o.stderr));
        up_state(&h, &[(KEY, r.clone())], &mut |_| {});
        for d in [h.root().join("out/s"), ctl(&h, "s")] {
            std::fs::remove_dir_all(d).unwrap();
        }
        let (by_command, compact_command, o) = controlled(&h, "s", &extra, &|all| sh_in_pane(&h, &first, &command, all));
        assert_eq!(o.status.code(), Some(0), "{case}: {}", text(&o.stderr));
        assert_eq!(by_command.0, by_ws_run.0, "{case}: argv");
        assert_eq!(by_command.1, by_ws_run.1, "{case}: environment");
        assert_eq!(by_command.2, by_ws_run.2, "{case}: cwd");
        assert_eq!(compact_command, compact_ws_run, "{case}: compaction");
        assert!(by_ws_run.0.iter().any(|a| a == "--resume=abc"), "{case}: {:?}", by_ws_run.0);
        srv.shutdown();
    }
}

#[test]
fn w_c9_a_pending_id_of_a_launcher_that_is_not_live_does_not_hold_it() { // diff review round 3, finding 5
    let Some(h) = Harness::new() else { return };
    let mut held = record(Agent::ClaudeCode, MemberState::Running, None, None);
    held.pending = Some(Pending { zygo_session: DEAD_ZS.into(), agent_session: Some("X1".into()), started_at: 1, ..Default::default() });
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[(KEY, held)], &mut |_| {});
    let pane = split(&h, "=zygo-app:", "cat");
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = Guarded::spawn(hand(&h, &pane, &["claude", "--", "--resume", "X1"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(json_file(&out.join("session-arg.json")), json!({"flag": "--resume", "id": "X1"}));
    let key = pane_key(&h, &pane);
    assert_eq!(stub_env(&out)["ZYGO_PANE"].as_str(), Some(key.as_str()));
    assert!(member(&h, &key).is_some());
}

#[test]
fn a_typed_id_that_does_not_exist_is_not_saved() { // diff review round 3, finding 6
    let Some(h) = Harness::new() else { return };
    let pane = split_in_workspace(&h, &mut |_| {});
    // The agent exits 1 before any hook (as Claude Code does for an id it does not know).
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_STUB_EXIT", "1".into()));
    let o = Guarded::spawn(hand(&h, &pane, &["claude", "--", "--resume", "typo"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert!(out.join("argv.json").exists());
    let key = pane_key(&h, &pane);
    let m = member(&h, &key).unwrap();
    assert_eq!((m.agent_session.as_deref(), m.pending.is_none()), (None, true));
    assert_eq!(m.zygo_session.as_deref(), stub_env(&out)["ZYGO_SESSION"].as_str());
    assert!(!raw_member(&h, &key).to_string().contains("typo"), "{}", raw_member(&h, &key));
    assert!(!text(&o.stderr).contains("is kept by the agent"), "{}", text(&o.stderr));
    // A signal ends the launch and no hook ran (the stub's hook shell fails): no resumable record with the typed id.
    let pane2 = split(&h, "=zygo-app:", "cat");
    let (_, mut env) = h.stub(Agent::ClaudeCode, "b");
    let c = ctl(&h, "b");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into_os_string()));
    env.push(("ZYGO_STUB_SHELL", "/bin/false".into()));
    let child = Guarded::spawn(hand(&h, &pane2, &["claude", "--", "--resume", "typo2"], &pairs(&env)));
    assert!(started(&c));
    // SAFETY: `kill` sends SIGTERM to this test's own child, the zygo launcher.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) }, 0);
    let o = child.output("signal");
    let key2 = pane_key(&h, &pane2);
    let m = member(&h, &key2).unwrap();
    assert_eq!((m.state, m.agent_session.as_deref()), (MemberState::Resumable, None), "{}", text(&o.stderr));
    assert!(!raw_member(&h, &key2).to_string().contains("typo2"), "{}", raw_member(&h, &key2));
}

#[test]
fn a_hand_start_exit_line_names_the_saved_launch() { // diff review round 3, finding 7
    let Some(h) = Harness::new() else { return };
    std::fs::write(h.root().join("config.toml"), "[profile.p1]\nprompt = \"P\"\n[profile.p2]\nprompt = \"Q\"\n").unwrap();
    let first = workspace_session(&h, Some(KEY), Some(WS_ID));
    let mut r = record(Agent::ClaudeCode, MemberState::Resumable, Some("abc"), Some(OLD_ZS));
    r.launch.profile = Some("p1".into());
    up_state(&h, &[(KEY, r)], &mut |_| {});
    let (_, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_STUB_EXIT", "3".into()));
    let o = Guarded::spawn(hand(&h, &first, &["claude", "--profile", "p2", "--", "--resume", "abc"], &pairs(&env))).output("hand start");
    assert_eq!(o.status.code(), Some(3), "{}", text(&o.stderr));
    let line = "zygo: claude exited 3; agent session abc is kept by the agent; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude --profile 'p1' -- --resume='abc'\n";
    assert_eq!(text(&o.stderr).matches(line).count(), 1, "{}", text(&o.stderr));
}

#[test]
fn a_not_member_launch_keeps_every_typed_token_in_order() { // diff review round 3, finding 9
    let Some(h) = Harness::new() else { return };
    // Claude Code takes its session tokens at index 0 (`launcher.rs` `insert_session_args`), so every typed token is
    // after the pre-assign.
    let (o, out, _) = refused_in_callback(&h, &["claude", "--", "--model", "m", "-p", "hi"], &mut |h, _| {
        h.store(WS_ID)
            .update_existing(Duration::from_secs(5), &mut |w| {
                w.phase = Phase::Down;
                Ok(())
            })
            .unwrap();
    });
    v1_with(&o, &out, "a member starts only in phase up or draining, not down");
    let a = argv(&out);
    assert!(a.windows(4).any(|w| w == ["--model", "m", "-p", "hi"]), "{a:?}");
    assert!(!a.iter().any(|t| t.starts_with("--session-id")), "{a:?}");
}

#[test]
fn w_c9_the_callback_refuses_a_second_start_of_one_own_session() { // diff review round 3, finding 11
    let Some(h) = Harness::new() else { return };
    workspace_session(&h, Some(KEY2), Some(WS_ID));
    up_state(&h, &[], &mut |_| {});
    let a = split(&h, "=zygo-app:", "cat");
    let b = split(&h, "=zygo-app:", "cat");
    let (ga, gb) = (h.root().join("gate-a"), h.root().join("gate-b"));
    let launch_a = Hold::new(&ga, "launch");
    let callback_a = Hold::new(&ga, "callback");
    let launch_b = Hold::new(&gb, "launch");
    let (out_a, mut env_a) = h.stub(Agent::ClaudeCode, "a");
    env_a.push(("ZYGO_TEST_MEMBER_GATE", ga.clone().into_os_string()));
    let (out_b, mut env_b) = h.stub(Agent::ClaudeCode, "b");
    env_b.push(("ZYGO_TEST_MEMBER_GATE", gb.clone().into_os_string()));
    let first = Guarded::spawn(hand(&h, &a, &["claude", "--", "--resume", "X1"], &pairs(&env_a)));
    let second = Guarded::spawn(hand(&h, &b, &["claude", "--", "--resume", "X1"], &pairs(&env_b)));
    // Both starts passed the checks before the launch (W§8.4).
    assert!(until(Duration::from_secs(20), || ga.join("launch").exists() && gb.join("launch").exists()));
    drop(launch_a);
    assert!(until(Duration::from_secs(20), || ga.join("callback").exists()));
    let key_a = pane_key(&h, &a);
    drop(launch_b);
    let o = second.output("second hand start");
    v1_with(&o, &out_b, &format!("agent session X1 belongs to pane {key_a}"));
    assert_eq!(member(&h, &pane_key(&h, &b)), None);
    drop(callback_a);
    let o = first.output("first hand start");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(stub_env(&out_a)["ZYGO_PANE"].as_str(), Some(key_a.as_str()));
}
