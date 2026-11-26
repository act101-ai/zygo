//! Responses client → Messages, and Messages SSE (and JSON) → Responses SSE (and JSON) (K§6.4.3, K§6.7).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{json, Map, Value};
use zygo_config::Facts;
use zygo_core::ids::ulid;

use super::reasoning::{decode_thinking, encode_thinking, MARK_MAX};
use super::sse::SseEvent;
use super::{input_of, s, Target, DOC_OMITTED};
use crate::errors::ZygoError;
use crate::pipeline::Adjustment;
use crate::session::sha256;
use crate::tee::HOLD_MAX;

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

/// A client name or type as an error message may echo it: its first `ECHO_MAX` characters, each control (Cc), format
/// (Cf), line separator (Zl) or paragraph separator (Zp) character replaced with `?`.
fn cut(x: &str) -> String {
    x.chars().take(ECHO_MAX).map(|c| if c.is_control() || is_format_or_separator(c) { '?' } else { c }).collect()
}

/// Unicode 15.1 general categories Cf, Zl and Zp (std has no category lookup).
pub(crate) fn is_format_or_separator(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
        | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
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

/// A Responses body as a Messages body for `t` (K§6.4.3 request obligations 1-11); `facts` = the resolved model's
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
    let text_obj = typed(req, "text", "an object", Value::is_object)?;
    let text_keys = text_obj.and_then(Value::as_object).into_iter().flat_map(|o| o.keys());
    if let Some(k) = text_keys.into_iter().find(|k| !matches!(k.as_str(), "format" | "verbosity")) {
        return Err(bad(format!("unknown field: text.{}", cut(k))));
    }
    // `text.verbosity` has no Messages counterpart: checked, then not sent (K§6.4.3).
    let verbosity = text_obj.and_then(|x| x.get("verbosity")).filter(|v| !v.is_null());
    if verbosity.is_some_and(|v| !matches!(v.as_str(), Some("low" | "medium" | "high"))) {
        return Err(bad("text.verbosity must be one of low, medium, high"));
    }
    let format = text_obj
        .map(|x| typed(x, "format", "an object", Value::is_object).map_err(|_| bad("text.format must be an object")))
        .transpose()?
        .flatten();
    let output_format = output_format(format)?;
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
    let kinds = table(req)?;
    let mut choice = tool_choice(req, &tools, &kinds)?;
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
    if effort.is_some() || output_format.is_some() {
        let mut oc = Map::new();
        if let Some(e) = effort {
            oc.insert("effort".into(), e.clone());
        }
        if let Some(f) = output_format {
            oc.insert("format".into(), f);
        }
        out.insert("output_config".into(), Value::Object(oc));
    }
    Ok((Value::Object(out), d.codes()))
}

/// The Messages `output_config.format` for a type-checked `text.format` (K§6.4.3 request obligation 11): `None` for
/// an absent format or type `text`; `{type: json_schema, schema}` for type `json_schema`, whose `name`, `strict` and
/// `description` are checked, then not sent; any other type → 400.
fn output_format(format: Option<&Value>) -> Result<Option<Value>, ZygoError> {
    let Some(f) = format else { return Ok(None) };
    match s(f, "type") {
        Some("text") => return Ok(None),
        Some("json_schema") => {}
        _ => return Err(bad("text.format: only types text and json_schema are accepted toward this upstream")),
    }
    let obj = f.as_object().into_iter().flat_map(|o| o.keys());
    if let Some(k) = obj.into_iter().find(|k| !matches!(k.as_str(), "type" | "name" | "schema" | "strict" | "description")) {
        return Err(bad(format!("unknown field: text.format.{}", cut(k))));
    }
    let schema = match f.get("schema") {
        None | Some(Value::Null) => return Err(bad("text.format: missing schema")),
        Some(x) if x.is_object() => x.clone(),
        Some(_) => return Err(bad("text.format.schema must be an object")),
    };
    for k in ["name", "description"] {
        if f.get(k).is_some_and(|v| !v.is_null() && !v.is_string()) {
            return Err(bad(format!("text.format.{k} must be a string")));
        }
    }
    if f.get("strict").is_some_and(|v| !v.is_null() && !v.is_boolean()) {
        return Err(bad("text.format.strict must be a boolean"));
    }
    Ok(Some(json!({"type": "json_schema", "schema": schema})))
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
            let name = match it.get("namespace") {
                None | Some(Value::Null) => name.to_owned(),
                Some(Value::String(ns)) => flat_name(ns, name),
                Some(_) => return Err(bad("function_call: namespace must be a string")),
            };
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

/// The Messages and chat tool name of the tool `name` inside the `namespace` tool `ns` (K§6.4.3 request obligation 6):
/// `<ns>__<name>` when that is 1-64 characters of `[A-Za-z0-9_-]`; else that string with each other character
/// replaced by `_`, cut to 47 characters, then `_` and the first 16 hex digits of SHA-256 of
/// `<len(ns)>:<ns><len(name)>:<name>` (lengths in UTF-8 bytes, decimal), so no character of a part moves the boundary.
/// A pure function of the pair, so a definition and a history call agree with no proxy state; it is never parsed
/// back (`Calls` holds the reverse table).
pub fn flat_name(ns: &str, name: &str) -> String {
    let plain = format!("{ns}__{name}");
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    if plain.len() <= 64 && plain.chars().all(ok) {
        return plain;
    }
    let mut cut: String = plain.chars().map(|c| if ok(c) { c } else { '_' }).take(47).collect();
    let h = hex::encode(sha256(&format!("{}:{ns}{}:{name}", ns.len(), name.len())));
    cut.push('_');
    cut.push_str(&h[..16]);
    cut
}

/// Push the Messages tool `name` onto `out`; a name already sent → 400 (obligation 6).
fn add_tool(out: &mut Vec<Value>, name: &str, schema: Value, desc: Option<&Value>) -> Result<(), ZygoError> {
    if out.iter().any(|t: &Value| s(t, "name") == Some(name)) {
        return Err(bad("tools: two tools have the same name"));
    }
    let mut x = json!({"name": name, "input_schema": schema});
    if let Some(desc) = desc.filter(|v| v.is_string()) {
        x["description"] = desc.clone();
    }
    out.push(x);
    Ok(())
}

/// A function tool's `parameters`, else `{"type":"object"}`.
fn parameters(tool: &Value) -> Value {
    tool.get("parameters").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({"type":"object"}))
}

