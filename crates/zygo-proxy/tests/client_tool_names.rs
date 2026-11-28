use zygo_core::Agent;
use zygo_proxy::window::{compact_tool, responses_compact_tool};

#[test]
fn compact_tool_reads_the_client_form() {
    let cc = "claude-code".parse::<Agent>().unwrap().spec();
    let oc = "opencode".parse::<Agent>().unwrap().spec();
    assert!(compact_tool(cc, "mcp__zygo__compact"));
    assert!(compact_tool(cc, "mcp__plugin_x_zygo__compact"));
    assert!(!compact_tool(cc, "zygo_compact"));
    assert!(!compact_tool(cc, "mcp__zygox__compact"));
    // zygo never lists `compact` to OpenCode, so no name of either form is its compact tool.
    for n in ["zygo_compact", "xzygo_compact", "foo_zygo_compact", "plugin-zygo_compact", "mcp__zygo__compact", "zygo__compactx"] {
        assert!(!compact_tool(oc, n), "{n}");
    }
}

#[test]
fn compact_tool_needs_the_compact_listing() {
    // A client armed for the window rewrite that is not listed `compact`: no name of its own form matches.
    let mut oc = "opencode".parse::<Agent>().unwrap().spec().clone();
    oc.window_rewrite = true;
    oc.lists_compact_tool = false;
    for n in ["zygo_compact", "xzygo_compact", "foo_zygo_compact"] {
        assert!(!compact_tool(&oc, n), "{n}");
    }
    let mut cc = "claude-code".parse::<Agent>().unwrap().spec().clone();
    cc.lists_compact_tool = false;
    assert!(!compact_tool(&cc, "mcp__zygo__compact"));
    // The same form with the listing on matches.
    oc.lists_compact_tool = true;
    assert!(compact_tool(&oc, "zygo_compact"));
}

#[test]
fn responses_compact_name_is_the_fixed_flat_form() {
    // The Responses form joins `<namespace>__<name>` (K§6.4.3) for every client (K§6.4.5): a client whose own
    // MCP form is OpenCode's still matches a namespaced `compact` call.
    let mut oc = "opencode".parse::<Agent>().unwrap().spec().clone();
    oc.lists_compact_tool = true;
    assert!(responses_compact_tool(&oc, Some("mcp__zygo"), "compact"));
    assert!(responses_compact_tool(&oc, Some("mcp__plugin_x_zygo"), "compact"));
    assert!(responses_compact_tool(&oc, None, "mcp__zygo__compact"));
    assert!(!responses_compact_tool(&oc, Some("mcp__zygox"), "compact"));
    assert!(!responses_compact_tool(&oc, None, "zygo_compact"));
    oc.lists_compact_tool = false;
    assert!(!responses_compact_tool(&oc, Some("mcp__zygo"), "compact"));
    // Claude Code keeps today's results.
    let cc = "claude-code".parse::<Agent>().unwrap().spec();
    assert!(responses_compact_tool(cc, Some("mcp__zygo"), "compact"));
    assert!(responses_compact_tool(cc, None, "mcp__zygo__compact"));
    for (ns, n) in [(Some("mcp__zygox"), "compact"), (Some("mcp__zygo"), "compactx"), (Some("zygo"), "compact"), (None, "zygo_compact"), (None, "mcp____compact")] {
        assert!(!responses_compact_tool(cc, ns, n), "{ns:?} {n}");
    }
    // OpenCode's own descriptor is not listed `compact`: no match.
    assert!(!responses_compact_tool("opencode".parse::<Agent>().unwrap().spec(), Some("mcp__zygo"), "compact"));
}
