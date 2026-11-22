use clap::{Args, Parser, Subcommand};
use std::time::SystemTime;
use zygo_core::exit::ExitCode;
use zygo_core::hook::set_slash_dispatch;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_proxy::client::ClientError;

pub mod claude;
pub mod compact;
pub mod compact_tool;
pub mod config;
pub mod doctor;
pub mod guards;
pub mod help;
pub mod hook;
pub mod keys;
pub mod marketplace;
pub mod mcp;
pub mod opencode;
pub mod plugin;
pub mod plugins;
pub mod profile;
pub mod redaction;
pub mod results;
pub mod serve;
pub mod status;
pub mod units;
pub mod update;
pub mod usage;
pub mod verbs;
pub mod work;

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
struct Launch {
    /// Model profile
    #[arg(long)]
    profile: Option<String>,
    /// Use the passthrough credential
    #[arg(long, hide = false)]
    passthrough: bool,
    /// Server URL
    #[arg(long)]
    server: Option<String>,
    /// Compaction window in tokens, or off
    #[arg(long)]
    compact: Option<String>,
    /// Skip permission prompts
    #[arg(long)]
    yolo: bool,
    /// Claude Code config directory (sets CLAUDE_CONFIG_DIR)
    #[arg(long)]
    config_dir: Option<String>,
    /// Arguments passed to the agent
    #[arg(last = true)]
    rest: Vec<String>,
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

/// Runs a verb: `args` are the raw tokens after the verb, split by `verb_tokens`.
fn run_verb(name: &str, raw: Vec<String>, slash: bool) -> anyhow::Result<String> {
    let verb = VERBS
        .iter()
        .find(|v| v.spec.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown verb `{name}`; try help"))?;
    let (save, args) = verb_tokens(raw);
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
    // §10.1 and its N71 note: results of ended sessions are swept at the next `zygo` command, except hooks and
    // `zygo mcp` (time-bounded; C6). I/O errors are ignored.
    if !matches!(cli.command, Cmd::Hook { .. } | Cmd::Mcp { .. }) {
        zygo_results::sweep(&Paths::from_env().cache_dir, SystemTime::now());
    }
    let verb = match &cli.command {
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
    };
    let raw: Vec<String> = std::env::args().skip(2).collect();
    match run_verb(verb, raw, false) {
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

#[cfg(test)]
mod tests {
    use super::{verb_tokens, Cli};
    use clap::Parser;

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
}
