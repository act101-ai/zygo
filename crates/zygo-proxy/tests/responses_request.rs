//! RS-1: Responses client → Messages request translation and the zygo mark on reasoning items (K§6.4.3, K§6.7).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use http::StatusCode;
use serde_json::{json, Value};
use zygo_proxy::errors::ZygoError;
use zygo_proxy::translate::reasoning::{
    decode, decode_thinking, encode, encode_thinking, MARK_MAX, THINKING_PREFIX,
};
use zygo_config::{Dialect, Effort, Facts, Reasoning};
use zygo_proxy::translate::facts;
use zygo_proxy::translate::responses_client::{check_fields, request, IGNORED, NO_STATE, REFUSED};
use zygo_proxy::translate::{translate_request, Target, DOC_OMITTED};

fn tgt() -> Target {
    Target { provider: "p-ant".into(), upstream_model: "up-ant".into() }
}

fn think() -> Value {
    json!({"type":"thinking","thinking":"t1","signature":"sig1"})
}

fn redacted() -> Value {
    json!({"type":"redacted_thinking","data":"R1"})
}

/// `THINKING_PREFIX` + standard base64 of the payload's JSON.
fn raw_mark(payload: &Value) -> String {
    format!("{THINKING_PREFIX}{}", STANDARD.encode(serde_json::to_vec(payload).unwrap()))
}

// ---- the mark (K§6.7) ----

#[test]
fn mark_round_trip() {
    for b in [think(), redacted()] {
        let v = encode_thinking(&tgt(), &b);
        assert!(v.starts_with(THINKING_PREFIX));
        assert_eq!(decode_thinking(&v, &tgt()), Some(b));
    }
}

#[test]
fn mark_undecodable_is_none() {
    let cases = [
        String::new(),
        "zygo1t:".to_string(),
        "zygo1t:!!!".to_string(),
        raw_mark(&json!({})),
        raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":{"type":"text","text":"x"}})),
        raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":{"type":"thinking","thinking":"x"}})),
        raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":{"type":"redacted_thinking"}})),
    ];
    for v in &cases {
        assert_eq!(decode_thinking(v, &tgt()), None, "{v}");
    }
}

#[test]
fn mark_over_bound_is_none() {
    let b = json!({"type":"thinking","thinking":"x".repeat(MARK_MAX),"signature":"s"});
    let v = encode_thinking(&tgt(), &b);
    assert!(v.len() > MARK_MAX);
    assert_eq!(decode_thinking(&v, &tgt()), None);
}

#[test]
fn mark_other_provider_or_model_is_none() {
    let v = encode_thinking(&tgt(), &think());
    assert_eq!(decode_thinking(&v, &Target { provider: "other".into(), ..tgt() }), None);
    assert_eq!(decode_thinking(&v, &Target { upstream_model: "other".into(), ..tgt() }), None);
}

#[test]
fn marks_do_not_cross() {
    assert_eq!(decode(&encode_thinking(&tgt(), &think())), None);
    assert_eq!(decode_thinking(&encode(&tgt(), "rs_1", "ENC"), &tgt()), None);
}

// ---- top-level field lists (K§11 item 12; obligation 8) ----

/// The `Err`, else panic.
fn err<T: std::fmt::Debug>(r: Result<T, ZygoError>) -> ZygoError {
    r.expect_err("expected an error")
}

fn assert_400(e: ZygoError, message: &str) {
    assert_eq!(e.status, StatusCode::BAD_REQUEST);
    assert_eq!(e.code, "zygo_bad_request");
    assert_eq!(e.message, message);
}

/// The value `each_ignore_field_accepted` sends for an ignored field.
fn ignored_value(f: &str) -> Value {
    match f {
        "truncation" => json!("disabled"),
        "metadata" | "client_metadata" | "stream_options" => json!({}),
        "include" => json!(["reasoning.encrypted_content"]),
        "store" => json!(false),
        _ => json!("x"),
    }
}

#[test]
fn unknown_top_level_field_400_names_it() {
    assert_400(err(check_fields(&json!({"model":"m","input":[],"frobnicate":1}))), "unknown field: frobnicate");
}

