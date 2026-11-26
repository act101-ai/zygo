use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use std::time::SystemTime;
use zygo_core::exit::ExitCode;
use zygo_core::hook::set_slash_dispatch;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_proxy::client::ClientError;

pub mod attach;
pub mod claude;
pub mod compact;
pub mod compact_tool;
pub mod config;
pub mod decision;
pub mod doctor;
pub mod down;
pub mod guards;
pub mod help;
pub mod hook;
pub mod keys;
pub mod loadout;
pub mod ls;
pub mod mail;
pub mod marketplace;
pub mod mcp;
pub mod member;
pub mod opencode;
pub mod plugin;
pub mod plugins;
pub mod profile;
pub mod redaction;
pub mod results;
pub mod serve;
pub mod status;
pub mod statusline;
pub mod units;
pub mod up;
pub mod update;
pub mod usage;
pub mod verbs;
pub mod work;
pub mod workspace;
pub mod ws;

pub use zygo_config::display::EscapedLines;
use verbs::{VerbCtx, VERBS};

/// Writes `text` to stdout and flushes. A closed reader (`BrokenPipe`) ends the write silently; any other error is one
/// stderr line. Never panics and never changes the exit status (`print!` panics on any write error).
pub fn print_out(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_all(text.as_bytes()).and_then(|()| out.flush())
        && e.kind() != std::io::ErrorKind::BrokenPipe {
            let _ = writeln!(std::io::stderr(), "zygo: cannot write to stdout: {e}");
        }
}

/// Writes `text` to stderr and flushes. Every write error is ignored; never panics and never changes the exit status
/// (`eprint!` panics on any write error).
pub fn print_err(text: &str) {
    use std::io::Write;
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(text.as_bytes()).and_then(|()| err.flush());
}

/// A verb error caused by wrong usage; the process exits 2 (§8.2) instead of 1.
#[derive(Debug)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// The OP5 text: native Windows has no workspace features (W§8.1 step 1).
pub(crate) const OP5: &str = "workspaces need tmux; run zygo in WSL";

/// `Err(UsageError(OP5))` on Windows, else `Ok`. The first call of every workspace verb, before any library call.
pub(crate) fn op5() -> Result<(), UsageError> {
    if cfg!(windows) { Err(UsageError(OP5.to_string())) } else { Ok(()) }
}

/// `op5` for a CLI arm: on `Err`, prints `<OP5>\n` on stderr and gives `Some(2)`.
pub(crate) fn op5_exit() -> Option<i32> {
    match op5() {
        Ok(()) => None,
        Err(e) => {
            print_err(&format!("{e}\n"));
            Some(ExitCode::Usage as i32)
        }
    }
}

/// The first `tmux` in the absolute entries of `PATH` (D2): the rule of the doctor probes.
pub(crate) fn find_tmux() -> Option<PathBuf> {
    let entries = doctor::absolute_entries(std::env::var_os("PATH").as_deref())?;
    which::which_in("tmux", Some(entries), std::env::current_dir().unwrap_or_default()).ok()
}

/// An absolute path that cannot exist (`/dev/null` is not a folder): the `tmux` when `find_tmux` finds none (D2).
pub(crate) const NO_TMUX_PATH: &str = "/dev/null/tmux";

/// `find_tmux()`, else `PathBuf::from(NO_TMUX_PATH)` (D2). Every workspace verb uses it; never a bare `tmux`.
pub(crate) fn tmux_program() -> PathBuf {
    find_tmux().unwrap_or_else(|| PathBuf::from(NO_TMUX_PATH))
}

/// A usable session id (D8): `ZYGO_SESSION` as `env` gives it (`std::env::var_os` outside tests), `None` when it is
/// unset or empty. Its users are the two reads that need a session id: `/zygo:down` (`down::session_of`) and the
/// `zygo down` CLI form (`down::cli_opts`). It does not decide "inside an agent": that rule counts an empty value as
/// set (AU64, AU95; `zygo-workspace/src/env.rs:62`, `up.rs:897`, `up.rs:1094`), so `zygo up` passes
/// `ZYGO_SESSION` as it is.
pub(crate) fn usable_session_id(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<std::ffi::OsString> {
    env("ZYGO_SESSION").filter(|v| !v.is_empty())
}

/// The user config only (D3, AU10, AU22): no repository and no team layer.
pub(crate) fn user_config(paths: &Paths) -> Result<zygo_config::Config, zygo_config::ConfigError> {
    zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None }).map(|m| m.config)
}

