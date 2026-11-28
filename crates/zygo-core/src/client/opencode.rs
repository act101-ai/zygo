//! OpenCode's descriptor and its pure functions.

use serde_json::{json, Map, Value};

use super::{ClientSpec, Dialect};

pub(super) static SPEC: ClientSpec = ClientSpec {
    id: "opencode",
    name: "OpenCode",
    program: "opencode",
    verb: "opencode",
    config_dir: ".opencode",
    install_hint: "zygo: opencode was not found on PATH; install OpenCode: https://opencode.ai/docs/",
    min_version: "1.18.16",
    supported: true,
    status_line: false,
    hook_input: normalize,
    dialect: Dialect::Anthropic,
    request_class_header: None,
    compaction_header: None,
    agent_id_header: None,
    agent_type_header: None,
    window_rewrite: false,
    passthrough_credential: None,
    cache_markers: true,
    redact_skip_tags: &["file", "shell"],
    // The base content pass applied the Claude Code harness rule to every client; OpenCode keeps it (K-D3).
    redact_harness_prefixes: &[
        "<system-reminder>",
        "<bash-stdout>",
        "<bash-stderr>",
        "<system-update>",
        "Stop hook feedback:",
        "SubagentStop hook feedback:",
    ],
    redact_harness_spans: &["command-name"],
    mcp_server,
    lists_compact_tool: false,
};

/// The server of `name` when it is the MCP tool `tool` in OpenCode's form `<server>_<tool>`.
pub(super) fn mcp_server<'a>(name: &'a str, tool: &str) -> Option<&'a str> {
    name.strip_suffix(tool)?.strip_suffix('_')
}

/// The Claude Code name of an OpenCode tool; any other name is unchanged.
fn tool_name(t: &str) -> &str {
    match t {
        "bash" => "Bash",
        "read" => "Read",
        "edit" => "Edit",
        "write" => "Write",
        "grep" => "Grep",
        "glob" => "Glob",
        other => other,
    }
}

/// `filePath` -> `file_path` and `newString` -> `new_string` when the new name is absent; every other key is copied.
fn map_args(args: &Map<String, Value>) -> Value {
    let mut out = args.clone();
    for (from, to) in [("filePath", "file_path"), ("newString", "new_string")] {
        if !args.contains_key(to)
            && let Some(v) = out.remove(from)
        {
            out.insert(to.into(), v);
        }
    }
    Value::Object(out)
}

/// The OpenCode plugin payload `{ input, output }` (plus top-level keys) as a Claude-shaped hook input (spec §8.7).
/// Anything that is not a tool hook payload is returned unchanged.
pub(super) fn normalize(v: Value) -> Value {
    let Some(tool) = v.pointer("/input/tool").and_then(Value::as_str) else { return v };
    let out = v.get("output");
    let mut o: Map<String, Value> = v.as_object().into_iter().flatten().filter(|(k, _)| *k != "input" && *k != "output").map(|(k, v)| (k.clone(), v.clone())).collect();
    o.insert("tool_name".into(), json!(tool_name(tool)));
    let text = out.and_then(|o| o.get("output")).and_then(Value::as_str);
    match (text, out.and_then(|o| o.get("args")).and_then(Value::as_object)) {
        (None, Some(args)) => {
            o.insert("hook_event_name".into(), json!("PreToolUse"));
            o.insert("tool_input".into(), map_args(args));
        }
        (Some(text), _) => {
            o.insert("hook_event_name".into(), json!("PostToolUse"));
            if let Some(a) = v.pointer("/input/args").and_then(Value::as_object) {
                o.insert("tool_input".into(), map_args(a));
            }
            if tool == "bash" {
                o.insert("tool_response".into(), json!({ "stdout": text }));
            }
        }
        (None, None) => return v,
    }
    Value::Object(o)
}
