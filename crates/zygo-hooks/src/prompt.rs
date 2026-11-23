//! `zygo hook prompt` (`UserPromptSubmit`): the `/zygo:<verb>` dispatcher (§17.1, §10.2.4).

use std::ffi::OsString;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};
use zygo_core::hook::{HookInput, HookOutput, SlashDispatch};
use zygo_core::paths::Paths;
use zygo_core::verbs::VERB_SPECS;

/// The wait for the verb; the hook itself ends within 5 s (§17.1).
pub const HOOK_DEADLINE: Duration = Duration::from_millis(4500);

static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/zygo:([a-z][a-z0-9-]*)(?:\s+(.*))?$").expect("valid regex"));

/// `zygo hook prompt` (§17.1, W§9.1 `UserPromptSubmit` row): `run_with` with `dispatch` and `deadline`. The `zygo`
/// binary passes the registered dispatcher, `HOOK_DEADLINE`, its paths and the process environment. When the verb
/// panicked (`VerbPanicked`) and `ZYGO_WORKSPACE` is set and not empty in `env` (the launcher sets it for members only,
/// AU58), the line reaches the model, so `workspace::mark_busy` marks the member busy (H1 attempt-2 review round 4,
/// T2); the error is still returned, so `guarded` writes its one stderr line (C6). A failed mark adds its text to that
/// line. The mark waits for `activity.lock` at most the smaller of `activity::LOCK_BOUND` and the time left of
/// `deadline`, measured from the hook start as in `run_with`, so after a panic seen near the deadline the hook still
/// ends within its 5 s bound (§17.1; round 4 follow-up F1). Without `ZYGO_WORKSPACE` nothing else is read: a launch
/// that is not a member keeps its v1 behaviour.
pub fn run_in_env(
    input: &HookInput, dispatch: Option<SlashDispatch>, deadline: Duration, paths: &Paths,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> anyhow::Result<HookOutput> {
    let start = Instant::now();
    match run_with(input, dispatch, deadline) {
        Err(e) if e.is::<VerbPanicked>() && env("ZYGO_WORKSPACE").is_some_and(|v| !v.is_empty()) => {
            let bound = deadline.saturating_sub(start.elapsed()).min(zygo_workspace::activity::LOCK_BOUND);
            match crate::workspace::mark_busy(paths, env, bound) {
                Ok(()) => Err(e),
                Err(m) => Err(anyhow::anyhow!("{e:#}; busy mark: {m:#}")),
            }
        }
        r => r,
    }
}

/// A `/zygo:<verb>` line, trailing whitespace trimmed: the line test of `run_with` (§17.1).
pub fn is_slash(prompt: &str) -> bool {
    RE.is_match(prompt.trim_end())
}

/// `run_with` takes the prompt `line` as a verb and blocks it from the model: a `/zygo:<verb>` line (`is_slash`) with
/// a registered `dispatch`. Every other line passes to the model. The member prompt hook uses this rule too, so a line
/// that reaches the model starts a busy turn (W§9.1; H1 review round 2 R1). The one exception is a verb that panics:
/// `run_with` then lets the consumed line pass (C6), and `run_in_env` marks a member busy (round 4 T2).
pub fn consumes(line: &str, dispatch: Option<SlashDispatch>) -> bool {
    dispatch.is_some() && is_slash(line)
}

/// The error of `run_with` for a verb whose thread ended without a reply (a panic): an internal error, so the hook
/// prints nothing, `guarded` writes one stderr line and the line reaches the model (base spec C6, §17.1 N28). Its text
/// is `` prompt: verb `<verb>` ended without a reply ``. Callers find it with `anyhow::Error::is::<VerbPanicked>()`.
#[derive(Debug)]
pub struct VerbPanicked(String);

impl std::fmt::Display for VerbPanicked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "prompt: verb `{}` ended without a reply", self.0)
    }
}

impl std::error::Error for VerbPanicked {}

