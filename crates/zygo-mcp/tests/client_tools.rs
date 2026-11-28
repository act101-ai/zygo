use zygo_config::Features;
use zygo_core::Agent;
use zygo_mcp::{tools, McpCtx};

#[test]
fn compact_listing_reads_the_descriptor() {
    for a in Agent::ALL {
        let listed = match a.as_str() {
            "claude-code" => true,
            "opencode" => false,
            other => panic!("no expected `compact` listing for the client `{other}`"),
        };
        for on in [true, false] {
            let ctx = McpCtx { agent: a, features: Features { compact_tool: on, ..Features::default() }, session_dir: None };
            let names: Vec<String> = tools::build_with(&ctx, |_| vec![], |_| vec![]).unwrap().iter().map(|t| t.def().name).collect();
            let want: Vec<String> = if on && listed { vec!["compact".to_string()] } else { vec![] };
            assert_eq!(names, want, "{a} compact_tool={on}");
        }
    }
    assert!("claude-code".parse::<Agent>().unwrap().spec().lists_compact_tool);
    assert!(!"opencode".parse::<Agent>().unwrap().spec().lists_compact_tool);
}
