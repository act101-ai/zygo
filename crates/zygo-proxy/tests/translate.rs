//! N13: dialect translation (§7.6, §14).

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use base64::Engine;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::keys::Secret;
use zygo_config::Dialect::{self, Openai, OpenaiChat};
use zygo_config::{load, KeyRef, LoadContext, Merged};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_proxy::pipeline::Adjustment;
use zygo_proxy::provider::KeyResolver;
use zygo_proxy::redact_stage::RedactLimits;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{ManualClock, SessionGrant, SessionRequest};
use zygo_proxy::tee::{COPY_MAX, HOLD_MAX};
use zygo_proxy::translate::reasoning::{decode, decode_thinking, encode, without_zygo_thinking, Signature};
use zygo_proxy::translate::responses_client::{response, Calls, Encoder};
use zygo_proxy::translate::sse::INPUT_DELTA_MAX;
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RequestRow, Store};
use zygo_proxy::translate::{
    message_events, translate_error, translate_request, translate_response, SseEvent, StreamMapper, Target, DOC_OMITTED,
};

fn tgt() -> Target {
    Target { provider: "p-oai".into(), upstream_model: "up-oai".into() }
}

fn codes(a: &[Adjustment]) -> Vec<&str> {
    a.iter().map(|x| x.0.as_str()).collect()
}

/// Any object anywhere in `v` has key `k`.
fn has_key(v: &Value, k: &str) -> bool {
    match v {
        Value::Object(o) => o.contains_key(k) || o.values().any(|x| has_key(x, k)),
        Value::Array(a) => a.iter().any(|x| has_key(x, k)),
        _ => false,
    }
}

fn tr(req: Value, to: Dialect) -> (Value, Vec<Adjustment>) {
    translate_request(&req, to, &tgt()).unwrap()
}

fn tool() -> Value {
    json!({"name":"get","description":"d","input_schema":{"type":"object","properties":{"a":{"type":"string"}}}})
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

fn thinking(text: &str, sig: &str) -> Value {
    json!({"type":"thinking","thinking":text,"signature":sig})
}

fn user(c: Value) -> Value {
    json!({"role":"user","content":c})
}

fn assistant(c: Value) -> Value {
    json!({"role":"assistant","content":c})
}

fn user_msg(text: &str) -> Value {
    json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})
}

fn reasoning_item(id: &str, enc: &str) -> Value {
    json!({"type":"reasoning","id":id,"encrypted_content":enc,"summary":[]})
}

/// The message list of test A1.5.
#[allow(non_snake_case)]
fn LOOP() -> Value {
    json!([{"role":"user","content":[{"type":"text","text":"look"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBO"}},{"type":"image","source":{"type":"url","url":"https://example.com/a.png"}}]},
        {"role":"assistant","content":[{"type":"text","text":"calling"},{"type":"tool_use","id":"call_1","name":"get","input":{"a":"x"}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":[{"type":"text","text":"r1"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBO"}}]},
                                  {"type":"tool_result","tool_use_id":"call_2","content":"plain","is_error":true},{"type":"text","text":"next"}]},
        {"role":"system","content":"be brief"},
        {"role":"system","content":[{"type":"text","text":"s-a"},{"type":"text","text":"s-b"}]}])
}

/// The request of test A1.3.
#[allow(non_snake_case)]
fn TOP() -> Value {
    let mut t = tool();
    t["cache_control"] = json!({"type":"ephemeral"});
    t["defer_loading"] = json!(true);
    json!({"model":"code","max_tokens":1024,"stream":true,"temperature":0.5,"top_p":0.9,"top_k":5,"stop_sequences":["X"],
        "metadata":{"user_id":"u"},"thinking":{"type":"adaptive"},"context_management":{"edits":[]},
        "output_config":{"effort":"medium","format":{"type":"json_schema","schema":{"type":"object"}}},
        "system":[{"type":"text","text":"S1","cache_control":{"type":"ephemeral"}},{"type":"text","text":"S2"}],
        "tools":[t],
        "tool_choice":{"type":"auto","disable_parallel_tool_use":true},
        "messages":[{"role":"user","content":"hi"}]})
}

fn req(messages: Value) -> Value {
    json!({"model":"code","max_tokens":8,"messages":messages})
}

// ---- A. Messages → Responses, and the signature ----

#[test]
fn req_oai_signature_codec() {
    let s = encode(&tgt(), "rs_1", "ENC1");
    assert!(s.starts_with("zygo1:"));
    let want = Signature { provider: "p-oai".into(), model: "up-oai".into(), id: "rs_1".into(), encrypted_content: "ENC1".into() };
    assert_eq!(decode(&s), Some(want));
    let raw = base64::engine::general_purpose::STANDARD.decode(&s[6..]).unwrap();
    let v: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(v, json!({"provider":"p-oai","model":"up-oai","id":"rs_1","encrypted_content":"ENC1"}));
    for bad in ["".to_string(), "native".into(), "zygo1:".into(), "zygo1:!!!".into(), format!("zygo1:{}", b64("{}"))] {
        assert_eq!(decode(&bad), None, "{bad}");
    }
    let d = format!("{:?}", decode(&s).unwrap());
    assert!(!d.contains("ENC1") && d.contains("4 bytes"), "{d}");
}

#[test]
fn req_oai_anthropic_target_drops_zygo_thinking() {
    let t = tgt();
    let body = req(json!([user(json!("hi")),
        assistant(json!([thinking("a", &encode(&t, "rs_1", "ENC1")), thinking("m", "zygo1:!!!"), thinking("b", "native"),
            {"type":"redacted_thinking","data":"R"}, {"type":"text","text":"ok"}])),
        user(json!("go on")),
        assistant(json!([thinking("c", &encode(&t, "rs_2", "ENC2"))]))]));
    let (out, n) = without_zygo_thinking(&body).unwrap();
    assert_eq!(n, 3);
    assert_eq!(out["messages"], json!([user(json!("hi")),
        assistant(json!([thinking("b", "native"), {"type":"redacted_thinking","data":"R"}, {"type":"text","text":"ok"}])),
        user(json!("go on"))]));
    assert_eq!(out["max_tokens"], json!(8));
    let clean = req(json!([user(json!("hi")),
        assistant(json!([thinking("b", "native"), {"type":"redacted_thinking","data":"R"}, {"type":"text","text":"ok"}]))]));
    assert_eq!(without_zygo_thinking(&clean), None);
}

#[test]
fn req_oai_fields() {
    let (out, adj) = tr(TOP(), Openai);
    assert_eq!(out, json!({"model":"up-oai","instructions":"S1\n\nS2",
        "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],
        "max_output_tokens":1024,"temperature":0.5,"top_p":0.9,"stream":true,
        "tools":[{"type":"function","name":"get","description":"d","parameters":{"type":"object","properties":{"a":{"type":"string"}}},"strict":false}],
        "tool_choice":"auto","parallel_tool_calls":false,
        "text":{"format":{"type":"json_schema","name":"output","schema":{"type":"object"}}},
        "store":false,"include":["reasoning.encrypted_content"]}));
    assert_eq!(codes(&adj), Vec::<&str>::new());
}

#[test]
fn req_oai_tool_choice() {
    for (tc, want) in [
        (json!({"type":"auto"}), json!("auto")),
        (json!({"type":"any"}), json!("required")),
        (json!({"type":"none"}), json!("none")),
        (json!({"type":"tool","name":"get"}), json!({"type":"function","name":"get"})),
    ] {
        let mut r = req(json!([user(json!("hi"))]));
        r["tools"] = json!([tool()]);
        r["tool_choice"] = tc;
        let (out, _) = tr(r, Openai);
        assert_eq!(out["tool_choice"], want);
        assert!(!has_key(&out, "parallel_tool_calls"));
    }
    let (out, _) = tr(req(json!([user(json!("hi"))])), Openai);
    assert!(!has_key(&out, "tool_choice"));
}

#[test]
fn req_oai_tool_loop_and_blocks() {
    let (out, adj) = tr(req(LOOP()), Openai);
    assert_eq!(out["input"], json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"look"},{"type":"input_image","image_url":"data:image/png;base64,iVBO"},{"type":"input_image","image_url":"https://example.com/a.png"}]},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"calling"}]},
        {"type":"function_call","call_id":"call_1","name":"get","arguments":"{\"a\":\"x\"}"},
        {"type":"function_call_output","call_id":"call_1","output":[{"type":"input_text","text":"r1"},{"type":"input_image","image_url":"data:image/png;base64,iVBO"}]},
        {"type":"function_call_output","call_id":"call_2","output":"plain"},
        {"type":"message","role":"user","content":[{"type":"input_text","text":"next"}]},
        {"type":"message","role":"developer","content":[{"type":"input_text","text":"be brief"}]},
        {"type":"message","role":"developer","content":[{"type":"input_text","text":"s-a"},{"type":"input_text","text":"s-b"}]}]));
    assert_eq!(codes(&adj), Vec::<&str>::new());
}

#[test]
fn req_oai_reasoning_in_original_position() {
    let t = tgt();
    let other_model = Target { provider: "p-oai".into(), upstream_model: "up-other".into() };
    let other_provider = Target { provider: "p-x".into(), upstream_model: "up-oai".into() };
    let (out, adj) = tr(req(json!([user(json!("hi")),
        assistant(json!([thinking("plan", &encode(&t, "rs_1", "ENC1")), {"type":"tool_use","id":"call_1","name":"get","input":{}}])),
        user(json!([{"type":"tool_result","tool_use_id":"call_1","content":"ok"}])),
        assistant(json!([thinking("", &encode(&other_model, "rs_2", "ENC2")), thinking("", &encode(&other_provider, "rs_3", "ENC3")),
            thinking("", "zygo1:!!!"), thinking("t", "native"), {"type":"redacted_thinking","data":"R"}, {"type":"text","text":"done"}]))])), Openai);
    assert_eq!(out["input"], json!([user_msg("hi"), reasoning_item("rs_1", "ENC1"),
        {"type":"function_call","call_id":"call_1","name":"get","arguments":"{}"},
        {"type":"function_call_output","call_id":"call_1","output":"ok"},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}]));
    assert_eq!(codes(&adj), ["thinking_dropped:5"]);
}

#[test]
fn req_oai_blocks_without_counterpart() {
    let mut r = req(json!([
        user(json!([{"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBE"}},
            {"type":"document","source":{"type":"text","media_type":"text/plain","data":"abc"}},
            {"type":"search_result","source":"s","title":"t","content":[]},
            {"type":"search_result","source":"s","title":"t","content":[]},
            {"type":"container_upload","file_id":"f"}])),
        assistant(json!([{"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{"query":"q"}},
            {"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[]},
            {"type":"text","text":"found"}]))]));
    r["tools"] = json!([{"name":"get","input_schema":{"type":"object"}}, {"type":"custom","name":"c2","input_schema":{"type":"object"}},
        {"type":"web_search_20250305","name":"web_search","max_uses":3}]);
    let (out, adj) = tr(r, Openai);
    assert_eq!(out["tools"], json!([{"type":"function","name":"get","parameters":{"type":"object"},"strict":false},
        {"type":"function","name":"c2","parameters":{"type":"object"},"strict":false}]));
    assert_eq!(out["input"], json!([{"type":"message","role":"user","content":[{"type":"input_file","filename":"document.pdf","file_data":"data:application/pdf;base64,JVBE"},{"type":"input_text","text":DOC_OMITTED}]},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"found"}]}]));
    assert_eq!(codes(&adj), ["server_tools_dropped:3", "documents_dropped:1", "blocks_dropped:container_upload:1", "blocks_dropped:search_result:2"]);
}

