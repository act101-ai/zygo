//! L3: one test per stub behavior of the W§18 preamble. No process-environment change.
#[path = "stub_agent/harness.rs"]
mod harness;
#[path = "../../zygo/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use serde_json::{json, Value};
use tempfile::TempDir;

const ID: &str = "11111111-1111-4111-8111-111111111111";
const BOUND: Duration = Duration::from_secs(10);

struct Stub { child: Child, out: PathBuf, ctl: PathBuf, t: TempDir }
impl Drop for Stub { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

/// Starts `stub-agent` directly in `t`: capture `t/out`, control `t/ctl`, cwd `t/work`, session dir `t/sd` with `token` = `tok-1`.
fn spawn_in(t: TempDir, args: &[&str], control: bool, url: &str) -> Stub {
    let d = |n: &str| { let p = t.path().join(n); std::fs::create_dir_all(&p).unwrap(); p };
    let (out, ctl, work, sd) = (d("out"), d("ctl"), d("work"), d("sd"));
    std::fs::write(sd.join("token"), "tok-1\n").unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_stub-agent"));
    c.args(args).current_dir(&work).stdin(Stdio::null())
        .env("ZYGO_STUB_OUT", &out).env("ZYGO_STUB_SHELL", harness::shell())
        .env("ZYGO_SESSION", "s-1").env("ZYGO_SESSION_DIR", &sd).env("ZYGO_URL", url)
        .env_remove("ZYGO_STUB_EXIT").env_remove("ZYGO_STUB_CONTROL").env_remove("OPENCODE_CONFIG_DIR").env_remove("ANTHROPIC_MODEL");
    if control { c.env("ZYGO_STUB_CONTROL", &ctl); }
    #[cfg(unix)] // a test run with SIGINT ignored (a background job) must not give the stub an ignored signal
    unsafe {
        use std::os::unix::process::CommandExt;
        c.pre_exec(|| { libc::signal(libc::SIGINT, libc::SIG_DFL); libc::signal(libc::SIGTERM, libc::SIG_DFL); Ok(()) });
    }
    Stub { child: c.spawn().unwrap(), out, ctl, t }
}
fn spawn(args: &[&str]) -> Stub { spawn_in(tempfile::tempdir().unwrap(), args, true, "") }

/// Polls every 10 ms until `ctl/done` has `n` lines; panics after `BOUND`.
fn wait_done(ctl: &Path, n: usize) -> Vec<String> {
    let end = Instant::now() + BOUND;
    loop {
        let l: Vec<String> = std::fs::read_to_string(ctl.join("done")).unwrap_or_default().lines().map(String::from).collect();
        if l.len() >= n { return l; }
        assert!(Instant::now() < end, "done has {l:?}, want {n} lines");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// Polls `try_wait` every 10 ms; panics after `BOUND`.
fn wait_exit(child: &mut Child) -> ExitStatus {
    let end = Instant::now() + BOUND;
    loop {
        if let Some(s) = child.try_wait().unwrap() { return s; }
        assert!(Instant::now() < end, "the stub did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// The JSON lines of a file; empty when the file is missing.
fn jsonl(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn exit_step_ends_the_stub_with_that_code() {
    let t = tempfile::tempdir().unwrap();
    harness::stub_control(&t.path().join("ctl"), "exit 7").unwrap(); // written before the start
    let mut s = spawn_in(t, &[], true, "");
    let pid = s.child.id();
    assert_eq!(wait_exit(&mut s.child).code(), Some(7));
    assert_eq!(wait_done(&s.ctl, 2), [format!("start {pid}"), "exit 7".to_string()]);
    // a second process on the same directory does not run the finished step again
    let t = std::mem::replace(&mut s.t, tempfile::tempdir().unwrap());
    let mut s2 = spawn_in(t, &[], true, "");
    assert_eq!(wait_done(&s2.ctl, 3)[2], format!("start {}", s2.child.id()));
    harness::stub_control(&s2.ctl, "exit 0").unwrap();
    assert_eq!(wait_exit(&mut s2.child).code(), Some(0));
    assert_eq!(wait_done(&s2.ctl, 4)[3], "exit 0");
}

#[test]
fn stub_stays_alive_until_a_signal() {
    #[cfg(unix)]
    let sigs = [libc::SIGTERM, libc::SIGINT];
    #[cfg(not(unix))]
    let sigs = [0];
    for sig in sigs {
        let mut s = spawn(&[]);
        wait_done(&s.ctl, 1);
        harness::stub_control(&s.ctl, "hook Stop").unwrap(); // alive: it still serves a step after the start
        wait_done(&s.ctl, 2);
        assert!(s.child.try_wait().unwrap().is_none());
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(unsafe { libc::kill(s.child.id() as i32, sig) }, 0);
            let st = wait_exit(&mut s.child);
            assert_eq!((st.signal(), st.code()), (Some(sig), None)); // death by the signal, not an exit code (AU73)
        }
        #[cfg(not(unix))]
        {
            let _ = sig;
            s.child.kill().unwrap();
            assert!(!wait_exit(&mut s.child).success());
        }
    }
}

#[test]
fn a_step_that_cannot_run_exits_2() {
    for step in ["jump", "hook SessionStart", "hook Stop [1]", "exit x", "request 5"] { // `request`: ZYGO_URL is empty
        let mut s = spawn(&[]);
        harness::stub_control(&s.ctl, step).unwrap();
        assert_eq!(wait_exit(&mut s.child).code(), Some(2), "{step}");
        assert_eq!(wait_done(&s.ctl, 1).len(), 1, "{step}"); // only the start line
    }
}

#[test]
fn session_argument_is_recorded() {
    for flag in ["--session-id", "--resume", "-s", "--session"] {
        let mut s = spawn_in(tempfile::tempdir().unwrap(), &["--model", "x", flag, ID], false, "");
        assert_eq!(wait_exit(&mut s.child).code(), Some(0));
        let v: Value = serde_json::from_slice(&std::fs::read(s.out.join("session-arg.json")).unwrap()).unwrap();
        assert_eq!(v, json!({"flag": flag, "id": ID}));
    }
    let mut s = spawn_in(tempfile::tempdir().unwrap(), &["--model", "x"], false, "");
    assert_eq!(wait_exit(&mut s.child).code(), Some(0));
    assert!(!s.out.join("session-arg.json").exists());
}

/// The `session-arg.json` record of a capture-mode stub started with `args`: `(flag, id)`, or `None` when it is not written.
fn recorded(args: &[&str]) -> Option<(String, String)> {
    let mut s = spawn_in(tempfile::tempdir().unwrap(), args, false, "");
    assert_eq!(wait_exit(&mut s.child).code(), Some(0), "{args:?}");
    let v: Value = serde_json::from_slice(&std::fs::read(s.out.join("session-arg.json")).ok()?).unwrap();
    Some((v["flag"].as_str().unwrap().to_string(), v["id"].as_str().unwrap().to_string()))
}
fn pair(flag: &str, id: &str) -> Option<(String, String)> { Some((flag.to_string(), id.to_string())) }

#[test]
fn session_argument_after_double_dash_is_a_prompt_word() { // FIX-13 round 2 R1, as the launcher's has_session_arg
    for args in [&["--", "--resume=x"][..], &["--", "--resume", "x"], &["--model", "x", "--", "--session-id=U1"]] {
        assert_eq!(recorded(args), None, "{args:?}");
    }
    assert_eq!(recorded(&["--resume=U1", "--", "--session-id=U2"]), pair("--resume", "U1"));
    let t = tempfile::tempdir().unwrap();
    let set = settings(t.path());
    let s = spawn_in(t, &["--settings", set.as_str(), "--", "--resume=x"], true, "");
    wait_done(&s.ctl, 1);
    let own = format!("00000000-0000-4000-8000-{:012x}", s.child.id());
    let h = jsonl(&s.out.join("hooks.jsonl"));
    assert_eq!(h.len(), 1);
    assert_cat(&h[0], "SessionStart", &input(&s, "SessionStart", &own, json!({"source": "startup"})));
}

#[test]
fn split_session_flag_followed_by_a_flag_has_no_id() { // FIX-13 round 2 R2
    assert_eq!(recorded(&["--resume", "--model", "x"]), None);
    assert_eq!(recorded(&["-s", "-m", "x"]), None);
    assert_eq!(recorded(&["--resume", "--model", "x", "--session-id", "U2"]), pair("--session-id", "U2"));
}

#[test]
fn one_token_session_argument_filters() { // FIX-13 D1, D2; round 2 R3
    assert_eq!(recorded(&["--session-id=", "--session-id=U2"]), pair("--session-id", "U2")); // an empty = value is no id
    assert_eq!(recorded(&["-s=x"]), None); // -s has no = form
    assert_eq!(recorded(&["--resume=a=b"]), pair("--resume", "a=b")); // the id is the text after the first =
    assert_eq!(recorded(&["--model", "x", "--resume"]), None); // a trailing split flag has no next token
    assert_eq!(recorded(&["--session=ses_1"]), pair("--session", "ses_1")); // OpenCode's one-token resume (AU3)
    assert_eq!(recorded(&["--session=-x"]), pair("--session", "-x"));
    assert_eq!(recorded(&["--session="]), None);
}

/// `cat` prints the hook's stdin, so `out` in `hooks.jsonl` is what the hook process received.
fn settings(dir: &Path) -> String {
    let cat = json!([{"hooks": [{"type": "command", "command": "cat"}]}]);
    let mut hooks = serde_json::Map::new();
    for e in ["SessionStart", "UserPromptSubmit", "Stop", "StopFailure", "SubagentStart", "SubagentStop"] {
        hooks.insert(e.into(), cat.clone());
    }
    hooks.insert("PreToolUse".into(), json!([
        {"matcher": "Edit|Write", "hooks": [{"type": "command", "command": "echo no"}]},
        {"matcher": "Agent", "hooks": [{"type": "command", "command": "cat"}]},
        {"matcher": "*", "hooks": [{"type": "command", "command": "echo star"}]},
    ]));
    let p = dir.join("settings.json");
    std::fs::write(&p, json!({"hooks": hooks}).to_string()).unwrap();
    p.to_string_lossy().into_owned()
}
/// The input the stub must send: the common keys plus `extra`.
fn input(s: &Stub, event: &str, id: &str, extra: Value) -> Value {
    let mut v = json!({"session_id": id, "transcript_path": s.out.join("transcript.jsonl").to_string_lossy(),
        "cwd": std::fs::read_to_string(s.out.join("cwd.txt")).unwrap(), "hook_event_name": event});
    v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    v
}
/// One `cat` call: the record holds `want`, and the hook process received exactly `want`.
fn assert_cat(line: &Value, event: &str, want: &Value) {
    assert_eq!((&line["event"], &line["command"], &line["code"], &line["input"]), (&json!(event), &json!("cat"), &json!(0), want));
    assert_eq!(&serde_json::from_str::<Value>(line["out"].as_str().unwrap()).unwrap(), want);
}

#[test]
fn session_start_hook_gets_the_session_id() {
    for (args, source) in [(vec!["--session-id", ID], "startup"), (vec!["--resume", ID], "resume"), (vec![], "startup")] {
        let t = tempfile::tempdir().unwrap();
        let set = settings(t.path());
        let mut a = args.clone();
        a.extend(["--settings", set.as_str()]);
        let s = spawn_in(t, &a, true, "");
        wait_done(&s.ctl, 1); // `start` is written after the SessionStart hooks
        let own = format!("00000000-0000-4000-8000-{:012x}", s.child.id());
        let id = if args.is_empty() { own.as_str() } else { ID };
        let h = jsonl(&s.out.join("hooks.jsonl"));
        assert_eq!(h.len(), 1);
        assert_cat(&h[0], "SessionStart", &input(&s, "SessionStart", id, json!({"source": source})));
    }
}

/// Through the real launcher and the settings that `ClaudeCode` generates: the `zygo` of the rig is a stub copy that prints its argv.
#[test]
fn launched_member_calls_the_generated_session_start_hook_and_returns_the_exit_step() {
    use harness::*;
    let rig = rig();
    let ctl = rig.root.join("ctl");
    stub_control(&ctl, "exit 7").unwrap();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| {
        o.args = vec!["--session-id".into(), ID.into()];
        o.env.0.push(("ZYGO_STUB_CONTROL".into(), ctl.clone().into_os_string()));
        o.env.0.push(("ZYGO_STUB_SHELL".into(), shell().into_os_string()));
    });
    assert_eq!(r.code, 7, "{}", r.stderr);
    assert_eq!(r.cap.session_arg, Some(("--session-id".to_string(), ID.to_string())));
    assert!(r.cap.shell.is_empty()); // control mode: no shell.json
    let h = jsonl(&rig.root.join("out/hooks.jsonl"));
    assert_eq!(h.len(), 1);
    assert_eq!((&h[0]["event"], &h[0]["out"], &h[0]["code"]), (&json!("SessionStart"), &json!("hook session-start"), &json!(0)));
    assert_eq!(h[0]["command"], format!("{} hook session-start", spec_quote(&rig.zygo)));
    assert_eq!((&h[0]["input"]["session_id"], &h[0]["input"]["source"]), (&json!(ID), &json!("startup")));
}

/// Sends `hook <event>` and then `step2`; returns the stub and the hook records after the SessionStart one.
fn emit(event: &str, step2: &str) -> (Stub, Vec<Value>) {
    let t = tempfile::tempdir().unwrap();
    let set = settings(t.path());
    let s = spawn_in(t, &["--session-id", ID, "--settings", set.as_str()], true, "");
    harness::stub_control(&s.ctl, &format!("hook {event}")).unwrap();
    harness::stub_control(&s.ctl, step2).unwrap();
    let done = wait_done(&s.ctl, 3); // each line is written after its hooks returned
    assert_eq!(done[1..], [format!("hook {event}"), step2.to_string()]);
    let h = jsonl(&s.out.join("hooks.jsonl"))[1..].to_vec();
    (s, h)
}
fn sub() -> Value { json!({"agent_id": "stub-sub-1", "agent_type": "claude"}) }
fn stop() -> Value { json!({"stop_hook_active": false, "background_tasks": []}) }
fn merged(a: Value, b: Value) -> Value { let mut a = a; a.as_object_mut().unwrap().extend(b.as_object().unwrap().clone()); a }

#[test]
fn user_prompt_submit_hook_on_request() {
    let (s, h) = emit("UserPromptSubmit", r#"hook UserPromptSubmit {"prompt":"/zygo:status"}"#);
    assert_eq!(h.len(), 2);
    assert_cat(&h[0], "UserPromptSubmit", &input(&s, "UserPromptSubmit", ID, json!({"prompt": "stub prompt"})));
    assert_cat(&h[1], "UserPromptSubmit", &input(&s, "UserPromptSubmit", ID, json!({"prompt": "/zygo:status"})));
}
#[test]
fn pre_tool_use_agent_hook_on_request() {
    let agent = json!({"tool_name": "Agent", "tool_use_id": "toolu_stub",
        "tool_input": {"description": "stub", "prompt": "stub", "subagent_type": "claude", "run_in_background": false}});
    let (s, h) = emit("PreToolUse", r#"hook PreToolUse {"tool_name":"Write"}"#);
    // `Agent`: the `Agent` entry and the `*` entry, in settings order; `Write`: the `Edit|Write` entry and the `*` entry
    assert_eq!(h.len(), 4);
    assert_cat(&h[0], "PreToolUse", &input(&s, "PreToolUse", ID, agent));
    assert_eq!((&h[1]["command"], &h[1]["out"]), (&json!("echo star"), &json!("star")));
    assert_eq!((&h[2]["command"], &h[2]["out"]), (&json!("echo no"), &json!("no")));
    assert_eq!((&h[3]["command"], &h[3]["input"]["tool_name"]), (&json!("echo star"), &json!("Write")));
}
#[test]
fn stop_hook_on_request() {
    let (s, h) = emit("Stop", r#"hook Stop {"agent_id":"a1","background_tasks":[{"id":"t1"}]}"#);
    assert_eq!(h.len(), 2);
    assert_cat(&h[0], "Stop", &input(&s, "Stop", ID, stop()));
    assert_cat(&h[1], "Stop", &input(&s, "Stop", ID, json!({"stop_hook_active": false, "agent_id": "a1", "background_tasks": [{"id": "t1"}]})));
}
#[test]
fn stop_failure_hook_on_request() {
    let (s, h) = emit("StopFailure", "hook StopFailure {}");
    assert_eq!(h.len(), 2);
    assert_cat(&h[0], "StopFailure", &input(&s, "StopFailure", ID, json!({})));
    assert_cat(&h[1], "StopFailure", &input(&s, "StopFailure", ID, json!({})));
}
#[test]
fn subagent_start_hook_on_request() {
    let (s, h) = emit("SubagentStart", r#"hook SubagentStart {"agent_id":"a2","agent_type":"zygo-code"}"#);
    assert_eq!(h.len(), 2);
    assert_cat(&h[0], "SubagentStart", &input(&s, "SubagentStart", ID, sub()));
    assert_cat(&h[1], "SubagentStart", &input(&s, "SubagentStart", ID, json!({"agent_id": "a2", "agent_type": "zygo-code"})));
}
#[test]
fn subagent_stop_hook_on_request() {
    let (s, h) = emit("SubagentStop", r#"hook SubagentStop {"agent_id":"a2"}"#);
    assert_eq!(h.len(), 2);
    assert_cat(&h[0], "SubagentStop", &input(&s, "SubagentStop", ID, merged(sub(), stop())));
    assert_cat(&h[1], "SubagentStop", &input(&s, "SubagentStop", ID, merged(merged(sub(), stop()), json!({"agent_id": "a2"}))));
}

#[test]
fn model_request_goes_to_the_proxy_and_stays_open() {
    use std::io::{Read, Write};
    use std::sync::mpsc::channel;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap()); // `ZYGO_URL` ends with `/`, as `Url::to_string` gives it
    let (req_tx, req_rx) = channel::<String>();
    let (go_tx, go_rx) = channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let (mut buf, mut b) = (Vec::new(), [0u8; 4096]);
        let text = loop { // headers, then `content-length` body bytes
            let n = c.read(&mut b).unwrap();
            assert!(n > 0, "the stub closed the request early");
            buf.extend_from_slice(&b[..n]);
            let t = String::from_utf8_lossy(&buf).into_owned();
            if let Some((head, body)) = t.split_once("\r\n\r\n") {
                let len: usize = head.lines().find_map(|h| h.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                if body.len() >= len { break t; }
            }
        };
        // The reply starts (the status is with the stub) and is held: the stream does not end until the test releases it.
        c.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n").unwrap();
        c.flush().unwrap();
        req_tx.send(text).unwrap(); // the event: the request is here and its reply is held
        go_rx.recv().unwrap();
        c.write_all(b"data: {}\n\n").unwrap();
    });
    let mut s = spawn_in(tempfile::tempdir().unwrap(), &[], true, &url);
    let start = format!("start {}", s.child.id());
    harness::stub_control(&s.ctl, "request 20000").unwrap();
    // Bounded wait for the held state. All that the stub wrote before it sent the request is on disk now.
    let req = req_rx.recv_timeout(BOUND).unwrap();
    let open = |s: &mut Stub| {
        assert_eq!(wait_done(&s.ctl, 1), std::slice::from_ref(&start),"the request step is finished while its reply is held");
        assert!(jsonl(&s.out.join("requests.jsonl")).is_empty());
        assert!(s.child.try_wait().unwrap().is_none(), "the stub ended while its reply is held");
    };
    open(&mut s);
    let (head, body) = req.split_once("\r\n\r\n").unwrap();
    let lines: Vec<&str> = head.split("\r\n").collect();
    assert_eq!(lines[0], "POST /anthropic/v1/messages HTTP/1.1");
    for h in ["content-type: application/json", "anthropic-version: 2023-06-01", "x-zygo-session: s-1", "x-api-key: tok-1", "connection: close"] {
        assert!(lines.contains(&h), "{h} not in {lines:?}");
    }
    assert_eq!(serde_json::from_str::<Value>(body).unwrap(),
        json!({"model": "lead", "max_tokens": 1, "stream": true, "messages": [{"role": "user", "content": "stub hold 20000"}]}));
    open(&mut s); // still open at the last moment before the release
    go_tx.send(()).unwrap();
    assert_eq!(wait_done(&s.ctl, 2)[1], "request 20000");
    assert_eq!(jsonl(&s.out.join("requests.jsonl")), [json!({"hold_ms": 20000, "status": 200})]);
    server.join().unwrap();
}

#[test]
fn common_stub_agent_is_the_built_binary() {
    let built = Path::new(env!("CARGO_BIN_EXE_stub-agent")).canonicalize().unwrap();
    assert_eq!(common::stub_agent().canonicalize().unwrap(), built);
    assert_eq!(common::stub_agent_in(built.parent().unwrap()).unwrap().canonicalize().unwrap(), built);
}
#[test]
fn common_stub_agent_names_the_build_command_when_missing() {
    let t = tempfile::tempdir().unwrap();
    let e = common::stub_agent_in(t.path()).unwrap_err();
    assert!(e.contains("cargo build -p zygo-launch --bin stub-agent"), "{e}");
    assert!(e.contains(&t.path().join(format!("stub-agent{}", std::env::consts::EXE_SUFFIX)).display().to_string()), "{e}");
}
#[test]
fn both_stub_control_helpers_write_the_same_steps_file() {
    let t = tempfile::tempdir().unwrap();
    let (a, b) = (t.path().join("a/ctl"), t.path().join("b/ctl")); // the helper makes the directory
    for step in ["hook Stop", "exit 3"] {
        harness::stub_control(&a, step).unwrap();
        common::stub_control(&b, step).unwrap();
    }
    assert_eq!(std::fs::read_to_string(a.join("steps")).unwrap(), "hook Stop\nexit 3\n");
    assert_eq!(std::fs::read(a.join("steps")).unwrap(), std::fs::read(b.join("steps")).unwrap());
}
