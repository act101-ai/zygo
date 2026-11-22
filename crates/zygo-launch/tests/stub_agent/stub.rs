//! The stub agent for the launch tests (N34). One executable, three roles by argv: `zygo` (`hook` or `mcp`),
//! the version probe (`--version`), and the agent (anything else), which records what it received into `ZYGO_STUB_OUT`.
//! `session-arg.json` names the session argument (`--session-id`, `--resume` or `-s`) when the argv has one.
//!
//! Control mode (L3, the workspace tests): when `ZYGO_STUB_CONTROL` names a directory, the agent does not exit after the
//! capture. It serves the lines of `<dir>/steps` in order (a test appends them with `stub_control`) and appends to
//! `<dir>/done` the line `start <pid>` (after it called the `SessionStart` hooks of the `--settings` file), then the text of each step when that step has finished. A step that cannot run
//! ends the stub with code 2 and no `done` line. The stub installs no signal handler: a signal ends it by the default
//! action. `ZYGO_STUB_EXIT` and the `shell.json` capture are not used in control mode.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Map, Value};

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCommandLineW() -> *const u16;
}

fn var(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

fn text(p: &Path) -> Option<String> {
    std::fs::read(p).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Every file under `dir` as `/`-separated relative path -> text.
fn tree(dir: &Path) -> Value {
    fn walk(base: &Path, d: &Path, out: &mut Map<String, Value>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Some(t) = text(&p) {
                let rel: Vec<String> = p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(rel.join("/"), Value::String(t));
            }
        }
    }
    let mut out = Map::new();
    walk(dir, dir, &mut out);
    Value::Object(out)
}

fn flag(argv: &[String], name: &str) -> Option<String> {
    argv.iter().position(|a| a == name).and_then(|i| argv.get(i + 1)).cloned()
}

/// The session argument: the first of `--session-id`, `--resume` (Claude Code) and `-s` (OpenCode) in `argv` that has a value.
fn session_arg(argv: &[String]) -> Option<(&'static str, String)> {
    argv.windows(2).find_map(|w| ["--session-id", "--resume", "-s"].into_iter().find(|f| w[0] == *f).map(|f| (f, w[1].clone())))
}

fn result(o: std::io::Result<std::process::Output>) -> Value {
    match o {
        Ok(o) => json!({"out": String::from_utf8_lossy(&o.stdout).trim(), "code": o.status.code().unwrap_or(-1)}),
        Err(_) => json!({"out": "", "code": -1}),
    }
}

fn commands(settings: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut add = |v: &Value| out.extend(v.as_str().map(String::from));
    add(&settings["apiKeyHelper"]);
    add(&settings["statusLine"]["command"]);
    for list in settings["hooks"].as_object().into_iter().flat_map(|m| m.values()) {
        for entry in list.as_array().into_iter().flatten() {
            for h in entry["hooks"].as_array().into_iter().flatten() {
                add(&h["command"]);
            }
        }
    }
    out
}

/// The hook events a `hook` step can name (`SessionStart` is called once, by the stub itself).
const EVENTS: [&str; 6] = ["UserPromptSubmit", "PreToolUse", "Stop", "StopFailure", "SubagentStart", "SubagentStop"];

fn append(p: &Path, line: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
    f.write_all(format!("{line}\n").as_bytes()).unwrap();
}

/// The stdin JSON of one hook call, in the shapes of `fixtures/claude-code/hooks/*.json`. `overlay` replaces top-level keys.
fn hook_input(event: &str, argv: &[String], out: &Path, overlay: Option<&Map<String, Value>>) -> Value {
    let arg = session_arg(argv);
    let id = arg.as_ref().map(|(_, id)| id.clone()).unwrap_or_else(|| format!("00000000-0000-4000-8000-{:012x}", std::process::id()));
    let mut v = json!({
        "session_id": id,
        "transcript_path": out.join("transcript.jsonl").to_string_lossy(),
        "cwd": std::env::current_dir().unwrap().to_string_lossy(),
        "hook_event_name": event,
    });
    let stop = json!({"stop_hook_active": false, "background_tasks": []});
    let sub = json!({"agent_id": "stub-sub-1", "agent_type": "claude"});
    let added = match event {
        "SessionStart" => vec![json!({"source": if matches!(arg, Some(("--resume", _))) { "resume" } else { "startup" }})],
        "UserPromptSubmit" => vec![json!({"prompt": "stub prompt"})],
        "PreToolUse" => vec![json!({
            "tool_name": "Agent",
            "tool_input": {"description": "stub", "prompt": "stub", "subagent_type": "claude", "run_in_background": false},
            "tool_use_id": "toolu_stub",
        })],
        "Stop" => vec![stop],
        "SubagentStart" => vec![sub],
        "SubagentStop" => vec![sub, stop],
        _ => vec![],
    };
    let m = v.as_object_mut().unwrap();
    for a in added {
        m.extend(a.as_object().unwrap().clone());
    }
    m.extend(overlay.cloned().unwrap_or_default());
    v
}

/// Calls the hooks that `settings` (the `--settings` file) has for `event`, in order, each with `input` on stdin, and
/// appends one record per call to `hooks.jsonl`. A `PreToolUse` entry runs only when its `matcher` allows the tool name.
fn call_hooks(settings: &Value, event: &str, input: &Value, out: &Path) {
    use std::io::Write;
    use std::process::Stdio;
    let sh = var("ZYGO_STUB_SHELL").unwrap_or_else(|| "/bin/sh".into());
    let tool = input["tool_name"].as_str().unwrap_or_default();
    for entry in settings["hooks"][event].as_array().into_iter().flatten() {
        if event == "PreToolUse" && entry["matcher"].as_str().is_some_and(|m| m != "*" && !m.split('|').any(|p| p == tool)) {
            continue;
        }
        for c in entry["hooks"].as_array().into_iter().flatten().filter_map(|h| h["command"].as_str()) {
            let o = Command::new(&sh).arg("-c").arg(c).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().and_then(|mut child| {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(input.to_string().as_bytes());
                }
                child.wait_with_output()
            });
            let r = result(o);
            append(&out.join("hooks.jsonl"), &json!({"event": event, "command": c, "input": input, "out": r["out"], "code": r["code"]}).to_string());
        }
    }
}