/// `user_config` for a CLI arm (D3): a load error prints `zygo <verb>: <error escaped>` and gives exit 11.
pub(crate) fn user_config_or_exit(paths: &Paths, verb: &str) -> Result<zygo_config::Config, i32> {
    user_config(paths).map_err(|e| {
        print_err(&format!("zygo {verb}: {}\n", zygo_core::esc(&e.to_string())));
        ExitCode::ConfigInvalid as i32
    })
}

/// The D10 text of a failed `std::env::current_dir()`, without the `zygo: ` prefix.
pub(crate) fn cwd_error(e: &std::io::Error) -> String {
    format!("cannot read the current folder: {}", zygo_core::esc(&e.to_string()))
}

/// The D10 text of a failed `std::env::current_exe()`, without the `zygo: ` prefix.
pub(crate) fn exe_error(e: &std::io::Error) -> String {
    format!("cannot find the zygo binary: {}", zygo_core::esc(&e.to_string()))
}

/// `std::env::current_dir()` for a CLI arm (D10): on error, prints `zygo: <cwd_error>` and gives exit 1.
pub(crate) fn cwd_or_exit() -> Result<PathBuf, i32> {
    std::env::current_dir().map_err(|e| {
        print_err(&format!("zygo: {}\n", cwd_error(&e)));
        1
    })
}

/// `std::env::current_exe()` for a CLI arm (D10): on error, prints `zygo: <exe_error>` and gives exit 1.
pub(crate) fn exe_or_exit() -> Result<PathBuf, i32> {
    std::env::current_exe().map_err(|e| {
        print_err(&format!("zygo: {}\n", exe_error(&e)));
        1
    })
}

/// The `VerbCtx` of a CLI form.
pub(crate) fn cli_ctx() -> VerbCtx {
    VerbCtx { slash: false, save: false, session_dir: SessionDir::from_env(), paths: Paths::from_env() }
}

/// The exit of a verb result: its text on stdout, or its error on stderr with exit 2 for a `UsageError`, the server
/// code for an unreachable server, else 1.
pub(crate) fn finish(r: anyhow::Result<String>) -> i32 {
    match r {
        Ok(out) => {
            if !out.is_empty() {
                print_out(&format!("{out}\n"));
            }
            0
        }
        Err(e) => {
            print_err(&format!("{}\n", zygo_config::display::error_text(&e)));
            if e.downcast_ref::<compact::CompactFailed>().is_some_and(|c| matches!(c.0, ClientError::Unreachable(_)))
                || e.downcast_ref::<zygo_launch::serve_client::Unreachable>().is_some()
            {
                ExitCode::ServerUnreachable as i32
            } else if e.downcast_ref::<UsageError>().is_some() {
                2
            } else {
                1
            }
        }
    }
}

#[derive(Parser)]
#[command(name = "zygo", version, about = "Wraps Claude Code and OpenCode", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

/// Trailing words and `--save` for a verb; the handler gets the raw argv.
#[derive(Args)]
struct VerbArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
    /// Also write the change to user config
    #[arg(long)]
    save: bool,
}

#[derive(Args)]
pub(crate) struct Launch {
    /// Model profile
    #[arg(long)]
    pub(crate) profile: Option<String>,
    /// Use the passthrough credential
    #[arg(long, hide = false)]
    pub(crate) passthrough: bool,
    /// Server URL
    #[arg(long)]
    pub(crate) server: Option<String>,
    /// Compaction window in tokens, or off
    #[arg(long)]
    pub(crate) compact: Option<String>,
    /// Skip permission prompts
    #[arg(long)]
    pub(crate) yolo: bool,
    /// Claude Code config directory (sets CLAUDE_CONFIG_DIR)
    #[arg(long)]
    pub(crate) config_dir: Option<String>,
    /// Member name in a workspace pane (M§6)
    #[arg(long)]
    pub(crate) name: Option<String>,
    /// Arguments passed to the agent
    #[arg(last = true)]
    pub(crate) rest: Vec<String>,
}