// ---- B. Messages → chat/completions ----

#[test]
fn req_chat_fields_and_messages() {
    let mut r = TOP();
    r["messages"] = LOOP();
    let (out, adj) = tr(r, OpenaiChat);
    assert_eq!(out, json!({"model":"up-oai","max_tokens":1024,"temperature":0.5,"top_p":0.9,"stop":["X"],"stream":true,"stream_options":{"include_usage":true},
        "tools":[{"type":"function","function":{"name":"get","description":"d","parameters":{"type":"object","properties":{"a":{"type":"string"}}}}}],
        "tool_choice":"auto","parallel_tool_calls":false,
        "response_format":{"type":"json_schema","json_schema":{"name":"output","schema":{"type":"object"}}},
        "messages":[{"role":"system","content":"S1\n\nS2"},
         {"role":"user","content":[{"type":"text","text":"look"},{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBO"}},{"type":"image_url","image_url":{"url":"https://example.com/a.png"}}]},
         {"role":"assistant","content":"calling","tool_calls":[{"id":"call_1","type":"function","function":{"name":"get","arguments":"{\"a\":\"x\"}"}}]},
         {"role":"tool","tool_call_id":"call_1","content":"r1"},
         {"role":"tool","tool_call_id":"call_2","content":"plain"},
         {"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBO"}},{"type":"text","text":"next"}]},
         {"role":"system","content":"be brief"},
         {"role":"system","content":"s-a\n\ns-b"}]}));
    assert_eq!(codes(&adj), Vec::<&str>::new());
    let (out, _) = tr(req(json!([user(json!("hi")), assistant(json!([{"type":"tool_use","id":"call_1","name":"get","input":{}}]))])), OpenaiChat);
    assert_eq!(out["messages"][1], json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get","arguments":"{}"}}]}));
}

#[test]
fn req_chat_non_stream() {
    let (out, _) = tr(req(json!([user(json!("hi"))])), OpenaiChat);
    assert!(!has_key(&out, "stream_options") && !has_key(&out, "stream"));
    let mut r = req(json!([user(json!("hi"))]));
    r["stream"] = json!(false);
    let (out, _) = tr(r, OpenaiChat);
    assert!(!has_key(&out, "stream_options"));
    assert_eq!(out["stream"], json!(false));
}

#[test]
fn req_chat_tool_choice() {
    for (tc, want) in [
        (json!({"type":"auto"}), json!("auto")),
        (json!({"type":"any"}), json!("required")),
        (json!({"type":"none"}), json!("none")),
        (json!({"type":"tool","name":"get"}), json!({"type":"function","function":{"name":"get"}})),
    ] {
        let mut r = req(json!([user(json!("hi"))]));
        r["tools"] = json!([tool()]);
        r["tool_choice"] = tc;
        let (out, _) = tr(r, OpenaiChat);
        assert_eq!(out["tool_choice"], want);
    }
}

#[test]
fn req_chat_drops_all_thinking_and_documents() {
    let (out, adj) = tr(req(json!([
        user(json!([{"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBE"}},
            {"type":"document","source":{"type":"text","media_type":"text/plain","data":"abc"}}])),
        assistant(json!([thinking("", &encode(&tgt(), "rs_1", "ENC1")), {"type":"redacted_thinking","data":"R"}, {"type":"text","text":"ok"}]))])), OpenaiChat);
    assert_eq!(out["messages"], json!([{"role":"user","content":[{"type":"text","text":DOC_OMITTED},{"type":"text","text":DOC_OMITTED}]},
        {"role":"assistant","content":"ok"}]));
    assert_eq!(codes(&adj), ["thinking_dropped:2", "documents_dropped:2"]);
}

#[test]
fn req_chat_fixture_request_both_dialects() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude-code/requests/q1-continued-subagent-request.json");
    let f: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let body = &f["body"];

    let (out, adj) = translate_request(body, Openai, &tgt()).unwrap();
    assert_eq!(codes(&adj), Vec::<&str>::new());
    assert_eq!(out["instructions"], json!("[omitted]"));
    let kinds: Vec<(&str, &str)> =
        out["input"].as_array().unwrap().iter().map(|i| (i["type"].as_str().unwrap(), i["role"].as_str().unwrap())).collect();
    assert_eq!(kinds, [("message", "user"), ("message", "developer"), ("message", "assistant"), ("message", "user"), ("message", "developer")]);
    let tools = out["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 10);
    assert!(tools.iter().all(|t| t["type"] == json!("function") && t["strict"] == json!(false)));
    assert!(!has_key(&out, "cache_control"));
    assert_eq!(out["max_output_tokens"], json!(128000));
    for k in ["thinking", "output_config", "metadata", "context_management"] {
        assert!(out.get(k).is_none(), "{k}");
    }

    let (out, adj) = translate_request(body, OpenaiChat, &tgt()).unwrap();
    assert_eq!(codes(&adj), Vec::<&str>::new());
    let roles: Vec<&str> = out["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "system", "assistant", "user", "system"]);
    assert_eq!(out["tools"].as_array().unwrap().len(), 10);
    assert!(!has_key(&out, "cache_control"));
    assert_eq!(out["max_tokens"], json!(128000));
    assert_eq!(out["stream_options"], json!({"include_usage":true}));
}

// ---- C. Non-streamed responses and errors back to Messages ----

#[allow(non_snake_case)]
fn RESP_JSON() -> Value {
    json!({"id":"resp_1","object":"response","model":"up-oai-2026","status":"completed",
        "output":[{"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"a"},{"type":"summary_text","text":"b"}],"encrypted_content":"ENC1"},
         {"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hello","annotations":[]},{"type":"refusal","refusal":"no"}]},
         {"type":"function_call","id":"fc_1","call_id":"call_9","name":"get","arguments":"{\"a\":1}","status":"completed"}],
        "usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":40},"output_tokens":7,"output_tokens_details":{"reasoning_tokens":3},"total_tokens":107}})
}

/// The C1.1 message with the signature `sig`.
fn resp_message(sig: &str) -> Value {
    json!({"id":"resp_1","type":"message","role":"assistant","model":"up-oai-2026",
        "content":[{"type":"thinking","thinking":"a\n\nb","signature":sig},{"type":"text","text":"hello"},{"type":"text","text":"no"},
                   {"type":"tool_use","id":"call_9","name":"get","input":{"a":1}}],
        "stop_reason":"tool_use","stop_sequence":null,
        "usage":{"input_tokens":60,"cache_read_input_tokens":40,"cache_creation_input_tokens":0,"output_tokens":7}})
}

