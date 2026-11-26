//! Responses client → Messages (K§6.4.3, K§6.7).

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};
use zygo_config::Facts;

use super::reasoning::decode_thinking;
use super::{input_of, s, Target, DOC_OMITTED};
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

/// The longest client-sent name or type echoed in an error message (K§6.7 hostile input).
const ECHO_MAX: usize = 64;

/// A client name or type as an error message may echo it: its first `ECHO_MAX` characters, each control character
/// replaced with `?`.
fn cut(x: &str) -> String {
    x.chars().take(ECHO_MAX).map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// The three top-level lists, keys in body order, the first failure returned: a refused or unknown field →
/// `400 zygo_bad_request` that names it.
pub fn check_fields(req: &Value) -> Result<(), ZygoError> {
    let obj = req.as_object().ok_or_else(|| ZygoError::bad_request("request body must be a JSON object"))?;
    for (k, v) in obj {
        let k = k.as_str();
        if REFUSED.contains(&k) {
            return Err(ZygoError::bad_request(format!("{k}: {NO_STATE}")));
        }
        if k == "truncation" && v.as_str() != Some("disabled") {
            return Err(ZygoError::bad_request("truncation: only \"disabled\" is accepted"));
        }
        if !MAPPED.contains(&k) && !IGNORED.contains(&k) {
            return Err(ZygoError::bad_request(format!("unknown field: {}", cut(k))));
        }
    }
    Ok(())
}

/// A Responses body as a Messages body for `t` (K§6.4.3 request obligations 1-10); `facts` = the resolved model's
/// catalog facts, the source of `max_tokens` when the client gives none. The output is built new, so no ignored field
/// is carried. `model` stays as the client sent it.
pub fn request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError> {
    check_fields(req)?;
    let model = req.get("model").filter(|v| v.is_string()).ok_or_else(|| bad("model must be a string"))?;
    let max_tokens = max_tokens(req, facts)?;
    let effort = typed(req, "reasoning", "an object", Value::is_object)?
        .map(|r| typed(r, "effort", "a string", Value::is_string).map_err(|_| bad("reasoning.effort must be a string")))
        .transpose()?
        .flatten();
    let format = typed(req, "text", "an object", Value::is_object)?
        .map(|x| typed(x, "format", "an object", Value::is_object).map_err(|_| bad("text.format must be an object")))
        .transpose()?
        .flatten();
    if format.is_some_and(|f| s(f, "type") != Some("text")) {
        return Err(bad("text.format: only type text is accepted toward this upstream"));
    }
    typed(req, "parallel_tool_calls", "a boolean", Value::is_boolean)?;
    let mut scalars = Vec::new();
    for (k, what, ok) in [
        ("temperature", "a number", Value::is_number as fn(&Value) -> bool),
        ("top_p", "a number", Value::is_number),
        ("stream", "a boolean", Value::is_boolean),
    ] {
        if let Some(v) = typed(req, k, what, ok)? {
            scalars.push((k, v.clone()));
        }
    }
    let mut d = Drops::default();
    let mut c = Conv { leading: true, ..Conv::default() };
    match req.get("instructions") {
        None | Some(Value::Null) => {}
        Some(Value::String(x)) if x.is_empty() => {}
        Some(Value::String(x)) => c.system.push(x.clone()),
        Some(_) => return Err(bad("instructions must be a string")),
    }
    match req.get("input") {
        Some(Value::String(x)) if x.is_empty() => {}
        Some(Value::String(x)) => c.push("user", text(x)),
        Some(Value::Array(items)) => {
            for it in items {
                item(&mut c, it, t, &mut d)?;
            }
            c.all_answered()?;
        }
        _ => return Err(bad("input must be a string or an array")),
    }
    if c.messages.first().is_none_or(|(role, _)| *role != "user") {
        return Err(bad("input: the conversation must start with a user message"));
    }
    adjacency(&c.messages)?;
    let mut tools = tools(req, &mut d)?;
    let mut choice = tool_choice(req, &tools)?;
    let from_history = history_tools(&c.messages, &tools);
    if !from_history.is_empty() {
        if tools.is_empty() {
            choice = Some(json!({"type":"none"}));
        }
        d.from_history = from_history.len();
        tools.extend(from_history);
    }

    let mut out = Map::new();
    out.insert("model".into(), model.clone());
    if !c.system.is_empty() {
        out.insert("system".into(), Value::String(c.system.join("\n\n")));
    }
    let messages = c.messages.into_iter().map(|(role, content)| json!({"role": role, "content": content})).collect();
    out.insert("messages".into(), Value::Array(messages));
    out.insert("max_tokens".into(), json!(max_tokens));
    for (k, v) in scalars {
        out.insert(k.into(), v);
    }
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
    }
    if let Some(tc) = choice {
        out.insert("tool_choice".into(), tc);
    }
    if let Some(e) = effort {
        out.insert("output_config".into(), json!({"effort": e}));
    }
    Ok((Value::Object(out), d.codes()))
}

