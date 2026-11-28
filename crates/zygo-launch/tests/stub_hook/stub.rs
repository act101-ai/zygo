//! The real-hook stub for the launch tests (FX-9 review F2, FX-34): `hook session-start --agent <a> [--event <e>]` with
//! `ZYGO_STUB_HOOK_PATHS=<root>` runs the real hook, `zygo_hooks::session_start::run_member` on stdin, the session
//! folder of `ZYGO_SESSION_DIR` and the paths of the launch tests' layout under `<root>` (`<root>/config.toml`,
//! `cache`, `data`, as `fx` in `tests/opencode.rs`), with the parse and the output of `zygo hook`
//! (`crates/zygo/src/cmd/hook.rs`): the output on stdout and exit 0. Any other argv, or no `ZYGO_STUB_HOOK_PATHS`, ends
//! it with code 2 and one stderr line.
//!
//! It is its own binary so that `stub-agent`, which the `zygo` tests copy into each harness home and scan byte by
//! byte, does not link `zygo_hooks` (FX-34).

use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;

fn flag(argv: &[String], name: &str) -> Option<String> {
    argv.iter().position(|a| a == name).and_then(|i| argv.get(i + 1)).cloned()
}

fn real_session_start(argv: &[String], root: &Path) {
    let agent = flag(argv, "--agent").and_then(|a| zygo_core::Agent::from_str(&a).ok()).unwrap_or(zygo_core::Agent::ClaudeCode);
    let event = flag(argv, "--event");
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
    real_session_start(&argv, Path::new(&root));
}