#[derive(Args)]
pub(crate) struct LaunchOpenCode {
    #[arg(long)]
    pub(crate) profile: Option<String>,
    #[arg(long)]
    pub(crate) server: Option<String>,
    #[arg(long)]
    pub(crate) compact: Option<String>,
    #[arg(long)]
    pub(crate) yolo: bool,
    #[arg(long)]
    pub(crate) name: Option<String>,
    #[arg(last = true)]
    pub(crate) rest: Vec<String>,
}

/// The `--compact` error line shared by the launch subcommands (exit 2).
pub(crate) fn bad_compact(agent: &str, v: &str) -> String {
    format!("zygo {agent}: bad --compact value `{v}`: use <tokens>, <n>k or <n>m from 100k to 10m, or off")
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch Claude Code
    Claude(Launch),
    /// Launch OpenCode
    Opencode(LaunchOpenCode),
    /// Serve mode
    Serve {
        #[arg(long)]
        listen: Option<String>,
        #[arg(long)]
        tls_cert: Option<String>,
        #[arg(long)]
        tls_key: Option<String>,
        #[command(subcommand)]
        sub: Option<ServeCmd>,
    },
    /// Mode, server, profile, work item, repo policy, session status
    Status,
    /// Check agents, keys, server and plugins
    Doctor,
    /// Set the compaction window: <tokens|on|off>
    Compact {
        arg: String,
        /// Session id; default `ZYGO_SESSION`
        #[arg(long)]
        session: Option<String>,
    },
    /// Model profiles: [<name>|none], list, show <name>, use <name>
    #[command(args_conflicts_with_subcommands = true)]
    Profile {
        /// Profile name, or `none`
        name: Option<String>,
        #[command(subcommand)]
        sub: Option<ProfileCmd>,
    },
    /// Current work item and report
    #[command(args_conflicts_with_subcommands = true)]
    Work {
        /// Work id, or `clear`
        id: Option<String>,
        #[command(subcommand)]
        sub: Option<WorkCmd>,
    },
    /// Usage and savings
    Usage {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        sync: bool,
    },
    /// Plugin marketplaces
    Marketplace {
        #[command(subcommand)]
        sub: MarketplaceCmd,
    },
    /// Plugins
    Plugins {
        #[command(subcommand)]
        sub: Option<PluginsCmd>,
    },
    /// Plugin tools
    Plugin {
        #[command(subcommand)]
        sub: PluginCmd,
    },
    /// Provider keys
    Keys {
        #[command(subcommand)]
        sub: KeysCmd,
    },
    /// Configuration
    Config {
        #[command(subcommand)]
        sub: ConfigCmd,
    },
    /// Replace the binary from the latest release
    Update {
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        check: bool,
    },
    /// Every verb, or one verb's details
    Help { verb: Option<String> },
    /// Result trimming: on|off|threshold <bytes>
    Results(VerbArgs),
    /// Redaction passes: [content on|off]
    Redaction(VerbArgs),
    /// Guard levels: [<guard> <level>]
    Guards(VerbArgs),
    /// Work units: on|off
    Units(VerbArgs),
    /// The compact MCP tool: on|off
    CompactTool(VerbArgs),
    /// Status line style: [compact|verbose]
    Statusline(VerbArgs),
    /// Start or restore the workspace of this repository
    Up(up::UpArgs),
    /// Stop the workspace of this folder or of <name>: drain, snapshot, stop; in the foreground, detached when run from an agent or from a pane of that workspace
    Down(down::DownArgs),
    /// List workspaces
    Ls,
    /// Attach to a running workspace
    Attach(attach::AttachArgs),
    /// Loadouts and their source
    Loadout(loadout::LoadoutArgs),
    /// Decision records: new, ls, show, deps, check
    // `-h`/`--help` reach `decision::run_in` in any position, which prints the usage line.
    #[command(disable_help_flag = true)]
    Decision {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Status of this session's or this folder's workspace
    Workspace,
    /// Workspace tools: forget, forget-pane
    Ws(ws::WsArgs),
    /// Workspace mail: send, read, ls, watch, sync
    Mail(mail::MailArgs),
    /// Internal: MCP server started by the agents
    Mcp {
        #[arg(long)]
        agent: Option<Agent>,
    },
    /// Internal: hook entry started by the agents
    Hook {
        name: String,
        /// claude-code or opencode
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        event: Option<String>,
    },
}

#[derive(Subcommand)]
enum ServeCmd {
    /// Manage serve tokens
    Token {
        #[command(subcommand)]
        sub: TokenCmd,
    },
    /// Reload the server config
    Reload,
}

#[derive(Subcommand)]
enum TokenCmd {
    /// Create a token
    New { label: String },
}

#[derive(Subcommand)]
enum ProfileCmd {
    List,
    Show { name: String },
    Use { name: String },
}

#[derive(Subcommand)]
enum WorkCmd {
    /// Work report
    Report {
        unit: Option<String>,
        #[arg(long)]
        since: Option<String>,
    },
}

#[derive(Subcommand)]
enum MarketplaceCmd {
    Add {
        source: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "ref")]
        git_ref: Option<String>,
        #[arg(long)]
        enable: Option<String>,
    },
    List,
    Update {
        name: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        accept: bool,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
enum PluginsCmd {
    List,
    Sync,
    Enable {
        spec: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    Disable { spec: Option<String> },
    Approve {
        spec: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum PluginCmd {
    /// Check a plugin directory
    Check { dir: String },
}

#[derive(Subcommand)]
enum KeysCmd {
    Set { provider: String },
    Rm { provider: String },
    List { provider: Option<String> },
}

#[derive(Subcommand)]
enum ConfigCmd {
    Show {
        #[arg(long)]
        layer: Option<String>,
    },
    Check,
}

/// Splits a verb's raw tokens into (`--save` seen, handler args). `--save` is removed;
/// the first `--` ends option handling: it is dropped and every later token passes through.
fn verb_tokens(raw: Vec<String>) -> (bool, Vec<String>) {
    let mut save = false;
    let mut options = true;
    let mut args = Vec::new();
    for t in raw {
        if options && t == "--" {
            options = false;
        } else if options && t == "--save" {
            save = true;
        } else {
            args.push(t);
        }
    }
    (save, args)
}

/// Runs a verb: `args` are the raw tokens after the verb, split by `verb_tokens` (not for `decision`).
fn run_verb(name: &str, raw: Vec<String>, slash: bool) -> anyhow::Result<String> {
    let verb = VERBS
        .iter()
        .find(|v| v.spec.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown verb `{name}`; try help"))?;
    // D§10.1: the `decision` reply carries `<args>` as given, so its tokens are not split.
    let (save, args) = if name == "decision" { (false, raw) } else { verb_tokens(raw) };
    let ctx = VerbCtx { slash, save, session_dir: SessionDir::from_env(), paths: Paths::from_env() };
    (verb.handler)(&ctx, &args)
}

fn slash(verb: &str, args: &str) -> anyhow::Result<String> {
    run_verb(verb, args.split_whitespace().map(str::to_owned).collect(), true)
}

/// The update check before an agent launch (auto-update design), with the process's own environment, binary and
/// terminal. `rest` holds the arguments passed to the agent. `Some(code)`: exit with `code` and do not launch.
/// A user config that does not load gives no check: the launch reports the error.
fn start_check(rest: &[String]) -> Option<i32> {
    let env = |k: &str| std::env::var_os(k);
    #[cfg(feature = "test-hooks")]
    let assume_tty = env("ZYGO_TEST_ASSUME_TTY").is_some_and(|v| v == "1");
    #[cfg(not(feature = "test-hooks"))]
    let assume_tty = false;
    let mut io = update::Terminal::new(assume_tty);
    let paths = Paths::from_env();
    // `[update]` comes from user config only: no repository and no team layer.
    let cfg = zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None }).ok()?.config;
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).ok()?;
    let args: Vec<std::ffi::OsString> = rest.iter().map(std::ffi::OsString::from).collect();
    match update::start_check(&paths, &cfg, &args, &env, &exe, &mut io) {
        update::StartOutcome::Continue => None,
        update::StartOutcome::Exit { code } => Some(code),
    }
}

/// Entry point of the binary; returns the process exit code.
pub fn run() -> i32 {
    set_slash_dispatch(slash);
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;
            let is_hook = std::env::args_os().nth(1).is_some_and(|a| a == "hook");
            if is_hook && !matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                // C6: a hook invocation never fails the agent.
                hook::error_line(e.to_string().lines().next().unwrap_or("invalid arguments"));
                return 0;
            }
            e.exit()
        }
    };
    // §10.1 and its N71 note: results of ended sessions are swept at the next `zygo` command, except hooks,
    // `zygo mcp` (time-bounded; C6) and `zygo ws` (AU71). I/O errors are ignored; the mail sweep (M§5) runs under the same rule.
    if !matches!(cli.command, Cmd::Hook { .. } | Cmd::Mcp { .. } | Cmd::Ws(_)) {
        let p = Paths::from_env();
        zygo_results::sweep(&p.cache_dir, &p.workspace_cache_dir(), SystemTime::now());
        // OP5: native Windows has no workspace features, so it has no mail sweep (the cfg of `op5`).
        if !cfg!(windows) {
            mail::sweep(&p, SystemTime::now());
        }
    }
    let command = match cli.command {
        Cmd::Ws(a) => return ws::dispatch(a),
        Cmd::Mail(a) => return mail::run(a),
        c => c,
    };
    let verb = match &command {
        Cmd::Claude(l) => return start_check(&l.rest).unwrap_or_else(|| claude::run(l)),
        Cmd::Opencode(a) => return start_check(&a.rest).unwrap_or_else(|| opencode::run(a)),
        Cmd::Serve { .. } => return serve::run(),
        Cmd::Doctor => return doctor::run(),
        Cmd::Plugin { sub: PluginCmd::Check { dir } } => return plugin::run(dir),
        Cmd::Keys { .. } => return keys::run(),
        Cmd::Config { .. } => return config::run(),
        Cmd::Update { version, check } => return update::run(version.as_deref(), *check),
        Cmd::Mcp { agent } => return mcp::run(*agent),
        Cmd::Hook { name, agent, event } => return hook::run(name, agent.as_deref(), event.clone()),
        Cmd::Up(a) => return up::run(a),
        Cmd::Down(a) => return down::run(a),
        Cmd::Ls => return finish(ls::verb(&cli_ctx(), &[])),
        Cmd::Attach(a) => return attach::run(a),
        Cmd::Loadout(a) => return loadout::run(a),
        Cmd::Decision { args } => return decision::run(args),
        Cmd::Workspace => return finish(workspace::verb(&cli_ctx(), &[])),
        Cmd::Ws(_) => unreachable!("taken above"),
        Cmd::Mail(_) => unreachable!("taken above"),
        Cmd::Status => "status",
        Cmd::Compact { .. } => "compact",
        Cmd::Profile { .. } => "profile",
        Cmd::Work { .. } => "work",
        Cmd::Usage { .. } => "usage",
        Cmd::Marketplace { .. } => "marketplace",
        Cmd::Plugins { .. } => "plugins",
        Cmd::Help { .. } => "help",
        Cmd::Results(_) => "results",
        Cmd::Redaction(_) => "redaction",
        Cmd::Guards(_) => "guards",
        Cmd::Units(_) => "units",
        Cmd::CompactTool(_) => "compact-tool",
        Cmd::Statusline(_) => "statusline",
    };
    let raw: Vec<String> = std::env::args().skip(2).collect();
    finish(run_verb(verb, raw, false))
}