#[allow(non_snake_case)]
fn CTX_CHAT() -> Value {
    json!({"error":{"message":"This model's maximum context length is 128000 tokens. However, your messages resulted in 130532 tokens. Please reduce the length of the messages.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}})
}

#[allow(non_snake_case)]
fn CTX_RESP() -> Value {
    json!({"error":{"message":"Your input exceeds the context window of this model. Please adjust your input and try again.","type":"invalid_request_error","param":"input","code":"context_length_exceeded"}})
}

#[test]
fn res_json_responses_to_message() {
    let out = translate_response(&RESP_JSON(), Openai, &tgt());
    assert_eq!(out, resp_message(&encode(&tgt(), "rs_1", "ENC1")));
    let mut r = RESP_JSON();
    r["usage"]["input_tokens_details"]["cached_tokens"] = json!(150);
    assert_eq!(translate_response(&r, Openai, &tgt())["usage"]["input_tokens"], json!(0));
}

#[test]
fn res_json_chat_to_message() {
    let r = json!({"id":"chatcmpl-1","object":"chat.completion","model":"up-chat","choices":[{"index":0,"message":{"role":"assistant","content":"hi","tool_calls":[{"id":"call_1","type":"function","function":{"name":"get","arguments":"{\"a\":1}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":50,"prompt_tokens_details":{"cached_tokens":10},"completion_tokens":5,"total_tokens":55}});
    assert_eq!(translate_response(&r, OpenaiChat, &tgt()), json!({"id":"chatcmpl-1","type":"message","role":"assistant","model":"up-chat","content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"call_1","name":"get","input":{"a":1}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":40,"cache_read_input_tokens":10,"cache_creation_input_tokens":0,"output_tokens":5}}));
    let r = json!({"id":"chatcmpl-2","model":"up-chat","choices":[{"index":0,"message":{"role":"assistant","content":null},"finish_reason":"stop"}]});
    let out = translate_response(&r, OpenaiChat, &tgt());
    assert_eq!(out["content"], json!([]));
    assert_eq!(out["stop_reason"], json!("end_turn"));
    assert_eq!(out["usage"], json!({"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":0}));
}

#[test]
fn res_json_stop_reasons() {
    let msg = json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"x"}]});
    let call = json!({"type":"function_call","call_id":"c","name":"get","arguments":"{}"});
    for (status, reason, output, want) in [
        ("completed", None, vec![msg.clone()], "end_turn"),
        ("completed", None, vec![msg.clone(), call.clone()], "tool_use"),
        ("incomplete", Some("max_output_tokens"), vec![msg.clone()], "max_tokens"),
        ("incomplete", Some("content_filter"), vec![msg.clone()], "refusal"),
    ] {
        let mut r = json!({"id":"r","status":status,"output":output});
        if let Some(x) = reason {
            r["incomplete_details"] = json!({"reason":x});
        }
        assert_eq!(translate_response(&r, Openai, &tgt())["stop_reason"], json!(want), "{status} {reason:?}");
    }
    let tc = json!([{"id":"call_1","type":"function","function":{"name":"get","arguments":"{}"}}]);
    for (finish, calls, want) in [
        (json!("stop"), false, "end_turn"),
        (json!("stop"), true, "tool_use"),
        (json!("length"), false, "max_tokens"),
        (json!("tool_calls"), false, "tool_use"),
        (json!("function_call"), false, "tool_use"),
        (json!("content_filter"), false, "refusal"),
        (Value::Null, false, "end_turn"),
    ] {
        let mut m = json!({"role":"assistant","content":"x"});
        if calls {
            m["tool_calls"] = tc.clone();
        }
        let r = json!({"id":"c","choices":[{"index":0,"message":m,"finish_reason":finish}]});
        assert_eq!(translate_response(&r, OpenaiChat, &tgt())["stop_reason"], json!(want), "{finish} {calls}");
    }
}

fn err(t: &str, m: &str) -> Value {
    json!({"type":"error","error":{"type":t,"message":m}})
}

/// Anthropic's other overflow text: input plus `max_tokens` over the context window. Claude Code parses it to retry
/// with a smaller `max_tokens`, so an `anthropic` client gets it unchanged (RS-3 review round 2, G1).
const ANTH_LIMIT: &str = "input length and `max_tokens` exceed context limit: 180000 + 32000 > 200000, decrease input length or `max_tokens` and try again";
/// The part Claude Code's pattern reads.
const ANTH_LIMIT_CORE: &str = "input length and `max_tokens` exceed context limit: 180000 + 32000 > 200000";

#[allow(non_snake_case)]
fn CTX_ANTH_LIMIT() -> Value {
    json!({"type":"error","error":{"type":"invalid_request_error","message":ANTH_LIMIT}})
}

#[test]
fn res_json_anthropic_limit_text_unchanged() {
    let (s, v) = translate_error(StatusCode::BAD_REQUEST, CTX_ANTH_LIMIT().to_string().as_bytes(), 1, None);
    assert_eq!((s, v), (StatusCode::BAD_REQUEST, err("invalid_request_error", ANTH_LIMIT)));
}

#[test]
fn res_json_context_errors() {
    let cases = [
        (400, CTX_CHAT(), 10, "prompt is too long: 130532 tokens > 128000 maximum"),
        (400, CTX_RESP(), 5000, "prompt is too long: 5000 tokens > 4999 maximum"),
        (400, json!({"error":{"message":"This model's maximum context length is 8192 tokens.","code":"context_length_exceeded"}}), 9000, "prompt is too long: 9000 tokens > 8192 maximum"),
        (400, json!({"object":"error","message":"This model's maximum context length is 32768 tokens. However, you requested 33000 tokens (32000 in the messages, 1000 in the completion). Please reduce the length of the messages or completion.","type":"BadRequestError","param":null,"code":400}), 1, "prompt is too long: 33000 tokens > 32768 maximum"),
        (400, json!({"error":{"code":400,"message":"the request exceeds the available context size, try increasing it","type":"exceed_context_size_error","n_prompt_tokens":5000,"n_ctx":4096}}), 1, "prompt is too long: 5000 tokens > 4096 maximum"),
        (413, CTX_RESP(), 5000, "prompt is too long: 5000 tokens > 4999 maximum"),
    ];
    for (status, body, estimate, m) in cases {
        let (s, v) = translate_error(StatusCode::from_u16(status).unwrap(), body.to_string().as_bytes(), estimate, None);
        assert_eq!((s, v), (StatusCode::BAD_REQUEST, err("invalid_request_error", m)), "{body}");
    }
}

#[test]
fn res_json_context_n_above_max() {
    let cases = [
        (json!({"error":{"message":"Input exceeds the maximum context length is 128000 tokens","code":"context_length_exceeded"}}), "prompt is too long: 128001 tokens > 128000 maximum"),
        (json!({"error":{"type":"exceed_context_size_error","n_prompt_tokens":100,"n_ctx":4096}}), "prompt is too long: 4097 tokens > 4096 maximum"),
    ];
    for (body, m) in cases {
        let (_, v) = translate_error(StatusCode::BAD_REQUEST, body.to_string().as_bytes(), 1000, None);
        assert_eq!(v, err("invalid_request_error", m), "{body}");
    }
}

#[test]
fn res_json_other_errors() {
    let same = |m: &str| json!({"error":{"message":m}}).to_string();
    let cases: Vec<(u16, String, Option<&str>, &str, &str)> = vec![
        (401, json!({"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}).to_string(), None, "authentication_error", "Incorrect API key provided"),
        (401, same("Incorrect API key provided: test-key-q7"), Some("test-key-q7"), "authentication_error", "Incorrect API key provided: [REDACTED]"),
        (400, same("bad param"), None, "invalid_request_error", "bad param"),
        (403, same("bad param"), None, "permission_error", "bad param"),
        (404, same("bad param"), None, "not_found_error", "bad param"),
        (413, same("too big"), None, "request_too_large", "too big"),
        (422, same("bad param"), None, "invalid_request_error", "bad param"),
        (429, same("bad param"), None, "rate_limit_error", "bad param"),
        (503, same("bad param"), None, "api_error", "bad param"),
        (529, same("bad param"), None, "overloaded_error", "bad param"),
        (500, "<html>oops</html>".into(), None, "api_error", "upstream error"),
        (400, json!({"error":"boom"}).to_string(), None, "invalid_request_error", "boom"),
    ];
    for (status, body, key, t, m) in cases {
        let st = StatusCode::from_u16(status).unwrap();
        assert_eq!(translate_error(st, body.as_bytes(), 0, key), (st, err(t, m)), "{status} {body}");
    }
}

// ---- D. Streaming ----

fn ev(name: &str, data: Value) -> SseEvent {
    SseEvent { event: name.into(), data }
}

/// `"event: <t>\ndata: <json>\n\n"` per event, with `"type": <t>` in the data.
fn sse(events: &[(&str, Value)]) -> String {
    events
        .iter()
        .map(|(t, d)| {
            let mut d = d.clone();
            d["type"] = json!(t);
            format!("event: {t}\ndata: {d}\n\n")
        })
        .collect()
}

fn err_ev(t: &str, m: &str) -> SseEvent {
    ev("error", json!({"type":"error","error":{"type":t,"message":m}}))
}

fn feed(m: &mut StreamMapper, s: &str) -> Vec<SseEvent> {
    m.push(s.as_bytes())
}

fn msg_start(id: &str, model: &str) -> SseEvent {
    ev("message_start", json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}))
}

fn block_start(i: u32, b: Value) -> SseEvent {
    ev("content_block_start", json!({"type":"content_block_start","index":i,"content_block":b}))
}

fn text_start(i: u32) -> SseEvent {
    block_start(i, json!({"type":"text","text":""}))
}

fn delta(i: u32, d: Value) -> SseEvent {
    ev("content_block_delta", json!({"type":"content_block_delta","index":i,"delta":d}))
}

fn text_d(i: u32, x: &str) -> SseEvent {
    delta(i, json!({"type":"text_delta","text":x}))
}

fn json_d(i: u32, x: &str) -> SseEvent {
    delta(i, json!({"type":"input_json_delta","partial_json":x}))
}

fn block_stop(i: u32) -> SseEvent {
    ev("content_block_stop", json!({"type":"content_block_stop","index":i}))
}

fn msg_delta(stop: &str, usage: [u64; 4]) -> SseEvent {
    ev("message_delta", json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},
        "usage":{"input_tokens":usage[0],"cache_read_input_tokens":usage[1],"cache_creation_input_tokens":usage[2],"output_tokens":usage[3]}}))
}

fn msg_stop() -> SseEvent {
    ev("message_stop", json!({"type":"message_stop"}))
}

fn resp_sse_events() -> Vec<(&'static str, Value)> {
    vec![
        ("response.created", json!({"response":{"id":"resp_1","model":"up-oai-2026","status":"in_progress","output":[],"usage":null}})),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}})),
        ("response.reasoning_summary_part.added", json!({"output_index":0,"summary_index":0})),
        ("response.reasoning_summary_text.delta", json!({"output_index":0,"summary_index":0,"delta":"a"})),
        ("response.reasoning_summary_part.added", json!({"output_index":0,"summary_index":1})),
        ("response.reasoning_summary_text.delta", json!({"output_index":0,"summary_index":1,"delta":"b"})),
        ("response.output_item.done", json!({"output_index":0,"item":{"type":"reasoning","id":"rs_1","encrypted_content":"ENC1"}})),
        ("response.output_item.added", json!({"output_index":1,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}})),
        ("response.content_part.added", json!({"output_index":1,"content_index":0,"part":{"type":"output_text","text":""}})),
        ("response.output_text.delta", json!({"output_index":1,"content_index":0,"delta":"Hel"})),
        ("response.output_text.delta", json!({"output_index":1,"content_index":0,"delta":"lo"})),
        ("response.content_part.done", json!({"output_index":1,"content_index":0,"part":{"type":"output_text","text":"Hello"}})),
        ("response.output_item.done", json!({"output_index":1,"item":{"type":"message"}})),
        ("response.output_item.added", json!({"output_index":2,"item":{"type":"function_call","id":"fc_1","call_id":"call_9","name":"get","arguments":""}})),
        ("response.function_call_arguments.delta", json!({"output_index":2,"delta":"{\"a\""})),
        ("response.function_call_arguments.delta", json!({"output_index":2,"delta":":1}"})),
        ("response.output_item.done", json!({"output_index":2,"item":{"type":"function_call","call_id":"call_9","arguments":"{\"a\":1}"}})),
        ("response.completed", json!({"response":{"id":"resp_1","status":"completed","model":"up-oai-2026","usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":40},"output_tokens":7}}})),
    ]
}

#[allow(non_snake_case)]
fn RESP_SSE() -> String {
    sse(&resp_sse_events())
}

fn resp_events(sig: &str) -> Vec<SseEvent> {
    vec![
        msg_start("resp_1", "up-oai-2026"),
        block_start(0, json!({"type":"thinking","thinking":"","signature":""})),
        delta(0, json!({"type":"thinking_delta","thinking":"a"})),
        delta(0, json!({"type":"thinking_delta","thinking":"\n\n"})),
        delta(0, json!({"type":"thinking_delta","thinking":"b"})),
        delta(0, json!({"type":"signature_delta","signature":sig})),
        block_stop(0),
        text_start(1),
        text_d(1, "Hel"),
        text_d(1, "lo"),
        block_stop(1),
        block_start(2, json!({"type":"tool_use","id":"call_9","name":"get","input":{}})),
        json_d(2, "{\"a\""),
        json_d(2, ":1}"),
        block_stop(2),
        msg_delta("tool_use", [60, 40, 0, 7]),
        msg_stop(),
    ]
}

/// A chat chunk with id `chatcmpl-1` and model `up-chat`.
fn chunk(rest: Value) -> String {
    let mut c = json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-chat"});
    for (k, v) in rest.as_object().unwrap() {
        c[k] = v.clone();
    }
    format!("data: {c}\n\n")
}

fn chat_delta(d: Value, finish: Value) -> String {
    chunk(json!({"choices":[{"index":0,"delta":d,"finish_reason":finish}]}))
}

const DONE: &str = "data: [DONE]\n\n";

#[allow(non_snake_case)]
fn CHAT_SSE() -> String {
    [
        chat_delta(json!({"role":"assistant","content":""}), Value::Null),
        chat_delta(json!({"content":"Hel"}), Value::Null),
        chat_delta(json!({"content":"lo"}), Value::Null),
        chat_delta(json!({"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get","arguments":""}}]}), Value::Null),
        chat_delta(json!({"tool_calls":[{"index":0,"function":{"arguments":"{\"a\""}}]}), Value::Null),
        chat_delta(json!({"tool_calls":[{"index":0,"function":{"arguments":":1}"}}]}), Value::Null),
        chat_delta(json!({}), json!("tool_calls")),
        chunk(json!({"choices":[],"usage":{"prompt_tokens":50,"prompt_tokens_details":{"cached_tokens":10},"completion_tokens":5}})),
        DONE.into(),
    ]
    .concat()
}

