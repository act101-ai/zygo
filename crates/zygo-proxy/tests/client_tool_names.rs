use zygo_core::Agent;
use zygo_proxy::pipeline::ModelRoute;
use zygo_proxy::window::{compact_tool, responses_compact_tool, route_compact_tool};

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
    // `compact_tool` reads `lists_compact_tool` only (not `window_rewrite`): a descriptor that is not listed `compact`
    // matches no name of its own form, and the same descriptor listed matches only that form's compact tool name.
    let mut oc = "opencode".parse::<Agent>().unwrap().spec().clone();
    oc.lists_compact_tool = false;
    for n in ["zygo_compact", "xzygo_compact", "foo_zygo_compact"] {
        assert!(!compact_tool(&oc, n), "{n}");
    }
    let mut cc = "claude-code".parse::<Agent>().unwrap().spec().clone();
    cc.lists_compact_tool = false;
    assert!(!compact_tool(&cc, "mcp__zygo__compact"));
    oc.lists_compact_tool = true;
    for n in ["zygo_compact", "xzygo_compact", "foo_zygo_compact", "plugin-zygo_compact"] {
        assert!(compact_tool(&oc, n), "{n}");
    }
    // Near misses of OpenCode's form `<server>_compact` with the server ending in `zygo`.
    for n in ["zygox_compact", "zygo_compactx", "zygo__compact", "zygocompact", "_compact", "compact", "zygo_Compact", "zygo_compac", "zyg_compact", "Zygo_compact", "zygo-compact", "mcp__zygo__compact"] {
        assert!(!compact_tool(&oc, n), "{n}");
    }
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

#[test]
fn compact_name_follows_the_request_route() {
    // A Responses-route request answered by an Anthropic or chat upstream is read as Messages lines; its tool names
    // are still the fixed flat join `<namespace>__<name>` (K§6.4.3, K§6.4.5), so they match as on the Responses form.
    let mut oc = "opencode".parse::<Agent>().unwrap().spec().clone();
    oc.lists_compact_tool = true;
    assert!(route_compact_tool(&oc, ModelRoute::Responses, "mcp__zygo__compact"));
    assert_eq!(
        route_compact_tool(&oc, ModelRoute::Responses, "mcp__zygo__compact"),
        responses_compact_tool(&oc, Some("mcp__zygo"), "compact")
    );
    assert!(!route_compact_tool(&oc, ModelRoute::Responses, "zygo_compact"));
    // A Messages-route request's names are in the client's own form.
    assert!(route_compact_tool(&oc, ModelRoute::Messages, "zygo_compact"));
    assert!(!route_compact_tool(&oc, ModelRoute::Messages, "mcp__zygo__compact"));
    oc.lists_compact_tool = false;
    for route in [ModelRoute::Messages, ModelRoute::Responses] {
        for n in ["zygo_compact", "mcp__zygo__compact"] {
            assert!(!route_compact_tool(&oc, route, n), "{route:?} {n}");
        }
    }
    // Claude Code's own form is the flat form: both routes keep today's results.
    let cc = "claude-code".parse::<Agent>().unwrap().spec();
    for route in [ModelRoute::Messages, ModelRoute::Responses] {
        assert!(route_compact_tool(cc, route, "mcp__zygo__compact"), "{route:?}");
        assert!(route_compact_tool(cc, route, "mcp__plugin_x_zygo__compact"), "{route:?}");
        for n in ["mcp__zygox__compact", "mcp__zygo__compactx", "zygo__compact", "zygo_compact", "mcp____compact", "compact"] {
            assert!(!route_compact_tool(cc, route, n), "{route:?} {n}");
        }
    }
}