/// Runs one step line. `Ok(Some(code))` is an `exit` step: the caller writes the `done` line, then exits.
fn step(line: &str, out: &Path, settings: &Value, argv: &[String]) -> Result<Option<i32>, String> {
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    match verb {
        "hook" => {
            let (event, overlay) = match rest.split_once(' ') {
                Some((e, j)) => (e, Some(j)),
                None => (rest, None),
            };
            if !EVENTS.contains(&event) {
                return Err(format!("unknown hook event `{event}`"));
            }
            let overlay = match overlay {
                Some(j) => Some(serde_json::from_str::<Map<String, Value>>(j).map_err(|e| format!("the hook input is not a JSON object: {e}"))?),
                None => None,
            };
            call_hooks(settings, event, &hook_input(event, argv, out, overlay.as_ref()), out);
            Ok(None)
        }
        "exit" => rest.parse::<i32>().map(Some).map_err(|e| format!("exit code: {e}")),
        _ => Err("unknown step".into()),
    }
}

/// Control mode: serves the lines of `ctl/steps` in order and writes each finished one to `ctl/done`.
fn control(ctl: &Path, out: &Path, settings: &Value, argv: &[String]) -> ! {
    let (steps, done) = (ctl.join("steps"), ctl.join("done"));
    std::fs::create_dir_all(ctl).unwrap();
    let mut cursor = text(&done).unwrap_or_default().lines().filter(|l| !l.starts_with("start ")).count();
    call_hooks(settings, "SessionStart", &hook_input("SessionStart", argv, out, None), out);
    append(&done, &format!("start {}", std::process::id()));
    loop {
        let all = text(&steps).unwrap_or_default();
        let Some(line) = all.split_inclusive('\n').filter_map(|l| l.strip_suffix('\n')).nth(cursor) else {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        };
        match step(line, out, settings, argv) {
            Ok(code) => {
                append(&done, line);
                cursor += 1;
                if let Some(c) = code {
                    std::process::exit(c);
                }
            }
            Err(reason) => {
                eprintln!("stub-agent: bad step: {line}: {reason}");
                std::process::exit(2);
            }
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    if argv == ["--version"] {
        println!("{}", var("ZYGO_STUB_VERSION").unwrap_or_default());
        return;
    }
    if matches!(argv.first().map(String::as_str), Some("hook" | "mcp")) {
        println!("{}", argv.join(" "));
        return;
    }
    let out = PathBuf::from(var("ZYGO_STUB_OUT").expect("ZYGO_STUB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let put = |name: &str, v: &Value| std::fs::write(out.join(name), v.to_string()).unwrap();
    put("argv.json", &json!(argv));
    let env: BTreeMap<String, String> = std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    put("env.json", &json!(env));
    std::fs::write(out.join("cwd.txt"), std::env::current_dir().unwrap().to_string_lossy().as_bytes()).unwrap();
    if let Some((flag, id)) = session_arg(&argv) {
        put("session-arg.json", &json!({"flag": flag, "id": id}));
    }
    #[cfg(windows)]
    {
        let p = unsafe { GetCommandLineW() };
        let mut n = 0;
        while unsafe { *p.add(n) } != 0 {
            n += 1;
        }
        let line = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) });
        std::fs::write(out.join("cmdline.txt"), line).unwrap();
    }
    let mut settings = Value::Null;
    if let Some(p) = flag(&argv, "--settings") {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            std::fs::write(out.join("settings-mode.txt"), format!("{mode:o}")).unwrap();
        }
        let t = text(Path::new(&p)).unwrap_or_default();
        std::fs::write(out.join("settings.json"), &t).unwrap();
        settings = serde_json::from_str(&t).unwrap_or(Value::Null);
    }
    let mut mcp = Value::Null;
    if let Some(p) = flag(&argv, "--mcp-config") {
        let t = text(Path::new(&p)).unwrap_or_default();
        std::fs::write(out.join("mcp.json"), &t).unwrap();
        mcp = serde_json::from_str(&t).unwrap_or(Value::Null);
    }
    if let Some(p) = flag(&argv, "--plugin-dir") {
        put("plugins.json", &tree(Path::new(&p)));
    }
    if let Some(d) = var("ZYGO_SESSION_DIR") {
        put("session.json", &tree(Path::new(&d)));
    }
    if let Some(d) = var("OPENCODE_CONFIG_DIR") {
        put("ocdir.json", &tree(Path::new(&d)));
    }
    let ctl = var("ZYGO_STUB_CONTROL").map(PathBuf::from);
    if let Some(sh) = var("ZYGO_STUB_SHELL").filter(|_| ctl.is_none()) {
        let mut shell = Map::new();
        for c in commands(&settings) {
            let r = result(Command::new(&sh).arg("-c").arg(&c).output());
            shell.insert(c, r);
        }
        put("shell.json", &Value::Object(shell));
    }
    let mut run = Map::new();
    for (name, s) in mcp["mcpServers"].as_object().into_iter().flatten() {
        let args: Vec<&str> = s["args"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let r = result(Command::new(s["command"].as_str().unwrap_or_default()).args(args).output());
        run.insert(name.clone(), r["out"].clone());
    }
    put("mcp-run.json", &Value::Object(run));
    if let Some(c) = &ctl {
        control(c, &out, &settings, &argv)
    }
    std::process::exit(var("ZYGO_STUB_EXIT").and_then(|v| v.parse().ok()).unwrap_or(0));
}
