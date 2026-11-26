//! Responses client → Messages (K§6.4.3, K§6.7).

use serde_json::Value;
use zygo_config::Facts;

use super::Target;
use crate::errors::ZygoError;
use crate::pipeline::Adjustment;

/// Top-level fields this translator maps (K§6.4.3).
pub const MAPPED: [&str; 12] = [
    "model",
    "instructions",
    "input",
    "tools",
    "tool_choice",
    "parallel_tool_calls",
    "max_output_tokens",
    "temperature",
    "top_p",
    "reasoning",
    "stream",
    "text",
];

/// Top-level fields accepted and not sent toward a non-Responses upstream (K§6.4.3).
pub const IGNORED: [&str; 10] = [
    "store",
    "include",
    "prompt_cache_key",
    "metadata",
    "client_metadata",
    "service_tier",
    "user",
    "safety_identifier",
    "stream_options",
    "truncation",
];

/// Top-level fields refused (K-D31).
pub const REFUSED: [&str; 4] = ["previous_response_id", "background", "conversation", "prompt"];

/// The text of a refused field's 400, after the field name.
pub const NO_STATE: &str = "zygo keeps no response state; send the full input with store=false";

/// Hosted-tool input items, dropped with `items_dropped:<type>:<n>` (K§6.4.3 obligation 6).
pub const HOSTED_ITEMS: [&str; 12] = [
    "web_search_call",
    "file_search_call",
    "computer_call",
    "computer_call_output",
    "local_shell_call",
    "local_shell_call_output",
    "code_interpreter_call",
    "image_generation_call",
    "mcp_call",
    "mcp_list_tools",
    "mcp_approval_request",
    "mcp_approval_response",
];

/// The three top-level lists: a refused or unknown field → `400 zygo_bad_request` that names it.
pub fn check_fields(req: &Value) -> Result<(), ZygoError> {
    let _ = req;
    todo!()
}

/// A Responses body as a Messages body for `t`; `facts` = the resolved model's catalog facts.
pub fn request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError> {
    let _ = (req, t, facts);
    todo!()
}