/// A `namespace` tool's nested `function` tools under their flat names (obligation 6); another nested type is
/// counted and dropped.
fn namespace(tool: &Value, out: &mut Vec<Value>, d: &mut Drops) -> Result<(), ZygoError> {
    let ns = field(tool, "namespace tool", "name")?;
    let nested = tool.get("tools").and_then(Value::as_array).ok_or_else(|| bad("namespace tool: missing tools"))?;
    for x in nested {
        match s(x, "type").ok_or_else(|| bad("tool: missing type"))? {
            "function" => {
                let n = field(x, "function tool", "name")?;
                add_tool(out, &flat_name(ns, n), parameters(x), x.get("description"))?;
            }
            other => *d.tools.entry(code_type(other).into()).or_default() += 1,
        }
    }
    Ok(())
}

/// Tool definitions (obligations 4, 6): `function` and `custom` map; a `namespace` flattens; another type is counted
/// and dropped.
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
            "function" => parameters(tool),
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
            "namespace" => {
                namespace(tool, &mut out, d)?;
                continue;
            }
            other => {
                *d.tools.entry(code_type(other).into()).or_default() += 1;
                continue;
            }
        };
        let name = field(tool, &format!("{ty} tool"), "name")?;
        add_tool(&mut out, name, schema, tool.get("description"))?;
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
/// `required` is a 400; a named tool must be one of them. `{type: function, name, namespace}` names a sent nested tool
/// by its pair (`kinds`, the request's `table`): a plain or custom tool of the flat name is not it.
fn tool_choice(req: &Value, tools: &[Value], kinds: &HashMap<String, ToolKind>) -> Result<Option<Value>, ZygoError> {
    let mut tc = match req.get("tool_choice") {
        None | Some(Value::Null) => None,
        Some(v) => Some(match (v.as_str(), s(v, "type"), s(v, "name")) {
            (Some("auto"), _, _) => json!({"type":"auto"}),
            (Some("none"), _, _) => json!({"type":"none"}),
            (Some("required"), _, _) => json!({"type":"any"}),
            (None, Some(ty @ ("function" | "custom")), Some(name)) => {
                let name = match v.get("namespace") {
                    None | Some(Value::Null) => name.to_owned(),
                    Some(Value::String(_)) if ty == "custom" => {
                        return Err(bad("tool_choice: a custom tool has no namespace"))
                    }
                    Some(Value::String(ns)) => {
                        let flat = flat_name(ns, name);
                        if kinds.get(&flat) != Some(&ToolKind::Namespaced(ns.clone(), name.to_owned())) {
                            return Err(bad("tool_choice: the named tool is not among the sent tools"));
                        }
                        flat
                    }
                    Some(_) => return Err(bad("tool_choice: namespace must be a string")),
                };
                if !tools.iter().any(|t| s(t, "name") == Some(name.as_str())) {
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

// ---- Messages → Responses (K§6.4.3 stream obligations 1-9) ----

/// The kind of tool a Messages tool name stands for (K§6.4.3 request obligation 6, stream obligation 2).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolKind {
    Function,
    /// A client `custom` tool: a call returns as a `custom_tool_call` item.
    Custom,
    /// A tool inside a client `namespace` tool, as (namespace, name): a call returns as a `function_call` item with
    /// `name` and `namespace`.
    Namespaced(String, String),
}

/// Each Messages tool name of the request and its kind, from `tools[]` (`function`, `custom`, and the nested
/// `function` tools of `namespace`, under the flat name) and from `input[]` (`function_call`, under the flat name
/// when it has a string `namespace`; `custom_tool_call`). A name of a `custom` tool and of a function is `Custom` (a
/// custom history call of a sent function's name returns as custom). A plain `function_call` of the flat name of a
/// pair the request sends in `tools` is that tool (Codex before 0.172 named MCP tools `mcp__<server>__<tool>`; a
/// session resumed across an upgrade holds such calls), so it binds to the pair. Any other name of a namespaced pair
/// and of anything else (a plain tool, another pair, or a plain call with no sent pair) → `400 "input: two tools have
/// the same name"` (fail closed: the call could not return under one kind). Malformed entries, which `request`
/// refuses, are ignored.
fn table(req: &Value) -> Result<HashMap<String, ToolKind>, ZygoError> {
    fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
        v.get(k).and_then(Value::as_array).map_or(&[][..], Vec::as_slice)
    }
    let mut out: HashMap<String, ToolKind> = HashMap::new();
    // The flat names of the nested tools sent in `tools`.
    let mut sent: HashSet<String> = HashSet::new();
    let mut add = |name: String, kind: ToolKind| -> Result<(), ZygoError> {
        use ToolKind::{Custom, Function};
        let merged = match (out.get(&name), &kind) {
            (None, _) => kind,
            (Some(old), _) if *old == kind => kind,
            (Some(Function | Custom), Function | Custom) => Custom,
            _ => return Err(bad("input: two tools have the same name")),
        };
        out.insert(name, merged);
        Ok(())
    };
    for t in list(req, "tools") {
        match (s(t, "type"), s(t, "name")) {
            (Some("function"), Some(n)) => add(n.to_owned(), ToolKind::Function)?,
            (Some("custom"), Some(n)) => add(n.to_owned(), ToolKind::Custom)?,
            (Some("namespace"), Some(ns)) => {
                for x in list(t, "tools").iter().filter(|x| s(x, "type") == Some("function")) {
                    if let Some(n) = s(x, "name") {
                        sent.insert(flat_name(ns, n));
                        add(flat_name(ns, n), ToolKind::Namespaced(ns.to_owned(), n.to_owned()))?;
                    }
                }
            }
            _ => {}
        }
    }
    for it in list(req, "input") {
        match (s(it, "type"), s(it, "name"), s(it, "namespace")) {
            (Some("function_call"), Some(n), Some(ns)) => {
                add(flat_name(ns, n), ToolKind::Namespaced(ns.to_owned(), n.to_owned()))?
            }
            (Some("function_call"), Some(n), None) if sent.contains(n) => {}
            (Some("function_call"), Some(n), None) => add(n.to_owned(), ToolKind::Function)?,
            (Some("custom_tool_call"), Some(n), _) => add(n.to_owned(), ToolKind::Custom)?,
            _ => {}
        }
    }
    Ok(out)
}

/// What the client's request says about its tools that the call items need (K§6.4.3 stream obligation 2).
#[derive(Debug, Clone, Default)]
pub struct Calls {
    /// Each Messages tool name of the request and its kind (`table`); the one source for the item and its events.
    kinds: HashMap<String, ToolKind>,
}

impl Calls {
    /// The request's tool names and kinds (`table`: `tools[]` and the call items of `input[]`). A request that
    /// `request` refuses for two tools of one name gives no kinds (every call returns as a plain `function_call`).
    pub fn of(req: &Value) -> Calls {
        Calls { kinds: table(req).unwrap_or_default() }
    }

    /// The kind of the tool `name`; a name the request does not hold is a function.
    fn kind(&self, name: &str) -> &ToolKind {
        self.kinds.get(name).unwrap_or(&ToolKind::Function)
    }

    fn is_custom(&self, name: &str) -> bool {
        *self.kind(name) == ToolKind::Custom
    }

    /// The only builder of a call item: a `tool_use` with id `call_id`, name `name` and arguments `args` (JSON text)
    /// as a `function_call` item, with the nested `name` and the `namespace` when `name` is the flat name of a client
    /// namespaced tool, or a `custom_tool_call` item for a client `custom` tool. `None` gives the
    /// `output_item.added` form.
    fn item(&self, ulid: &str, oi: usize, call_id: &str, name: &str, args: Option<&str>) -> Value {
        let status = if args.is_some() { "completed" } else { "in_progress" };
        match self.kind(name) {
            ToolKind::Namespaced(ns, n) => json!({"type":"function_call","id":item_id("fc", ulid, oi),
                "call_id":call_id,"name":n,"namespace":ns,"arguments":args.unwrap_or(""),"status":status}),
            ToolKind::Custom => {
                let input = args.map_or_else(String::new, custom_input);
                json!({"type":"custom_tool_call","id":item_id("ctc", ulid, oi),"call_id":call_id,"name":name,
                    "input":input,"status":status})
            }
            ToolKind::Function => json!({"type":"function_call","id":item_id("fc", ulid, oi),"call_id":call_id,
                "name":name,"arguments":args.unwrap_or(""),"status":status}),
        }
    }
}

/// A custom tool call's `input`: the string field `input` of the arguments, else the arguments as received.
fn custom_input(args: &str) -> String {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| s(&v, "input").map(str::to_owned))
        .unwrap_or_else(|| args.to_owned())
}

fn item_id(p: &str, ulid: &str, oi: usize) -> String {
    format!("{p}_{ulid}_{oi}")
}

/// A `message` item; `None` gives the `output_item.added` form.
fn text_item(id: &str, text: Option<&str>) -> Value {
    match text {
        Some(t) => json!({"type":"message","id":id,"role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":t,"annotations":[]}]}),
        None => json!({"type":"message","id":id,"role":"assistant","status":"in_progress","content":[]}),
    }
}

/// The `output_item.added` form of a `reasoning` item.
fn reasoning_added(id: &str) -> Value {
    json!({"type":"reasoning","id":id,"summary":[]})
}

/// A `reasoning` item that carries `block` (a `thinking` or `redacted_thinking` block) zygo-marked for `t`, with
/// `summary` as its summary text (none when empty). A mark over `MARK_MAX` would not decode (K§6.7): the item then has
/// no `encrypted_content` and `unmarked` counts it (stream obligation 2).
fn reasoning_item(id: &str, summary: &str, block: &Value, t: &Target, unmarked: &mut usize) -> Value {
    let summary = if summary.is_empty() { json!([]) } else { json!([{"type":"summary_text","text":summary}]) };
    let mut item = json!({"type":"reasoning","id":id,"summary":summary});
    let mark = encode_thinking(t, block);
    if mark.len() > MARK_MAX {
        *unmarked += 1;
    } else {
        item["encrypted_content"] = json!(mark);
    }
    item
}

fn thinking_block(thinking: &str, signature: &str) -> Value {
    json!({"type":"thinking","thinking":thinking,"signature":signature})
}

fn redacted_block(data: &str) -> Value {
    json!({"type":"redacted_thinking","data":data})
}

/// Messages usage: input, cache read, cache write, output (absent = 0).
type Usage = [u64; 4];

fn usage_fields(u: Option<&Value>) -> Usage {
    let n = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    [n("input_tokens"), n("cache_read_input_tokens"), n("cache_creation_input_tokens"), n("output_tokens")]
}

/// Responses usage (stream obligation 5).
fn response_usage(u: Usage) -> Value {
    let input = u[0] + u[1] + u[2];
    json!({"input_tokens":input,"input_tokens_details":{"cached_tokens":u[1]},"output_tokens":u[3],
        "total_tokens":input + u[3]})
}

/// The Response object with no output.
fn response_object(ulid: &str, model: &str, status: &str) -> Value {
    json!({"id":format!("resp_{ulid}"),"object":"response","model":model,"status":status,"output":[]})
}

/// The terminal form of R for `stop_reason` (stream obligation 5): `max_tokens` and
/// `model_context_window_exceeded` → incomplete with `max_output_tokens`, `refusal` → incomplete with
/// `content_filter`, any other (`pause_turn` too: its server tools are dropped) → completed.
fn finished(mut r: Value, stop_reason: Option<&str>, u: Usage) -> Value {
    let reason = match stop_reason {
        Some("max_tokens" | "model_context_window_exceeded") => Some("max_output_tokens"),
        Some("refusal") => Some("content_filter"),
        _ => None,
    };
    if let Some(reason) = reason {
        r["status"] = json!("incomplete");
        r["incomplete_details"] = json!({"reason":reason});
    } else {
        r["status"] = json!("completed");
    }
    r["usage"] = response_usage(u);
    r
}

/// A server tool block, dropped with `server_tool_dropped` (stream obligation 3).
fn is_server_tool(ty: &str) -> bool {
    ty == "server_tool_use" || ty.ends_with("_tool_result")
}

/// The adjustment codes of one response: `server_tool_dropped` (obligation 3), `reasoning_unmarked:<n>` (obligation
/// 2).
fn codes(dropped: bool, unmarked: usize) -> Vec<Adjustment> {
    let mut out = Vec::new();
    if dropped {
        out.push(Adjustment("server_tool_dropped".into()));
    }
    if unmarked > 0 {
        out.push(Adjustment(format!("reasoning_unmarked:{unmarked}")));
    }
    out
}

/// The `response.failed` messages of obligation 9, by the held bytes that would pass `HOLD_MAX`: a partial line, the
/// current event's `data` lines, a text or thinking block's text, a tool call's arguments.
const LINE_TOO_LONG: &str = "upstream stream line too long";
const EVENT_TOO_LONG: &str = "upstream event too long";
const BLOCK_TOO_LONG: &str = "upstream content block too long";
const CALL_TOO_LONG: &str = "upstream tool call too long";

/// The Messages event types the encoder reads; any other (`ping` too) is dropped (stream obligation 7).
const EVENTS: [&str; 7] = [
    "message_start",
    "content_block_start",
    "content_block_delta",
    "content_block_stop",
    "message_delta",
    "message_stop",
    "error",
];

/// A Messages body as one Response object (stream obligation 8), with its adjustment codes; the items are those of
/// the stream's `output_item.done` events.
pub fn response(msg: &Value, model: &str, t: &Target, calls: &Calls) -> (Value, Vec<Adjustment>) {
    let ulid = ulid();
    let mut output = Vec::new();
    let (mut dropped, mut unmarked) = (false, 0);
    for b in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
        let oi = output.len();
        let str_of = |k: &str| s(b, k).unwrap_or("");
        match str_of("type") {
            "text" => output.push(text_item(&item_id("msg", &ulid, oi), Some(str_of("text")))),
            "thinking" => {
                let block = thinking_block(str_of("thinking"), str_of("signature"));
                output.push(reasoning_item(&item_id("rs", &ulid, oi), str_of("thinking"), &block, t, &mut unmarked));
            }
            "redacted_thinking" => {
                let block = redacted_block(str_of("data"));
                output.push(reasoning_item(&item_id("rs", &ulid, oi), "", &block, t, &mut unmarked));
            }
            "tool_use" => {
                let args = b.get("input").map_or_else(|| "{}".to_owned(), Value::to_string);
                output.push(calls.item(&ulid, oi, str_of("id"), str_of("name"), Some(&args)));
            }
            ty => dropped |= is_server_tool(ty),
        }
    }
    let u = usage_fields(msg.get("usage"));
    let mut r = finished(response_object(&ulid, model, "completed"), s(msg, "stop_reason"), u);
    r["output"] = Value::Array(output);
    (r, codes(dropped, unmarked))
}

