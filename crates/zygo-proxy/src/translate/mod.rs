//! Dialect translation (§7.6): an `anthropic` client to an `openai` (Responses) or `openai-chat` upstream.

pub mod chat;
pub mod facts;
pub mod reasoning;
pub mod responses;
pub mod responses_client;
pub mod schema;
pub mod sse;

pub use sse::{message_events, take_calls_dropped, SseEvent, StreamMapper};

use std::collections::BTreeMap;

use http::StatusCode;
use serde_json::{json, Value};
use zygo_config::Dialect;

use crate::errors::ZygoError;
use crate::pipeline::Adjustment;
use crate::provider::Candidate;
use reasoning::Signature;

pub const DOC_OMITTED: &str = "[document omitted: the model cannot read it here]";

/// The provider a translated request goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub provider: String,
    pub upstream_model: String,
}

impl Target {
    pub fn of(c: &Candidate) -> Target {
        Target { provider: c.provider.name.clone(), upstream_model: c.provider.upstream_model.clone() }
    }
}

/// `to` = Openai | OpenaiChat; `model` = `target.upstream_model`. `to = Anthropic`, or `messages` not an array →
/// `400 zygo_bad_request`.
pub fn translate_request(req: &Value, to: Dialect, target: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError> {
    let mut d = Drops::default();
    let out = match to {
        Dialect::Openai => responses::request(req, target, &mut d)?,
        Dialect::OpenaiChat => chat::request(req, target, &mut d)?,
        Dialect::Anthropic => return Err(ZygoError::bad_request("no translation to this dialect")),
    };
    Ok((out, d.codes()))
}

/// A Responses (`from = Openai`) or chat (`OpenaiChat`) body as a Messages body.
pub fn translate_response(resp: &Value, from: Dialect, target: &Target) -> Value {
    match from {
        Dialect::OpenaiChat => chat::response(resp, target),
        _ => responses::response(resp, target),
    }
}

/// `resp` has the shape of a `from` response body: an `output` array (Responses) or a `choices` array (chat).
pub fn is_response(resp: &Value, from: Dialect) -> bool {
    let k = if from == Dialect::OpenaiChat { "choices" } else { "output" };
    resp.get(k).is_some_and(Value::is_array)
}

/// An upstream error as a Messages error: a context-limit refusal (400 or 413) → `400 prompt is too long`; else the
/// upstream status with the Anthropic type of that status. `key`'s value is replaced in the message (C3).
pub fn translate_error(status: StatusCode, body: &[u8], estimate: u64, key: Option<&str>) -> (StatusCode, Value) {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if matches!(status.as_u16(), 400 | 413)
        && let Some(b) = context_error(error_object(&v), estimate)
    {
        return (StatusCode::BAD_REQUEST, b);
    }
    let m = error_message(&v).map_or_else(|| "upstream error".into(), |m| scrub(m, key));
    (status, error_body(error_type(status.as_u16()), &m))
}

pub(crate) fn error_body(t: &str, m: &str) -> Value {
    json!({"type":"error","error":{"type":t,"message":m}})
}

fn error_type(status: u16) -> &'static str {
    match status {
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        529 => "overloaded_error",
        400..=499 => "invalid_request_error",
        _ => "api_error",
    }
}

/// `body.error` when it is an object, else the body itself (vLLM's top-level form).
pub(crate) fn error_object(v: &Value) -> &Value {
    match v.get("error") {
        Some(e) if e.is_object() => e,
        _ => v,
    }
}

/// `error.message`, or `error` when it is a string.
pub(crate) fn error_message(v: &Value) -> Option<&str> {
    error_object(v).get("message").and_then(Value::as_str).or_else(|| v.get("error").and_then(Value::as_str))
}

/// Every occurrence of the key's value replaced with `[REDACTED]` (C3).
pub(crate) fn scrub(m: &str, key: Option<&str>) -> String {
    match key.filter(|k| !k.is_empty()) {
        Some(k) => m.replace(k, "[REDACTED]"),
        None => m.into(),
    }
}

/// A context-limit refusal as `prompt is too long: <n> tokens > <max> maximum` (§7.6 and its note); `None` when `err`
/// is another error.
pub(crate) fn context_error(err: &Value, estimate: u64) -> Option<Value> {
    let m = s(err, "message").unwrap_or("").to_lowercase();
    let hit = s(err, "code") == Some("context_length_exceeded")
        || s(err, "type") == Some("exceed_context_size_error")
        || ["maximum context length", "context window", "context size"].iter().any(|p| m.contains(p));
    if !hit {
        return None;
    }
    let after = |p: &str| {
        let i = m.find(p)? + p.len();
        let digits: String = m[i..].chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u64>().ok()
    };
    let n = err
        .get("n_prompt_tokens")
        .and_then(Value::as_u64)
        .or_else(|| after("resulted in "))
        .or_else(|| after("requested "))
        .unwrap_or(estimate);
    // A known `max`: `n` is at least `max + 1`, so the message always reads `n > max`.
    let (n, max) = match err.get("n_ctx").and_then(Value::as_u64).or_else(|| after("maximum context length is ")) {
        Some(max) => (n.max(max.saturating_add(1)), max),
        None => (n, n.saturating_sub(1)),
    };
    Some(error_body("invalid_request_error", &format!("prompt is too long: {n} tokens > {max} maximum")))
}

