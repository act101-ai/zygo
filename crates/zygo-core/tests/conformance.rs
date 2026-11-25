//! The conformance check and generator (K§6.2.5). CF-1: the input model and check items 1-9 on fixture inputs.
#[allow(dead_code)] // later nodes read the rest of the model
#[path = "conformance/model.rs"]
mod model;

use zygo_core::Agent;

#[test]
fn check_agent_all() {
    assert_eq!(Agent::ALL, [Agent::ClaudeCode, Agent::OpenCode]);
    for a in Agent::ALL {
        assert_eq!(a.as_str().parse::<Agent>().unwrap(), a);
    }
}
