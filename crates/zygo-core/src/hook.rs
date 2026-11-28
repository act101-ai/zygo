use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::sync::OnceLock;

use serde_json::Value;

use crate::Agent;

pub struct HookInput {
    pub agent: Agent,
    pub event: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub has_agent_id: bool,
    pub agent_id: Option<String>,
    pub cli_event: Option<String>,
    pub raw: Value,
}

impl HookInput {
    pub fn parse(agent: Agent, bytes: &[u8]) -> anyhow::Result<HookInput> {
        let mut v: Value = serde_json::from_slice(bytes)?;
        v = (agent.spec().hook_input)(v);
        let Some(obj) = v.as_object() else {
            anyhow::bail!("hook input is not a JSON object");
        };
        let text = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_owned);
        Ok(HookInput {
            agent,
            event: text("hook_event_name"),
            tool_name: text("tool_name"),
            tool_input: obj.get("tool_input").cloned(),
            has_agent_id: obj.contains_key("agent_id"),
            agent_id: text("agent_id"),
            cli_event: None,
            raw: v,
        })
    }

    /// Input for a hook that reads no stdin.
    pub fn empty(agent: Agent) -> HookInput {
        HookInput {
            agent,
            event: None,
            tool_name: None,
            tool_input: None,
            has_agent_id: false,
            agent_id: None,
            cli_event: None,
            raw: Value::Object(Default::default()),
        }
    }
}

pub enum HookOutput {
    None,
    Json(Value),
    Text(String),
}

impl HookOutput {
    pub fn to_stdout(&self) -> String {
        match self {
            HookOutput::None => String::new(),
            HookOutput::Json(v) => v.to_string(),
            HookOutput::Text(s) => s.clone(),
        }
    }
}

pub type HookFn = fn(&HookInput) -> anyhow::Result<HookOutput>;

/// C6 wrapper: a hook never breaks the agent. Exit code is always 0.
pub fn guarded(f: &dyn Fn(&[u8]) -> anyhow::Result<HookOutput>, stdin: &[u8]) -> (String, i32) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(AssertUnwindSafe(|| f(stdin)));
    panic::set_hook(previous);
    let msg = match result {
        Ok(Ok(out)) => return (out.to_stdout(), 0),
        Ok(Err(e)) => format!("{e:#}"),
        Err(p) => p
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string()),
    };
    error_line(&msg);
    (String::new(), 0)
}

/// One line to stderr, `zygo hook: <msg>` with each newline and CR in `msg` a space; a write error is ignored (C6).
pub fn error_line(msg: &str) {
    let _ = writeln!(std::io::stderr(), "zygo hook: {}", msg.replace(['\n', '\r'], " "));
}

/// `args` is the text after the verb, unsplit.
pub type SlashDispatch = fn(verb: &str, args: &str) -> anyhow::Result<String>;

static SLASH: OnceLock<SlashDispatch> = OnceLock::new();

pub fn set_slash_dispatch(f: SlashDispatch) {
    let _ = SLASH.set(f);
}

pub fn slash_dispatch() -> Option<SlashDispatch> {
    SLASH.get().copied()
}
