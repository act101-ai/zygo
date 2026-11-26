//! RS-1: Responses client → Messages request translation and the zygo mark on reasoning items (K§6.4.3, K§6.7).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use zygo_proxy::translate::reasoning::{
    decode, decode_thinking, encode, encode_thinking, MARK_MAX, THINKING_PREFIX,
};
use zygo_proxy::translate::Target;

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