fn bad(message: impl Into<String>) -> ZygoError {
    ZygoError::bad_request(message)
}

/// `v[k]` when present and not null; a value that fails `ok` → `400 "<k> must be <what>"`.
fn typed<'a>(v: &'a Value, k: &str, what: &str, ok: fn(&Value) -> bool) -> Result<Option<&'a Value>, ZygoError> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(x) if ok(x) => Ok(Some(x)),
        Some(_) => Err(bad(format!("{k} must be {what}"))),
    }
}

fn text(x: &str) -> Value {
    json!({"type":"text","text":x})
}

/// A text block with empty text: dropped everywhere (K§6.4.3 obligation 2).
fn is_empty_text(b: &Value) -> bool {
    s(b, "type") == Some("text") && s(b, "text") == Some("")
}

/// The string field `k` of an item of type `ty`, or `400 "<ty>: missing <k>"`.
fn field<'a>(it: &'a Value, ty: &str, k: &str) -> Result<&'a str, ZygoError> {
    s(it, k).ok_or_else(|| bad(format!("{ty}: missing {k}")))
}

/// The client's `max_output_tokens`, else the catalog's (obligation 7).
fn max_tokens(req: &Value, facts: Option<&Facts>) -> Result<u64, ZygoError> {
    match req.get("max_output_tokens") {
        None | Some(Value::Null) => facts.map(|f| f.max_output_tokens).filter(|&n| n > 0).ok_or_else(|| {
            bad("max_output_tokens is required: the model's catalog states no max_output_tokens")
        }),
        Some(v) => v.as_u64().filter(|&n| n > 0).ok_or_else(|| bad("max_output_tokens must be a positive integer")),
    }
}

/// A client type as it may appear in an adjustment code: kept when it is 1-32 characters of `[a-z0-9_]`, else
/// `other` (a code is one header value; a newline, comma or colon would break or forge one).
fn code_type(ty: &str) -> &str {
    let ok = (1..=32).contains(&ty.len()) && ty.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok { ty } else { "other" }
}

/// What the translation removed or changed, for the adjustment codes.
#[derive(Default)]
struct Drops {
    custom: bool,
    reasoning: u32,
    documents: u32,
    tools: BTreeMap<String, u32>,
    items: BTreeMap<String, u32>,
    /// Tools declared from the call history.
    from_history: usize,
}

impl Drops {
    /// `custom_tool_as_function`, `reasoning_dropped`, `documents_dropped`, `tools_dropped` and `items_dropped` by
    /// type, then `tools_from_history`; no zero count.
    fn codes(&self) -> Vec<Adjustment> {
        let mut out = Vec::new();
        if self.custom {
            out.push(Adjustment("custom_tool_as_function".into()));
        }
        for (name, n) in [("reasoning_dropped", self.reasoning), ("documents_dropped", self.documents)] {
            if n > 0 {
                out.push(Adjustment(format!("{name}:{n}")));
            }
        }
        out.extend(self.tools.iter().map(|(t, n)| Adjustment(format!("tools_dropped:{t}:{n}"))));
        out.extend(self.items.iter().map(|(t, n)| Adjustment(format!("items_dropped:{t}:{n}"))));
        if self.from_history > 0 {
            out.push(Adjustment(format!("tools_from_history:{}", self.from_history)));
        }
        out
    }
}

