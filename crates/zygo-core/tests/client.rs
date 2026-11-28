//! The client module `zygo_core::client` (K§6.1.1, K§6.1.2 concerns 1 and 2): the descriptor values, the id
//! round trip, `Agent::DEFAULT`, the hook input function and `Dialect` in core. No test names a client variant by
//! path: clients come from `Agent::ALL`, `Agent::DEFAULT` or an id.

use serde::de::IntoDeserializer;
use serde::Deserialize;
use serde_json::json;
use zygo_core::hook::HookInput;
use zygo_core::Agent;

#[test]
fn client_specs_hold_the_concern_1_values() {
    assert_eq!(Agent::ALL.len(), 2);
    let c = Agent::ALL[0].spec();
    assert_eq!(c.id, "claude-code");
    assert_eq!(c.name, "Claude Code");
    assert_eq!(c.program, "claude");
    assert_eq!(c.verb, "claude");
    assert_eq!(c.config_dir, ".claude");
    assert_eq!(c.install_hint, "zygo: claude not found on PATH; install Claude Code: https://code.claude.com/docs/en/setup");
    assert_eq!(c.min_version, "2.1.328");
    assert!(c.supported);
    assert!(c.status_line);
    let o = Agent::ALL[1].spec();
    assert_eq!(o.id, "opencode");
    assert_eq!(o.name, "OpenCode");
    assert_eq!(o.program, "opencode");
    assert_eq!(o.verb, "opencode");
    assert_eq!(o.config_dir, ".opencode");
    assert_eq!(o.install_hint, "zygo: opencode was not found on PATH; install OpenCode: https://opencode.ai/docs/");
    assert_eq!(o.min_version, "1.18.16");
    assert!(o.supported);
    assert!(!o.status_line);
}

#[test]
fn client_ids_round_trip_through_from_str_display_and_serde() {
    for a in Agent::ALL {
        let id = a.spec().id;
        assert_eq!(a.as_str(), id);
        assert_eq!(a.to_string(), id);
        assert_eq!(id.parse::<Agent>().unwrap(), a);
        assert_eq!(serde_json::to_value(a).unwrap(), json!(id));
        assert_eq!(serde_json::from_value::<Agent>(json!(id)).unwrap(), a);
    }
}

#[test]
fn unknown_client_id_is_refused_with_the_same_text() {
    assert_eq!("codex".parse::<Agent>().unwrap_err().to_string(), "unknown agent `codex` (claude-code or opencode)");
}

/// The serde form is built from the descriptor ids: an unknown id, or an id in another case, is refused with the
/// unknown-variant text that names every known id; a value that is not a string is refused.
#[test]
fn unknown_client_id_is_refused_by_serde() {
    for bad in ["codex", "Claude-Code", ""] {
        let e = serde_json::from_value::<Agent>(json!(bad)).unwrap_err().to_string();
        assert_eq!(e, format!("unknown variant `{bad}`, expected `claude-code` or `opencode`"));
    }
    // serde's own value deserializer: its text does not hang on the `serde_json` features of the build (with
    // `arbitrary_precision`, which zygo-proxy and zygo-plugins turn on, a JSON number reads as "number").
    let number = Agent::deserialize(IntoDeserializer::<serde::de::value::Error>::into_deserializer(1u64));
    assert_eq!(number.unwrap_err().to_string(), "invalid type: integer `1`, expected a client id");
    let text = |v| serde_json::from_value::<Agent>(v).unwrap_err().to_string();
    assert!(text(json!(1)).ends_with(", expected a client id"), "{}", text(json!(1)));
    assert_eq!(text(json!(null)), "invalid type: null, expected a client id");
    // The enum-map form of the old derive; `Serialize` never wrote it.
    assert_eq!(text(json!({"opencode": null})), "invalid type: map, expected a client id");
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Holder {
    agent: Agent,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct Flat {
    #[serde(flatten)]
    holder: Holder,
    other: u32,
}

/// The id is read from borrowed JSON text, from TOML and through a `#[serde(flatten)]` field (serde's buffered
/// content), and written back as the id; the TOML table form of the old derive is refused.
#[test]
fn client_ids_read_from_json_text_toml_and_flatten() {
    for a in Agent::ALL {
        let id = a.spec().id;
        assert_eq!(serde_json::from_str::<Agent>(&format!("\"{id}\"")).unwrap(), a);
        let t = format!("agent = \"{id}\"\n");
        assert_eq!(toml::from_str::<Holder>(&t).unwrap(), Holder { agent: a });
        assert_eq!(toml::to_string(&Holder { agent: a }).unwrap(), t);
        let f: Flat = serde_json::from_str(&format!("{{\"agent\": \"{id}\", \"other\": 7}}")).unwrap();
        assert_eq!(f, Flat { holder: Holder { agent: a }, other: 7 });
    }
    assert!(toml::from_str::<Holder>("[agent.opencode]\n").is_err());
    let e = serde_json::from_value::<Flat>(json!({"agent": "codex", "other": 7})).unwrap_err().to_string();
    assert_eq!(e, "unknown variant `codex`, expected `claude-code` or `opencode`");
}

#[test]
fn default_client_is_claude_code() {
    assert_eq!(Agent::DEFAULT.spec().id, "claude-code");
    assert_eq!(zygo_core::session_view::SessionView::default().agent, Agent::DEFAULT);
}

#[test]
fn hook_input_reads_the_descriptor() {
    let v = json!({"input": {"tool": "bash"}, "output": {"args": {"command": "ls"}}});
    assert_eq!((Agent::DEFAULT.spec().hook_input)(v.clone()), v);
    let oc = "opencode".parse::<Agent>().unwrap();
    let want = json!({"tool_name": "Bash", "hook_event_name": "PreToolUse", "tool_input": {"command": "ls"}});
    assert_eq!((oc.spec().hook_input)(v.clone()), want);
    let bytes = v.to_string();
    let h = HookInput::parse(oc, bytes.as_bytes()).unwrap();
    assert_eq!(h.event.as_deref(), Some("PreToolUse"));
    assert_eq!(h.tool_name.as_deref(), Some("Bash"));
    let d = HookInput::parse(Agent::DEFAULT, bytes.as_bytes()).unwrap();
    assert_eq!(d.event, None);
    assert_eq!(d.tool_name, None);
    assert_eq!(d.raw, v);
}

#[test]
fn dialect_keeps_its_serde_names() {
    use zygo_core::Dialect;
    for (d, s) in [(Dialect::Anthropic, "anthropic"), (Dialect::Openai, "openai"), (Dialect::OpenaiChat, "openai-chat")] {
        assert_eq!(serde_json::to_value(d).unwrap(), json!(s));
        assert_eq!(serde_json::from_value::<Dialect>(json!(s)).unwrap(), d);
    }
}
