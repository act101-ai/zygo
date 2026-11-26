//! RS-8: GET /openai/v1/models in the PC7 shape, the other /openai/v1/ paths, chat/completions stays 404 (K§6.4.1,
//! K§3, K§11 item 22). Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use support::{codex_request, Harness, MODEL};
use zygo_config::profile::Profile;
use zygo_config::{CustomRole, Dialect};
use zygo_core::Agent;

const M: &str = "openai/v1/models";
const OAI_JSON: &str = "application/json";

/// `fixtures/codex/<rel>`, read-only.
fn fixture(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex").join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn json_of(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(b)))
}

fn start(extra_toml: &str) -> Harness {
    Harness::start(Dialect::Anthropic, Agent::ClaudeCode, extra_toml)
}

#[test]
fn models_list_is_the_role_list_in_the_pc7_shape() {
    let h = start("[role.rs8p]\nmodel = \"gpt-mock\"\ndescription = \"rs8p role\"\n");
    // The path Codex sent (PC7), under the OpenAI prefix.
    let req = json_of(&fixture("models/request.json"));
    let path = format!("openai/v1/{}", req["path"].as_str().unwrap().strip_prefix("/v1/").unwrap());
    assert_eq!(path, "openai/v1/models?client_version=0.172.0");
    let (s, hd, b) = h.get(&path);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hd["content-type"].to_str().unwrap().starts_with(OAI_JSON));
    let v = json_of(&b);
    assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["models"]);
    let mut want: Vec<String> = zygo_config::builtin::defaults().role.into_keys().collect();
    want.push("rs8p".into());
    want.sort();
    let models = v["models"].as_array().unwrap();
    assert_eq!(models.iter().map(|m| m["slug"].as_str().unwrap()).collect::<Vec<_>>(), want);
    assert!(models.iter().all(|m| m["slug"] != MODEL), "no catalog id");
    let shape = json_of(&fixture("models/catalog-shape.json"));
    let keys = |m: &Value| m.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>();
    for m in models {
        assert_eq!(keys(m), keys(&shape["models"][0]), "{m}");
    }
    assert_eq!(
        models.iter().find(|m| m["slug"] == "rs8p").unwrap(),
        &json!({
            "slug": "rs8p", "display_name": "rs8p", "description": "rs8p role",
            "supported_reasoning_levels": [], "default_reasoning_level": null,
            "shell_type": "shell_command", "visibility": "list", "supported_in_api": true, "priority": 1,
            "support_verbosity": false, "truncation_policy": {"mode": "bytes", "limit": 10000},
            "experimental_supported_tools": [], "base_instructions": ""
        })
    );
    assert!(h.upstream_paths().is_empty());
}

#[test]
fn models_identity_errors_openai_shape() {
    let h = start("");
    let bearer = format!("Bearer {}", h.token());
    let bearer = bearer.as_str();
    let cases = [
        (vec![], 401, "zygo_token_invalid"),
        (vec![("authorization", bearer)], 401, "zygo_token_invalid"),
        (vec![("authorization", "Bearer wrong-token"), ("x-zygo-session", h.session_id())], 401, "zygo_token_invalid"),
        (vec![("authorization", bearer), ("x-zygo-session", "no-such-session")], 401, "zygo_session_unknown"),
    ];
    for (hd, status, code) in cases {
        let (s, _, b) = h.get_as(M, &hd);
        assert_eq!(s, status, "{hd:?}");
        let v = json_of(&b);
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["error"], "{hd:?}: {v}");
        assert_eq!(v["error"]["type"], code, "{hd:?}");
    }
    assert!(h.upstream_paths().is_empty());
}

#[test]
fn chat_completions_stays_not_in_v1() {
    let h = start("");
    let want = json!({"error":{"type":"zygo_route_not_in_v1","message":"this route is not in zygo v1"}});
    for (s, _, b) in [h.post("openai/v1/chat/completions", &json!({})), h.post_as("openai/v1/chat/completions", &[], &json!({}))] {
        assert_eq!((s.as_u16(), json_of(&b)), (404, want.clone()));
    }
    assert!(h.upstream_paths().is_empty());
}

