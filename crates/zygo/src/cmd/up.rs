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
/// call), then the user config (D3) and the team `[limits] max_members` (R§9.4) with its 1-32 range (exit 1, FX-17), the folder and the binary (D10), then `up::up` or, with `--all`, `up::up_all`
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
    let (config, lines) = with_team(&verb, &merged, super::verbs::team(&verb, "up"));
    for l in &lines {
        print_err(&format!("{l}\n"));
    }
    if let Some(line) = max_members_line(&config) {
        print_err(&format!("{line}\n"));
        return 1;
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

/// R§9.4 bullet 1, FX-17: the `config check` rule `limits.max_members` (`zygo-config/src/check.rs:119-120`) on the
/// `max_members` that `up` uses (user layer, or team-merged as `with_team` gives it), as `check_jobs` applies the
/// `job` and `pool` rules (`zygo-workspace/src/up.rs:797-808`); `Some` is the stderr line. `zygo_config::load` does
/// not check the range.
fn max_members_line(config: &Config) -> Option<String> {
    let mut one = Config::default();
    one.limits.max_members = config.limits.max_members;
    zygo_config::check(&one).into_iter().find(|v| v.path == "limits.max_members").map(|v| format!("zygo: {v}; nothing started"))
}

/// R§9.4, R§12: the user config of `user` with the team `[limits] max_members` of the team read `team`, and the lines
/// `run` prints on stderr: the warnings the team layer causes, as the launcher prints them
/// (`zygo-launch/src/launcher.rs:306-308`): the team-merged warnings that `user` does not have, less `MODEL_IGNORED`
/// (a user-layer warning; `zygo hook` drops it the same way, `zygo-work/src/hooks.rs:74-76`); or, when the team layer
/// cannot be read or the team-merged config does not load, one line, and the user config only (as `zygo doctor`,
/// `cmd/doctor.rs:414-426`): `up` needs no team server.
fn with_team(verb: &VerbCtx, user: &Merged, team: anyhow::Result<Option<toml::Table>>) -> (Config, Vec<String>) {
    let why = match team {
        Ok(None) => return (user.config.clone(), vec![]),
        Ok(Some(t)) => match zygo_config::load(&LoadContext { paths: verb.paths.clone(), repo: None, team: Some(t) }) {
            Ok(m) => {
                let lines = m
                    .warnings
                    .iter()
                    .filter(|w| w.as_str() != zygo_config::MODEL_IGNORED && !user.warnings.contains(w))
                    .map(|w| format!("zygo: {w}"))
                    .collect();
                return (with_team_max_members(user.config.clone(), Some(&m.config)), lines);
            }
            Err(e) => e.to_string(),
        },
        Err(e) => {
            let s = e.to_string();
            s.strip_prefix("zygo up: ").unwrap_or(&s).to_string()
        }
    };
    (user.config.clone(), vec![format!("zygo up: team config not read ({}); [limits] max_members from user config", zygo_core::esc(&why))])
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
#[path = "../../tests/dead_port/mod.rs"]
mod dead_port;

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{default_loadout_set, max_members_line, opts, with_team, with_team_max_members, UpArgs};

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

    /// R§9.4 bullet 1, FX-17: the line is the `config check` rule `limits.max_members` (`zygo-config/src/check.rs:119-120`)
    /// with `; nothing started`, for a value outside 1-32 only; the built-in default 8 gives none. Kills: an off-by-one
    /// range and a line that differs from `config check`.
    #[test]
    fn max_members_line_is_the_config_check_rule() {
        for v in [0, 33, u32::MAX] {
            let mut c = zygo_config::Config::default();
            c.limits.max_members = v;
            assert_eq!(max_members_line(&c).as_deref(), Some("zygo: limits.max_members: must be 1-32; nothing started"), "{v}");
        }
        for v in [1, 8, 32] {
            let mut c = zygo_config::Config::default();
            c.limits.max_members = v;
            assert_eq!(max_members_line(&c), None, "{v}");
        }
        assert_eq!(zygo_config::Config::default().limits.max_members, 8);
        assert_eq!(max_members_line(&zygo_config::Config::default()), None);
    }

    /// R§9.4 bullet 1, FX-17: the check takes the value `with_team` gives (the user layer over the team layer,
    /// `zygo-config/src/layers.rs:27-28`). Kills: a check on the user layer only, or on the team value instead of the
    /// merged value.
    #[test]
    fn max_members_line_of_the_used_value() {
        let t = tempfile::tempdir().unwrap();
        let dir = crate::cmd::verbs::tests::sd(t.path());
        let ctx = crate::cmd::compact::tests::ctx(t.path(), Some(&dir));
        let user = || zygo_config::load(&zygo_config::LoadContext { paths: ctx.paths.clone(), repo: None, team: None }).unwrap();
        let used = || with_team(&ctx, &user(), crate::cmd::verbs::team(&ctx, "up")).0;
        std::fs::write(&ctx.paths.config_file, "").unwrap();
        assert_eq!(used().limits.max_members, 8);
        assert!(max_members_line(&used()).is_none(), "no layer");
        let cases: [(&str, &str, bool); 5] = [
            ("", "[limits]\nmax_members = 0\n", true),
            ("", "[limits]\nmax_members = 33\n", true),
            ("", "[limits]\nmax_members = 1\n", false),
            ("[limits]\nmax_members = 32\n", "[limits]\nmax_members = 0\n", false),
            ("[limits]\nmax_members = 0\n", "[limits]\nmax_members = 5\n", true),
        ];
        for (u, team, want) in cases {
            std::fs::write(&ctx.paths.config_file, u).unwrap();
            std::fs::write(dir.join("team.toml"), team).unwrap();
            assert_eq!(max_members_line(&used()).is_some(), want, "user {u:?}, team {team:?}");
        }
    }

    /// R§9.4, R§12, R5: `with_team` through the session `team.toml` of `verbs::team`: a team `max_members` is taken
    /// with the team-layer warning; a team layer that cannot be read or loaded gives the user config and one line (as
    /// `zygo doctor`). Kills: a refusal on a team error and a dropped team warning. That `run` calls `with_team` is
    /// proven by `up_reads_the_team_max_members_of_the_session` (`crates/zygo/tests/workspace_restore.rs`).
    #[test]
    fn team_read_of_run() {
        let t = tempfile::tempdir().unwrap();
        let dir = crate::cmd::verbs::tests::sd(t.path());
        let ctx = crate::cmd::compact::tests::ctx(t.path(), Some(&dir));
        let user = || zygo_config::load(&zygo_config::LoadContext { paths: ctx.paths.clone(), repo: None, team: None }).unwrap();
        let default = user().config.limits.max_members;
        // No team layer: the user config, no line.
        assert_eq!(with_team(&ctx, &user(), crate::cmd::verbs::team(&ctx, "up")).1, Vec::<String>::new());
        // A team value, and the AU22 warning of a team `[loadout]`.
        std::fs::write(dir.join("team.toml"), "[limits]\nmax_members = 4\n[loadout.x]\n").unwrap();
        let (c, lines) = with_team(&ctx, &user(), crate::cmd::verbs::team(&ctx, "up"));
        assert_eq!(c.limits.max_members, 4);
        assert_eq!(lines, ["zygo: [loadout] and [workspace] are read from user config only; ignored"]);
        // A team layer that cannot be read.
        std::fs::write(dir.join("team.toml"), "[limits\n").unwrap();
        let (c, lines) = with_team(&ctx, &user(), crate::cmd::verbs::team(&ctx, "up"));
        assert_eq!(c.limits.max_members, default);
        assert_eq!(
            lines,
            ["zygo up: team config not read (cannot read team.toml: not valid TOML); [limits] max_members from user config"]
        );
        // A team layer that does not load.
        std::fs::write(dir.join("team.toml"), "[limits]\nmax_members = \"x\"\n").unwrap();
        let (c, lines) = with_team(&ctx, &user(), crate::cmd::verbs::team(&ctx, "up"));
        assert_eq!(c.limits.max_members, default);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("zygo up: team config not read (") && lines[0].ends_with("); [limits] max_members from user config"), "{lines:?}");
    }

    /// R§9.4, FX-14 (2): `up` prints the warnings the team layer causes, not the user-layer ones (`layers.rs:330-332`,
    /// `MODEL_IGNORED` `layers.rs:499`). Kills: printing every team-merged warning.
    #[test]
    fn team_read_prints_only_team_layer_warnings() {
        let t = tempfile::tempdir().unwrap();
        let dir = crate::cmd::verbs::tests::sd(t.path());
        let ctx = crate::cmd::compact::tests::ctx(t.path(), Some(&dir));
        let model = "[[model]]\nid = \"m1\"\n[model.facts]\nreasoning = \"always\"\neffort_levels = [\"low\", \"high\"]\ncontext_window = 200000\nmax_output_tokens = 64000\n\
                     [[model.provider]]\nname = \"p\"\nbase_url = \"https://p.example.invalid\"\nupstream_model = \"u\"\ndialects = [\"anthropic\"]\nkey = \"env:ZYGO_FX14_KEY\"\n";
        std::fs::write(&ctx.paths.config_file, format!("[[marketplace]]\nname = \"act101\"\nsource = \"github:other/x\"\n{model}")).unwrap();
        std::fs::write(dir.join("team.toml"), "[limits]\nmax_members = 4\n[loadout.x]\n[statusline]\nstyle = \"x\"\n").unwrap();
        let user = zygo_config::load(&zygo_config::LoadContext { paths: ctx.paths.clone(), repo: None, team: None }).unwrap();
        assert_eq!(user.warnings, ["config: user marketplace[0]: a user entry cannot change the built-in source; ignored"]);
        let (c, lines) = with_team(&ctx, &user, crate::cmd::verbs::team(&ctx, "up"));
        assert_eq!(c.limits.max_members, 4);
        assert_eq!(lines, [
            "zygo: config: [statusline] in team config ignored; only user config may set them",
            "zygo: [loadout] and [workspace] are read from user config only; ignored",
        ]);
    }

    /// R§9.4, FX-14 (4): the server branch of `verbs::team` (`verbs.rs:118-127`) as `up` uses it: the user
    /// `max_members`, and one line or none. Kills: a team error that refuses, drops the line or loses the user value.
    #[test]
    fn team_read_of_the_server_branch() {
        const TAIL: &str = "); [limits] max_members from user config";
        let t = tempfile::tempdir().unwrap();
        let ctx = crate::cmd::compact::tests::ctx(t.path(), None);
        let user = |cfg: &str| {
            std::fs::write(&ctx.paths.config_file, format!("[limits]\nmax_members = 3\n{cfg}")).unwrap();
            zygo_config::load(&zygo_config::LoadContext { paths: ctx.paths.clone(), repo: None, team: None }).unwrap()
        };
        // A file-only store: no OS store (Decision 3).
        let keys = ctx.paths.data_dir.join("keys.toml");
        let store = || -> Box<dyn zygo_config::keys::SecretStore> {
            Box::new(zygo_config::keys::Auto::new(None, zygo_config::keys::FileStore::new(keys.clone())))
        };
        let read = |u: &zygo_config::Merged| with_team(&ctx, u, crate::cmd::verbs::team_with(&ctx, "up", &store));
        // `from_config`: no server, no team layer, no line.
        let (c, lines) = read(&user(""));
        assert_eq!((c.limits.max_members, lines), (3, vec![]));
        // `from_config` refused: no token.
        let closed = super::dead_port::closed_url();
        let (c, lines) = read(&user(&format!("[server]\nurl = \"{closed}\"\n")));
        assert_eq!(c.limits.max_members, 3);
        assert_eq!(lines, [format!("zygo up: team config not read (serve mode needs a team token: [server] token is not set or names an empty value{TAIL}")]);
        // Server unreachable: the closed port, token `keychain:fx14` from the keys file (Decision 3).
        std::fs::create_dir_all(&ctx.paths.data_dir).unwrap();
        std::fs::write(ctx.paths.data_dir.join("keys.toml"), "fx14 = \"t\"\n").unwrap();
        let (c, lines) = read(&user(&format!("[server]\nurl = \"{closed}\"\ntoken = \"keychain:fx14\"\n")));
        assert_eq!((c.limits.max_members, lines.len()), (3, 1), "{lines:?}");
        assert!(lines[0].starts_with("zygo up: team config not read (server unreachable: ") && lines[0].ends_with(TAIL), "{lines:?}");
        // Team config refused: a loopback server that answers 401.
        let body = serde_json::json!({ "error": { "type": "bad_token", "message": "no" } }).to_string();
        let sd = crate::cmd::compact::tests::canned_session(t.path(), "S1", 401, body);
        let url = std::fs::read_to_string(zygo_core::session_dir::SessionDir::new(sd).url()).unwrap();
        let (c, lines) = read(&user(&format!("[server]\nurl = \"{url}\"\ntoken = \"keychain:fx14\"\n")));
        assert_eq!(c.limits.max_members, 3);
        assert_eq!(lines, [format!("zygo up: team config not read (team config refused: bad_token: no{TAIL}")]);
        // A `base` load that fails (Decision 5).
        let u = user("");
        std::fs::write(&ctx.paths.config_file, "[limits\n").unwrap();
        let (c, lines) = read(&u);
        assert_eq!((c.limits.max_members, lines.len()), (3, 1), "{lines:?}");
        assert!(lines[0].starts_with("zygo up: team config not read (user config ") && lines[0].ends_with(TAIL), "{lines:?}");
    }

    /// FX-14 R1: `verbs::team_with` builds the secret store only after `base` loads and only when `[server] url` is
    /// set (the store is read only then, `zygo-launch/src/serve_client.rs:304`). A factory that records its calls
    /// stands in for `default_store`; no OS store. Kills: a store (an OS keychain probe) built before `base` or with no
    /// server.
    #[test]
    fn team_read_builds_the_store_only_after_base_and_with_a_server() {
        let t = tempfile::tempdir().unwrap();
        let ctx = crate::cmd::compact::tests::ctx(t.path(), None);
        let called = std::cell::Cell::new(0);
        let keys = ctx.paths.data_dir.join("keys.toml");
        let store = || -> Box<dyn zygo_config::keys::SecretStore> {
            called.set(called.get() + 1);
            Box::new(zygo_config::keys::Auto::new(None, zygo_config::keys::FileStore::new(keys.clone())))
        };
        // `base` fails: no store.
        std::fs::write(&ctx.paths.config_file, "[limits\n").unwrap();
        assert!(crate::cmd::verbs::team_with(&ctx, "up", &store).is_err());
        assert_eq!(called.get(), 0);
        // No `[server]`: no team layer, no store.
        std::fs::write(&ctx.paths.config_file, "").unwrap();
        assert!(crate::cmd::verbs::team_with(&ctx, "up", &store).unwrap().is_none());
        assert_eq!(called.get(), 0);
        // A server: the store is built once.
        let closed = super::dead_port::closed_url();
        std::fs::write(&ctx.paths.config_file, format!("[server]\nurl = \"{closed}\"\n")).unwrap();
        assert!(crate::cmd::verbs::team_with(&ctx, "up", &store).is_err());
        assert_eq!(called.get(), 1);
    }
}
