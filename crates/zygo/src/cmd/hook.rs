use std::io::{Read, Write};
use std::str::FromStr;

use zygo_core::hook::{guarded, HookFn, HookInput};
use zygo_core::Agent;

pub struct Hook {
    pub name: &'static str,
    pub run: HookFn,
    /// False only for `token` (apiKeyHelper gives no stdin).
    pub reads_stdin: bool,
}

pub static TABLE: &[Hook] = &[
    Hook { name: "token", run: zygo_hooks::token::run, reads_stdin: false },
    Hook { name: "statusline", run: zygo_hooks::statusline::run, reads_stdin: true },
    Hook { name: "trim", run: zygo_hooks::trim::run, reads_stdin: true },
    Hook { name: "session-start", run: zygo_hooks::session_start::run, reads_stdin: true },
    Hook { name: "role-guard", run: zygo_hooks::role_guard::run, reads_stdin: true },
    Hook { name: "prompt", run: zygo_hooks::prompt::run, reads_stdin: true },
    Hook { name: "pre-compact", run: zygo_hooks::pre_compact::run, reads_stdin: true },
    Hook { name: "work", run: zygo_work::hooks::run, reads_stdin: true },
    Hook { name: "guard", run: zygo_work::guards::run, reads_stdin: true },
];

pub use zygo_core::hook::error_line;

/// `zygo hook <name>`: always exit 0 (C6).
pub fn run(name: &str, agent: Option<&str>, event: Option<String>) -> i32 {
    let agent = match agent {
        Some(a) => match Agent::from_str(a) {
            Ok(a) => a,
            Err(e) => {
                error_line(&format!("{e:#}"));
                return 0;
            }
        },
        None => std::env::var("ZYGO_AGENT")
            .ok()
            .and_then(|a| Agent::from_str(&a).ok())
            .unwrap_or(Agent::ClaudeCode),
    };
    let Some(hook) = TABLE.iter().find(|h| h.name == name) else {
        error_line(&format!("unknown hook `{name}`"));
        return 0;
    };
    let (out, code) = if hook.reads_stdin {
        let mut stdin = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut stdin);
        guarded(
            &|b| {
                let mut i = HookInput::parse(agent, b)?;
                i.cli_event = event.clone();
                (hook.run)(&i)
            },
            &stdin,
        )
    } else {
        guarded(
            &|_| {
                let mut i = HookInput::empty(agent);
                i.cli_event = event.clone();
                (hook.run)(&i)
            },
            b"",
        )
    };
    let mut so = std::io::stdout();
    let _ = so.write_all(out.as_bytes());
    let _ = so.flush();
    code
}
