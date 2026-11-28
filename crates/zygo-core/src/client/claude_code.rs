//! Claude Code's descriptor and its pure functions.

use serde_json::Value;

use super::ClientSpec;

pub(super) static SPEC: ClientSpec = ClientSpec {
    id: "claude-code",
    name: "Claude Code",
    program: "claude",
    verb: "claude",
    config_dir: ".claude",
    install_hint: "zygo: claude not found on PATH; install Claude Code: https://code.claude.com/docs/en/setup",
    min_version: "2.1.328",
    supported: true,
    status_line: true,
    hook_input,
};

/// Claude Code's hook JSON is the common form: it is returned unchanged.
pub(super) fn hook_input(v: Value) -> Value {
    v
}