#[test]
fn unknown_field_name_is_cut_to_64() {
    let k = "k".repeat(64) + &"z".repeat(500);
    let mut b = json!({"model":"m","input":[]});
    b[k.as_str()] = json!(1);
    assert_400(err(check_fields(&b)), &format!("unknown field: {}", "k".repeat(64)));
}

#[test]
fn each_ignore_field_accepted() {
    for f in IGNORED {
        let mut b = json!({"model":"m","input":[]});
        b[f] = ignored_value(f);
        assert!(check_fields(&b).is_ok(), "{f}");
    }
}

#[test]
fn truncation_other_than_disabled_400() {
    let b = json!({"model":"m","input":[],"truncation":"auto"});
    assert_400(err(check_fields(&b)), "truncation: only \"disabled\" is accepted");
}

#[test]
fn each_refused_field_400() {
    for f in REFUSED {
        let mut b = json!({"model":"m","input":[]});
        b[f] = json!("x");
        assert_400(err(check_fields(&b)), &format!("{f}: {NO_STATE}"));
    }
}

#[test]
fn body_not_object_400() {
    assert_400(err(check_fields(&json!([1]))), "request body must be a JSON object");
}

// ---- request obligations 1-10 and the captured Codex requests (K§11 item 11) ----

fn facts() -> Facts {
    Facts { max_output_tokens: 32000, ..Facts::default() }
}

/// The `body` of every `fixtures/codex/requests/*.json` record, with its file name; asserts at least 4.
fn fixtures() -> Vec<(String, Value)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex/requests");
    let mut out: Vec<(String, Value)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let rec: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
            (p.file_name().unwrap().to_string_lossy().into_owned(), rec["body"].clone())
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(out.len() >= 4, "{} fixtures", out.len());
    out
}

fn fixture(name: &str) -> Value {
    fixtures().into_iter().find(|(n, _)| n == name).unwrap().1
}

fn body(input: Value) -> Value {
    json!({"model":"m","input":input,"max_output_tokens":100})
}

fn tr(b: &Value) -> (Value, Vec<String>) {
    let (m, a) = request(b, &tgt(), Some(&facts())).unwrap();
    (m, a.into_iter().map(|x| x.0).collect())
}

fn msg(role: &str, part: &str, t: &str) -> Value {
    json!({"type":"message","role":role,"content":[{"type":part,"text":t}]})
}
fn user(t: &str) -> Value {
    msg("user", "input_text", t)
}
fn asst(t: &str) -> Value {
    msg("assistant", "output_text", t)
}
fn dev(t: &str) -> Value {
    msg("developer", "input_text", t)
}
fn sys(t: &str) -> Value {
    msg("system", "input_text", t)
}
fn fc(call_id: &str, name: &str, arguments: &str) -> Value {
    json!({"type":"function_call","call_id":call_id,"name":name,"arguments":arguments})
}
fn fco(call_id: &str, output: &str) -> Value {
    json!({"type":"function_call_output","call_id":call_id,"output":output})
}
fn reasoning(e: &str) -> Value {
    json!({"type":"reasoning","summary":[],"encrypted_content":e})
}
fn text(t: &str) -> Value {
    json!({"type":"text","text":t})
}

/// Every content block of every message.
fn blocks(m: &Value) -> Vec<Value> {
    m["messages"].as_array().unwrap().iter().flat_map(|x| x["content"].as_array().unwrap().clone()).collect()
}

/// "Valid Messages request" (node file, Planning notes).
fn assert_valid(m: &Value) {
    assert!(m["model"].is_string());
    assert!(m["max_tokens"].as_u64().is_some_and(|n| n > 0));
    let msgs = m["messages"].as_array().unwrap();
    assert!(!msgs.is_empty());
    for (i, x) in msgs.iter().enumerate() {
        assert_eq!(x["role"], if i % 2 == 0 { "user" } else { "assistant" }, "message {i}");
    }
    for b in blocks(m) {
        let ty = b["type"].as_str().unwrap();
        assert!(
            ["text", "image", "document", "tool_use", "tool_result", "thinking", "redacted_thinking"].contains(&ty),
            "{ty}"
        );
    }
    for to in [Dialect::Openai, Dialect::OpenaiChat] {
        let (_, codes) = translate_request(m, to, &tgt()).unwrap();
        assert!(!codes.iter().any(|c| c.0.starts_with("blocks_dropped:")), "{codes:?}");
    }
}