/// The Messages conversation being built.
#[derive(Default)]
struct Conv {
    system: Vec<String>,
    /// No item other than a `developer`/`system` message seen yet (obligation 1).
    leading: bool,
    messages: Vec<(&'static str, Vec<Value>)>,
    /// Every call's `call_id` so far.
    calls: HashSet<String>,
    /// Calls with no output yet: `call_id` and the call's item type.
    open: Vec<(String, &'static str)>,
}

/// The fixed text of an unanswered call's 400.
const NO_OUTPUT: &str = "no output before the next user message or the end of input";

impl Conv {
    /// A call of item type `ty` (obligation 3): its `call_id` is unique.
    fn call(&mut self, ty: &'static str, id: &str) -> Result<(), ZygoError> {
        if !self.calls.insert(id.to_owned()) {
            return Err(bad(format!("{ty}: duplicate call_id")));
        }
        self.open.push((id.to_owned(), ty));
        Ok(())
    }

    /// An output of item type `ty`: it answers an earlier open call of its kind with the same `call_id`.
    fn output(&mut self, ty: &str, id: &str) -> Result<(), ZygoError> {
        let kind = ty.strip_suffix("_output").unwrap_or(ty);
        match self.open.iter().position(|(c, k)| c == id && *k == kind) {
            Some(i) => {
                self.open.remove(i);
                Ok(())
            }
            None => Err(bad(format!("{ty}: no earlier matching call"))),
        }
    }

    /// Before a user message and at the end of `input`: every call has its output.
    fn all_answered(&self) -> Result<(), ZygoError> {
        match self.open.first() {
            Some((_, ty)) => Err(bad(format!("{ty}: {NO_OUTPUT}"))),
            None => Ok(()),
        }
    }

    /// Appends `b` to the last message when it has `role`, else starts one; in a user message `tool_result` blocks
    /// come first (obligation 2).
    fn push(&mut self, role: &'static str, b: Value) {
        match self.messages.last_mut() {
            Some((r, content)) if *r == role => {
                if s(&b, "type") == Some("tool_result") {
                    let at = content.iter().take_while(|x| s(x, "type") == Some("tool_result")).count();
                    content.insert(at, b);
                } else {
                    content.push(b);
                }
            }
            _ => self.messages.push((role, vec![b])),
        }
    }
}

/// One `input` item (obligations 1-6, 9).
fn item(c: &mut Conv, it: &Value, t: &Target, d: &mut Drops) -> Result<(), ZygoError> {
    let ty = match it.get("type") {
        Some(Value::String(ty)) => ty.as_str(),
        Some(_) => return Err(bad("input item: type must be a string")),
        None if it.get("role").is_some() => "message",
        None => return Err(bad("input item: missing type")),
    };
    if ty != "message" {
        c.leading = false;
    }
    match ty {
        "message" => message(c, it, d)?,
        "function_call" => {
            let (id, name, args) = (field(it, ty, "call_id")?, field(it, ty, "name")?, field(it, ty, "arguments")?);
            c.call("function_call", id)?;
            c.push("assistant", json!({"type":"tool_use","id":id,"name":name,"input":input_of(Some(args))}));
        }
        "custom_tool_call" => {
            let (id, name, input) = (field(it, ty, "call_id")?, field(it, ty, "name")?, field(it, ty, "input")?);
            c.call("custom_tool_call", id)?;
            c.push("assistant", json!({"type":"tool_use","id":id,"name":name,"input":{"input":input}}));
            d.custom = true;
        }
        "function_call_output" | "custom_tool_call_output" => {
            let id = field(it, ty, "call_id")?;
            c.output(ty, id)?;
            let content = match it.get("output") {
                Some(Value::String(x)) => Value::String(x.clone()),
                Some(Value::Array(parts)) => {
                    let mut blocks: Vec<Value> = parts.iter().map(|p| part(p, d)).collect::<Result<_, _>>()?;
                    blocks.retain(|b| !is_empty_text(b));
                    Value::Array(blocks)
                }
                _ => return Err(bad(format!("{ty}: missing output"))),
            };
            c.push("user", json!({"type":"tool_result","tool_use_id":id,"content":content}));
        }
        "reasoning" => match s(it, "encrypted_content").and_then(|v| decode_thinking(v, t)) {
            Some(b) => c.push("assistant", b),
            None => d.reasoning += 1,
        },
        _ if HOSTED_ITEMS.contains(&ty) => *d.items.entry(code_type(ty).into()).or_default() += 1,
        _ => return Err(bad(format!("unknown input item type: {}", cut(ty)))),
    }
    Ok(())
}

/// A `message` item: leading `developer`/`system` text to the system text, a later one as user text in place.
fn message(c: &mut Conv, it: &Value, d: &mut Drops) -> Result<(), ZygoError> {
    let role = field(it, "message", "role")?;
    let mut blocks: Vec<Value> = match it.get("content") {
        Some(Value::String(x)) => vec![text(x)],
        Some(Value::Array(parts)) => parts.iter().map(|p| part(p, d)).collect::<Result<_, _>>()?,
        _ => return Err(bad("message: missing content")),
    };
    blocks.retain(|b| !is_empty_text(b));
    if blocks.is_empty() && matches!(role, "user" | "assistant" | "developer" | "system") {
        return Ok(());
    }
    if role == "user" {
        c.all_answered()?;
    }
    let role = match role {
        "developer" | "system" => {
            if c.leading && blocks.iter().all(|b| s(b, "type") == Some("text")) {
                c.system.extend(blocks.iter().filter_map(|b| s(b, "text")).filter(|x| !x.is_empty()).map(str::to_owned));
                return Ok(());
            }
            "user"
        }
        "user" => "user",
        "assistant" => "assistant",
        other => return Err(bad(format!("message: unknown role: {}", cut(other)))),
    };
    c.leading = false;
    for b in blocks {
        c.push(role, b);
    }
    Ok(())
}

/// One content part as a Messages block (obligation 2).
fn part(p: &Value, d: &mut Drops) -> Result<Value, ZygoError> {
    let ty = s(p, "type").ok_or_else(|| bad("content part: missing type"))?;
    Ok(match ty {
        "input_text" | "output_text" => text(field(p, ty, "text")?),
        "refusal" => text(field(p, ty, "refusal")?),
        "input_image" => {
            let url = field(p, ty, "image_url")?;
            match data_url(url) {
                Some((mt, data)) => json!({"type":"image","source":{"type":"base64","media_type":mt,"data":data}}),
                None => json!({"type":"image","source":{"type":"url","url":url}}),
            }
        }
        "input_file" => match s(p, "file_data").and_then(data_url).filter(|(mt, _)| *mt == "application/pdf") {
            Some((mt, data)) => json!({"type":"document","source":{"type":"base64","media_type":mt,"data":data}}),
            None => {
                d.documents += 1;
                text(DOC_OMITTED)
            }
        },
        _ => return Err(bad(format!("unknown content part type: {}", cut(ty)))),
    })
}

/// `data:<media type>;base64,<data>` as its two parts.
fn data_url(u: &str) -> Option<(&str, &str)> {
    u.strip_prefix("data:")?.split_once(";base64,")
}

/// Tool definitions (obligations 4, 6): `function` and `custom` map; another type is counted and dropped.
fn tools(req: &Value, d: &mut Drops) -> Result<Vec<Value>, ZygoError> {
    let list = match req.get("tools") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad("tools must be an array")),
    };
    let mut out = Vec::new();
    for tool in list {
        let ty = s(tool, "type").ok_or_else(|| bad("tool: missing type"))?;
        let schema = match ty {
            "function" => tool.get("parameters").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({"type":"object"})),
            "custom" => {
                let mut input = json!({"type":"string"});
                if let Some(def) = tool
                    .get("format")
                    .filter(|f| s(f, "type") == Some("grammar"))
                    .and_then(|f| f.get("definition"))
                    .filter(|v| v.is_string())
                {
                    input["description"] = def.clone();
                }
                d.custom = true;
                json!({"type":"object","properties":{"input":input},"required":["input"]})
            }
            other => {
                // The `namespace` drop is RS-9's to replace (flatten into function tools).
                *d.tools.entry(code_type(other).into()).or_default() += 1;
                continue;
            }
        };
        let mut x = json!({"name": field(tool, &format!("{ty} tool"), "name")?, "input_schema": schema});
        if let Some(desc) = tool.get("description").filter(|v| v.is_string()) {
            x["description"] = desc.clone();
        }
        out.push(x);
    }
    Ok(out)
}

