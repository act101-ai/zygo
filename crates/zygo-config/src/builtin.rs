//! Layer 1 of §6.1: catalog, roles, prompts, redaction patterns, schema rules, agent minimums.
use std::collections::BTreeMap;

use semver::Version;
use serde::Deserialize;
use zygo_core::Agent;

use crate::builtin_patterns::builtin_patterns;
use crate::schema::{Catalog, Config, Marketplace, ModelRow, Role, SchemaRule};

#[derive(Deserialize)]
struct CatalogFile { model: Vec<ModelRow> }
#[derive(Deserialize)]
struct RolesFile { main_role: String, role: BTreeMap<String, Role> }
#[derive(Deserialize)]
struct RulesFile { schema_rule: Vec<SchemaRule> }
#[derive(Deserialize)]
struct MarketplaceFile { marketplace: Vec<Marketplace> }

/// The built-in layer as a `Config`: `Config::default()` with the catalog, `main_role`, roles, the built-in marketplace (§15.9), `redaction.pattern = builtin_patterns()` and `schema_rule = schema_rules()` from the data files.
pub fn defaults() -> Config {
    let catalog: CatalogFile =
        toml::from_str(include_str!("../data/catalog.toml")).expect("built-in catalog is valid TOML");
    let roles: RolesFile =
        toml::from_str(include_str!("../data/roles.toml")).expect("built-in roles are valid TOML");
    let market: MarketplaceFile =
        toml::from_str(include_str!("../data/marketplace.toml")).expect("built-in marketplace is valid TOML");
    let mut c = Config {
        catalog: Catalog(catalog.model),
        main_role: roles.main_role,
        role: roles.role,
        marketplaces: market.marketplace,
        schema_rule: schema_rules(),
        ..Config::default()
    };
    c.redaction.pattern = builtin_patterns();
    c
}

/// The built-in `[[schema_rule]]` set in its fixed order, parsed from `data/schema_rules.toml`.
pub fn schema_rules() -> Vec<SchemaRule> {
    let f: RulesFile = toml::from_str(include_str!("../data/schema_rules.toml"))
        .expect("built-in schema rules are valid TOML");
    f.schema_rule
}

/// The minimum supported version of `agent` from its descriptor (`Agent::spec().min_version`), parsed with `Version::parse`.
pub fn min_agent_version(agent: Agent) -> Version {
    Version::parse(agent.spec().min_version).expect("minimum version is semver")
}