#[cfg(test)]
mod tests {
    use super::{verb_tokens, Cli, Cmd};
    use clap::{CommandFactory, Parser};

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn verb_tokens_double_dash() {
        assert_eq!(verb_tokens(v(&["on", "--save"])), (true, v(&["on"])));
        assert_eq!(verb_tokens(v(&["--save", "on"])), (true, v(&["on"])));
        assert_eq!(verb_tokens(v(&["on"])), (false, v(&["on"])));
        assert_eq!(verb_tokens(v(&["--", "--save"])), (false, v(&["--save"])));
        assert_eq!(verb_tokens(v(&["a", "--save", "--", "b", "--save", "--"])), (true, v(&["a", "b", "--save", "--"])));
    }

    /// D§10.1: the `/zygo:decision` reply carries `<args>` as given, `--` and `--save` included.
    #[test]
    fn slash_decision_keeps_its_args() {
        let r = super::slash("decision", "show -- X").unwrap();
        assert_eq!(r, "run in your shell: zygo decision show -- X");
        let r = super::slash("decision", "ls --save -- --save").unwrap();
        assert_eq!(r, "run in your shell: zygo decision ls --save -- --save");
    }

    #[test]
    fn plugins_enable_and_approve_accept_yes() {
        assert!(Cli::try_parse_from(["zygo", "plugins", "enable", "team:demo", "--yes"]).is_ok());
        assert!(Cli::try_parse_from(["zygo", "plugins", "approve", "team:demo", "--yes"]).is_ok());
        assert_eq!(verb_tokens(v(&["enable", "team:demo", "--yes"])), (false, v(&["enable", "team:demo", "--yes"])));
        assert!(Cli::try_parse_from(["zygo", "plugins", "sync", "--yes"]).is_err());
    }

