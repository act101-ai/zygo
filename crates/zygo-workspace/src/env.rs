//! Environment cleaning. Owner: node W4 (AU64, AU93).
//! The functions compare names only. They read no value and write no log and no output (W-C7).
use std::ffi::{OsStr, OsString};

/// Removed in every case: the per-session zygo values (AU39) and the Claude Code markers (AU64).
pub const REMOVE_ALWAYS: &[&str] = &[
    "ZYGO_SESSION",
    "ZYGO_SESSION_DIR",
    "ZYGO_AGENT",
    "ZYGO_URL",
    "ZYGO_REPO",
    "ZYGO_WORK",
    "ZYGO_MAIN_ROLE",
    "ZYGO_PANE",
    "ZYGO_WORKSPACE",
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
];

/// Removed only when `ZYGO_SESSION` is set (the caller runs inside a zygo agent): the values that the launcher
/// sets for its agent child (AU64).
pub const REMOVE_INSIDE_AGENT: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_CUSTOM_HEADERS",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_GATEWAY_HINT_HEADERS",
    "DISABLE_LOGIN_COMMAND",
    "DISABLE_LOGOUT_COMMAND",
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_CONFIG_DIR",
];

fn listed(list: &[&str], name: &OsStr) -> bool {
    list.iter().any(|n| name == OsStr::new(n))
}

/// The environment of tmux and of `ws run` (AU64): `vars` without the rows named in [`REMOVE_ALWAYS`] and, when
/// `vars` holds a row named `ZYGO_SESSION` (any value, also an empty one), without the rows named in
/// [`REMOVE_INSIDE_AGENT`]. All other rows are returned unchanged, in their order.
///
/// Provider keys and `ZYGO_TEAM_TOKEN` are kept: the in-process proxy of each pane's launcher reads them from its
/// own environment, and the launcher keeps them out of the agent child (AU64). `ZYGO_CONFIG` is a user setting and
/// is kept (AU39).
///
/// A name is compared byte for byte, with no conversion and no case fold, so a name that is not UTF-8 matches no
/// list name. No value is read. The function does not read the environment of the process (the caller passes it),
/// does not set `ZYGO_WORKSPACE` again (AU93: `ws run` does that), and writes no log.
pub fn cleaned(vars: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let inside_agent = vars.iter().any(|(name, _)| name == OsStr::new("ZYGO_SESSION"));
    vars.into_iter().filter(|(name, _)| !listed(REMOVE_ALWAYS, name) && !(inside_agent && listed(REMOVE_INSIDE_AGENT, name))).collect()
}