#[test]
fn fixtures_translate_to_valid_messages() {
    for (name, b) in fixtures() {
        let (m, _) = tr(&b);
        assert_valid(&m);
        assert_eq!(m["model"], b["model"], "{name}");
        assert_eq!(m["max_tokens"], 32000, "{name}");
    }
}

#[test]
fn item_ids_do_not_change_translation() {
    for (name, b) in fixtures() {
        let mut c = b.clone();
        for item in c["input"].as_array_mut().unwrap() {
            if item.get("id").is_some() {
                item["id"] = json!("x");
            }
        }
        assert_eq!(tr(&b), tr(&c), "{name}");
    }
}

#[test]
fn ob1_system_text_and_later_developer_item() {
    let b = json!({"model":"m","max_output_tokens":100,"instructions":"I",
        "input":[dev("D1"), sys("S1"), user("u1"), asst("a1"), dev("D2"), user("u2")]});
    let (m, _) = tr(&b);
    assert_eq!(m["system"], "I\n\nD1\n\nS1");
    assert_eq!(
        m["messages"],
        json!([{"role":"user","content":[text("u1")]}, {"role":"assistant","content":[text("a1")]},
            {"role":"user","content":[text("D2"), text("u2")]}])
    );
}

#[test]
fn ob1_fixture_developer_items_in_system() {
    let b = fixture("first-turn.json");
    let (m, _) = tr(&b);
    let system = m["system"].as_str().unwrap();
    let devs: Vec<&str> = b["input"].as_array().unwrap()[..2]
        .iter()
        .flat_map(|i| i["content"].as_array().unwrap())
        .filter(|p| p["type"] == "input_text")
        .map(|p| p["text"].as_str().unwrap())
        .collect();
    assert!(!devs.is_empty());
    for d in &devs {
        assert!(system.contains(d));
    }
    for blk in blocks(&m) {
        assert!(!devs.iter().any(|d| blk["text"].as_str() == Some(d)));
    }
}

#[test]
fn ob2_message_parts() {
    let b = body(json!([
        {"type":"message","role":"user","content":[
            {"type":"input_text","text":"a"},
            {"type":"input_image","image_url":"data:image/png;base64,QUJD"},
            {"type":"input_image","image_url":"https://e.x/i.png"},
            {"type":"input_file","file_data":"data:application/pdf;base64,UERG"},
            {"type":"input_file","file_id":"file_1"}]},
        {"type":"message","role":"assistant","content":[
            {"type":"output_text","text":"b"}, {"type":"refusal","refusal":"c"}]}
    ]));
    let (m, codes) = tr(&b);
    assert_eq!(
        m["messages"][0]["content"],
        json!([text("a"),
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}},
            {"type":"image","source":{"type":"url","url":"https://e.x/i.png"}},
            {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"UERG"}},
            text(DOC_OMITTED)])
    );
    assert_eq!(m["messages"][1]["content"], json!([text("b"), text("c")]));
    assert!(codes.contains(&"documents_dropped:1".to_string()), "{codes:?}");
    let (m, _) = tr(&body(json!("hi")));
    assert_eq!(m["messages"], json!([{"role":"user","content":[text("hi")]}]));
}

#[test]
fn ob2_unknown_part_400() {
    for (part, message) in [
        (json!({"type":"input_audio"}), "unknown content part type: input_audio"),
        (json!({"type":"input_image"}), "input_image: missing image_url"),
    ] {
        let b = body(json!([{"type":"message","role":"user","content":[part]}]));
        assert_400(err(request(&b, &tgt(), Some(&facts()))), message);
    }
}