/// Anthropic usage from upstream numbers: `input_tokens` excludes the cached part (§14).
pub(crate) fn usage(input: u64, cached: u64, output: u64) -> Value {
    json!({"input_tokens":input.saturating_sub(cached),"cache_read_input_tokens":cached,"cache_creation_input_tokens":0,"output_tokens":output})
}

/// The usage object of a Responses or chat body as Anthropic usage; absent → zeros.
pub(crate) fn usage_of(u: Option<&Value>, from: Dialect) -> Value {
    let (i, d, o) = match from {
        Dialect::OpenaiChat => ("prompt_tokens", "prompt_tokens_details", "completion_tokens"),
        _ => ("input_tokens", "input_tokens_details", "output_tokens"),
    };
    let n = |v: Option<&Value>| v.and_then(Value::as_u64).unwrap_or(0);
    let u = u.unwrap_or(&Value::Null);
    usage(n(u.get(i)), n(u.get(d).and_then(|x| x.get("cached_tokens"))), n(u.get(o)))
}

/// Responses `status` and `incomplete_details.reason` as a Messages `stop_reason`.
pub(crate) fn responses_stop(status: Option<&str>, reason: Option<&str>, tool: bool) -> &'static str {
    match (status, reason) {
        (Some("incomplete"), Some("max_output_tokens")) => "max_tokens",
        (Some("incomplete"), Some("content_filter")) => "refusal",
        (Some("completed"), _) if tool => "tool_use",
        _ => "end_turn",
    }
}

/// A chat `finish_reason` as a Messages `stop_reason`.
pub(crate) fn chat_stop(finish: Option<&str>, tool: bool) -> &'static str {
    match finish {
        Some("length") => "max_tokens",
        Some("tool_calls" | "function_call") => "tool_use",
        Some("content_filter") => "refusal",
        _ if tool => "tool_use",
        _ => "end_turn",
    }
}

/// A Messages body; `id` and `model` from the upstream, else `""` and the target's `upstream_model`.
pub(crate) fn message(resp: &Value, t: &Target, content: Vec<Value>, stop: &str, usage: Value) -> Value {
    json!({"id":s(resp, "id").unwrap_or(""),"type":"message","role":"assistant","model":s(resp, "model").unwrap_or(&t.upstream_model),
        "content":content,"stop_reason":stop,"stop_sequence":null,"usage":usage})
}

