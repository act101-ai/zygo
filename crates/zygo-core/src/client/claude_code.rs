//! Claude Code's descriptor and its pure functions.

use serde_json::Value;

use super::{ClientSpec, Dialect};

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
    dialect: Dialect::Anthropic,
    request_class_header: Some("x-claude-code-request-class"),
    compaction_header: Some("x-claude-code-compaction"),
    agent_id_header: Some("x-claude-code-agent-id"),
    agent_type_header: Some("x-claude-code-agent-type"),
    window_rewrite: true,
    passthrough_credential: Some("claude-ai"),
    cache_markers: false,
    redact_skip_tags: &[],
    redact_harness_prefixes: &[
        "<system-reminder>",
        "<bash-stdout>",
        "<bash-stderr>",
        "<system-update>",
        "Stop hook feedback:",
        "SubagentStop hook feedback:",
    ],
    redact_harness_spans: &["command-name"],
};

/// Claude Code's hook JSON is the common form: it is returned unchanged.
pub(super) fn hook_input(v: Value) -> Value {
    v
}