enum Kind {
    Text,
    Thinking,
    Redacted,
    /// A tool call; `delta`: a non-empty `input_json_delta` came (until then the block's `text` holds the start's
    /// `input` as compact JSON).
    Call { custom: bool, call_id: String, name: String, delta: bool },
}

/// The open content block: its Messages `index`, its item's `output_index` and id, and its held text, thinking,
/// redacted data or arguments (`text`) and signature (`sig`).
struct Block {
    index: u64,
    oi: usize,
    iid: String,
    kind: Kind,
    text: String,
    sig: String,
    /// A thinking block's summary part was opened.
    part: bool,
}

impl Block {
    fn held(&self) -> usize {
        self.text.len() + self.sig.len()
    }
}

/// Messages SSE → Responses SSE for one response (K§6.4.3 stream obligations 1-7, 9).
///
/// The event sequence it emits, kept in this one place (PC-12 serves exactly this from the mock). Every event's
/// `data` has `type` = the event name and `sequence_number` rising by one from 0. R = `{id: "resp_<ULID>", object:
/// "response", model: <client model>, status, output: []}`; `oi` = `output_index`, counted over emitted items from 0;
/// `iid` = `item_id`, `<msg|rs|fc|ctc>_<ULID>_<oi>`.
///
/// 1. Start, at the first event that is not `ping` or of unknown type: `response.created {response: R}`,
///    `response.in_progress {response: R}`, R `status` `in_progress`.
/// 2. Per content block, in order:
///    - `text`: `response.output_item.added {oi, item}`; `response.content_part.added {iid, oi, content_index: 0,
///      part}`; per non-empty `text_delta` `response.output_text.delta {iid, oi, content_index: 0, delta}`; at stop
///      `response.output_text.done {iid, oi, content_index: 0, text}`, `response.content_part.done {iid, oi,
///      content_index: 0, part}`, `response.output_item.done {oi, item}`.
///    - `thinking`: `response.output_item.added`; at the first non-empty `thinking_delta`
///      `response.reasoning_summary_part.added {iid, oi, summary_index: 0, part}`, then per non-empty delta
///      `response.reasoning_summary_text.delta {iid, oi, summary_index: 0, delta}`; at stop, if a part was opened,
///      `response.reasoning_summary_text.done {…, text}`, `response.reasoning_summary_part.done {…, part}`; then
///      `response.output_item.done`.
///    - `redacted_thinking`: `response.output_item.added`, `response.output_item.done`.
///    - A reasoning item whose mark would pass `MARK_MAX` has no `encrypted_content`; adjustment
///      `reasoning_unmarked:<n>`.
///    - `tool_use`: `response.output_item.added`; per non-empty `input_json_delta`
///      `response.function_call_arguments.delta {iid, oi, delta}`; at stop `response.function_call_arguments.done
///      {iid, oi, arguments}`, `response.output_item.done`. A flat name of a client namespaced tool returns with
///      the nested `name` and the `namespace` (K§6.4.3 stream obligation 2).
///    - `tool_use` of a client `custom` tool (in `tools` or a `custom_tool_call` of `input`):
///      `response.output_item.added`; at stop
///      `response.custom_tool_call_input.delta {iid, oi, delta: input}`, `response.custom_tool_call_input.done
///      {iid, oi, input}`, `response.output_item.done`.
///    - `server_tool_use`, `*_tool_result`: no event, adjustment `server_tool_dropped` (obligation 3). Another block
///      type: no event.
/// 3. Exactly one terminal event. At `message_stop`: `response.completed {response: R, status completed, usage}`, or
///    `response.incomplete {response: R, status incomplete, incomplete_details: {reason}, usage}` with reason
///    `max_output_tokens` for `stop_reason` `max_tokens` or `model_context_window_exceeded` and `content_filter` for
///    `refusal` (`pause_turn` and the rest complete). On an upstream `error` event (its message cut as a 400 echo),
///    unparsable `data`, held bytes over `HOLD_MAX` (the message names the largest held part: stream line, event,
///    content block, tool call), `fail` or an early end: `response.failed {response: R, status failed, error: {code,
///    message}}`.
///    R's `output` stays `[]`: each item reached the client in its `response.output_item.done` (obligation 5).
pub struct Encoder {
    model: String,
    t: Target,
    calls: Calls,
    ulid: String,
    seq: u64,
    started: bool,
    ended: bool,
    /// The bytes after the last newline: a partial line.
    buf: Vec<u8>,
    /// The current event's `data` lines, joined with `\n`.
    data: Option<Vec<u8>>,
    open: Option<Block>,
    next_oi: usize,
    usage: Usage,
    stop_reason: Option<String>,
    dropped: bool,
    unmarked: usize,
}