/// Tool-call `arguments` as a `tool_use` input; not a JSON object → `{}`.
pub(crate) fn input_of(args: Option<&str>) -> Value {
    args.and_then(|a| serde_json::from_str::<Value>(a).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// What a translation removed (§7.6), for the adjustment codes.
#[derive(Debug, Default)]
pub(crate) struct Drops {
    thinking: u32,
    server_tools: u32,
    documents: u32,
    blocks: BTreeMap<String, u32>,
}

impl Drops {
    /// `thinking_dropped`, `server_tools_dropped`, `documents_dropped`, then `blocks_dropped` by type; no zero count.
    fn codes(&self) -> Vec<Adjustment> {
        let mut out = Vec::new();
        for (name, n) in [("thinking_dropped", self.thinking), ("server_tools_dropped", self.server_tools), ("documents_dropped", self.documents)] {
            if n > 0 {
                out.push(Adjustment(format!("{name}:{n}")));
            }
        }
        out.extend(self.blocks.iter().map(|(t, n)| Adjustment(format!("blocks_dropped:{t}:{n}"))));
        out
    }

    fn block(&mut self, t: &str) {
        *self.blocks.entry(t.into()).or_default() += 1;
    }
}

/// One Anthropic content block as the translations see it.
pub(crate) enum Part<'a> {
    Text(&'a str),
    /// The image URL: a `data:` URL or the source URL.
    Image(String),
    /// A base64 PDF as a `data:` URL (Responses only).
    File(String),
    ToolUse(&'a Value),
    ToolResult(&'a Value),
    /// A thinking block of the target, decoded (Responses only).
    Reasoning(Signature),
}

pub(crate) fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// The blocks of `content` (a string is one text block); what has no counterpart toward `to` is counted in `d`.
pub(crate) fn parts<'a>(content: &'a Value, to: Dialect, t: &Target, d: &mut Drops) -> Vec<Part<'a>> {
    let blocks = match content {
        Value::String(x) => return vec![Part::Text(x)],
        Value::Array(a) => a,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for b in blocks {
        let ty = s(b, "type").unwrap_or("");
        match ty {
            "text" => out.push(Part::Text(s(b, "text").unwrap_or(""))),
            "image" => {
                let src = &b["source"];
                match s(src, "type") {
                    Some("base64") => out.push(Part::Image(data_url(src))),
                    Some("url") => out.push(Part::Image(s(src, "url").unwrap_or("").into())),
                    _ => d.block(ty),
                }
            }
            "document" => {
                let src = &b["source"];
                if to == Dialect::Openai && s(src, "type") == Some("base64") && s(src, "media_type") == Some("application/pdf") {
                    out.push(Part::File(data_url(src)));
                } else {
                    d.documents += 1;
                    out.push(Part::Text(DOC_OMITTED));
                }
            }
            "tool_use" => out.push(Part::ToolUse(b)),
            "tool_result" => out.push(Part::ToolResult(b)),
            "thinking" | "redacted_thinking" => {
                let own = (to == Dialect::Openai && ty == "thinking")
                    .then(|| s(b, "signature").and_then(reasoning::decode))
                    .flatten()
                    .filter(|g| g.provider == t.provider && g.model == t.upstream_model);
                match own {
                    Some(g) => out.push(Part::Reasoning(g)),
                    None => d.thinking += 1,
                }
            }
            "server_tool_use" => d.server_tools += 1,
            _ if ty.ends_with("_tool_result") => d.server_tools += 1,
            _ => d.block(ty),
        }
    }
    out
}

fn data_url(src: &Value) -> String {
    format!("data:{};base64,{}", s(src, "media_type").unwrap_or(""), s(src, "data").unwrap_or(""))
}

/// The text of a `system` field or a system message: a string, or its text blocks.
pub(crate) fn texts(v: &Value) -> Vec<&str> {
    match v {
        Value::String(x) => vec![x.as_str()],
        Value::Array(a) => a.iter().filter(|b| s(b, "type") == Some("text")).filter_map(|b| s(b, "text")).collect(),
        _ => Vec::new(),
    }
}

/// The top-level `system`, text blocks joined with "\n\n".
pub(crate) fn system(req: &Value) -> Option<String> {
    req.get("system").map(|v| texts(v).join("\n\n"))
}

/// A translated tool: `name`, `description` when present, `parameters` = `input_schema`. Server tools are counted.
pub(crate) struct Tool<'a> {
    pub name: &'a str,
    pub description: Option<&'a Value>,
    pub parameters: Value,
}

pub(crate) fn tools<'a>(req: &'a Value, d: &mut Drops) -> Vec<Tool<'a>> {
    let mut out = Vec::new();
    for t in req.get("tools").and_then(Value::as_array).into_iter().flatten() {
        if s(t, "type").is_some_and(|ty| ty != "custom") {
            d.server_tools += 1;
            continue;
        }
        out.push(Tool {
            name: s(t, "name").unwrap_or(""),
            description: t.get("description"),
            parameters: t.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object"})),
        });
    }
    out
}

pub(crate) enum Choice<'a> {
    Auto,
    Required,
    None,
    Tool(&'a str),
}

/// `tool_choice` and whether `disable_parallel_tool_use` is true.
pub(crate) fn tool_choice(req: &Value) -> (Option<Choice<'_>>, bool) {
    let Some(tc) = req.get("tool_choice") else { return (None, false) };
    let c = match s(tc, "type") {
        Some("auto") => Some(Choice::Auto),
        Some("any") => Some(Choice::Required),
        Some("none") => Some(Choice::None),
        Some("tool") => Some(Choice::Tool(s(tc, "name").unwrap_or(""))),
        _ => None,
    };
    (c, tc.get("disable_parallel_tool_use").and_then(Value::as_bool) == Some(true))
}

/// The schema of `output_config.format` when its type is `json_schema`.
pub(crate) fn json_schema(req: &Value) -> Option<&Value> {
    let f = req.get("output_config")?.get("format")?;
    (s(f, "type") == Some("json_schema")).then(|| f.get("schema")).flatten()
}

/// `messages`, or `400 zygo_bad_request`.
pub(crate) fn messages(req: &Value) -> Result<&Vec<Value>, ZygoError> {
    req.get("messages").and_then(Value::as_array).ok_or_else(|| ZygoError::bad_request("messages must be an array"))
}

/// Copies `from` to `to` in `out` when present.
pub(crate) fn copy(req: &Value, out: &mut serde_json::Map<String, Value>, from: &str, to: &str) {
    if let Some(v) = req.get(from) {
        out.insert(to.into(), v.clone());
    }
}