fn chat_events() -> Vec<SseEvent> {
    vec![
        msg_start("chatcmpl-1", "up-chat"),
        text_start(0),
        text_d(0, "Hel"),
        text_d(0, "lo"),
        block_stop(0),
        block_start(1, json!({"type":"tool_use","id":"call_1","name":"get","input":{}})),
        json_d(1, "{\"a\":1}"),
        block_stop(1),
        msg_delta("tool_use", [40, 10, 0, 5]),
        msg_stop(),
    ]
}

fn oai() -> StreamMapper {
    StreamMapper::new(Openai, &tgt())
}

fn chat() -> StreamMapper {
    StreamMapper::new(OpenaiChat, &tgt())
}

#[test]
fn res_sse_responses_events() {
    let mut m = oai();
    assert_eq!(feed(&mut m, &RESP_SSE()), resp_events(&encode(&tgt(), "rs_1", "ENC1")));
    assert!(m.ended());
}

#[test]
fn res_sse_chat_events() {
    let mut m = chat();
    assert_eq!(feed(&mut m, &CHAT_SSE()), chat_events());
    assert!(m.ended());
}

#[test]
fn res_sse_split_anywhere() {
    for (fixture, new) in [(RESP_SSE(), oai as fn() -> StreamMapper), (CHAT_SSE(), chat)] {
        let s = fixture.replacen("\"Hel\"", "\"é\"", 1);
        assert_ne!(s, fixture);
        let whole = feed(&mut new(), &s);
        assert!(whole.iter().any(|e| e.data["delta"]["text"] == json!("é")));
        let mut m = new();
        let bytes: Vec<SseEvent> = s.as_bytes().iter().flat_map(|b| m.push(std::slice::from_ref(b))).collect();
        assert_eq!(bytes, whole);
        assert_eq!(feed(&mut new(), &fixture.replace('\n', "\r\n")), feed(&mut new(), &fixture));
    }
}

#[test]
fn res_sse_data_forms() {
    let s = concat!(
        "data:{\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"}}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\n",
        "data: \"choices\":[{\"index\":0,\"delta\":{\"content\":\"y\"}}]}\n\n",
        ": comment\n\n",
        "data: [DONE]\n\n",
    );
    let out = feed(&mut chat(), s);
    let texts: Vec<&Value> = out.iter().filter(|e| e.data["delta"]["type"] == json!("text_delta")).map(|e| &e.data["delta"]["text"]).collect();
    assert_eq!(texts, [&json!("x"), &json!("y")]);
    assert_eq!(out.last(), Some(&msg_stop()));
}

#[test]
fn res_sse_terminal_variants() {
    let created = ("response.created", json!({"response":{"id":"resp_1","model":"up-oai-2026","status":"in_progress"}}));
    // (a)
    let s = sse(&[
        created.clone(),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"message","role":"assistant","content":[]}})),
        ("response.content_part.added", json!({"output_index":0,"content_index":0,"part":{"type":"output_text","text":""}})),
        ("response.output_text.delta", json!({"output_index":0,"content_index":0,"delta":"x"})),
        ("response.incomplete", json!({"response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":5,"output_tokens":2}}})),
    ]);
    assert_eq!(feed(&mut oai(), &s), [msg_start("resp_1", "up-oai-2026"), text_start(0), text_d(0, "x"), block_stop(0),
        msg_delta("max_tokens", [5, 0, 0, 2]), msg_stop()]);
    // (b)
    let s = sse(&[created.clone(), ("response.failed", json!({"response":{"status":"failed","error":{"code":"server_error","message":"boom"}}}))]);
    let mut m = oai();
    assert_eq!(feed(&mut m, &s), [msg_start("resp_1", "up-oai-2026"), err_ev("api_error", "boom")]);
    assert!(m.ended());
    // (c)
    assert_eq!(feed(&mut oai(), &sse(&[("error", json!({"code":"x","message":"bad"}))])), [err_ev("api_error", "bad")]);
    // (d)
    let s = chat_delta(json!({"role":"assistant","content":""}), Value::Null) + "data: {\"error\":{\"message\":\"boom\"}}\n\n";
    assert_eq!(feed(&mut chat(), &s), [msg_start("chatcmpl-1", "up-chat"), err_ev("api_error", "boom")]);
    // (e)
    let mut m = oai().errors(0, Some("test-key-q7"));
    assert_eq!(feed(&mut m, &sse(&[("error", json!({"code":"x","message":"bad key test-key-q7"}))])), [err_ev("api_error", "bad key [REDACTED]")]);
    // (f)
    assert_eq!(msg_stop().to_bytes(), b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");
}

#[test]
fn res_sse_in_stream_context_error() {
    let s = sse(&[
        ("response.created", json!({"response":{"id":"resp_1","model":"up-oai-2026"}})),
        ("response.failed", json!({"response":{"error":{"code":"context_length_exceeded","message":"Your input exceeds the context window of this model."}}})),
    ]);
    assert_eq!(feed(&mut oai().errors(5000, None), &s), [msg_start("resp_1", "up-oai-2026"),
        err_ev("invalid_request_error", "prompt is too long: 5000 tokens > 4999 maximum")]);
    let s = chat_delta(json!({"role":"assistant","content":""}), Value::Null)
        + "data: {\"error\":{\"message\":\"This model's maximum context length is 128000 tokens. However, your messages resulted in 130532 tokens.\",\"code\":\"context_length_exceeded\"}}\n\n";
    assert_eq!(feed(&mut chat().errors(10, None), &s), [msg_start("chatcmpl-1", "up-chat"),
        err_ev("invalid_request_error", "prompt is too long: 130532 tokens > 128000 maximum")]);
}

/// Inside a translated stream, Anthropic's limit text reaches the Messages client unchanged (RS-3 review round 2, G1).
#[test]
fn res_sse_in_stream_anthropic_limit_text_unchanged() {
    let s = chat_delta(json!({"role":"assistant","content":""}), Value::Null)
        + &format!("data: {}\n\n", json!({"error":{"message":ANTH_LIMIT,"type":"invalid_request_error"}}));
    let out = feed(&mut chat().errors(10, None), &s);
    assert_eq!(out, [msg_start("chatcmpl-1", "up-chat"), err_ev("api_error", ANTH_LIMIT)]);
}

#[test]
fn res_sse_chat_stop_with_tool_call() {
    for (finish, want) in [("stop", "tool_use"), ("length", "max_tokens")] {
        let s = [
            chat_delta(json!({"role":"assistant","content":""}), Value::Null),
            chat_delta(json!({"tool_calls":[{"index":0,"function":{"name":"get","arguments":"{}"}}]}), Value::Null),
            chat_delta(json!({}), json!(finish)),
            DONE.into(),
        ]
        .concat();
        assert_eq!(feed(&mut chat(), &s), [msg_start("chatcmpl-1", "up-chat"),
            block_start(0, json!({"type":"tool_use","id":"call_chatcmpl-1_0","name":"get","input":{}})),
            json_d(0, "{}"), block_stop(0), msg_delta(want, [0, 0, 0, 0]), msg_stop()]);
    }
}

#[test]
fn res_sse_chat_tool_calls_without_index() {
    // Gemini's OpenAI-compatible endpoint omits `index` on streamed tool calls: the position in the array is the index.
    let s = [
        chat_delta(json!({"tool_calls":[
            {"id":"c1","type":"function","function":{"name":"a","arguments":"{\"x\":1}"}},
            {"id":"c2","type":"function","function":{"name":"b","arguments":"{\"y\":2}"}}]}), Value::Null),
        chat_delta(json!({}), json!("tool_calls")),
        DONE.into(),
    ]
    .concat();
    assert_eq!(feed(&mut chat(), &s), [msg_start("chatcmpl-1", "up-chat"),
        block_start(0, json!({"type":"tool_use","id":"c1","name":"a","input":{}})), json_d(0, "{\"x\":1}"), block_stop(0),
        block_start(1, json!({"type":"tool_use","id":"c2","name":"b","input":{}})), json_d(1, "{\"y\":2}"), block_stop(1),
        msg_delta("tool_use", [0, 0, 0, 0]), msg_stop()]);
}

#[test]
fn res_sse_chat_interleaved_tool_indices() {
    let s = [
        chat_delta(json!({"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"a","arguments":"{\"x\""}}]}), Value::Null),
        chat_delta(json!({"tool_calls":[{"index":1,"id":"c2","type":"function","function":{"name":"b","arguments":"{}"}}]}), Value::Null),
        chat_delta(json!({"tool_calls":[{"index":0,"function":{"arguments":":1}"}}]}), Value::Null),
        chat_delta(json!({}), json!("tool_calls")),
        DONE.into(),
    ]
    .concat();
    assert_eq!(feed(&mut chat(), &s), [msg_start("chatcmpl-1", "up-chat"),
        block_start(0, json!({"type":"tool_use","id":"c1","name":"a","input":{}})), json_d(0, "{\"x\":1}"), block_stop(0),
        block_start(1, json!({"type":"tool_use","id":"c2","name":"b","input":{}})), json_d(1, "{}"), block_stop(1),
        msg_delta("tool_use", [0, 0, 0, 0]), msg_stop()]);
}

#[test]
fn res_sse_responses_done_only_and_refusal() {
    let s = sse(&[
        ("response.created", json!({"response":{"id":"r8","model":"m8"}})),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_2","name":"get","arguments":""}})),
        ("response.output_item.done", json!({"output_index":0,"item":{"type":"function_call","call_id":"call_2","arguments":"{\"a\":2}"}})),
        ("response.output_item.added", json!({"output_index":1,"item":{"type":"message","role":"assistant","content":[]}})),
        ("response.content_part.added", json!({"output_index":1,"content_index":0,"part":{"type":"output_text","text":""}})),
        ("response.content_part.done", json!({"output_index":1,"content_index":0,"part":{"type":"output_text","text":"late"}})),
        ("response.content_part.added", json!({"output_index":1,"content_index":1,"part":{"type":"refusal","refusal":""}})),
        ("response.refusal.delta", json!({"output_index":1,"content_index":1,"delta":"no"})),
        ("response.content_part.done", json!({"output_index":1,"content_index":1,"part":{"type":"refusal","refusal":"no"}})),
        ("response.output_item.done", json!({"output_index":1,"item":{"type":"message"}})),
        ("response.web_search_call.searching", json!({"output_index":2})),
        ("response.in_progress", json!({"response":{}})),
        ("response.completed", json!({"response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":1}}})),
    ]);
    assert_eq!(feed(&mut oai(), &s), [
        msg_start("r8", "m8"),
        block_start(0, json!({"type":"tool_use","id":"call_2","name":"get","input":{}})),
        json_d(0, "{\"a\":2}"),
        block_stop(0),
        text_start(1),
        text_d(1, "late"),
        block_stop(1),
        text_start(2),
        text_d(2, "no"),
        block_stop(2),
        msg_delta("tool_use", [3, 0, 0, 1]),
        msg_stop(),
    ]);
}

