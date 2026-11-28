//! CC-4: the §12.2 passthrough row of a client is the one its descriptor names (K§6.1.2 concern 9).

use zygo_core::Agent;
use zygo_proxy::passthrough::{CredentialType, TYPES};

#[test]
fn credential_type_reads_the_descriptor() {
    for a in Agent::ALL {
        assert_eq!(CredentialType::for_agent(a).map(|t| t.name), a.spec().passthrough_credential, "{a}");
        if let Some(n) = a.spec().passthrough_credential {
            assert!(TYPES.iter().any(|t| t.name == n), "{a}: no row {n}");
        }
    }
    assert_eq!(CredentialType::for_agent("claude-code".parse().unwrap()).map(|t| t.name), Some("claude-ai"));
    assert_eq!(CredentialType::for_agent("opencode".parse().unwrap()), None);
}