/// Messages tool adjacency (K§6.4.3 obligation 3): an assistant message with `tool_use` blocks has no thinking block
/// after its first `tool_use`, and the next message is a user message with a `tool_result` for each of them.
fn adjacency(messages: &[(&'static str, Vec<Value>)]) -> Result<(), ZygoError> {
    for (i, (role, content)) in messages.iter().enumerate() {
        if *role != "assistant" {
            continue;
        }
        let Some(first) = content.iter().position(|b| s(b, "type") == Some("tool_use")) else { continue };
        if content[first..].iter().any(|b| matches!(s(b, "type"), Some("thinking" | "redacted_thinking"))) {
            return Err(bad("input: a reasoning item follows a tool call in the same assistant turn"));
        }
        let answered = |id: &str| match messages.get(i + 1) {
            Some(("user", next)) => {
                next.iter().any(|b| s(b, "type") == Some("tool_result") && s(b, "tool_use_id") == Some(id))
            }
            _ => false,
        };
        let ids = content.iter().filter(|b| s(b, "type") == Some("tool_use")).filter_map(|b| s(b, "id"));
        if !ids.into_iter().all(answered) {
            return Err(bad(
                "input: each tool call's output must come in the next user turn, before other assistant content",
            ));
        }
    }
    Ok(())
}

/// One tool per distinct `tool_use` name in `messages` that `tools` does not define, in first-use order (K§6.4.3
/// obligation 6): Anthropic refuses a `tool_use` block whose name no sent tool defines.
fn history_tools(messages: &[(&'static str, Vec<Value>)], tools: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for b in messages.iter().flat_map(|(_, content)| content).filter(|b| s(b, "type") == Some("tool_use")) {
        let Some(name) = s(b, "name") else { continue };
        if !tools.iter().chain(&out).any(|t| s(t, "name") == Some(name)) {
            out.push(json!({"name":name,"description":"history only","input_schema":{"type":"object"}}));
        }
    }
    out
}

/// `tool_choice` with `parallel_tool_calls` (obligation 7), against the `tools` sent: with none, both are omitted and
/// `required` is a 400; a named tool must be one of them.
fn tool_choice(req: &Value, tools: &[Value]) -> Result<Option<Value>, ZygoError> {
    let mut tc = match req.get("tool_choice") {
        None | Some(Value::Null) => None,
        Some(v) => Some(match (v.as_str(), s(v, "type"), s(v, "name")) {
            (Some("auto"), _, _) => json!({"type":"auto"}),
            (Some("none"), _, _) => json!({"type":"none"}),
            (Some("required"), _, _) => json!({"type":"any"}),
            (None, Some("function" | "custom"), Some(name)) => {
                if !tools.iter().any(|t| s(t, "name") == Some(name)) {
                    return Err(bad("tool_choice: the named tool is not among the sent tools"));
                }
                json!({"type":"tool","name":name})
            }
            _ => return Err(bad("tool_choice: unknown form")),
        }),
    };
    if tools.is_empty() {
        if tc.as_ref().is_some_and(|tc| s(tc, "type") == Some("any")) {
            return Err(bad("tool_choice: required, but no tool is sent"));
        }
        return Ok(None);
    }
    if req.get("parallel_tool_calls").and_then(Value::as_bool) == Some(false) {
        let tc = tc.get_or_insert_with(|| json!({"type":"auto"}));
        if s(tc, "type") != Some("none") {
            tc["disable_parallel_tool_use"] = json!(true);
        }
    }
    Ok(tc)
}