#[test]
fn res_sse_line_over_bound() {
    let events = resp_sse_events();
    let mut m = oai();
    let mut out = feed(&mut m, &sse(&events[..1]));
    out.extend(feed(&mut m, &format!("data: {}", "x".repeat(HOLD_MAX + 1))));
    assert_eq!(out, [msg_start("resp_1", "up-oai-2026"), err_ev("api_error", "upstream stream line too long")]);
    assert!(m.ended());
    assert_eq!(feed(&mut m, &sse(&events[7..8])), []);
}

#[test]
fn res_sse_finish_tail_and_early_end() {
    let s = CHAT_SSE();
    let mut m = chat();
    let out = feed(&mut m, s.strip_suffix("\n\n").unwrap());
    assert_eq!(out, chat_events()[..8]);
    assert!(!m.ended());
    assert_eq!(m.finish(), [msg_delta("tool_use", [40, 10, 0, 5]).to_bytes(), msg_stop().to_bytes()].concat());
    assert!(m.ended());

    let mut m = oai();
    feed(&mut m, &sse(&resp_sse_events()[..11]));
    assert_eq!(m.finish(), err_ev("api_error", "upstream stream ended early").to_bytes());

    let mut m = oai();
    feed(&mut m, &RESP_SSE());
    assert_eq!(m.finish(), Vec::<u8>::new());
}

// ---- E. The proxy step and the routes (HTTP) ----

const M: &str = "anthropic/v1/messages";
const ANTH_SSE: &str = concat!(
    "event: message_start\n",
    r#"data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":{"input_tokens":120,"cache_read_input_tokens":30,"cache_creation_input_tokens":7,"output_tokens":1}}}"#,
    "\n\n",
    "event: message_delta\n",
    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#,
    "\n\n",
    "event: message_stop\n",
    r#"data: {"type":"message_stop"}"#,
    "\n\n",
);
/// §20 #25 value; not credential-shaped (two digits must follow `oat`).
const OAT_BEARER: &str = "Bearer sk-ant-oat-x";

#[derive(Clone)]
struct Reply {
    status: u16,
    ctype: &'static str,
    body: String,
}

fn sse_reply(s: &str) -> Reply {
    Reply { status: 200, ctype: "text/event-stream", body: s.into() }
}

fn json_reply(status: u16, v: Value) -> Reply {
    Reply { status, ctype: "application/json", body: v.to_string() }
}

#[derive(Clone)]
struct Got {
    path: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

/// Path and header names only.
impl fmt::Debug for Got {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Got").field("path", &self.path).field("headers", &self.headers.keys().collect::<Vec<_>>()).finish()
    }
}

#[derive(Default)]
struct UpState {
    got: Mutex<Vec<Got>>,
    script: Mutex<HashMap<String, VecDeque<Reply>>>,
}

/// A fake upstream on 127.0.0.1:0 with its own runtime thread; records each request.
struct Upstream {
    addr: SocketAddr,
    st: Arc<UpState>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

/// "/a/v1/messages" → "/a", "/v1/messages" → "".
fn prefix(path: &str) -> String {
    if path.starts_with("/v1/") {
        return String::new();
    }
    let rest = &path[1..];
    format!("/{}", &rest[..rest.find('/').unwrap_or(rest.len())])
}

/// The scripted reply for the path's prefix (default `ANTH_SSE`), in 3 chunks at byte thirds.
async fn upstream_handler(st: Arc<UpState>, req: Request) -> Response {
    let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let headers = req.headers().clone();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
    let p = prefix(&path);
    st.got.lock().unwrap().push(Got { path, headers, body });
    let r = st.script.lock().unwrap().get_mut(&p).and_then(|q| q.pop_front()).unwrap_or_else(|| sse_reply(ANTH_SSE));
    let b = r.body.into_bytes();
    let (a, c) = (b.len() / 3, 2 * b.len() / 3);
    let parts: Vec<Result<Bytes, std::io::Error>> =
        [&b[..a], &b[a..c], &b[c..]].iter().map(|s| Ok(Bytes::copy_from_slice(s))).collect();
    Response::builder()
        .status(r.status)
        .header("content-type", r.ctype)
        .body(Body::from_stream(futures_util::stream::iter(parts)))
        .unwrap()
}

impl Upstream {
    fn start() -> Upstream {
        let st = Arc::new(UpState::default());
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let s2 = st.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let l = tokio::net::TcpListener::from_std(l).unwrap();
                let app = axum::Router::new().fallback(move |req: Request| upstream_handler(s2.clone(), req));
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, st, _stop: tx }
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }
    fn script(&self, prefix: &str, r: Vec<Reply>) {
        self.st.script.lock().unwrap().insert(prefix.to_string(), r.into());
    }
    fn got(&self) -> Vec<Got> {
        self.st.got.lock().unwrap().clone()
    }
    fn count(&self, p: &str) -> usize {
        self.got().iter().filter(|g| prefix(&g.path) == p).count()
    }
    /// The last request under `p`.
    fn at(&self, p: &str) -> Got {
        self.got().into_iter().filter(|g| prefix(&g.path) == p).last().unwrap_or_else(|| panic!("no request at {p:?}"))
    }
}

struct TestKeys(HashMap<String, Secret>);

impl KeyResolver for TestKeys {
    fn resolve(&self, r: &KeyRef) -> Option<Secret> {
        match r {
            KeyRef::Keychain(n) => self.0.get(n).cloned(),
            KeyRef::Env(_) => None,
        }
    }
}

impl std::ops::Index<&str> for TestKeys {
    type Output = Secret;
    fn index(&self, n: &str) -> &Secret {
        &self.0[n]
    }
}

/// Built at run time (no credential-shaped literal).
fn keys() -> TestKeys {
    TestKeys(["k-oai"].iter().map(|n| (n.to_string(), Secret::new(format!("test-key-{n}-{}", ulid())))).collect())
}

/// One `[[model.provider]]` block; an `openai*` provider has `key = "keychain:k-oai"`.
fn prov(name: &str, url: &str, dialects: &str, upstream: &str) -> String {
    let key = if dialects.contains("openai") { "key = \"keychain:k-oai\"\n" } else { "" };
    format!("[[model.provider]]\nname = \"{name}\"\nbase_url = \"{url}\"\ndialects = {dialects}\nupstream_model = \"{upstream}\"\n{key}\n")
}

fn model(id: &str, provs: &[String]) -> String {
    format!("[[model]]\nid = \"{id}\"\n{}\n", provs.concat())
}

fn config(u: &str) -> String {
    let (o, c, a) = ("[\"openai\"]", "[\"openai-chat\"]", "[\"anthropic\"]");
    let mut t = String::from("[role.code]\nmodel = \"test-oai\"\n\n");
    t += &model("test-oai", &[prov("p-oai", &format!("{u}/oai"), o, "up-oai")]);
    t += &model("test-oai2", &[prov("p-oai", &format!("{u}/oai2"), o, "up-oai2")]);
    t += &model("test-fb", &[prov("p-r1", &format!("{u}/r1"), o, "up-r"), prov("p-r2", &format!("{u}/r2"), o, "up-r")]);
    t += &model("test-mix", &[prov("p-resp", &format!("{u}/resp"), o, "up-mix"), prov("p-chat", &format!("{u}/chat"), c, "up-mix-chat")]);
    t += &model("test-both", &[prov("p-both", &format!("{u}/both"), "[\"openai\",\"openai-chat\"]", "up-both")]);
    t += &model("test-anth", &[prov("p-anth", &format!("{u}/anth"), a, "test-anth")]);
    t += &model("test-xa", &[prov("p-x", &format!("{u}/x"), o, "up-x"), prov("p-xa", &format!("{u}/xa"), a, "test-xa")]);
    t += &model("test-x", &[prov("p-x1", &format!("{u}/x1"), o, "up-x1")]);
    t += &model("test-pt-oai", &[prov("p-pt", &format!("{u}/pt"), o, "claude-test-1")]);
    t
}

fn paths(d: &TempDir) -> Paths {
    let d = d.path();
    Paths {
        config_file: d.join("config.toml"),
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    }
}

fn cfg(d: &TempDir, toml: &str) -> Merged {
    std::fs::write(d.path().join("config.toml"), toml).unwrap();
    load(&LoadContext { paths: paths(d), repo: None, team: None }).unwrap()
}

/// A pass that must finish never hits its deadline.
fn generous() -> RedactLimits {
    RedactLimits {
        secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None },
        content_budget: Duration::from_secs(60),
        ..Default::default()
    }
}

struct T {
    d: TempDir,
    up: Upstream,
    anth: Upstream,
    k: TestKeys,
    h: ProxyHandle,
}

fn setup() -> T {
    let d = TempDir::new().unwrap();
    let (up, anth) = (Upstream::start(), Upstream::start());
    let k = keys();
    let o = StartOpts {
        clock: ManualClock::new(),
        keys: Arc::new(TestKeys(k.0.clone())),
        usage_db: Some(d.path().join("data/usage.db")),
        upstream_test: Some(vec![("api.anthropic.com".into(), anth.addr)]),
        redact_limits: Some(generous()),
        ..Default::default()
    };
    let h = Proxy::start_local_with(Arc::new(cfg(&d, &config(&up.url()))), o).unwrap();
    T { d, up, anth, k, h }
}

fn grant(t: &T, passthrough: bool) -> SessionGrant {
    t.h.create_session(SessionRequest {
        agent: Agent::ClaudeCode,
        agent_version: "2.1.328".into(),
        profile: None,
        passthrough,
        repo: None,
        work: None,
        stored_values: Vec::new(),
        known_plugins: Default::default(),
    })
    .unwrap()
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// A request to the proxy with exactly `headers` (plus content-type when a body is sent).
fn send(h: &ProxyHandle, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> (u16, HeaderMap, Vec<u8>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut rb = http_client().request(m, h.url().join(path).unwrap());
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.header("content-type", "application/json").body(b);
        }
        let r = rb.send().await.unwrap();
        let s = r.status().as_u16();
        let hd = r.headers().clone();
        (s, hd, r.bytes().await.unwrap().to_vec())
    })
}

