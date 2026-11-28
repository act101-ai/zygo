//! The real-hook stub for the launch tests (FX-9 review F2, FX-34): `hook session-start --agent <a> [--event <e>]` with
//! `ZYGO_STUB_HOOK_PATHS=<root>` runs the real hook, `zygo_hooks::session_start::run_member` on stdin, the session
//! folder of `ZYGO_SESSION_DIR` and the paths of the launch tests' layout under `<root>` (`<root>/config.toml`,
//! `cache`, `data`, as `fx` in `tests/opencode.rs`), with the parse and the output of `zygo hook`
//! (`crates/zygo/src/cmd/hook.rs`): the output on stdout and exit 0. A bad or missing `--agent` or `--event` value is
//! one `zygo hook:` line on stderr, no hook run and exit 0, as `zygo hook` writes it; no `--agent` reads `ZYGO_AGENT`.
//! Any other argv, or no `ZYGO_STUB_HOOK_PATHS`, ends it with code 2 and one `stub-hook:` stderr line.
//!
//! It is its own binary so that `stub-agent`, which the `zygo` tests copy into each harness home and scan byte by
//! byte, does not link `zygo_hooks` (FX-34).

use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;

/// The `--agent` and `--event` values, in that order.
type Flags = (Option<String>, Option<String>);

/// The `--agent` and `--event` values of `args` (the tokens after `hook session-start`), each as `--<flag> <value>` or
/// `--<flag>=<value>`, as clap parses them for `zygo hook` (`crates/zygo/src/cmd/mod.rs`, `Cmd::Hook`): a flag with no
/// next token, or a next token that starts with `-`, has no value. `Err` is the first line of the clap error, as
/// `crates/zygo/src/cmd/mod.rs` writes it for `zygo hook`; `None` is a token this stub does not serve.
fn parse(args: &[String]) -> Option<Result<Flags, String>> {
    let (mut agent, mut event) = (None, None);
    let mut i = 0;
    while i < args.len() {
        let (name, inline) = match args[i].split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (args[i].as_str(), None),
        };
        let (slot, shown) = match name {
            "--agent" => (&mut agent, "--agent <AGENT>"),
            "--event" => (&mut event, "--event <EVENT>"),
            _ => return None,
        };
        let value = match inline {
            Some(v) => v,
            None => match args.get(i + 1).filter(|v| !v.starts_with('-')) {
                Some(v) => {
                    i += 1;
                    v.clone()
                }
                None => return Some(Err(format!("error: a value is required for '{shown}' but none was supplied"))),
            },
        };
        *slot = Some(value);
        i += 1;
    }
    Some(Ok((agent, event)))
}

/// `zygo hook session-start` (`crates/zygo/src/cmd/hook.rs` `run`): a bad `--agent` value is one `zygo hook:` line and
/// no hook run; no `--agent` reads `ZYGO_AGENT`, and an unset or bad `ZYGO_AGENT` is Claude Code.
fn real_session_start(agent: Option<&str>, event: Option<String>, root: &Path) {
    let agent = match agent {
        Some(a) => match zygo_core::Agent::from_str(a) {
            Ok(a) => a,
            Err(e) => {
                zygo_core::hook::error_line(&format!("{e:#}"));
                return;
            }
        },
        None => std::env::var("ZYGO_AGENT").ok().and_then(|a| zygo_core::Agent::from_str(&a).ok()).unwrap_or(zygo_core::Agent::ClaudeCode),
    };
    let paths = zygo_core::paths::Paths {
        config_file: root.join("config.toml"),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"),
        marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"),
        approvals_file: root.join("data/approvals.toml"),
        usage_db: root.join("data/usage.db"),
    };
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let (out, _) = zygo_core::hook::guarded(
        &|b| {
            let mut i = zygo_core::hook::HookInput::parse(agent, b)?;
            i.cli_event = event.clone();
            let Some(dir) = zygo_core::session_dir::SessionDir::from_env() else {
                return Ok(zygo_core::hook::HookOutput::None);
            };
            zygo_hooks::session_start::run_member(&i, &dir, &paths, Path::new("/nonexistent/zygo"), &mut std::io::stdout())
        },
        &stdin,
    );
    let mut so = std::io::stdout();
    let _ = so.write_all(out.as_bytes());
    let _ = so.flush();
}

fn main() {
    let argv: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    if argv.first().map(String::as_str) != Some("hook") || argv.get(1).map(String::as_str) != Some("session-start") {
        eprintln!("stub-hook: only `hook session-start` is served, not `{}`", argv.join(" "));
        std::process::exit(2);
    }
    let Some(root) = std::env::var_os("ZYGO_STUB_HOOK_PATHS") else {
        eprintln!("stub-hook: ZYGO_STUB_HOOK_PATHS is not set");
        std::process::exit(2);
    };
    match parse(&argv[2..]) {
        Some(Ok((agent, event))) => real_session_start(agent.as_deref(), event, Path::new(&root)),
        Some(Err(line)) => zygo_core::hook::error_line(&line),
        None => {
            eprintln!("stub-hook: only `--agent` and `--event` are served, not `{}`", argv[2..].join(" "));
            std::process::exit(2);
        }
    }
}
