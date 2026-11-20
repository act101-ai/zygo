//! Messages ↔ Responses (§7.6).

use serde_json::{json, Map, Value};
use zygo_config::Dialect;

use super::reasoning::encode;
use super::{
    copy, input_of, json_schema, message, messages, parts, responses_stop, s, system, texts, tool_choice, tools, usage_of, Choice,
    Drops, Part, Target,
};
use crate::errors::ZygoError;

pub(crate) fn request(req: &Value, t: &Target, d: &mut Drops) -> Result<Value, ZygoError> {
    let mut input = Vec::new();
    for m in messages(req)? {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = m.get("content").unwrap_or(&Value::Null);
        if role == "system" {
            let c: Vec<Value> = texts(content).into_iter().map(|x| json!({"type":"input_text","text":x})).collect();
            if !c.is_empty() {
                input.push(json!({"type":"message","role":"developer","content":c}));
            }
            continue;
        }
        let text_type = if role == "assistant" { "output_text" } else { "input_text" };
        let mut msg: Vec<Value> = Vec::new();
        let flush = |msg: &mut Vec<Value>, input: &mut Vec<Value>| {
            if !msg.is_empty() {
                input.push(json!({"type":"message","role":role,"content":std::mem::take(msg)}));
            }
        };
        for p in parts(content, Dialect::Openai, t, d) {
            match p {
                Part::Text(x) => msg.push(json!({"type":text_type,"text":x})),
                Part::Image(u) => msg.push(json!({"type":"input_image","image_url":u})),
                Part::File(u) => msg.push(file(u)),
                Part::ToolUse(b) => {
                    flush(&mut msg, &mut input);
                    let args = serde_json::to_string(b.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".into());
                    input.push(json!({"type":"function_call","call_id":b["id"],"name":b["name"],"arguments":args}));
                }
                Part::ToolResult(b) => {
                    flush(&mut msg, &mut input);
                    input.push(json!({"type":"function_call_output","call_id":b["tool_use_id"],"output":output(b, t, d)}));
                }
                Part::Reasoning(g) => {
                    flush(&mut msg, &mut input);
                    input.push(json!({"type":"reasoning","id":g.id,"encrypted_content":g.encrypted_content,"summary":[]}));
                }
            }
        }
        flush(&mut msg, &mut input);
    }

    let mut out = Map::new();
    out.insert("model".into(), json!(t.upstream_model));
    if let Some(s) = system(req) {
        out.insert("instructions".into(), json!(s));
    }
    out.insert("input".into(), Value::Array(input));
    copy(req, &mut out, "max_tokens", "max_output_tokens");
    copy(req, &mut out, "temperature", "temperature");
    copy(req, &mut out, "top_p", "top_p");
    copy(req, &mut out, "stream", "stream");
    let tools: Vec<Value> = tools(req, d)
        .into_iter()
        .map(|x| {
            let mut f = json!({"type":"function","name":x.name});
            if let Some(desc) = x.description {
                f["description"] = desc.clone();
            }
            f["parameters"] = x.parameters;
            f["strict"] = json!(false);
            f
        })
        .collect();
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
    }
    let (choice, serial) = tool_choice(req);
    if let Some(c) = choice {
        out.insert(
            "tool_choice".into(),
            match c {
                Choice::Auto => json!("auto"),
                Choice::Required => json!("required"),
                Choice::None => json!("none"),
                Choice::Tool(n) => json!({"type":"function","name":n}),
            },
        );
    }
    if serial {
        out.insert("parallel_tool_calls".into(), json!(false));
    }
    if let Some(schema) = json_schema(req) {
        out.insert("text".into(), json!({"format":{"type":"json_schema","name":"output","schema":schema}}));
    }
    out.insert("store".into(), json!(false));
    out.insert("include".into(), json!(["reasoning.encrypted_content"]));
    Ok(Value::Object(out))
}

fn file(data_url: String) -> Value {
    json!({"type":"input_file","filename":"document.pdf","file_data":data_url})
}

/// A `tool_result`'s content: a string stays a string; blocks become `input_text` / `input_image` / `input_file`.
fn output(b: &Value, t: &Target, d: &mut Drops) -> Value {
    match b.get("content") {
        Some(Value::String(x)) => json!(x),
        Some(c) => Value::Array(
            parts(c, Dialect::Openai, t, d)
                .into_iter()
                .filter_map(|p| match p {
                    Part::Text(x) => Some(json!({"type":"input_text","text":x})),
                    Part::Image(u) => Some(json!({"type":"input_image","image_url":u})),
                    Part::File(u) => Some(file(u)),
                    _ => None,
                })
                .collect(),
        ),
        None => json!(""),
    }
}

pub(crate) fn response(resp: &Value, t: &Target) -> Value {
    let mut content = Vec::new();
    let mut tool = false;
    for item in resp.get("output").and_then(Value::as_array).into_iter().flatten() {
        match s(item, "type") {
            Some("reasoning") => {
                let text: Vec<&str> = item.get("summary").and_then(Value::as_array).into_iter().flatten().filter_map(|x| s(x, "text")).collect();
                let sig = s(item, "encrypted_content").map_or_else(String::new, |e| encode(t, s(item, "id").unwrap_or(""), e));
                content.push(json!({"type":"thinking","thinking":text.join("\n\n"),"signature":sig}));
            }
            Some("message") => {
                for c in item.get("content").and_then(Value::as_array).into_iter().flatten() {
                    match s(c, "type") {
                        Some("output_text") => content.push(json!({"type":"text","text":s(c, "text").unwrap_or("")})),
                        Some("refusal") => content.push(json!({"type":"text","text":s(c, "refusal").unwrap_or("")})),
                        _ => {}
                    }
                }
            }
            Some("function_call") => {
                tool = true;
                content.push(json!({"type":"tool_use","id":s(item, "call_id").unwrap_or(""),"name":s(item, "name").unwrap_or(""),
                    "input":input_of(s(item, "arguments"))}));
            }
            _ => {}
        }
    }
    let reason = resp.get("incomplete_details").and_then(|d| s(d, "reason"));
    let stop = responses_stop(s(resp, "status"), reason, tool);
    message(resp, t, content, stop, usage_of(resp.get("usage"), Dialect::Openai))
}
