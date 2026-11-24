#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    None,
    Session,
    SessionSave,
    Repository,
    NextLaunch,
    UserConfig,
}

#[derive(Debug)]
pub struct VerbSpec {
    pub name: &'static str,
    pub args: &'static str,
    pub summary: &'static str,
    pub scope: Scope,
}

static SPECS: [VerbSpec; 18] = [
    VerbSpec { name: "help", args: "[<verb>]", summary: "Every verb with its arguments and current value; with a verb, its details", scope: Scope::None },
    VerbSpec { name: "status", args: "", summary: "As `zygo status`", scope: Scope::None },
    VerbSpec { name: "usage", args: "", summary: "The session's savings", scope: Scope::None },
    VerbSpec { name: "compact", args: "<tokens|on|off>", summary: "Set the compaction window for the session", scope: Scope::Session },
    VerbSpec { name: "results", args: "on|off|threshold <bytes>", summary: "Result trimming", scope: Scope::SessionSave },
    VerbSpec { name: "redaction", args: "[content on|off]", summary: "Shows both passes; switches the content pass; the secret pass is status only", scope: Scope::SessionSave },
    VerbSpec { name: "guards", args: "[<guard> <level>]", summary: "Levels with their sources; sets one guard's level, never below its floor", scope: Scope::SessionSave },
    VerbSpec { name: "units", args: "on|off", summary: "Work units", scope: Scope::SessionSave },
    VerbSpec { name: "work", args: "[<id>|clear]", summary: "Current work item", scope: Scope::Repository },
    VerbSpec { name: "plugins", args: "[list|enable|disable|approve <m>:<p>]", summary: "Plugins, act-lite navigation included", scope: Scope::NextLaunch },
    VerbSpec { name: "marketplace", args: "[list|add|update|remove …]", summary: "Plugin marketplaces", scope: Scope::UserConfig },
    VerbSpec { name: "profile", args: "[<name>|none]", summary: "Default profile", scope: Scope::NextLaunch },
    VerbSpec { name: "compact-tool", args: "on|off", summary: "The `compact` MCP tool", scope: Scope::NextLaunch },
    VerbSpec { name: "statusline", args: "[compact|verbose]", summary: "Status line style", scope: Scope::SessionSave },
    VerbSpec {
        name: "down",
        args: "[--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]",
        summary: "zygo down: stop the workspace of this folder or of <name>, in the foreground (detached from an agent or from a pane of that workspace). /zygo:down takes only --now and stops the workspace of this session, detached",
        scope: Scope::None,
    },
    VerbSpec { name: "ls", args: "", summary: "One line per workspace with its state", scope: Scope::None },
    VerbSpec { name: "workspace", args: "", summary: "Status of this session's or this folder's workspace", scope: Scope::None },
    VerbSpec { name: "decision", args: "ls|show|deps|check …", summary: "Decision records; /zygo:decision replies with the shell command", scope: Scope::None },
];

pub static VERB_SPECS: &[VerbSpec] = &SPECS;

/// The arguments of `/zygo:down` (W§12, OP11): only `--now`. The `down` spec's `args` are those of `zygo down`.
pub const DOWN_SLASH_ARGS: &str = "[--now]";