    #[test]
    fn marketplace_add_enable_takes_a_value() {
        assert!(Cli::try_parse_from(["zygo", "marketplace", "add", "/x", "--enable", "all"]).is_ok());
        assert!(Cli::try_parse_from(["zygo", "marketplace", "add", "/x", "--enable"]).is_err());
        assert!(Cli::try_parse_from(["zygo", "marketplace", "update", "team", "--accept"]).is_ok());
        assert!(Cli::try_parse_from(["zygo", "marketplace", "update", "--all"]).is_ok());
    }

    #[test]
    fn plugins_without_subcommand_is_list() {
        assert!(Cli::try_parse_from(["zygo", "plugins"]).is_ok());
        assert!(Cli::try_parse_from(["zygo", "plugins", "list"]).is_ok());
    }

    #[test]
    fn compact_takes_a_session_value_and_no_save() {
        assert!(Cli::try_parse_from(["zygo", "compact", "600k", "--session", "S1"]).is_ok());
        assert!(Cli::try_parse_from(["zygo", "compact", "600k", "--session", "S1", "--save"]).is_err());
        assert!(Cli::try_parse_from(["zygo", "compact", "600k", "--session"]).is_err());
    }

    #[test] // M§6: `--name <n>` on both launch verbs; DR-4 carried item: the help names `new`
    fn launch_name_parses_for_both_agents_and_help_lists_decision_new() {
        let Cmd::Claude(l) = Cli::try_parse_from(["zygo", "claude", "--name", "elena", "--", "-p"]).unwrap().command else { panic!() };
        assert_eq!((l.name.as_deref(), l.rest.clone()), (Some("elena"), vec!["-p".to_string()]));
        let Cmd::Opencode(a) = Cli::try_parse_from(["zygo", "opencode", "--name", "elena"]).unwrap().command else { panic!() };
        assert_eq!(a.name.as_deref(), Some("elena"));
        let Cmd::Claude(l) = Cli::try_parse_from(["zygo", "claude"]).unwrap().command else { panic!() };
        assert_eq!(l.name, None);
        assert!(Cli::command().render_help().to_string().contains("Decision records: new, ls, show, deps, check"));
    }

