use zygo_core::{Agent, ClientSpec};

fn spec(id: &str) -> &'static ClientSpec {
    id.parse::<Agent>().unwrap().spec()
}

#[test]
fn client_specs_hold_the_concern_5_values() {
    let c = spec("claude-code");
    assert!(c.lists_compact_tool);
    assert_eq!((c.mcp_server)("mcp__zygo__compact", "compact"), Some("zygo"));
    assert_eq!((c.mcp_server)("mcp__plugin_x_zygo__compact", "compact"), Some("plugin_x_zygo"));
    assert_eq!((c.mcp_server)("mcp__act-lite__symbols_batch", "symbols_batch"), Some("act-lite"));
    assert_eq!((c.mcp_server)("mcp__act-lite__symbols_batch", "batch"), None);
    assert_eq!((c.mcp_server)("mcp____compact", "compact"), Some(""));
    for n in ["mcp__zygo__compactx", "zygo__compact", "compact", "zygo_compact", "mcp_zygo_compact"] {
        assert_eq!((c.mcp_server)(n, "compact"), None, "{n}");
    }
    let o = spec("opencode");
    assert!(!o.lists_compact_tool);
    assert_eq!((o.mcp_server)("zygo_probe_tool", "probe_tool"), Some("zygo"));
    assert_eq!((o.mcp_server)("user-mcp_probe_tool", "probe_tool"), Some("user-mcp"));
    assert_eq!((o.mcp_server)("act101-act-lite_symbols_batch", "symbols_batch"), Some("act101-act-lite"));
    assert_eq!((o.mcp_server)("mcp__zygo__compact", "compact"), Some("mcp__zygo_"));
    for n in ["zygocompact", "compact", "zygo_compactx"] {
        assert_eq!((o.mcp_server)(n, "compact"), None, "{n}");
    }
}
