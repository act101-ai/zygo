//! CC-4: the `[cache] insert_markers` default is each client's descriptor value (K§6.1.2 concern 9, K-D3).

use std::collections::BTreeMap;

use zygo_core::Agent;

#[test]
fn insert_markers_default_reads_the_descriptor() {
    let want: BTreeMap<String, bool> = Agent::ALL.iter().map(|a| (a.spec().id.to_string(), a.spec().cache_markers)).collect();
    assert_eq!(zygo_config::CacheConfig::default().insert_markers, want);
    assert_eq!(want, BTreeMap::from([("claude-code".to_string(), false), ("opencode".to_string(), true)]));
}
