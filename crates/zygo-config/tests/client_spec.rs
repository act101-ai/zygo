//! `zygo-config` reads the client descriptor (K§6.1.2 concern 1): the minimum versions and `Dialect` from core.

use semver::Version;
use serde_json::json;
use zygo_config::builtin::min_agent_version;
use zygo_core::Agent;

#[test]
fn min_agent_version_reads_the_descriptor() {
    for a in Agent::ALL {
        assert_eq!(min_agent_version(a), Version::parse(a.spec().min_version).unwrap(), "{a}");
    }
    assert_eq!(min_agent_version(Agent::ALL[0]), Version::new(2, 1, 328));
    assert_eq!(min_agent_version(Agent::ALL[1]), Version::new(1, 18, 16));
}

#[test]
fn schema_dialect_is_the_core_dialect() {
    let d: zygo_core::Dialect = zygo_config::schema::Dialect::OpenaiChat;
    let e: zygo_config::Dialect = zygo_core::Dialect::Anthropic;
    assert_eq!(serde_json::to_value(d).unwrap(), json!("openai-chat"));
    assert_eq!(e, zygo_core::Dialect::Anthropic);
}