    #[test] // M§8 CLI rows send, read, ls
    fn mail_args_parse() {
        use super::mail::{MailArgs, MailCmd};
        use std::path::PathBuf;
        let mail = |a: &[&str]| -> Result<MailArgs, clap::Error> {
            let argv: Vec<&str> = ["zygo", "mail"].iter().chain(a).copied().collect();
            Cli::try_parse_from(argv).map(|c| match c.command {
                Cmd::Mail(m) => m,
                _ => panic!("not mail"),
            })
        };
        let Some(MailCmd::Send { to, subject, file, stdin }) = mail(&["send", "bob", "--subject", "s", "--file", "f"]).unwrap().sub else { panic!() };
        assert_eq!((to.as_str(), subject.as_deref(), file, stdin), ("bob", Some("s"), Some(PathBuf::from("f")), None));
        let Some(MailCmd::Send { stdin, .. }) = mail(&["send", "bob", "-"]).unwrap().sub else { panic!() };
        assert_eq!(stdin.as_deref(), Some("-"));
        assert!(mail(&["send", "bob", "--file", "f", "-"]).is_err());
        assert!(mail(&["send", "bob", "x"]).is_err());
        let Some(MailCmd::Read { name, max }) = mail(&["read", "--name", "n", "--max", "3"]).unwrap().sub else { panic!() };
        assert_eq!((name.as_deref(), max), (Some("n"), 3));
        let Some(MailCmd::Read { max, .. }) = mail(&["read"]).unwrap().sub else { panic!() };
        assert_eq!(max, 10);
        let Some(MailCmd::Ls { name }) = mail(&["ls", "bob"]).unwrap().sub else { panic!() };
        assert_eq!(name.as_deref(), Some("bob"));
        assert!(mail(&[]).unwrap().sub.is_none());
    }
}