#[test]
fn ob3_function_calls_grouped() {
    let b = body(json!([user("go"), fc("c1", "f", "{\"a\":1}"), fc("c2", "g", "{}"), fco("c1", "r1"), fco("c2", "r2"), user("next")]));
    let (m, _) = tr(&b);
    assert_eq!(
        m["messages"],
        json!([
            {"role":"user","content":[text("go")]},
            {"role":"assistant","content":[
                {"type":"tool_use","id":"c1","name":"f","input":{"a":1}},
                {"type":"tool_use","id":"c2","name":"g","input":{}}]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"c1","content":"r1"},
                {"type":"tool_result","tool_use_id":"c2","content":"r2"},
                text("next")]}
        ])
    );
    let (m, _) = tr(&body(json!([user("go"), fc("c1", "f", "{}"), dev("D"), fco("c1", "r")])));
    assert_eq!(
        m["messages"][2]["content"],
        json!([{"type":"tool_result","tool_use_id":"c1","content":"r"}, text("D")])
    );
}

#[test]
fn ob4_custom_tool_call_and_definition() {
    let b = fixture("after-custom-tool-call-output.json");
    let def = b["tools"].as_array().unwrap().iter().find(|t| t["type"] == "custom").unwrap()["format"]["definition"].clone();
    let call_input =
        b["input"].as_array().unwrap().iter().find(|i| i["type"] == "custom_tool_call").unwrap()["input"].clone();
    let (m, codes) = tr(&b);
    let tool = m["tools"].as_array().unwrap().iter().find(|t| t["name"] == "apply_patch").unwrap();
    assert_eq!(
        tool["input_schema"],
        json!({"type":"object","properties":{"input":{"type":"string","description":def}},"required":["input"]})
    );
    let all = blocks(&m);
    let tu = all.iter().find(|x| x["type"] == "tool_use" && x["id"] == "call_pc3_2").unwrap();
    assert_eq!(tu["name"], "apply_patch");
    assert_eq!(tu["input"], json!({"input": call_input}));
    assert!(all.iter().any(|x| x["type"] == "tool_result" && x["tool_use_id"] == "call_pc3_2"));
    assert!(codes.contains(&"custom_tool_as_function".to_string()), "{codes:?}");
}

#[test]
fn ob5_marked_reasoning_in_place() {
    let b = body(json!([
        user("go"),
        reasoning(&encode_thinking(&tgt(), &think())),
        reasoning(&encode_thinking(&tgt(), &redacted())),
        fc("c1", "f", "{}"),
        fco("c1", "r")
    ]));
    let (m, codes) = tr(&b);
    assert_eq!(
        m["messages"][1],
        json!({"role":"assistant","content":[think(), redacted(), {"type":"tool_use","id":"c1","name":"f","input":{}}]})
    );
    assert!(!codes.iter().any(|c| c.starts_with("reasoning_dropped")), "{codes:?}");
}

#[test]
fn ob5_unmarked_or_foreign_reasoning_dropped() {
    let no_thinking = |m: &Value| !blocks(m).iter().any(|x| x["type"] == "thinking");
    let (m, codes) = tr(&fixture("turn-with-reasoning.json"));
    assert!(no_thinking(&m));
    assert!(codes.contains(&"reasoning_dropped:2".to_string()), "{codes:?}");
    let b = body(json!([
        user("go"),
        reasoning(&encode_thinking(&Target { provider: "other".into(), ..tgt() }, &think())),
        reasoning("zygo1t:!!!"),
        reasoning(&encode(&tgt(), "rs", "E")),
        {"type":"reasoning","summary":[]},
        asst("a")
    ]));
    let (m, codes) = tr(&b);
    assert!(no_thinking(&m));
    assert!(codes.contains(&"reasoning_dropped:4".to_string()), "{codes:?}");
}

#[test]
fn ob6_tools_and_hosted_items_dropped() {
    let b = fixture("first-turn.json");
    let (m, codes) = tr(&b);
    assert!(codes.contains(&"tools_dropped:namespace:1".to_string()), "{codes:?}");
    assert!(codes.contains(&"tools_dropped:web_search:1".to_string()), "{codes:?}");
    let ns = b["tools"].as_array().unwrap().iter().find(|t| t["type"] == "namespace").unwrap();
    let mut gone = vec![ns["name"].clone()];
    gone.extend(ns["tools"].as_array().unwrap().iter().map(|t| t["name"].clone()));
    let tools = m["tools"].as_array().unwrap();
    assert!(!tools.iter().any(|t| gone.contains(&t["name"])));
    for f in b["tools"].as_array().unwrap().iter().filter(|t| t["type"] == "function") {
        let t = tools.iter().find(|t| t["name"] == f["name"]).unwrap();
        assert_eq!(t["input_schema"], f["parameters"]);
    }
    let (_, codes) = tr(&body(json!([user("go"), {"type":"web_search_call","id":"ws","status":"completed"}, asst("a")])));
    assert!(codes.contains(&"items_dropped:web_search_call:1".to_string()), "{codes:?}");
}