/// A line that `consumes` takes blocks with the verb's reply or error text; an unknown verb runs `help`. A verb that
/// does not reply within `deadline` blocks with the shell command; one that panics gives the error `VerbPanicked`,
/// so the prompt passes (C6). That is the one case where `consumes` is true and the line reaches the model.
pub fn run_with(input: &HookInput, dispatch: Option<SlashDispatch>, deadline: Duration) -> anyhow::Result<HookOutput> {
    let start = Instant::now();
    let Some(prompt) = input.raw.get("prompt").and_then(Value::as_str) else { return Ok(HookOutput::None) };
    let Some(dispatch) = dispatch.filter(|_| consumes(prompt, dispatch)) else { return Ok(HookOutput::None) };
    // Trailing whitespace (a final newline) is not part of the line (§17.1).
    let Some(c) = RE.captures(prompt.trim_end()) else { return Ok(HookOutput::None) };
    let name = c.get(1).map_or("", |m| m.as_str());
    let (verb, args) = if VERB_SPECS.iter().any(|s| s.name == name) {
        (name.to_string(), c.get(2).map_or("", |m| m.as_str()).to_string())
    } else {
        ("help".to_string(), String::new())
    };
    let (tx, rx) = mpsc::channel();
    let (v, a) = (verb.clone(), args.clone());
    // The worker is not joined: on a timeout it ends with the process.
    std::thread::spawn(move || {
        let _ = tx.send(dispatch(&v, &a));
    });
    let text = match rx.recv_timeout(deadline.saturating_sub(start.elapsed())) {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => zygo_config::display::error_text(&e),
        Err(RecvTimeoutError::Timeout) if args.is_empty() => format!("run in your shell: zygo {verb}"),
        Err(RecvTimeoutError::Timeout) => format!("run in your shell: zygo {verb} {}", zygo_core::esc(&args)),
        // The verb thread died (a panic): an internal error, so the prompt passes (C6; H1 round 4 T1).
        Err(RecvTimeoutError::Disconnected) => return Err(VerbPanicked(verb).into()),
    };
    Ok(HookOutput::Json(json!({
        "decision": "block",
        "reason": format!("zygo: {text}"),
        "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "suppressOriginalPrompt": true }
    })))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Condvar, Mutex};

    use serde_json::{json, Value};
    use zygo_core::hook::guarded;
    use zygo_core::Agent;

    use super::*;

    type Calls = Mutex<Vec<(String, String)>>;

    fn input(prompt: Option<&str>) -> HookInput {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks/UserPromptSubmit-main.json");
        let mut v: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
        match prompt {
            Some(t) => v["prompt"] = json!(t),
            None => {
                v.as_object_mut().unwrap().remove("prompt");
            }
        }
        HookInput::parse(Agent::ClaudeCode, v.to_string().as_bytes()).unwrap()
    }

    fn block(text: &str) -> Value {
        json!({
            "decision": "block",
            "reason": format!("zygo: {text}"),
            "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "suppressOriginalPrompt": true }
        })
    }

    fn json_of(out: HookOutput) -> Value {
        match out {
            HookOutput::Json(v) => v,
            _ => panic!("not JSON"),
        }
    }

    fn record(calls: &Calls, verb: &str, args: &str) {
        calls.lock().unwrap().push((verb.to_string(), args.to_string()));
    }

    fn taken(calls: &Calls) -> Vec<(String, String)> {
        std::mem::take(&mut *calls.lock().unwrap())
    }

    fn pair(v: &str, a: &str) -> Vec<(String, String)> {
        vec![(v.to_string(), a.to_string())]
    }

    #[test]
    fn blocks_with_the_reply() {
        static CALLS: Calls = Mutex::new(Vec::new());
        fn d(v: &str, a: &str) -> anyhow::Result<String> {
            record(&CALLS, v, a);
            Ok("compact 600k for this session".into())
        }
        let out = run_with(&input(Some("/zygo:compact 600k")), Some(d), HOOK_DEADLINE).unwrap();
        assert_eq!(json_of(out), block("compact 600k for this session"));
        assert_eq!(taken(&CALLS), pair("compact", "600k"));
    }

    #[test]
    fn error_text_is_the_reason() {
        fn d(_: &str, _: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("compact failed: x"))
        }
        let out = run_with(&input(Some("/zygo:compact 600k")), Some(d), HOOK_DEADLINE).unwrap();
        assert_eq!(json_of(out), block("compact failed: x"));
    }

    #[test]
    fn unknown_verb_runs_help() {
        static CALLS: Calls = Mutex::new(Vec::new());
        fn d(v: &str, a: &str) -> anyhow::Result<String> {
            record(&CALLS, v, a);
            Ok("fake help".into())
        }
        let out = run_with(&input(Some("/zygo:frob a")), Some(d), HOOK_DEADLINE).unwrap();
        assert_eq!(json_of(out)["reason"], "zygo: fake help");
        assert_eq!(taken(&CALLS), pair("help", ""));
    }

    #[test]
    fn no_args() {
        static CALLS: Calls = Mutex::new(Vec::new());
        fn d(v: &str, a: &str) -> anyhow::Result<String> {
            record(&CALLS, v, a);
            Ok("s".into())
        }
        run_with(&input(Some("/zygo:status")), Some(d), HOOK_DEADLINE).unwrap();
        assert_eq!(taken(&CALLS), pair("status", ""));
    }

    #[test]
    fn newline_after_the_verb_is_whitespace() {
        static CALLS: Calls = Mutex::new(Vec::new());
        fn d(v: &str, a: &str) -> anyhow::Result<String> {
            record(&CALLS, v, a);
            Err(anyhow::anyhow!("bad"))
        }
        let out = run_with(&input(Some("/zygo:compact\nmore")), Some(d), HOOK_DEADLINE).unwrap();
        assert_eq!(json_of(out), block("bad"));
        assert_eq!(taken(&CALLS), pair("compact", "more"));
    }

    #[test]
    fn others_pass() {
        static CALLS: Calls = Mutex::new(Vec::new());
        fn d(v: &str, a: &str) -> anyhow::Result<String> {
            record(&CALLS, v, a);
            Ok("x".into())
        }
        for p in ["hello", " /zygo:compact", "/zygo:Compact", "/zygo:", "/zygo:compact 600k\nmore"] {
            assert!(matches!(run_with(&input(Some(p)), Some(d), HOOK_DEADLINE).unwrap(), HookOutput::None), "{p:?}");
        }
        assert!(matches!(run_with(&input(None), Some(d), HOOK_DEADLINE).unwrap(), HookOutput::None));
        assert!(matches!(run_with(&input(Some("/zygo:status")), None, HOOK_DEADLINE).unwrap(), HookOutput::None));
        assert!(taken(&CALLS).is_empty());
    }

    /// R1, round 4 T1: `consumes` is the rule of `run_with` for a dispatcher that replies or fails: it blocks exactly
    /// the lines that `consumes` takes. A panicking verb is the one case where `consumes` is true and `run_with` lets
    /// the line pass: it gives an error, so `guarded` writes one stderr line and the hook exits 0 (base spec C6, N28).
    #[test]
    fn consumes_is_the_rule_of_run_with() {
        fn d(_: &str, _: &str) -> anyhow::Result<String> {
            Ok("x".into())
        }
        fn e(_: &str, _: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("failed"))
        }
        fn p(_: &str, _: &str) -> anyhow::Result<String> {
            panic!("verb panicked")
        }
        let lines = ["hello", " /zygo:compact", "/zygo:Compact", "/zygo:", "/zygo:compact 600k\nmore", "/zygo:status", "/zygo:status \n", "/zygo:frob a", "/zygo:compact\nmore"];
        for dispatch in [Some(d as SlashDispatch), Some(e as SlashDispatch), None] {
            for line in lines {
                let blocked = matches!(run_with(&input(Some(line)), dispatch, HOOK_DEADLINE).unwrap(), HookOutput::Json(_));
                assert_eq!(blocked, consumes(line, dispatch), "{line:?} {}", dispatch.is_some());
            }
        }
        assert!(consumes("/zygo:status", Some(d)) && !consumes("/zygo:status", None) && !consumes("hello", Some(d)));
        // The panic case: every line that `consumes` takes is an error (no block, the line passes); every other line
        // passes with no output.
        for line in lines {
            match run_with(&input(Some(line)), Some(p), HOOK_DEADLINE) {
                Err(err) => {
                    assert!(consumes(line, Some(p)), "{line:?}");
                    assert!(err.is::<VerbPanicked>(), "{line:?}: {err:#}");
                }
                Ok(out) => {
                    assert!(!consumes(line, Some(p)), "{line:?}");
                    assert!(matches!(out, HookOutput::None), "{line:?}");
                }
            }
        }
    }

    #[test]
    fn slow_verb_replies_with_the_shell_command() {
        static GATE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
        fn d(_: &str, _: &str) -> anyhow::Result<String> {
            let mut open = GATE.0.lock().unwrap();
            while !*open {
                open = GATE.1.wait(open).unwrap();
            }
            Ok("late".into())
        }
        assert_eq!(HOOK_DEADLINE, Duration::from_millis(4500));
        let out = run_with(&input(Some("/zygo:compact 600k")), Some(d), Duration::from_millis(100)).unwrap();
        *GATE.0.lock().unwrap() = true;
        GATE.1.notify_all();
        assert_eq!(json_of(out), block("run in your shell: zygo compact 600k"));
    }

    /// Round 2 F4: the typed words in the timeout reply are escaped.
    #[test]
    fn slow_verb_reply_escapes_the_words() {
        static GATE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
        fn d(_: &str, _: &str) -> anyhow::Result<String> {
            let mut open = GATE.0.lock().unwrap();
            while !*open {
                open = GATE.1.wait(open).unwrap();
            }
            Ok("late".into())
        }
        let out = run_with(&input(Some("/zygo:compact 600k\u{1b}[2K\u{202e}")), Some(d), Duration::from_millis(100)).unwrap();
        *GATE.0.lock().unwrap() = true;
        GATE.1.notify_all();
        assert_eq!(json_of(out), block("run in your shell: zygo compact 600k\\u{1b}[2K\\u{202e}"));
    }

    /// Round 4 T1 (base spec C6, N28): a verb that panics is an internal error. `run_with` gives the error kind
    /// `VerbPanicked`, `guarded` prints nothing, writes one stderr line and exits 0, and the line reaches the model.
    #[test]
    fn panicking_verb_is_an_error() {
        fn d(_: &str, _: &str) -> anyhow::Result<String> {
            panic!("verb panicked")
        }
        let Err(err) = run_with(&input(Some("/zygo:status")), Some(d), HOOK_DEADLINE) else { panic!("not an error") };
        assert!(err.is::<VerbPanicked>());
        assert_eq!(format!("{err:#}"), "prompt: verb `status` ended without a reply");
        let bytes = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "/zygo:status" }).to_string();
        let out = guarded(
            &|b| run_with(&HookInput::parse(Agent::ClaudeCode, b)?, Some(d), HOOK_DEADLINE),
            bytes.as_bytes(),
        );
        assert_eq!(out, (String::new(), 0));
    }


    #[test]
    fn error_reason_is_escaped_unless_escaped_lines() {
        fn d(_: &str, _: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("bad\u{1b}[2K\nzygo: forged"))
        }
        let out = run_with(&input(Some("/zygo:compact 600k")), Some(d), HOOK_DEADLINE).unwrap();
        assert_eq!(json_of(out), block("bad\\u{1b}[2K\\nzygo: forged"));
        fn l(_: &str, _: &str) -> anyhow::Result<String> {
            Err(zygo_config::display::EscapedLines::new(["a", "b"]).into())
        }
        let out = run_with(&input(Some("/zygo:compact 600k")), Some(l), HOOK_DEADLINE).unwrap();
        assert_eq!(json_of(out), block("a\nb"));
    }
}