#[test]
fn other_openai_paths_served_or_404_openai_shape() {
    let h = start("");
    // PC1: the paths Codex calls, without the query.
    let txt = String::from_utf8(fixture("transport/paths.txt")).unwrap();
    let pc1: BTreeSet<(String, String)> = txt
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f[1].to_string(), f[2].to_string())
        })
        .collect();
    assert_eq!(pc1, BTreeSet::from([("GET".into(), "/v1/models".into()), ("POST".into(), "/v1/responses".into())]));
    // Both are served.
    assert_eq!(h.get(M).0, 200);
    assert_eq!(h.post("openai/v1/responses", &codex_request("first-turn")).0, 400);
    assert_eq!(h.upstream_paths().len(), 1);
    // Every other /openai/v1/ path: 404 in the OpenAI shape.
    let not_found = json!({"error":{"type":"zygo_not_found","message":"no such route"}});
    for (m, p) in [("POST", "openai/v1/embeddings"), ("GET", "openai/v1/files"), ("POST", "openai/v1/rs8p/x")] {
        let (s, _, b) = if m == "POST" { h.post_as(p, &[], &json!({})) } else { h.get_as(p, &[]) };
        assert_eq!((s.as_u16(), json_of(&b)), (404, not_found.clone()), "{m} {p}");
    }
    let (s, _, b) = h.get_as("openai/v1/responses", &[]);
    assert_eq!((s.as_u16(), json_of(&b)), (405, json!({"error":{"type":"zygo_method_not_allowed","message":"method not allowed"}})));
    // The Anthropic side keeps its shape.
    let (s, _, b) = h.get_as("anthropic/v1/rs8p", &[]);
    assert_eq!(
        (s.as_u16(), json_of(&b)),
        (404, json!({"type":"error","error":{"type":"zygo_not_found","message":"no such route"}}))
    );
    assert_eq!(h.upstream_paths().len(), 1);
}

#[test]
fn models_suspended_session_openai_shape() {
    let h = start("");
    let (id, token) = h.team_session();
    let bearer = format!("Bearer {token}");
    let who = [("authorization", bearer.as_str()), ("x-zygo-session", id.as_str())];
    assert_eq!(h.get_as(M, &who).0, 200);
    // §7.2: a team session with no heartbeat for 5 min is suspended.
    h.advance(Duration::from_secs(6 * 60));
    let (s, _, b) = h.get_as(M, &who);
    assert_eq!(
        (s.as_u16(), json_of(&b)),
        (401, json!({"error":{"type":"zygo_session_suspended","message":"session suspended: no heartbeat for 5 min"}}))
    );
}

#[test]
fn models_with_profile_match_the_anthropic_list() {
    let d = zygo_config::builtin::defaults();
    let off = d.role.keys().find(|k| *k != &d.main_role && *k != "code").unwrap().clone();
    let p = Profile {
        name: "rs8p".into(),
        overrides: BTreeMap::from([("code".to_string(), MODEL.to_string())]),
        disabled: vec![off.clone()],
        roles: BTreeMap::from([(
            "rs8p-bench".to_string(),
            CustomRole { model: MODEL.into(), description: "rs8p bench".into(), prompt: "rs8p".into(), writes: false },
        )]),
        ..Default::default()
    };
    let h = Harness::start_with_profile(Dialect::Anthropic, Agent::ClaudeCode, "", Some(p));
    let (s, _, b) = h.get(M);
    assert_eq!(s, 200);
    let oai = json_of(&b)["models"].as_array().unwrap().clone();
    let (s, _, b) = h.get("anthropic/v1/models");
    assert_eq!(s, 200);
    let ant = json_of(&b)["data"].as_array().unwrap().clone();
    // The role part of the Anthropic list (ids and descriptions) equals the OpenAI list; only catalog ids follow it.
    let pairs = |v: &[Value], id: &str| {
        v.iter()
            .map(|m| (m[id].as_str().unwrap().to_string(), m["description"].as_str().unwrap_or("").to_string()))
            .collect::<Vec<_>>()
    };
    let roles = pairs(&oai, "slug");
    assert_eq!(pairs(&ant[..roles.len()], "id"), roles);
    assert!(ant[roles.len()..].iter().all(|m| m.get("description").is_none()), "only catalog ids follow the roles");
    let names: Vec<&str> = roles.iter().map(|r| r.0.as_str()).collect();
    assert!(!names.contains(&off.as_str()), "disabled role {off} is absent");
    assert_eq!(roles.last().unwrap(), &("rs8p-bench".to_string(), "rs8p bench".to_string()));
    let code = &roles.iter().find(|r| r.0 == "code").unwrap().1;
    assert!(code.ends_with(&format!(" · profile rs8p: {MODEL}")), "{code}");
}

#[test]
fn openai_prefix_itself_is_openai_shaped() {
    let h = start("");
    let ant = json!({"type":"error","error":{"type":"zygo_not_found","message":"no such route"}});
    let oai = json!({"error":{"type":"zygo_not_found","message":"no such route"}});
    for (p, want) in [("openai/v1", &oai), ("openai/v1x/models", &ant), ("openai/v10/models", &ant)] {
        let (s, _, b) = h.get_as(p, &[]);
        assert_eq!((s.as_u16(), &json_of(&b)), (404, want), "{p}");
    }
}