#[test]
fn ob7_mapped_fields_then_facts() {
    let b = json!({"model":"m","input":[user("go")],"tools":[{"type":"function","name":"f","parameters":{"type":"object"}}],
        "tool_choice":"required","parallel_tool_calls":false,"max_output_tokens":99999,"temperature":0.5,"top_p":0.9,
        "stream":true,"reasoning":{"effort":"xhigh","summary":"auto"}});
    let (mut m, _) = tr(&b);
    assert_eq!(m["tool_choice"], json!({"type":"any","disable_parallel_tool_use":true}));
    assert_eq!(m["max_tokens"], json!(99999));
    assert_eq!(m["temperature"], json!(0.5));
    assert_eq!(m["top_p"], json!(0.9));
    assert_eq!(m["stream"], json!(true));
    assert_eq!(m["output_config"], json!({"effort":"xhigh"}));
    let f = Facts {
        reasoning: Reasoning::Always,
        effort_levels: vec![Effort::Low, Effort::Medium, Effort::High],
        max_output_tokens: 32000,
        sampling: false,
        ..Facts::default()
    };
    let codes: Vec<String> = facts::apply(&mut m, &f).into_iter().map(|a| a.0).collect();
    for c in ["effort_clamped:xhigh>high", "max_output_clamped:99999>32000", "sampling_dropped"] {
        assert!(codes.contains(&c.to_string()), "{codes:?}");
    }
    let choice = |tc: Value| {
        let mut b = body(json!([user("go")]));
        b["tools"] = json!([{"type":"function","name":"f","parameters":{"type":"object"}}]);
        b["tool_choice"] = tc;
        request(&b, &tgt(), Some(&facts())).map(|(m, _)| m["tool_choice"].clone())
    };
    assert_eq!(choice(json!("auto")).unwrap(), json!({"type":"auto"}));
    assert_eq!(choice(json!("none")).unwrap(), json!({"type":"none"}));
    assert_eq!(choice(json!({"type":"function","name":"f"})).unwrap(), json!({"type":"tool","name":"f"}));
    assert_eq!(choice(json!({"type":"custom","name":"f"})).unwrap(), json!({"type":"tool","name":"f"}));
    assert_400(err(choice(json!({"type":"web_search"}))), "tool_choice: unknown form");
}

#[test]
fn ob7_max_tokens_from_catalog_or_400() {
    let b = json!({"model":"m","input":[user("go")]});
    let (m, _) = request(&b, &tgt(), Some(&facts())).unwrap();
    assert_eq!(m["max_tokens"], json!(32000));
    let message = "max_output_tokens is required: the model's catalog states no max_output_tokens";
    assert_400(err(request(&b, &tgt(), None)), message);
    assert_400(err(request(&b, &tgt(), Some(&Facts::default()))), message);
}

#[test]
fn ob8_request_applies_field_lists() {
    let mut b = body(json!([user("go")]));
    b["frobnicate"] = json!(1);
    assert_400(err(request(&b, &tgt(), Some(&facts()))), "unknown field: frobnicate");
    let mut b = body(json!([user("go")]));
    b["previous_response_id"] = json!("r");
    assert_400(err(request(&b, &tgt(), Some(&facts()))), &format!("previous_response_id: {NO_STATE}"));
}

#[test]
fn ob9_unknown_or_incomplete_item_400() {
    for (item, message) in [
        (json!({"type":"mystery_item"}), "unknown input item type: mystery_item"),
        (json!({"type":"function_call_output","output":"x"}), "function_call_output: missing call_id"),
        (json!({"type":"function_call","call_id":"c","arguments":"{}"}), "function_call: missing name"),
        (json!({"type":"custom_tool_call","call_id":"c","name":"n"}), "custom_tool_call: missing input"),
        (json!({"type":"message","content":[]}), "message: missing role"),
    ] {
        assert_400(err(request(&body(json!([user("go"), item])), &tgt(), Some(&facts()))), message);
    }
}

