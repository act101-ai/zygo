//! `zygo-config` reads the client descriptor (K§6.1.2 concern 1): the minimum versions and `Dialect` from core.

use semver::Version;
use serde_json::json;
use zygo_config::builtin::min_agent_version;
use zygo_config::{load_with_builtin, LoadContext};
use zygo_core::paths::Paths;
use zygo_core::{Agent, Dialect};

#[test]
fn min_agent_version_reads_the_descriptor() {
    for a in Agent::ALL {
        assert_eq!(min_agent_version(a), Version::parse(a.spec().min_version).unwrap(), "{a}");
    }
    assert_eq!(min_agent_version(Agent::ALL[0]), Version::new(2, 1, 328));
    assert_eq!(min_agent_version(Agent::ALL[1]), Version::new(1, 18, 16));
}

/// A user config file with each dialect name, read by the config loader, gives the `zygo_core::Dialect` values; the
/// loaded value written back as JSON keeps its name.
#[test]
fn config_dialects_load_as_the_core_dialect() {
    let d = tempfile::tempdir().unwrap();
    let config_file = d.path().join("config.toml");
    std::fs::write(
        &config_file,
        "[[model]]\nid = \"m\"\n[[model.provider]]\nname = \"p\"\nbase_url = \"https://p.example.invalid\"\n\
         upstream_model = \"u\"\ndialects = [\"anthropic\", \"openai\", \"openai-chat\"]\n",
    )
    .unwrap();
    let paths = Paths {
        config_file,
        cache_dir: d.path().join("cache"),
        data_dir: d.path().join("data"),
        sessions_dir: d.path().join("cache/sessions"),
        marketplaces_dir: d.path().join("cache/marketplaces"),
        plugin_store: d.path().join("data/plugins"),
        approvals_file: d.path().join("data/approvals.toml"),
        usage_db: d.path().join("data/usage.db"),
    };
    let m = load_with_builtin(&LoadContext { paths, repo: None, team: None }, toml::Table::new()).unwrap();
    let got: Vec<Dialect> = m.config.catalog.0[0].provider[0].dialects.clone();
    assert_eq!(got, [Dialect::Anthropic, Dialect::Openai, Dialect::OpenaiChat]);
    assert_eq!(serde_json::to_value(&got).unwrap(), json!(["anthropic", "openai", "openai-chat"]));
}
