use zygo_core::Agent;
use zygo_proxy::window::compact_tool;

#[test]
fn compact_tool_reads_the_client_form() {
    let cc = "claude-code".parse::<Agent>().unwrap().spec();
    let oc = "opencode".parse::<Agent>().unwrap().spec();
    assert!(compact_tool(cc, "mcp__zygo__compact"));
    assert!(compact_tool(cc, "mcp__plugin_x_zygo__compact"));
    assert!(!compact_tool(cc, "zygo_compact"));
    assert!(!compact_tool(cc, "mcp__zygox__compact"));
    assert!(compact_tool(oc, "zygo_compact"));
    assert!(compact_tool(oc, "plugin-zygo_compact"));
    assert!(!compact_tool(oc, "mcp__zygo__compact"));
    assert!(!compact_tool(oc, "zygo__compactx"));
}