#[test]
fn ob10_ignored_fields_absent() {
    let allowed =
        ["model", "system", "messages", "max_tokens", "temperature", "top_p", "stream", "tools", "tool_choice", "output_config"];
    for (name, mut b) in fixtures() {
        for f in IGNORED {
            b[f] = ignored_value(f);
        }
        let (m, _) = tr(&b);
        for k in m.as_object().unwrap().keys() {
            assert!(allowed.contains(&k.as_str()), "{name}: {k}");
        }
    }
}

#[test]
fn text_format_rules() {
    let mut b = body(json!([user("go")]));
    b["text"] = json!({"format":{"type":"text"},"verbosity":"low"});
    let (m, _) = tr(&b);
    assert!(m.get("text").is_none());
    b["text"] = json!({"format":{"type":"json_schema","name":"o","schema":{}}});
    assert_400(err(request(&b, &tgt(), Some(&facts()))), "text.format: only type text is accepted toward this upstream");
}

// ---- review round 1 findings ----

fn rq(b: &Value) -> Result<(Value, Vec<String>), ZygoError> {
    request(b, &tgt(), Some(&facts())).map(|(m, a)| (m, a.into_iter().map(|x| x.0).collect()))
}

const NO_USER_FIRST: &str = "input: the conversation must start with a user message";

#[test]
fn f1_conversation_must_start_with_user() {
    for input in [
        json!([dev("D"), asst("a"), user("u")]),
        json!([dev("D"), fc("c1", "f", "{}"), fco("c1", "r")]),
        json!([]),
        json!([dev("D")]),
    ] {
        assert_400(err(rq(&body(input.clone()))), NO_USER_FIRST);
    }
}

#[test]
fn f2_dropped_type_in_code_is_safe() {
    let mut b = body(json!([user("go")]));
    b["tools"] = json!([{"type":"a\nb"}, {"type":"weird:9,count_estimated"}, {"type":"wéb"}, {"type":"x".repeat(33)},
        {"type":"ok_type_9"}]);
    let (_, codes) = rq(&b).unwrap();
    assert!(codes.contains(&"tools_dropped:other:4".to_string()), "{codes:?}");
    assert!(codes.contains(&"tools_dropped:ok_type_9:1".to_string()), "{codes:?}");
    for c in &codes {
        assert!(c.bytes().all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == b'_' || x == b':'), "{c}");
        assert_eq!(c.matches(':').count(), 2, "{c}");
    }
}

#[test]
fn f3_echoed_names_have_no_control_characters() {
    let evil = "\u{1b}[31m\r\nX-Injected: 1";
    let shown = "?[31m??X-Injected: 1";
    let mut b = body(json!([user("go")]));
    b[evil] = json!(1);
    assert_400(err(rq(&b)), &format!("unknown field: {shown}"));
    let cases = [
        (json!({"type":evil}), format!("unknown input item type: {shown}")),
        (json!({"type":"message","role":evil,"content":[]}), format!("message: unknown role: {shown}")),
        (json!({"type":"message","role":"user","content":[{"type":evil}]}), format!("unknown content part type: {shown}")),
    ];
    for (item, message) in cases {
        assert_400(err(rq(&body(json!([user("go"), item])))), &message);
    }
}

#[test]
fn f4_decoded_block_keeps_known_keys_only() {
    let extra = json!({"cache_control":{"type":"ephemeral"},"extra":1});
    for (b, want) in [(think(), think()), (redacted(), redacted())] {
        let mut b = b.clone();
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        let v = raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":b}));
        assert_eq!(decode_thinking(&v, &tgt()), Some(want));
    }
}