fn auth(g: &SessionGrant) -> Vec<(&str, &str)> {
    vec![("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn pt(g: &SessionGrant) -> Vec<(&str, &str)> {
    vec![("x-zygo-token", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn post(t: &T, g: &SessionGrant, body: &Value) -> (u16, HeaderMap, Vec<u8>) {
    send(&t.h, "POST", M, &auth(g), Some(body.to_string().into_bytes()))
}

/// The usage rows of `g`, once `n` rows were written in all.
fn rows(t: &T, g: &SessionGrant, n: u64) -> Vec<RequestRow> {
    assert!(t.h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
    Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap()
}

fn hv<'a>(h: &'a HeaderMap, n: &str) -> &'a str {
    h.get(n).map(|v| v.to_str().unwrap()).unwrap_or("")
}

fn pin(t: &T, g: &SessionGrant, model: &str) -> Option<String> {
    t.h.session(&g.session).unwrap().pins.lock().unwrap().get(model).cloned()
}

fn body_json(g: &Got) -> Value {
    serde_json::from_slice(&g.body).unwrap()
}

/// The SSE events of a response body.
fn events(b: &[u8]) -> Vec<SseEvent> {
    let s = std::str::from_utf8(b).unwrap();
    s.split("\n\n")
        .filter(|e| !e.trim().is_empty())
        .map(|e| {
            let mut name = String::new();
            let mut data = Value::Null;
            for l in e.lines() {
                if let Some(x) = l.strip_prefix("event: ") {
                    name = x.into();
                } else if let Some(x) = l.strip_prefix("data: ") {
                    data = serde_json::from_str(x).unwrap();
                }
            }
            ev(&name, data)
        })
        .collect()
}

/// The `content` the events build.
fn assemble(evs: &[SseEvent]) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    for e in evs {
        let d = &e.data;
        let i = d["index"].as_u64().unwrap_or(0) as usize;
        match e.event.as_str() {
            "content_block_start" => {
                blocks.push(d["content_block"].clone());
                partial.push(String::new());
            }
            "content_block_delta" => {
                let x = &d["delta"];
                let b = &mut blocks[i];
                match x["type"].as_str().unwrap() {
                    "thinking_delta" => b["thinking"] = json!(b["thinking"].as_str().unwrap().to_string() + x["thinking"].as_str().unwrap()),
                    "text_delta" => b["text"] = json!(b["text"].as_str().unwrap().to_string() + x["text"].as_str().unwrap()),
                    "signature_delta" => b["signature"] = x["signature"].clone(),
                    "input_json_delta" => partial[i] += x["partial_json"].as_str().unwrap(),
                    _ => {}
                }
            }
            "content_block_stop" if blocks[i]["type"] == json!("tool_use") && !partial[i].is_empty() => {
                blocks[i]["input"] = serde_json::from_str(&partial[i]).unwrap();
            }
            _ => {}
        }
    }
    Value::Array(blocks)
}

fn adj(h: &HeaderMap) -> &str {
    hv(h, "x-zygo-adjustments")
}

fn tokens(r: &RequestRow) -> (u64, u64, u64, u64) {
    (r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens)
}

fn msgs(model: &str, messages: Value) -> Value {
    json!({"model":model,"max_tokens":64,"stream":true,"messages":messages})
}

#[test]
fn wire_tool_calls_and_usage_streamed() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/oai", vec![sse_reply(&RESP_SSE())]);
    let r = post(&t, &g, &json!({"model":"code","max_tokens":64,"stream":true,"tools":[tool()],"messages":[{"role":"user","content":"hi"}]}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert!(hv(&r.1, "content-type").starts_with("text/event-stream"));
    assert!(r.1.get("content-length").is_none());
    assert_eq!((hv(&r.1, "x-zygo-model"), hv(&r.1, "x-zygo-role")), ("test-oai", "code"));
    let target = Target { provider: "p-oai".into(), upstream_model: "up-oai".into() };
    assert_eq!(events(&r.2), resp_events(&encode(&target, "rs_1", "ENC1")));
    let got = t.up.at("/oai");
    assert_eq!(got.path, "/oai/v1/responses");
    let b = body_json(&got);
    assert_eq!((&b["model"], &b["store"], &b["include"], &b["stream"]), (&json!("up-oai"), &json!(false), &json!(["reasoning.encrypted_content"]), &json!(true)));
    assert_eq!(b["tools"][0]["strict"], json!(false));
    let rw = rows(&t, &g, 1);
    let x = &rw[0];
    assert_eq!((x.provider.as_str(), x.model.as_str(), x.role_served.as_deref(), x.status), ("p-oai", "test-oai", Some("code"), 200));
    assert_eq!(tokens(x), (60, 7, 40, 0));
}

#[test]
fn wire_non_anthropic_headers() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/oai", vec![sse_reply(&RESP_SSE())]);
    let mut hd = auth(&g);
    hd.extend_from_slice(&[("authorization", "Bearer client-bearer-x"), ("anthropic-beta", "b1"), ("anthropic-version", "2023-06-01"),
        ("x-claude-code-request-class", "main"), ("x-claude-code-agent-id", "a1"), ("x-zygo-extra", "z")]);
    let r = send(&t.h, "POST", M, &hd, Some(msgs("code", json!([user(json!("hi"))])).to_string().into_bytes()));
    assert_eq!(r.0, 200);
    let h = t.up.at("/oai").headers;
    assert_eq!(hv(&h, "authorization"), format!("Bearer {}", t.k["k-oai"].expose()));
    assert!(h.get("x-api-key").is_none());
    assert!(!h.keys().any(|k| k.as_str().starts_with("x-zygo-") || k.as_str().starts_with("x-claude-code-")));
    assert!(h.get("anthropic-beta").is_none() && h.get("anthropic-version").is_none());
    assert!(!h.values().any(|v| v.to_str().unwrap_or("").contains(&g.token)));
}

#[test]
fn wire_round_trip_tool_loop() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/oai", vec![sse_reply(&RESP_SSE()), sse_reply(&RESP_SSE())]);
    let r = post(&t, &g, &msgs("code", json!([user(json!("hi"))])));
    assert_eq!(r.0, 200);
    let c = assemble(&events(&r.2));
    assert_eq!(c[0]["type"], json!("thinking"));
    assert!(c[0]["signature"].as_str().unwrap().starts_with("zygo1:"));
    assert_eq!(c[2], json!({"type":"tool_use","id":"call_9","name":"get","input":{"a":1}}));
    let r = post(&t, &g, &msgs("code", json!([user(json!("hi")), {"role":"assistant","content":c},
        user(json!([{"type":"tool_result","tool_use_id":"call_9","content":"42"}]))])));
    assert_eq!(r.0, 200);
    assert_eq!(body_json(&t.up.at("/oai"))["input"], json!([user_msg("hi"), reasoning_item("rs_1", "ENC1"),
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"}]},
        {"type":"function_call","call_id":"call_9","name":"get","arguments":"{\"a\":1}"},
        {"type":"function_call_output","call_id":"call_9","output":"42"}]));
    assert!(!adj(&r.1).contains("thinking_dropped"));
}

/// A chat upstream relays Anthropic's limit text: the Messages client gets it unchanged (RS-3 review round 2, G1).
#[test]
fn wire_anthropic_limit_text_unchanged_from_chat() {
    let t = setup();
    let g = grant(&t, false);
    // `test-mix`: the Responses provider fails over (503) to the chat provider.
    t.up.script("/resp", vec![json_reply(503, json!({"error":{"message":"busy"}}))]);
    t.up.script("/chat", vec![json_reply(400, json!({"error":{"message":ANTH_LIMIT,"type":"invalid_request_error"}}))]);
    let r = post(&t, &g, &msgs("test-mix", json!([user(json!("hi"))])));
    assert_eq!(t.up.count("/chat"), 1);
    assert_eq!(r.0, 400);
    let v: Value = serde_json::from_slice(&r.2).unwrap();
    assert_eq!(v, err("invalid_request_error", ANTH_LIMIT));
    assert!(v["error"]["message"].as_str().unwrap().contains(ANTH_LIMIT_CORE));
}

#[test]
fn wire_context_error_prompt_too_long() {
    let t = setup();
    let g = grant(&t, false);
    let key = t.k["k-oai"].expose().to_string();
    t.up.script("/r1", vec![json_reply(400, CTX_CHAT()), json_reply(401, json!({"error":{"message":format!("Incorrect API key provided: {key}")}}))]);
    let r = post(&t, &g, &msgs("test-fb", json!([user(json!("hi"))])));
    assert_eq!(r.0, 400);
    assert_eq!(hv(&r.1, "content-type"), "application/json");
    assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), err("invalid_request_error", "prompt is too long: 130532 tokens > 128000 maximum"));
    assert_eq!(t.up.count("/r2"), 0);
    let r = post(&t, &g, &msgs("test-fb", json!([user(json!("hi"))])));
    assert_eq!(r.0, 401);
    assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), err("authentication_error", "Incorrect API key provided: [REDACTED]"));
    assert!(!String::from_utf8_lossy(&r.2).contains(&key));
    let st: Vec<u16> = rows(&t, &g, 2).iter().map(|x| x.status).collect();
    assert_eq!(st, [400, 401]);
}

#[test]
fn wire_thinking_dropped_for_other_target() {
    let t = setup();
    let g = grant(&t, false);
    let z1 = encode(&Target { provider: "p-oai".into(), upstream_model: "up-oai".into() }, "rs_1", "ENC1");
    let a_msgs = |model: &str| msgs(model, json!([user(json!("hi")),
        assistant(json!([thinking("x", &z1), thinking("y", "sig-native"), {"type":"text","text":"ok"}]))]));
    // (a)
    let r = post(&t, &g, &a_msgs("test-anth"));
    assert_eq!(r.0, 200);
    assert_eq!(body_json(&t.up.at("/anth"))["messages"][1]["content"], json!([thinking("y", "sig-native"), {"type":"text","text":"ok"}]));
    assert!(adj(&r.1).contains("thinking_dropped:1"));
    assert!(rows(&t, &g, 1)[0].adjustments.iter().any(|a| a == "thinking_dropped:1"));
    // (b)
    let native = msgs("test-anth", json!([user(json!("hi")), assistant(json!([thinking("y", "sig-native"), {"type":"text","text":"ok"}]))]));
    let sent = native.to_string().into_bytes();
    let r = send(&t.h, "POST", M, &auth(&g), Some(sent.clone()));
    assert_eq!(r.0, 200);
    assert_eq!(t.up.at("/anth").body, sent);
    // (c)
    t.up.script("/oai2", vec![sse_reply(&RESP_SSE())]);
    let r = post(&t, &g, &a_msgs("test-oai2"));
    assert_eq!(r.0, 200);
    assert!(!body_json(&t.up.at("/oai2"))["input"].as_array().unwrap().iter().any(|i| i["type"] == json!("reasoning")));
    assert!(adj(&r.1).contains("thinking_dropped:2"));
    // (d)
    let z = encode(&Target { provider: "p-r1".into(), upstream_model: "up-r".into() }, "rs_1", "ENC1");
    let fb = msgs("test-fb", json!([user(json!("hi")), assistant(json!([thinking("x", &z), {"type":"text","text":"ok"}])), user(json!("go"))]));
    t.up.script("/r1", vec![json_reply(500, json!({}))]);
    t.up.script("/r2", vec![sse_reply(&RESP_SSE()), sse_reply(&RESP_SSE())]);
    let r = post(&t, &g, &fb);
    assert_eq!(r.0, 200);
    let reasoning = |p: &str| body_json(&t.up.at(p))["input"].as_array().unwrap().iter().any(|i| i == &reasoning_item("rs_1", "ENC1"));
    assert!(reasoning("/r1"));
    assert!(!reasoning("/r2"));
    assert!(adj(&r.1).contains("thinking_dropped:1"));
    assert_eq!(pin(&t, &g, "test-fb").as_deref(), Some("p-r2"));
    // (e)
    let n1 = t.up.count("/r1");
    let r = post(&t, &g, &fb);
    assert_eq!(r.0, 200);
    assert_eq!(t.up.count("/r1"), n1);
    assert_eq!(t.up.count("/r2"), 2);
    assert!(adj(&r.1).contains("thinking_dropped:1"));
}

