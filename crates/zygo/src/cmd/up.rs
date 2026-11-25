//! `zygo up [<name>] [--loadout <l>] [--reset] [--no-attach] [--all]` (W§12, W§8).

use std::ffi::OsString;
use std::path::PathBuf;

use zygo_config::{Config, Layer, LoadContext, Merged};
use zygo_core::paths::Paths;
use zygo_workspace::up::{self, UpOpts};

use super::{cwd_or_exit, exe_or_exit, op5_exit, print_err, tmux_program, ExitCode};

#[derive(clap::Args)]
pub struct UpArgs {
    /// Workspace name; default: the repository's workspace
    pub name: Option<String>,
    /// The loadout to start
    #[arg(long)]
    pub loadout: Option<String>,
    /// Start from the loadout, not from the snapshot
    #[arg(long)]
    pub reset: bool,
    /// Do not attach after the start
    #[arg(long)]
    pub no_attach: bool,
    /// Restore every workspace that was up
    #[arg(long)]
    pub all: bool,
}

/// The CLI form: the OP5 check first, then the update check (auto-update design; `y` ends `up` before any library
/// call), then the user config (D3), the folder and the binary (D10), then `up::up` or, with `--all`, `up::up_all`
/// (which refuses a name, `--loadout` and `--reset` itself).
pub fn run(a: &UpArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    if let Some(code) = super::start_check(&[]) {
        return code;
    }
    let paths = Paths::from_env();
    // The user config only (D3, AU10, AU22), kept as `Merged` for `default_loadout_set`.
    let merged = match zygo_config::load(&LoadContext { paths: paths.clone(), repo: None, team: None }) {
        Ok(m) => m,
        Err(e) => {
            print_err(&format!("zygo up: {}\n", zygo_core::esc(&e.to_string())));
            return ExitCode::ConfigInvalid as i32;
        }
    };
    let cwd = match cwd_or_exit() {
        Ok(d) => d,
        Err(code) => return code,
    };
    let zygo = match exe_or_exit() {
        Ok(p) => p,
        Err(code) => return code,
    };
    let opts = UpOpts {
        default_loadout_set: default_loadout_set(&merged),
        ..opts(a, cwd, zygo, paths, merged.config, &|k| std::env::var_os(k))
    };
    if a.all { up::up_all(&opts) } else { up::up(&opts) }
}

/// R§10 `up` row: true when the user config file sets `[workspace] default_loadout` (as `definition_lines`,
/// `cmd/doctor.rs`).
fn default_loadout_set(m: &Merged) -> bool { m.source_of("workspace.default_loadout") == Layer::User }

/// The `UpOpts` of the CLI form; `env` gives `TMUX` and `ZYGO_SESSION` (`std::env::var_os` outside tests).
/// `ZYGO_SESSION` goes to the library as it is, an empty value too (D8): the library counts an empty value as set for
/// each "inside an agent" decision (AU64, AU95; `zygo-workspace/src/env.rs:62`, `up.rs:897`, `up.rs:1094`).
fn opts(a: &UpArgs, cwd: PathBuf, zygo: PathBuf, paths: Paths, config: Config, env: &dyn Fn(&str) -> Option<OsString>) -> UpOpts {
    UpOpts {
        name: a.name.clone(),
        loadout: a.loadout.clone(),
        reset: a.reset,
        no_attach: a.no_attach,
        cwd,
        zygo,
        tmux: tmux_program(),
        tmux_env: env("TMUX"),
        zygo_session: env("ZYGO_SESSION"),
        vars: std::env::vars_os().collect(),
        paths,
        config,
        default_loadout_set: false,
        git: None,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{default_loadout_set, opts, UpArgs};

    /// D8 (M1 review round 3, F3): `zygo up` gives the library `ZYGO_SESSION` as it is. An empty value is `Some("")`:
    /// the library counts it as set for each "inside an agent" decision (AU64, AU95; `zygo-workspace/src/env.rs:62`,
    /// `up.rs:897`, `up.rs:1094`). Kills: `zygo up` filtering an empty value.
    #[test]
    fn zygo_up_passes_zygo_session_as_it_is() {
        let t = tempfile::tempdir().unwrap();
        let a = UpArgs { name: None, loadout: None, reset: false, no_attach: false, all: false };
        fn with(v: &'static str) -> impl Fn(&str) -> Option<OsString> {
            move |k: &str| if k == "ZYGO_SESSION" { Some(OsString::from(v)) } else { None }
        }
        let make = |env: &dyn Fn(&str) -> Option<OsString>| {
            let paths = crate::cmd::compact::tests::paths(t.path());
            opts(&a, t.path().into(), t.path().join("zygo"), paths, zygo_config::Config::default(), env)
        };
        assert_eq!(make(&with("")).zygo_session, Some("".into()));
        assert_eq!(make(&|_: &str| None).zygo_session, None);
        assert_eq!(make(&with("s01")).zygo_session, Some("s01".into()));
    }

    /// R§10 `up` row: `[workspace] default_loadout` chooses only when the user config file sets it. Kills: the
    /// built-in `solo` counting as the user's choice.
    #[test]
    fn default_loadout_set_only_when_the_user_file_sets_it() {
        let t = tempfile::tempdir().unwrap();
        let paths = crate::cmd::compact::tests::paths(t.path());
        let m = || zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None }).unwrap();
        assert!(!default_loadout_set(&m()));
        std::fs::write(&paths.config_file, "[workspace]\ndefault_loadout = \"solo\"\n").unwrap();
        assert!(default_loadout_set(&m()));
    }
}