#[test]
fn f5_tool_choice_against_sent_tools() {
    // No tool left to send: tool_choice and the parallel flag are omitted; `required` is a 400.
    for (tools, tc, parallel) in [
        (json!(null), json!("auto"), json!(null)),
        (json!(null), json!(null), json!(false)),
        (json!([{"type":"web_search"}]), json!("none"), json!(false)),
    ] {
        let mut b = body(json!([user("go")]));
        for (k, v) in [("tools", tools), ("tool_choice", tc), ("parallel_tool_calls", parallel)] {
            if !v.is_null() {
                b[k] = v;
            }
        }
        let (m, _) = rq(&b).unwrap();
        assert!(m.get("tool_choice").is_none(), "{b}");
    }
    for tools in [json!(null), json!([{"type":"web_search"}])] {
        let mut b = body(json!([user("go")]));
        if !tools.is_null() {
            b["tools"] = tools;
        }
        b["tool_choice"] = json!("required");
        assert_400(err(rq(&b)), "tool_choice: required, but no tool is sent");
    }
    // A named tool that is not sent, also one inside a dropped namespace.
    let not_sent = "tool_choice: the named tool is not among the sent tools";
    let mut b = body(json!([user("go")]));
    b["tools"] = json!([{"type":"function","name":"f","parameters":{"type":"object"}}]);
    b["tool_choice"] = json!({"type":"function","name":"nope"});
    assert_400(err(rq(&b)), not_sent);
    let mut b = fixture("first-turn.json");
    let ns = b["tools"].as_array().unwrap().iter().find(|t| t["type"] == "namespace").unwrap().clone();
    b["tool_choice"] = json!({"type":"function","name":ns["tools"][0]["name"]});
    assert_400(err(rq(&b)), not_sent);
}

#[test]
fn f6_mapped_field_of_wrong_type_400() {
    let cases = [
        ("stream", json!("yes"), "stream must be a boolean"),
        ("parallel_tool_calls", json!(0), "parallel_tool_calls must be a boolean"),
        ("temperature", json!("hot"), "temperature must be a number"),
        ("top_p", json!([1]), "top_p must be a number"),
        ("reasoning", json!("high"), "reasoning must be an object"),
        ("reasoning", json!({"effort":3}), "reasoning.effort must be a string"),
        ("text", json!("plain"), "text must be an object"),
        ("text", json!({"format":"text"}), "text.format must be an object"),
    ];
    for (k, v, message) in cases {
        let mut b = body(json!([user("go")]));
        b[k] = v;
        assert_400(err(rq(&b)), message);
    }
    // null is absent: accepted and not sent.
    let mut b = body(json!([user("go")]));
    for k in ["stream", "parallel_tool_calls", "temperature", "top_p", "reasoning", "text"] {
        b[k] = Value::Null;
    }
    let (m, _) = rq(&b).unwrap();
    for k in ["stream", "temperature", "top_p", "output_config", "tool_choice"] {
        assert!(m.get(k).is_none(), "{k}");
    }
}

#[test]
fn f7_short_form_only_without_type() {
    let (m, _) = rq(&body(json!([{"role":"user","content":[{"type":"input_text","text":"go"}]}]))).unwrap();
    assert_eq!(m["messages"], json!([{"role":"user","content":[text("go")]}]));
    for ty in [json!(5), json!(null), json!({"x":1})] {
        let item = json!({"type":ty,"role":"user","content":[{"type":"input_text","text":"x"}]});
        assert_400(err(rq(&body(json!([user("go"), item])))), "input item: type must be a string");
    }
}

