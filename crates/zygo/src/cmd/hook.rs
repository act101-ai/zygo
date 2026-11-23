use std::io::{Read, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use zygo_core::hook::{guarded, HookFn, HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_hooks::statusline;

pub struct Hook {
    pub name: &'static str,
    pub run: HookFn,
    /// False for `token` (apiKeyHelper gives no stdin) and for `statusline`, which reads its own stdin with a
    /// bound (AU24).
    pub reads_stdin: bool,
}

pub static TABLE: &[Hook] = &[
    Hook { name: "token", run: zygo_hooks::token::run, reads_stdin: false },
    Hook { name: STATUSLINE, run: statusline_hud, reads_stdin: false },
    Hook { name: "trim", run: zygo_hooks::trim::run, reads_stdin: true },
    Hook { name: "session-start", run: zygo_hooks::session_start::run, reads_stdin: true },
    Hook { name: "role-guard", run: zygo_hooks::role_guard::run, reads_stdin: true },
    Hook { name: "prompt", run: prompt_hook, reads_stdin: true },
    Hook { name: "pre-compact", run: zygo_hooks::pre_compact::run, reads_stdin: true },
    Hook { name: "work", run: zygo_work::hooks::run, reads_stdin: true },
    Hook { name: "guard", run: zygo_work::guards::run, reads_stdin: true },
    Hook { name: "workspace", run: workspace_hook, reads_stdin: true },
];

pub use zygo_core::hook::error_line;

/// The name of the status line hook in `TABLE`.
const STATUSLINE: &str = "statusline";

/// `zygo hook statusline` (H§7): the HUD on stdout, one row or two, with no final newline. Without
/// `ZYGO_SESSION_DIR` it prints nothing (H§6.8). This is the one place that reads the real environment, the real
/// stdin and the terminal for the HUD. The style comes from `overrides.json`, then `view.json` (AU1).
fn statusline_hud(_input: &HookInput) -> anyhow::Result<HookOutput> {
    let start = Instant::now();
    let env = |key: &str| std::env::var_os(key);
    let Some(root) = env("ZYGO_SESSION_DIR").filter(|v| !v.is_empty()).map(PathBuf::from) else {
        return Ok(HookOutput::None);
    };
    let dir = SessionDir::new(root);
    let style = statusline::style(&Overrides::load(&dir), SessionView::load(&dir).ok().as_ref());
    let render_env = statusline::render::RenderEnv::from_env(&env, style);
    let (source, width) = statusline::data::width_source(&env, &statusline::width::tty_columns);
    statusline::data::log_width(&dir, source, width);
    let data = statusline::data::collect(
        Box::new(std::io::stdin()),
        &dir.root,
        &Paths::from_env(),
        &env,
        start + statusline::BOUND,
    );
    Ok(HookOutput::Text(statusline::render::render(&data, &render_env)))
}

/// `zygo hook prompt` (§17.1, W§9.1): the registered dispatcher, the paths and the real environment for
/// `prompt::run_in_env`, which reads `ZYGO_WORKSPACE` only after a verb panicked (H1 attempt-2 review round 4, T2).
fn prompt_hook(input: &HookInput) -> anyhow::Result<HookOutput> {
    zygo_hooks::prompt::run_in_env(
        input,
        zygo_core::hook::slash_dispatch(),
        zygo_hooks::prompt::HOOK_DEADLINE,
        &Paths::from_env(),
        &|k| std::env::var_os(k),
    )
}

/// `zygo hook workspace --event <event>` (workspaces design W§9.1, H1 D18): the one place that reads the real
/// environment, the zygo path and the paths for the member hooks; `workspace::run` takes them as inputs.
fn workspace_hook(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(event) = input.cli_event.as_deref() else { anyhow::bail!("workspace: --event is required") };
    let out = zygo_hooks::workspace::run(
        event,
        &serde_json::to_vec(&input.raw)?,
        &Paths::from_env(),
        &|k| std::env::var_os(k),
        &std::env::current_exe().unwrap_or_default(),
    )?;
    Ok(match out {
        Some(text) => HookOutput::Text(text),
        None => HookOutput::None,
    })
}

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
    if hook.name == STATUSLINE {
        // The workers of the status line can still run after `guarded` restores the panic hook, so the silent
        // hook is set for the whole process first; a worker panic then writes only its own line (H§7.7, C6).
        std::panic::set_hook(Box::new(|_| {}));
    }
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