#[test]
fn wire_chat_fallback_streamed() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/resp", vec![json_reply(500, json!({}))]);
    t.up.script("/chat", vec![sse_reply(&CHAT_SSE())]);
    let r = post(&t, &g, &msgs("test-mix", json!([user(json!("hi"))])));
    assert_eq!(r.0, 200);
    assert_eq!(events(&r.2), chat_events());
    let got = t.up.at("/chat");
    assert_eq!(got.path, "/chat/v1/chat/completions");
    let b = body_json(&got);
    assert_eq!(b["model"], json!("up-mix-chat"));
    assert_eq!(b["stream_options"], json!({"include_usage":true}));
    assert!(b.get("store").is_none());
    assert_eq!(pin(&t, &g, "test-mix").as_deref(), Some("p-chat"));
    let rw = rows(&t, &g, 1);
    assert_eq!(rw[0].provider, "p-chat");
    assert_eq!(tokens(&rw[0]), (40, 5, 10, 0));
}

#[test]
fn wire_non_stream_json() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/both", vec![json_reply(200, RESP_JSON())]);
    let r = post(&t, &g, &json!({"model":"test-both","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}));
    assert_eq!(t.up.at("/both").path, "/both/v1/responses");
    assert_eq!(r.0, 200);
    assert_eq!(hv(&r.1, "content-type"), "application/json");
    let target = Target { provider: "p-both".into(), upstream_model: "up-both".into() };
    assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), resp_message(&encode(&target, "rs_1", "ENC1")));
    let cl = hv(&r.1, "content-length");
    assert!(cl.is_empty() || cl == r.2.len().to_string(), "{cl}");
    assert_eq!(tokens(&rows(&t, &g, 1)[0]), (60, 7, 40, 0));
}

#[test]
fn wire_non_stream_over_bound() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/oai", vec![json_reply(200, json!({"pad":"x".repeat(COPY_MAX)}))]);
    let r = post(&t, &g, &json!({"model":"test-oai","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}));
    assert_eq!(r.0, 502);
    assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), err("zygo_upstream_too_large", "upstream response over 4 MiB"));
    assert_eq!(rows(&t, &g, 1)[0].status, 502);
}

#[test]
fn wire_translation_error_falls_back() {
    let t = setup();
    let g = grant(&t, false);
    let sent = json!({"model":"test-xa","max_tokens":8,"messages":"oops"}).to_string().into_bytes();
    let r = send(&t.h, "POST", M, &auth(&g), Some(sent.clone()));
    assert_eq!(r.0, 200);
    assert_eq!(t.up.count("/x"), 0);
    assert_eq!(t.up.at("/xa").body, sent);
    let r = send(&t.h, "POST", M, &auth(&g), Some(json!({"model":"test-x","max_tokens":8,"messages":"oops"}).to_string().into_bytes()));
    assert_eq!((r.0, serde_json::from_slice::<Value>(&r.2).unwrap()), (400, err("zygo_bad_request", "messages must be an array")));
    assert_eq!(t.up.count("/x1"), 0);
    assert_eq!(rows(&t, &g, 2).iter().map(|r| r.status).collect::<Vec<_>>(), [200, 400]);
}

#[test]
fn wire_openai_routes_not_in_v1() {
    let t = setup();
    let g = grant(&t, false);
    let not_in_v1 = json!({"error":{"type":"zygo_route_not_in_v1","message":"this route is not in zygo v1"}});
    for hd in [vec![], auth(&g)] {
        let r = send(&t.h, "POST", "openai/v1/chat/completions", &hd, Some(b"{}".to_vec()));
        assert_eq!(r.0, 404);
        assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), not_in_v1);
    }
    let r = send(&t.h, "GET", "openai/v1/responses", &auth(&g), None);
    assert_eq!((r.0, serde_json::from_slice::<Value>(&r.2).unwrap()["error"]["type"].clone()), (405, json!("zygo_method_not_allowed")));
    let r = send(&t.h, "POST", "openai/v1/other", &auth(&g), Some(b"{}".to_vec()));
    assert_eq!((r.0, serde_json::from_slice::<Value>(&r.2).unwrap()["error"]["type"].clone()), (404, json!("zygo_not_found")));
    assert!(t.up.got().is_empty());
    assert!(Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap().is_empty());
}

#[test]
fn wire_passthrough_never_translated() {
    let t = setup();
    let g = grant(&t, true);
    let mut hd = pt(&g);
    hd.push(("authorization", OAT_BEARER));
    let z = encode(&Target { provider: "p-pt".into(), upstream_model: "claude-test-1".into() }, "rs_1", "ENC1");
    let body = msgs("claude-test-1", json!([user(json!("hi")), assistant(json!([thinking("x", &z), {"type":"text","text":"ok"}]))]));
    let r = send(&t.h, "POST", M, &hd, Some(body.to_string().into_bytes()));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    let got = t.anth.at("");
    assert_eq!(got.path, "/v1/messages");
    let b = body_json(&got);
    assert_eq!(b["messages"][1]["content"], json!([{"type":"text","text":"ok"}]));
    assert!(b.get("input").is_none() && b.get("store").is_none() && b.get("instructions").is_none());
    assert!(adj(&r.1).contains("thinking_dropped:1"));
    assert_eq!(t.up.count("/pt"), 0);
    let sent = msgs("claude-test-1", json!([user(json!("hi")), assistant(json!([{"type":"text","text":"ok"}]))])).to_string().into_bytes();
    let r = send(&t.h, "POST", M, &hd, Some(sent.clone()));
    assert_eq!(r.0, 200);
    assert_eq!(t.anth.at("").body, sent);
}

#[test]
fn wire_bad_response_502() {
    let t = setup();
    let g = grant(&t, false);
    let want = err("zygo_upstream_bad_response", "upstream response could not be translated");
    // An SSE body without the SSE content type (some local servers), JSON that is no response body, and no JSON.
    t.up.script("/oai", vec![
        Reply { status: 200, ctype: "text/plain", body: RESP_SSE() },
        json_reply(200, json!({"foo":1})),
        Reply { status: 200, ctype: "application/json", body: "not json".into() },
    ]);
    let plain = json!({"model":"test-oai","max_tokens":64,"messages":[{"role":"user","content":"hi"}]});
    for body in [msgs("test-oai", json!([user(json!("hi"))])), plain.clone(), plain] {
        let r = post(&t, &g, &body);
        assert_eq!(r.0, 502, "{}", String::from_utf8_lossy(&r.2));
        assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), want);
    }
    assert!(rows(&t, &g, 3).iter().all(|r| r.status == 502));
}

#[test]
fn wire_stream_request_json_reply_as_sse() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/both", vec![json_reply(200, RESP_JSON())]);
    let r = post(&t, &g, &msgs("test-both", json!([user(json!("hi"))])));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert!(hv(&r.1, "content-type").starts_with("text/event-stream"));
    assert!(r.1.get("content-length").is_none());
    let target = Target { provider: "p-both".into(), upstream_model: "up-both".into() };
    let evs = events(&r.2);
    assert_eq!(evs[0], msg_start("resp_1", "up-oai-2026"));
    assert_eq!(assemble(&evs), resp_message(&encode(&target, "rs_1", "ENC1"))["content"]);
    assert_eq!(evs[evs.len() - 2..], [msg_delta("tool_use", [60, 40, 0, 7]), msg_stop()]);
    assert_eq!(tokens(&rows(&t, &g, 1)[0]), (60, 7, 40, 0));
}

#[test]
fn wire_pin_only_after_accepted_body() {
    let t = setup();
    let g = grant(&t, false);
    t.up.script("/oai", vec![json_reply(200, json!({"pad":"x".repeat(COPY_MAX)})), json_reply(200, json!({"foo":1})), json_reply(200, RESP_JSON())]);
    let b = json!({"model":"test-oai","max_tokens":64,"messages":[{"role":"user","content":"hi"}]});
    assert_eq!(post(&t, &g, &b).0, 502);
    assert_eq!(pin(&t, &g, "test-oai"), None, "an over-bound body must not pin");
    assert_eq!(post(&t, &g, &b).0, 502);
    assert_eq!(pin(&t, &g, "test-oai"), None, "a bad response body must not pin");
    assert_eq!(post(&t, &g, &b).0, 200);
    assert_eq!(pin(&t, &g, "test-oai").as_deref(), Some("p-oai"));
}

// ---- review round 3 ----

#[test]
fn res_sse_held_calls_bound_counts_names_and_ids() {
    // 8 calls, each a new index with a 1 MiB name and no arguments: 8 MiB held.
    let mut m = chat();
    let big = "n".repeat(1 << 20);
    let mut out = Vec::new();
    for i in 0..8u64 {
        out.extend(feed(&mut m, &chat_delta(json!({"tool_calls":[{"index":i,"id":format!("c{i}"),"type":"function","function":{"name":big,"arguments":""}}]}), Value::Null)));
    }
    assert!(m.ended(), "8 MiB of held tool calls must hit the 4 MiB bound");
    assert_eq!(out.last().map(|e| (e.event.as_str(), e.data["error"]["message"].as_str())), Some(("error", Some("upstream tool call too long"))));
}

#[test]
fn res_sse_many_empty_calls_bounded() {
    // 70 000 calls with only an index: each costs a fixed overhead, so the count passes 4 MiB.
    let mut m = chat();
    let mut out = Vec::new();
    for b in 0..70u64 {
        let calls: Vec<Value> = (0..1000u64).map(|i| json!({"index": b * 1000 + i})).collect();
        out.extend(feed(&mut m, &chat_delta(json!({"tool_calls":calls}), Value::Null)));
        if m.ended() {
            break;
        }
    }
    assert!(m.ended(), "many empty tool calls must hit the 4 MiB bound");
    assert_eq!(out.last().map(|e| e.data["error"]["message"].as_str()), Some(Some("upstream tool call too long")));
}