#[test]
fn f8_tool_call_pairing() {
    let ctc = |id: &str| json!({"type":"custom_tool_call","call_id":id,"name":"apply_patch","input":"p"});
    let ctco = |id: &str| json!({"type":"custom_tool_call_output","call_id":id,"output":"ok"});
    let no_out = "no output before the next user message or the end of input";
    let cases = [
        (json!([user("go"), fco("c9", "r")]), "function_call_output: no earlier matching call".to_string()),
        (json!([user("go"), fc("c1", "f", "{}"), ctco("c1")]), "custom_tool_call_output: no earlier matching call".into()),
        (json!([user("go"), ctc("c1"), fco("c1", "r")]), "function_call_output: no earlier matching call".into()),
        (json!([user("go"), fc("c1", "f", "{}"), fco("c1", "r"), fco("c1", "r")]), "function_call_output: no earlier matching call".into()),
        (json!([user("go"), fc("c1", "f", "{}"), fco("c1", "r"), fc("c1", "f", "{}"), fco("c1", "r")]), "function_call: duplicate call_id".into()),
        (json!([user("go"), fc("c1", "f", "{}"), ctc("c1"), fco("c1", "r")]), "custom_tool_call: duplicate call_id".into()),
        (json!([user("go"), fc("c1", "f", "{}"), user("x"), fco("c1", "r")]), format!("function_call: {no_out}")),
        (json!([user("go"), fc("c1", "f", "{}")]), format!("function_call: {no_out}")),
        (json!([user("go"), ctc("c2")]), format!("custom_tool_call: {no_out}")),
    ];
    for (input, message) in cases {
        assert_400(err(rq(&body(input.clone()))), &message);
    }
    // A developer item between a call and its output is not a user message.
    let (m, _) = rq(&body(json!([user("go"), ctc("c2"), dev("D"), ctco("c2"), user("next")]))).unwrap();
    assert_valid(&m);
}

#[test]
fn f9_empty_text_blocks_dropped() {
    let b = body(json!([
        {"type":"message","role":"user","content":[{"type":"input_text","text":""},{"type":"input_text","text":"go"}]},
        asst(""),
        dev(""),
        user("u"),
        fc("c1", "f", "{}"),
        {"type":"function_call_output","call_id":"c1","output":[{"type":"input_text","text":""},{"type":"input_text","text":"r"}]},
        {"type":"message","role":"user","content":[]}
    ]));
    let (m, _) = rq(&b).unwrap();
    assert_eq!(
        m["messages"],
        json!([
            {"role":"user","content":[text("go"), text("u")]},
            {"role":"assistant","content":[{"type":"tool_use","id":"c1","name":"f","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"c1","content":[text("r")]}]}
        ])
    );
    assert_400(err(rq(&body(json!("")))), NO_USER_FIRST);
    // A user message left with no block is dropped, so the conversation starts with the assistant.
    assert_400(err(rq(&body(json!([user(""), asst("a")])))), NO_USER_FIRST);
}

#[test]
fn f9_output_array_maps_parts() {
    let b = body(json!([user("go"), fc("c1", "f", "{}"),
        {"type":"function_call_output","call_id":"c1","output":[{"type":"input_text","text":"r"},
            {"type":"input_image","image_url":"https://e.x/i.png"}]}]));
    let (m, _) = rq(&b).unwrap();
    assert_eq!(
        m["messages"][2]["content"][0],
        json!({"type":"tool_result","tool_use_id":"c1","content":[text("r"),
            {"type":"image","source":{"type":"url","url":"https://e.x/i.png"}}]})
    );
}

#[test]
fn f9_parallel_false_without_tool_choice() {
    let mut b = body(json!([user("go")]));
    b["tools"] = json!([{"type":"function","name":"f","parameters":{"type":"object"}}]);
    b["parallel_tool_calls"] = json!(false);
    let (m, _) = rq(&b).unwrap();
    assert_eq!(m["tool_choice"], json!({"type":"auto","disable_parallel_tool_use":true}));
}

#[test]
fn f9_item_and_part_errors() {
    let long = "é".repeat(100);
    let cut = "é".repeat(64);
    let cases = [
        (json!({"type":long}), format!("unknown input item type: {cut}")),
        (json!({"type":"message","role":"user","content":[{"type":long}]}), format!("unknown content part type: {cut}")),
        (json!({"content":[]}), "input item: missing type".to_string()),
        (json!({"type":"message","role":"tool","content":[]}), "message: unknown role: tool".to_string()),
    ];
    for (item, message) in cases {
        assert_400(err(rq(&body(json!([user("go"), item])))), &message);
    }
}

#[test]
fn f9_other_hosted_items_dropped() {
    let b = body(json!([user("go"), {"type":"local_shell_call","id":"l","status":"completed"},
        {"type":"mcp_call","id":"m"}, asst("a")]));
    let (_, codes) = rq(&b).unwrap();
    assert!(codes.contains(&"items_dropped:local_shell_call:1".to_string()), "{codes:?}");
    assert!(codes.contains(&"items_dropped:mcp_call:1".to_string()), "{codes:?}");
}
