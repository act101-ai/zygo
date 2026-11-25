//! `zygo up [<name>] [--loadout <l>] [--reset] [--no-attach] [--all]` (W§12, W§8).

use std::ffi::OsString;
use std::path::PathBuf;

use zygo_config::{Config, Layer, LoadContext, Merged};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_workspace::up::{self, UpOpts};

use super::verbs::VerbCtx;
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
/// call), then the user config (D3) and the team `[limits] max_members` (R§9.4), the folder and the binary (D10), then `up::up` or, with `--all`, `up::up_all`
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
    // R§9.4, R§12: the team layer gives `[limits] max_members` only.
    let verb = VerbCtx { slash: false, save: false, session_dir: SessionDir::from_env(), paths: paths.clone() };
    let (config, lines) = with_team(&verb, merged.config.clone());
    for l in &lines {
        print_err(&format!("{l}\n"));
    }
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
        ..opts(a, cwd, zygo, paths, config, &|k| std::env::var_os(k))
    };
    if a.all { up::up_all(&opts) } else { up::up(&opts) }
}

/// R§10 `up` row: true when the user config file sets `[workspace] default_loadout` (as `definition_lines`,
/// `cmd/doctor.rs`).
fn default_loadout_set(m: &Merged) -> bool { m.source_of("workspace.default_loadout") == Layer::User }

/// R§9.4, R§12: `user` with the team `[limits] max_members`, and the lines `run` prints on stderr: the team-merged
/// warnings, as the launcher prints them (`zygo-launch/src/launcher.rs:306-308`); or, when the team layer cannot be
/// read or the team-merged config does not load, one line, and the user config only (as `zygo doctor`,
/// `cmd/doctor.rs:414-426`): `up` needs no team server.
fn with_team(verb: &VerbCtx, user: Config) -> (Config, Vec<String>) {
    let why = match super::verbs::team(verb, "up") {
        Ok(None) => return (user, vec![]),
        Ok(Some(t)) => match zygo_config::load(&LoadContext { paths: verb.paths.clone(), repo: None, team: Some(t) }) {
            Ok(m) => {
                let lines = m.warnings.iter().map(|w| format!("zygo: {w}")).collect();
                return (with_team_max_members(user, Some(&m.config)), lines);
            }
            Err(e) => e.to_string(),
        },
        Err(e) => {
            let s = e.to_string();
            s.strip_prefix("zygo up: ").unwrap_or(&s).to_string()
        }
    };
    (user, vec![format!("zygo up: team config not read ({}); [limits] max_members from user config", zygo_core::esc(&why))])
}

/// R§9.4, R§12: `user` with `[limits] max_members` from the team-merged config `team` (which has the user layer over
/// the team layer); every other key stays user-only (D3, AU10, AU22).
fn with_team_max_members(mut user: Config, team: Option<&Config>) -> Config {
    if let Some(t) = team {
        user.limits.max_members = t.limits.max_members;
    }
    user
}

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

    use super::{default_loadout_set, opts, with_team, with_team_max_members, UpArgs};

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

    /// R§9.4, R§12: the helper takes `[limits] max_members` from a team-merged config; the user layer wins over the
    /// team layer (`zygo-config/src/layers.rs:27-28`); every other key stays user-only (D3, AU10, AU22). Kills: the
    /// helper taking other team keys or ignoring the team value.
    #[test]
    fn team_max_members_only() {
        let t = tempfile::tempdir().unwrap();
        let paths = crate::cmd::compact::tests::paths(t.path());
        let team: toml::Table = toml::from_str("[limits]\nmax_members = 4\nsubagent_max_turns = 7\n").unwrap();
        let load = |team: Option<toml::Table>| zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team }).unwrap().config;
        let user = load(None);
        let merged = load(Some(team.clone()));
        let c = with_team_max_members(user.clone(), Some(&merged));
        assert_eq!(c.limits.max_members, 4);
        assert_eq!(c.limits.subagent_max_turns, user.limits.subagent_max_turns);
        assert_eq!(with_team_max_members(user.clone(), None).limits.max_members, user.limits.max_members);
        std::fs::write(&paths.config_file, "[limits]\nmax_members = 6\n").unwrap();
        let c = with_team_max_members(load(None), Some(&load(Some(team))));
        assert_eq!(c.limits.max_members, 6);
    }

    /// R§9.4, R§12, R5: the team read of `run`, driven through the session `team.toml` of `verbs::team`. A team
    /// `max_members` is taken, with the team-merged warnings as the launcher prints them; a team layer that cannot be
    /// read or loaded gives the user config and one line (as `zygo doctor`). Kills: `run` loading with `team: None`
    /// only, a refusal on a team error, and dropped team warnings.
    #[test]
    fn team_read_of_run() {
        let t = tempfile::tempdir().unwrap();
        let dir = crate::cmd::verbs::tests::sd(t.path());
        let ctx = crate::cmd::compact::tests::ctx(t.path(), Some(&dir));
        let user = || zygo_config::load(&zygo_config::LoadContext { paths: ctx.paths.clone(), repo: None, team: None }).unwrap().config;
        let default = user().limits.max_members;
        // No team layer: the user config, no line.
        assert_eq!(with_team(&ctx, user()).1, Vec::<String>::new());
        // A team value, and the AU22 warning of a team `[loadout]`.
        std::fs::write(dir.join("team.toml"), "[limits]\nmax_members = 4\n[loadout.x]\n").unwrap();
        let (c, lines) = with_team(&ctx, user());
        assert_eq!(c.limits.max_members, 4);
        assert_eq!(lines, ["zygo: [loadout] and [workspace] are read from user config only; ignored"]);
        // A team layer that cannot be read.
        std::fs::write(dir.join("team.toml"), "[limits\n").unwrap();
        let (c, lines) = with_team(&ctx, user());
        assert_eq!(c.limits.max_members, default);
        assert_eq!(
            lines,
            ["zygo up: team config not read (cannot read team.toml: not valid TOML); [limits] max_members from user config"]
        );
        // A team layer that does not load.
        std::fs::write(dir.join("team.toml"), "[limits]\nmax_members = \"x\"\n").unwrap();
        let (c, lines) = with_team(&ctx, user());
        assert_eq!(c.limits.max_members, default);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("zygo up: team config not read (") && lines[0].ends_with("); [limits] max_members from user config"), "{lines:?}");
    }
}