impl Encoder {
    pub fn new(model: &str, t: &Target, calls: Calls) -> Encoder {
        Encoder {
            model: model.to_owned(),
            t: t.clone(),
            calls,
            ulid: ulid(),
            seq: 0,
            started: false,
            ended: false,
            buf: Vec::new(),
            data: None,
            open: None,
            next_oi: 0,
            usage: [0; 4],
            stop_reason: None,
            dropped: false,
            unmarked: 0,
        }
    }

    /// Messages SSE bytes in, Responses events out. Each byte is scanned once. Only a partial line is copied whole;
    /// a complete line is read from `chunk`. Before any byte is held, the held total is checked against `HOLD_MAX`
    /// (obligation 9).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        let mut rest = chunk;
        while !self.ended {
            let Some(i) = rest.iter().position(|&b| b == b'\n') else {
                if self.hold([rest.len(), 0, 0], &mut out) {
                    self.buf.extend_from_slice(rest);
                }
                break;
            };
            let head = &rest[..i];
            rest = &rest[i + 1..];
            if self.buf.is_empty() {
                self.line(head, &mut out);
            } else if self.hold([head.len(), 0, 0], &mut out) {
                self.buf.extend_from_slice(head);
                let line = std::mem::take(&mut self.buf);
                self.line(&line, &mut out);
            }
        }
        out
    }

    /// One `response.failed` when the stream started and has not ended; else nothing (before the start the caller
    /// owns the HTTP status, K§6.4.4).
    pub fn fail(&mut self, code: &str, message: &str) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if self.started && !self.ended {
            self.failed(code, message, &mut out);
        }
        out
    }

    /// The held tail as a last line and event; with no terminal event then, `response.failed` `upstream stream ended
    /// early` (after a start, when none came).
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        let tail = std::mem::take(&mut self.buf);
        if !tail.is_empty() {
            self.line(&tail, &mut out);
        }
        if let Some(d) = self.data.take().filter(|_| !self.ended) {
            self.dispatch(&d, &mut out);
        }
        if !self.ended {
            self.failed("api_error", "upstream stream ended early", &mut out);
        }
        out
    }

    /// A terminal event was emitted.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The held bytes: the partial line, the current event's `data` and the open item's text, thinking or arguments.
    pub fn held(&self) -> usize {
        self.buf.len() + self.data.as_ref().map_or(0, Vec::len) + self.open.as_ref().map_or(0, Block::held)
    }

    /// `server_tool_dropped`, once, when a server tool block was dropped (obligation 3); `reasoning_unmarked:<n>` for
    /// the reasoning items sent with no mark (obligation 2).
    pub fn adjustments(&self) -> Vec<Adjustment> {
        codes(self.dropped, self.unmarked)
    }

    /// One line, `\r` stripped: a `data` line is held, a blank line dispatches the held event, others are ignored.
    fn line(&mut self, line: &[u8], out: &mut Vec<SseEvent>) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            if let Some(d) = self.data.take() {
                self.dispatch(&d, out);
            }
        } else if let Some(d) = line.strip_prefix(b"data:") {
            let d = d.strip_prefix(b" ").unwrap_or(d);
            if !self.hold([0, d.len() + usize::from(self.data.is_some()), 0], out) {
                return;
            }
            match &mut self.data {
                Some(held) => {
                    held.push(b'\n');
                    held.extend_from_slice(d);
                }
                None => self.data = Some(d.to_vec()),
            }
        }
    }

    /// Whether `extra` more bytes of the partial line, the current event's `data` and the open block can be held
    /// within `HOLD_MAX` (obligation 9). If not, everything held is freed and `response.failed` names the largest held
    /// part.
    fn hold(&mut self, extra: [usize; 3], out: &mut Vec<SseEvent>) -> bool {
        let block = self.open.as_ref();
        let [line, data, held] = [
            self.buf.len() + extra[0],
            self.data.as_ref().map_or(0, Vec::len) + extra[1],
            block.map_or(0, Block::held) + extra[2],
        ];
        if line.saturating_add(data).saturating_add(held) <= HOLD_MAX {
            return true;
        }
        let m = match block {
            Some(b) if held >= line.max(data) && matches!(b.kind, Kind::Call { .. }) => CALL_TOO_LONG,
            Some(_) if held >= line.max(data) => BLOCK_TOO_LONG,
            _ if data >= line => EVENT_TOO_LONG,
            _ => LINE_TOO_LONG,
        };
        self.failed("api_error", m, out);
        false
    }

    fn free(&mut self) {
        self.buf = Vec::new();
        self.data = None;
        self.open = None;
    }

    fn emit(&mut self, name: &str, mut data: Value, out: &mut Vec<SseEvent>) {
        data["type"] = json!(name);
        data["sequence_number"] = json!(self.seq);
        self.seq += 1;
        out.push(SseEvent { event: name.to_owned(), data });
    }

    fn r(&self, status: &str) -> Value {
        response_object(&self.ulid, &self.model, status)
    }

    fn start(&mut self, out: &mut Vec<SseEvent>) {
        self.started = true;
        let r = self.r("in_progress");
        self.emit("response.created", json!({"response":r}), out);
        self.emit("response.in_progress", json!({"response":r}), out);
    }

    /// Frees all held bytes, starts the stream if needed, emits `response.failed` and ends it (obligation 6).
    fn failed(&mut self, code: &str, message: &str, out: &mut Vec<SseEvent>) {
        self.free();
        if !self.started {
            self.start(out);
        }
        let mut r = self.r("failed");
        r["error"] = json!({"code":code,"message":message});
        self.emit("response.failed", json!({"response":r}), out);
        self.ended = true;
    }

    /// One whole event's `data`, by its `type` (obligations 2-7).
    fn dispatch(&mut self, d: &[u8], out: &mut Vec<SseEvent>) {
        let Ok(v) = serde_json::from_slice::<Value>(d) else {
            return self.failed("api_error", "upstream event is not JSON", out);
        };
        let ty = s(&v, "type").unwrap_or("");
        if !EVENTS.contains(&ty) {
            return;
        }
        if !self.started {
            self.start(out);
        }
        let index = v.get("index").and_then(Value::as_u64);
        match ty {
            "message_start" => self.usage = usage_fields(v.get("message").and_then(|m| m.get("usage"))),
            "content_block_start" => {
                self.close(out);
                if let (Some(i), Some(b)) = (index, v.get("content_block")) {
                    self.open_block(i, b, out);
                }
            }
            "content_block_delta" => {
                if let (Some(i), Some(d)) = (index, v.get("delta")) {
                    self.delta(i, d, out);
                }
            }
            "content_block_stop" => {
                if self.open.as_ref().is_some_and(|b| Some(b.index) == index) {
                    self.close(out);
                }
            }
            "message_delta" => {
                if let Some(r) = v.get("delta").and_then(|d| s(d, "stop_reason")) {
                    self.stop_reason = Some(r.to_owned());
                }
                for (have, new) in self.usage.iter_mut().zip(usage_fields(v.get("usage"))) {
                    *have = (*have).max(new);
                }
            }
            "message_stop" => {
                self.close(out);
                let r = finished(self.r("in_progress"), self.stop_reason.as_deref(), self.usage);
                let name = if r["status"] == "incomplete" { "response.incomplete" } else { "response.completed" };
                self.emit(name, json!({"response":r}), out);
                self.ended = true;
            }
            _ => {
                let e = v.get("error");
                let code = e.and_then(|e| s(e, "type")).filter(|c| is_code(c)).unwrap_or("api_error");
                let message = cut(e.and_then(|e| s(e, "message")).unwrap_or(""));
                self.failed(code, &message, out);
            }
        }
    }

    fn open_block(&mut self, index: u64, cb: &Value, out: &mut Vec<SseEvent>) {
        let str_of = |k: &str| s(cb, k).unwrap_or("");
        let oi = self.next_oi;
        let rs = || reasoning_added(&item_id("rs", &self.ulid, oi));
        let (kind, text, item) = match str_of("type") {
            "text" => (Kind::Text, String::new(), text_item(&item_id("msg", &self.ulid, oi), None)),
            "thinking" => (Kind::Thinking, String::new(), rs()),
            "redacted_thinking" => (Kind::Redacted, str_of("data").to_owned(), rs()),
            "tool_use" => {
                let (call_id, name) = (str_of("id").to_owned(), str_of("name").to_owned());
                let item = self.calls.item(&self.ulid, oi, &call_id, &name, None);
                let args = cb.get("input").map_or_else(|| "{}".to_owned(), Value::to_string);
                (Kind::Call { custom: self.calls.is_custom(&name), call_id, name, delta: false }, args, item)
            }
            ty => {
                self.dropped |= is_server_tool(ty);
                return;
            }
        };
        self.next_oi += 1;
        let iid = s(&item, "id").unwrap_or("").to_owned();
        let is_text = matches!(kind, Kind::Text);
        self.open = Some(Block { index, oi, iid: iid.clone(), kind, text, sig: String::new(), part: false });
        self.emit("response.output_item.added", json!({"output_index":oi,"item":item}), out);
        if is_text {
            let part = json!({"type":"output_text","text":"","annotations":[]});
            let ev = json!({"item_id":iid,"output_index":oi,"content_index":0,"part":part});
            self.emit("response.content_part.added", ev, out);
        }
    }

    /// A delta of the open block; another index, a delta type that is not the block's, or an empty value is dropped.
    /// A value that would take the held bytes past `HOLD_MAX` ends the stream (obligation 9).
    fn delta(&mut self, index: u64, d: &Value, out: &mut Vec<SseEvent>) {
        let Some(b) = self.open.as_ref().filter(|b| b.index == index) else { return };
        let key = match (&b.kind, s(d, "type")) {
            (Kind::Text, Some("text_delta")) => "text",
            (Kind::Thinking, Some("thinking_delta")) => "thinking",
            (Kind::Thinking, Some("signature_delta")) => "signature",
            (Kind::Call { .. }, Some("input_json_delta")) => "partial_json",
            _ => return,
        };
        let Some(x) = s(d, key).filter(|x| !x.is_empty()) else { return };
        if !self.hold([0, 0, x.len()], out) {
            return;
        }
        let Some(mut b) = self.open.take() else { return };
        let (iid, oi) = (b.iid.clone(), b.oi);
        match (key, &mut b.kind) {
            ("text", _) => {
                b.text.push_str(x);
                let ev = json!({"item_id":iid,"output_index":oi,"content_index":0,"delta":x});
                self.emit("response.output_text.delta", ev, out);
            }
            ("thinking", _) => {
                if !b.part {
                    b.part = true;
                    let part = json!({"type":"summary_text","text":""});
                    let ev = json!({"item_id":iid,"output_index":oi,"summary_index":0,"part":part});
                    self.emit("response.reasoning_summary_part.added", ev, out);
                }
                b.text.push_str(x);
                let ev = json!({"item_id":iid,"output_index":oi,"summary_index":0,"delta":x});
                self.emit("response.reasoning_summary_text.delta", ev, out);
            }
            ("signature", _) => b.sig.push_str(x),
            (_, Kind::Call { custom, delta, .. }) => {
                if !*delta {
                    *delta = true;
                    b.text.clear();
                }
                b.text.push_str(x);
                if !*custom {
                    let ev = json!({"item_id":iid,"output_index":oi,"delta":x});
                    self.emit("response.function_call_arguments.delta", ev, out);
                }
            }
            _ => {}
        }
        self.open = Some(b);
    }

    /// The open block's done events, if one is open.
    fn close(&mut self, out: &mut Vec<SseEvent>) {
        let Some(b) = self.open.take() else { return };
        let (iid, oi) = (b.iid.as_str(), b.oi);
        let item = match &b.kind {
            Kind::Text => {
                let ev = json!({"item_id":iid,"output_index":oi,"content_index":0,"text":b.text});
                self.emit("response.output_text.done", ev, out);
                let part = json!({"type":"output_text","text":b.text,"annotations":[]});
                let ev = json!({"item_id":iid,"output_index":oi,"content_index":0,"part":part});
                self.emit("response.content_part.done", ev, out);
                text_item(iid, Some(&b.text))
            }
            Kind::Thinking => {
                if b.part {
                    let ev = json!({"item_id":iid,"output_index":oi,"summary_index":0,"text":b.text});
                    self.emit("response.reasoning_summary_text.done", ev, out);
                    let part = json!({"type":"summary_text","text":b.text});
                    let ev = json!({"item_id":iid,"output_index":oi,"summary_index":0,"part":part});
                    self.emit("response.reasoning_summary_part.done", ev, out);
                }
                reasoning_item(iid, &b.text, &thinking_block(&b.text, &b.sig), &self.t, &mut self.unmarked)
            }
            Kind::Redacted => reasoning_item(iid, "", &redacted_block(&b.text), &self.t, &mut self.unmarked),
            Kind::Call { custom, call_id, name, .. } => {
                if *custom {
                    let input = custom_input(&b.text);
                    let ev = json!({"item_id":iid,"output_index":oi,"delta":input});
                    self.emit("response.custom_tool_call_input.delta", ev, out);
                    let ev = json!({"item_id":iid,"output_index":oi,"input":input});
                    self.emit("response.custom_tool_call_input.done", ev, out);
                } else {
                    let ev = json!({"item_id":iid,"output_index":oi,"arguments":b.text});
                    self.emit("response.function_call_arguments.done", ev, out);
                }
                self.calls.item(&self.ulid, oi, call_id, name, Some(&b.text))
            }
        };
        self.emit("response.output_item.done", json!({"output_index":oi,"item":item}), out);
    }
}

/// An upstream error type usable as a Responses error `code`: 1-32 characters of `[a-z0-9_]`.
fn is_code(c: &str) -> bool {
    (1..=32).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
