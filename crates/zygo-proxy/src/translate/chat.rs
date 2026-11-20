//! Messages ↔ chat/completions (§7.6).

use serde_json::{json, Map, Value};
use zygo_config::Dialect;

use super::{
    chat_stop, copy, input_of, json_schema, message, messages, parts, s, system, texts, tool_choice, tools, usage_of, Choice, Drops,
    Part, Target,
};
use crate::errors::ZygoError;

pub(crate) fn request(req: &Value, t: &Target, d: &mut Drops) -> Result<Value, ZygoError> {
    let mut out_msgs = Vec::new();
    if let Some(s) = system(req) {
        out_msgs.push(json!({"role":"system","content":s}));
    }
    for m in messages(req)? {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = m.get("content").unwrap_or(&Value::Null);
        match role {
            "system" => out_msgs.push(json!({"role":"system","content":texts(content).join("\n\n")})),
            "assistant" => {
                let mut text: Vec<&str> = Vec::new();
                let mut calls = Vec::new();
                for p in parts(content, Dialect::OpenaiChat, t, d) {
                    match p {
                        Part::Text(x) => text.push(x),
                        Part::ToolUse(b) => {
                            let args = serde_json::to_string(b.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".into());
                            calls.push(json!({"id":b["id"],"type":"function","function":{"name":b["name"],"arguments":args}}));
                        }
                        _ => {}
                    }
                }
                if text.is_empty() && calls.is_empty() {
                    continue;
                }
                let mut a = json!({"role":"assistant","content": if text.is_empty() { Value::Null } else { json!(text.join("\n\n")) }});
                if !calls.is_empty() {
                    a["tool_calls"] = Value::Array(calls);
                }
                out_msgs.push(a);
            }
            _ => {
                // Tool messages first; images of the tool results open the user message that follows them.
                let mut user = Vec::new();
                let mut rest = Vec::new();
                for p in parts(content, Dialect::OpenaiChat, t, d) {
                    match p {
                        Part::Text(x) => rest.push(json!({"type":"text","text":x})),
                        Part::Image(u) => rest.push(image(u)),
                        Part::ToolResult(b) => {
                            let mut text: Vec<&str> = Vec::new();
                            for q in parts(b.get("content").unwrap_or(&Value::Null), Dialect::OpenaiChat, t, d) {
                                match q {
                                    Part::Text(x) => text.push(x),
                                    Part::Image(u) => user.push(image(u)),
                                    _ => {}
                                }
                            }
                            out_msgs.push(json!({"role":"tool","tool_call_id":b["tool_use_id"],"content":text.join("\n\n")}));
                        }
                        _ => {}
                    }
                }
                user.extend(rest);
                if !user.is_empty() {
                    out_msgs.push(json!({"role":"user","content":user}));
                }
            }
        }
    }

    let mut out = Map::new();
    out.insert("model".into(), json!(t.upstream_model));
    copy(req, &mut out, "max_tokens", "max_tokens");
    copy(req, &mut out, "temperature", "temperature");
    copy(req, &mut out, "top_p", "top_p");
    copy(req, &mut out, "stop_sequences", "stop");
    copy(req, &mut out, "stream", "stream");
    if req.get("stream").and_then(Value::as_bool) == Some(true) {
        out.insert("stream_options".into(), json!({"include_usage": true}));
    }
    let tools: Vec<Value> = tools(req, d)
        .into_iter()
        .map(|x| {
            let mut f = json!({"name":x.name});
            if let Some(desc) = x.description {
                f["description"] = desc.clone();
            }
            f["parameters"] = x.parameters;
            json!({"type":"function","function":f})
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
                Choice::Tool(n) => json!({"type":"function","function":{"name":n}}),
            },
        );
    }
    if serial {
        out.insert("parallel_tool_calls".into(), json!(false));
    }
    if let Some(schema) = json_schema(req) {
        out.insert("response_format".into(), json!({"type":"json_schema","json_schema":{"name":"output","schema":schema}}));
    }
    out.insert("messages".into(), Value::Array(out_msgs));
    Ok(Value::Object(out))
}

fn image(url: String) -> Value {
    json!({"type":"image_url","image_url":{"url":url}})
}

pub(crate) fn response(resp: &Value, t: &Target) -> Value {
    let choice = resp.get("choices").and_then(|c| c.get(0)).unwrap_or(&Value::Null);
    let m = choice.get("message").unwrap_or(&Value::Null);
    let mut content = Vec::new();
    if let Some(x) = s(m, "content").filter(|x| !x.is_empty()) {
        content.push(json!({"type":"text","text":x}));
    }
    let mut tool = false;
    for c in m.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
        tool = true;
        let f = c.get("function").unwrap_or(&Value::Null);
        content.push(json!({"type":"tool_use","id":s(c, "id").unwrap_or(""),"name":s(f, "name").unwrap_or(""),"input":input_of(s(f, "arguments"))}));
    }
    let stop = chat_stop(s(choice, "finish_reason"), tool);
    message(resp, t, content, stop, usage_of(resp.get("usage"), Dialect::OpenaiChat))
}
