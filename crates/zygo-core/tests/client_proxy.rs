//! CC-4: the concern 9 facts (proxy, redaction, usage and work-join policy) of each client descriptor (K§6.1.2).

use zygo_core::{Agent, Dialect};

#[test]
fn client_specs_hold_the_concern_9_values() {
    let c = "claude-code".parse::<Agent>().unwrap().spec();
    assert_eq!(c.dialect, Dialect::Anthropic);
    assert_eq!(c.request_class_header, Some("x-claude-code-request-class"));
    assert_eq!(c.compaction_header, Some("x-claude-code-compaction"));
    assert_eq!(c.agent_id_header, Some("x-claude-code-agent-id"));
    assert_eq!(c.agent_type_header, Some("x-claude-code-agent-type"));
    assert!(c.window_rewrite);
    assert_eq!(c.passthrough_credential, Some("claude-ai"));
    assert!(!c.cache_markers);
    assert!(c.redact_skip_tags.is_empty());
    let o = "opencode".parse::<Agent>().unwrap().spec();
    assert_eq!(o.dialect, Dialect::Anthropic);
    assert_eq!((o.request_class_header, o.compaction_header, o.agent_id_header, o.agent_type_header), (None, None, None, None));
    assert!(!o.window_rewrite);
    assert_eq!(o.passthrough_credential, None);
    assert!(o.cache_markers);
    assert_eq!(o.redact_skip_tags, &["file", "shell"][..]);
}
