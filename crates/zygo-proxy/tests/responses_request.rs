//! RS-1: Responses client → Messages request translation and the zygo mark on reasoning items (K§6.4.3, K§6.7).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use http::StatusCode;
use serde_json::{json, Value};
use zygo_proxy::errors::ZygoError;
use zygo_proxy::translate::reasoning::{
    decode, decode_thinking, encode, encode_thinking, MARK_MAX, THINKING_PREFIX,
};
use zygo_proxy::translate::responses_client::{check_fields, IGNORED, NO_STATE, REFUSED};
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