#[test]
fn res_sse_held_calls_and_partial_line_share_bound() {
    // 3 MiB of held arguments, then a 2 MiB partial line: together over 4 MiB.
    let mut m = chat();
    let mut out = feed(&mut m, &chat_delta(json!({"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"a","arguments":"x".repeat(3 << 20)}}]}), Value::Null));
    assert!(!m.ended());
    out.extend(m.push(format!("data: {}", "y".repeat(2 << 20)).as_bytes()));
    assert!(m.ended(), "held calls plus the partial line must share the 4 MiB bound");
    assert_eq!(out.last().map(|e| e.data["error"]["message"].as_str()), Some(Some("upstream tool call too long")));
}

#[test]
fn res_sse_no_index_calls_in_separate_chunks() {
    // Two index-less calls, one per chunk, with different ids: two tool_use blocks.
    let s = [
        chat_delta(json!({"tool_calls":[{"id":"c1","type":"function","function":{"name":"a","arguments":"{\"x\":1}"}}]}), Value::Null),
        chat_delta(json!({"tool_calls":[{"id":"c2","type":"function","function":{"name":"b","arguments":"{\"y\":2}"}}]}), Value::Null),
        chat_delta(json!({}), json!("tool_calls")),
        DONE.into(),
    ]
    .concat();
    let out = feed(&mut chat(), &s);
    assert_eq!(out, [msg_start("chatcmpl-1", "up-chat"),
        block_start(0, json!({"type":"tool_use","id":"c1","name":"a","input":{}})), json_d(0, "{\"x\":1}"), block_stop(0),
        block_start(1, json!({"type":"tool_use","id":"c2","name":"b","input":{}})), json_d(1, "{\"y\":2}"), block_stop(1),
        msg_delta("tool_use", [0, 0, 0, 0]), msg_stop()]);
}

/// A Messages body with `content`, as an Anthropic upstream sends it.
fn msg_body(content: Value) -> Value {
    json!({"id":"msg_1","type":"message","role":"assistant","model":"up","content":content,
        "stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":3,"output_tokens":2}})
}

/// `message_events` of `m` through the Responses encoder: its events and its adjustment codes.
fn encoded(m: &Value) -> (Vec<SseEvent>, Vec<String>) {
    let bytes: Vec<u8> = message_events(m).iter().flat_map(SseEvent::to_bytes).collect();
    let mut e = Encoder::new("role-x", &tgt(), Calls::default());
    let mut out = e.push(&bytes);
    out.extend(e.finish());
    (out, e.adjustments().into_iter().map(|a| a.0).collect())
}

#[test] // K§6.4.3 stream obligation 2: a redacted_thinking block of a JSON body keeps its block and its item
fn message_events_keeps_redacted_thinking() {
    let red = json!({"type":"redacted_thinking","data":"EmwKAhgB"});
    let m = msg_body(json!([red.clone(), {"type":"text","text":"hi"}]));
    let evs = message_events(&m);
    let names: Vec<&str> = evs.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(names, ["message_start", "content_block_start", "content_block_stop", "content_block_start",
        "content_block_delta", "content_block_stop", "message_delta", "message_stop"]);
    assert_eq!(evs[1].data, json!({"type":"content_block_start","index":0,"content_block":red}));
    assert_eq!(evs[2].data, json!({"type":"content_block_stop","index":0}));
    assert_eq!(evs[3].data["content_block"], json!({"type":"text","text":""}));
    assert_eq!(evs[4].data["delta"], json!({"type":"text_delta","text":"hi"}));
    let (out, codes) = encoded(&m);
    let items: Vec<&Value> = out.iter().filter(|e| e.event == "response.output_item.done").map(|e| &e.data["item"]).collect();
    assert_eq!(items.len(), 2);
    assert_eq!((&items[0]["type"], &items[0]["summary"]), (&json!("reasoning"), &json!([])));
    assert_eq!(decode_thinking(items[0]["encrypted_content"].as_str().unwrap(), &tgt()), Some(red));
    assert_eq!(items[1]["type"], "message");
    assert_eq!(items[1]["content"][0]["text"], "hi");
    assert!(codes.is_empty(), "{codes:?}");
}

#[test] // K§6.4.3 stream obligation 3: server tool blocks of a JSON body are dropped with server_tool_dropped
fn message_events_drops_server_tool_blocks() {
    let use_ = json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{"query":"q"}});
    let res = json!({"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[]});
    let m = msg_body(json!([use_.clone(), res.clone(), {"type":"text","text":"hi"}]));
    let evs = message_events(&m);
    let starts: Vec<&Value> = evs.iter().filter(|e| e.event == "content_block_start").map(|e| &e.data["content_block"]).collect();
    assert_eq!(starts, [&use_, &res, &json!({"type":"text","text":""})]);
    let deltas: Vec<&Value> = evs.iter().filter(|e| e.event == "content_block_delta").map(|e| &e.data).collect();
    assert_eq!(deltas, [&json!({"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"hi"}})]);
    let (out, codes) = encoded(&m);
    let items: Vec<&Value> = out.iter().filter(|e| e.event == "response.output_item.done").map(|e| &e.data["item"]).collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["content"][0]["text"], "hi");
    assert!(!out.iter().any(|e| e.data.to_string().contains("srvtoolu_1")));
    assert_eq!(out.last().unwrap().event, "response.completed");
    assert_eq!(codes, ["server_tool_dropped"]);
}

#[test] // RS-10: unknown block types and blocks with no `type` take the fallback arm: no empty message item, same items as the JSON path
fn message_events_unknown_blocks_match_json_path() {
    let m = msg_body(json!([
        {"type":"mcp_tool_use","id":"mcptoolu_1","name":"n","server_name":"s","input":{}},
        {"type":"mcp_tool_result","tool_use_id":"mcptoolu_1","is_error":false,"content":[]},
        {"type":"container_upload","file_id":"file_1"},
        {"type":"future_block_v9","x":1},
        {},
        {"type":"text","text":"hi"},
    ]));
    let (out, codes) = encoded(&m);
    let without_id = |i: &Value| {
        let mut i = i.clone();
        i.as_object_mut().unwrap().remove("id");
        i
    };
    let items: Vec<Value> = out.iter().filter(|e| e.event == "response.output_item.done").map(|e| without_id(&e.data["item"])).collect();
    assert!(!items.iter().any(|i| i["type"] == "message" && i["content"][0]["text"] == ""), "{items:?}");
    let (r, json_codes) = response(&m, "role-x", &tgt(), &Calls::default());
    let json_items: Vec<Value> = r["output"].as_array().unwrap().iter().map(without_id).collect();
    assert_eq!(items, json_items);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["content"][0]["text"], "hi");
    assert_eq!(codes, json_codes.into_iter().map(|a| a.0).collect::<Vec<_>>());
    assert_eq!(out.last().unwrap().event, "response.completed");
}

#[test] // RS-13, K§6.4.3 stream obligation 8: input_json_delta pieces of at most INPUT_DELTA_MAX, cut on a char boundary
fn message_events_cuts_tool_input_on_char_boundaries() {
    // Compact input `{"a":"` + c × n + `"}`: the first cut is the longest 6 + k·len(c) <= INPUT_DELTA_MAX.
    for (c, n, first) in [('a', 200_000, INPUT_DELTA_MAX), ('€', 50_000, INPUT_DELTA_MAX - 1), ('😀', 40_000, INPUT_DELTA_MAX - 2)] {
        let input = json!({"a": c.to_string().repeat(n)});
        let m = msg_body(json!([{"type":"tool_use","id":"toolu_1","name":"exec_command","input":input}]));
        let evs = message_events(&m);
        assert_eq!(evs[1].data, json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{}}}));
        let deltas: Vec<&Value> = evs.iter().filter(|e| e.event == "content_block_delta").map(|e| &e.data).collect();
        assert!(deltas.iter().all(|d| d["index"] == 0 && d["delta"]["type"] == "input_json_delta"), "{c}");
        let pieces: Vec<&str> = deltas.iter().map(|d| d["delta"]["partial_json"].as_str().unwrap()).collect();
        assert!(pieces.len() > 1, "{c}");
        assert_eq!(pieces[0].len(), first, "{c}");
        assert!(pieces.iter().all(|p| !p.is_empty() && p.len() <= INPUT_DELTA_MAX), "{c}");
        assert!(pieces[..pieces.len() - 1].iter().all(|p| p.len() > INPUT_DELTA_MAX - 4), "{c}");
        assert_eq!(pieces.concat(), input.to_string(), "{c}");
        assert_eq!(evs[evs.len() - 3].data, json!({"type":"content_block_stop","index":0}));
    }
}

#[test] // RS-13: an empty input `{}`, and a tool_use with no input, is one input_json_delta `{}`
fn message_events_empty_tool_input_one_piece() {
    let m = msg_body(json!([{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{}},
        {"type":"tool_use","id":"toolu_2","name":"exec_command"}]));
    let deltas: Vec<Value> = message_events(&m).into_iter().filter(|e| e.event == "content_block_delta").map(|e| e.data).collect();
    assert_eq!(deltas, [
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
    ]);
}

#[test] // RS-13, K§6.4.3 stream obligations 8 and 9: 1.5 MiB of `"` passes the encoder; each event stays under 2 × INPUT_DELTA_MAX plus framing
fn message_events_large_tool_input_within_hold_max() {
    let input = json!({"a": "\"".repeat(3 << 19)});
    let m = msg_body(json!([{"type":"tool_use","id":"toolu_1","name":"exec_command","input":input}]));
    let evs = message_events(&m);
    let longest = evs.iter().map(|e| e.to_bytes().len()).max().unwrap();
    assert!(longest < 2 * INPUT_DELTA_MAX + 256, "{longest}");
    let (out, codes) = encoded(&m);
    assert_eq!(out.last().unwrap().event, "response.completed", "{:?}", out.last().unwrap().data);
    let items: Vec<&Value> = out.iter().filter(|e| e.event == "response.output_item.done").map(|e| &e.data["item"]).collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["type"], "function_call");
    assert_eq!(items[0]["arguments"], input.to_string());
    assert!(codes.is_empty(), "{codes:?}");
}

#[test] // RS-13 round 1 review F1, K§6.4.3 stream obligation 8: a body of `"` as close to COPY_MAX as fits still completes
fn message_events_tool_input_near_copy_max_completes() {
    let body = |n: usize| msg_body(json!([{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"a":"\"".repeat(n)}}]));
    let base = serde_json::to_vec(&body(0)).unwrap().len();
    let m = body((COPY_MAX - base) / 2);
    let size = serde_json::to_vec(&m).unwrap().len();
    assert!(size <= COPY_MAX && size + 2 > COPY_MAX, "{size}");
    let (want, _) = response(&m, "role-x", &tgt(), &Calls::default());
    let (out, codes) = encoded(&m);
    assert_eq!(out.last().unwrap().event, "response.completed", "{:?}", out.last().unwrap().data);
    let items: Vec<&Value> = out.iter().filter(|e| e.event == "response.output_item.done").map(|e| &e.data["item"]).collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["arguments"], want["output"][0]["arguments"]);
    assert!(codes.is_empty(), "{codes:?}");
}
