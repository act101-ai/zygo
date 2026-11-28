use std::path::PathBuf;

use semver::Version;
use zygo_config::builtin::{defaults, min_agent_version, schema_rules};
use zygo_config::*;
use zygo_core::paths::Paths;
use zygo_core::Agent;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[must_use = "the directory is removed when this value drops; bind it"]
struct Cx {
    _d: tempfile::TempDir,
    c: LoadContext,
}
impl std::ops::Deref for Cx {
    type Target = LoadContext;
    fn deref(&self) -> &LoadContext {
        &self.c
    }
}

fn ctx() -> Cx {
    let t = tempfile::Builder::new().prefix("zygo-n4-").tempdir().unwrap();
    let d = t.path().to_path_buf();
    let paths = Paths {
        config_file: d.join("config.toml"),
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    };
    Cx { c: LoadContext { paths, repo: None, team: None }, _d: t }
}

fn loaded() -> Merged {
    load(&ctx()).unwrap()
}

const IDS: [&str; 6] =
    ["claude-fable-5-2", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "gpt-6.2-astra", "gpt-6.2-sol"];

#[test]
fn builtin_config_passes_check() {
    assert_eq!(check(&loaded().config), vec![]);
    assert_eq!(check(&defaults()), vec![]);
}

#[test]
fn load_returns_builtin_as_layer_1() {
    let m = loaded();
    let d = defaults();
    assert_eq!(m.config.catalog, d.catalog);
    assert_eq!(m.config.role, d.role);
    assert_eq!(m.config.schema_rule, d.schema_rule);
    assert_eq!(m.source_of("role.lead.model"), Layer::Builtin);
}

#[test]
fn every_row_has_source_and_date() {
    let d = defaults();
    let ids: Vec<&str> = d.catalog.0.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, IDS);
    for (i, row) in d.catalog.0.iter().enumerate() {
        let f = row.facts.as_ref().unwrap_or_else(|| panic!("{} has no facts", row.id));
        assert!(f.source.as_deref().unwrap().starts_with("https://"), "{}", row.id);
        // The Anthropic rows were checked again when HUD-4 added `cache_write_5m` and `long_context`.
        let verified_on = if i < 4 { "2026-11-21" } else { "2026-11-18" };
        assert_eq!(f.verified_on.as_deref(), Some(verified_on), "{}", row.id);
        assert_eq!(row.provider.len(), 1, "{}", row.id);
        let p = &row.provider[0];
        let pr = p.price_usd_per_mtok.as_ref().unwrap();
        assert!(pr.input > 0.0 && pr.output > 0.0 && pr.cache_read > 0.0 && pr.cache_write > 0.0, "{}", row.id);
        assert_eq!(row.soft_context_window, None, "{}", row.id);
        let (name, dialect) = if i < 4 { ("anthropic", Dialect::Anthropic) } else { ("openai", Dialect::Openai) };
        assert_eq!(p.name, name, "{}", row.id);
        assert_eq!(p.dialects, vec![dialect], "{}", row.id);
    }
}

#[test]
fn facts_match_vendor_docs() {
    use Effort::*;
    let d = defaults();
    let all = vec![Low, Medium, High, Xhigh, Max];
    // (id, reasoning, context_window, max_output_tokens, sampling, forced_tool_choice)
    let table = [
        ("claude-fable-5-2", Reasoning::Always, 1_000_000, 128_000, false, false),
        ("claude-opus-5-5", Reasoning::Always, 1_000_000, 128_000, false, false),
        ("claude-sonnet-5-5", Reasoning::Always, 1_000_000, 128_000, false, false),
        ("claude-haiku-5-5", Reasoning::Switchable, 1_000_000, 128_000, false, true),
        ("gpt-6.2-astra", Reasoning::Always, 922_000, 128_000, false, false),
        ("gpt-6.2-sol", Reasoning::Always, 922_000, 128_000, false, false),
    ];
    for (id, reasoning, cw, mo, sampling, forced) in table {
        let f = d.catalog.get(id).unwrap().facts.as_ref().unwrap();
        assert_eq!(f.reasoning, reasoning, "{id}");
        assert_eq!(f.context_window, cw, "{id}");
        assert_eq!(f.max_output_tokens, mo, "{id}");
        assert_eq!(f.sampling, sampling, "{id}");
        assert_eq!(f.forced_tool_choice, forced, "{id}");
        assert_eq!(f.effort_levels, all, "{id}");
    }
    let h = d.catalog.get("claude-haiku-5-5").unwrap().facts.as_ref().unwrap();
    assert_eq!(h.reasoning_default_on, Some(true));
}

#[test]
fn roles_map_as_spec_6_4() {
    let d = defaults();
    assert_eq!(d.main_role, "lead");
    let want = [
        ("lead", "claude-opus-5-5", false),
        ("plan", "claude-opus-5-5", true),
        ("triage", "claude-opus-5-5", true),
        ("review", "claude-opus-5-5", false),
        ("design", "claude-fable-5-2", true),
        ("code", "claude-sonnet-5-5", true),
        ("read", "claude-haiku-5-5", false),
    ];
    assert_eq!(d.role.len(), want.len());
    for (name, model, writes) in want {
        let r = &d.role[name];
        assert_eq!(r.model, model, "{name}");
        assert_eq!(r.writes, writes, "{name}");
        assert!(d.catalog.get(&r.model).is_some(), "{name}");
        assert!(r.dialect_models.is_empty(), "{name}");
    }
}

#[test]
fn prompts_equal_spec_6_5_verbatim() {
    let spec = std::fs::read_to_string(root().join("docs/specs/2026-11-18-zygo-design.md")).unwrap();
    let sec = spec.split("### 6.5").nth(1).unwrap().split("\n## 7.").next().unwrap();
    let d = defaults();
    let mut found = 0;
    for line in sec.lines() {
        let Some(rest) = line.strip_prefix("| `") else { continue };
        let (role, text) = rest.split_once("` | ").unwrap();
        let text = text.strip_suffix(" |").unwrap();
        assert_eq!(d.role[role].prompt, text, "{role}");
        found += 1;
    }
    assert_eq!(found, 7);
    assert_eq!(d.role["lead"].description, "Orchestrates the session: plans, briefs subagents, checks their work.");
}

#[test]
fn redaction_patterns_in_builtin_layer() {
    assert!(!builtin_patterns().is_empty());
    assert_eq!(defaults().redaction.pattern, builtin_patterns());
    assert_eq!(loaded().config.redaction.pattern, builtin_patterns());
}

#[test]
fn show_lists_builtin_keys() {
    let m = loaded();
    let out = m.show(None, &|_| true);
    for line in [
        "role.lead.model = \"claude-opus-5-5\"  # builtin",
        "model.claude-opus-5-5.facts.context_window = 1000000  # builtin",
        "model.claude-opus-5-5.provider.anthropic.key = \"env:ANTHROPIC_API_KEY\" <set>  # builtin",
        "redaction.pattern.anthropic_api_key.action = \"redact\"  # builtin",
        "schema_rule.\"openai:closed_object\".severity = \"refuse\"  # builtin",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line}\n{out}");
    }
    assert!(m.show(Some(Layer::User), &|_| true).is_empty());
}

const A: &str = "https://platform.claude.com/docs/en/build-with-claude/structured-outputs";
const O: &str = "https://developers.openai.com/api/docs/guides/structured-outputs";

const EVIDENCE: [(&str, &str, &str); 14] = [
    ("anthropic", "map_object", "https://platform.claude.com/docs/en/build-with-claude/structured-outputs#objects-with-arbitrary-keys"),
    ("openai", "map_object", O),
    ("anthropic", "one_of", "https://platform.claude.com/docs/en/build-with-claude/structured-outputs#json-schema-limitations"),
    ("anthropic", "all_of", A),
    ("openai", "all_of", O),
    ("openai", "conditional", O),
    ("anthropic", "closed_object", A),
    ("openai", "closed_object", O),
    ("anthropic", "numeric_bounds", A),
    ("anthropic", "string_length", A),
    ("anthropic", "array_bounds", A),
    ("anthropic", "pattern", A),
    ("openai", "optional_property", O),
    ("anthropic", "optional_properties_max", "https://platform.claude.com/docs/en/build-with-claude/structured-outputs#schema-complexity-limits"),
];

#[test]
fn schema_rules_have_fixed_order_and_evidence() {
    let rules = schema_rules();
    assert_eq!(rules.len(), EVIDENCE.len());
    for (r, (family, code, url)) in rules.iter().zip(EVIDENCE) {
        assert_eq!((r.family.name(), r.code.as_str()), (family, code));
        assert!(url.starts_with(A) || url.starts_with(O), "{family}:{code}");
        let key = format!("{family}:{code}");
        let limit = match key.as_str() {
            "anthropic:array_bounds" => Some(1),
            "anthropic:optional_properties_max" => Some(24),
            _ => None,
        };
        assert_eq!(r.limit, limit, "{key}");
        let sev = if key == "anthropic:pattern" { RuleSeverity::Warn } else { RuleSeverity::Refuse };
        assert_eq!(r.severity, sev, "{key}");
    }
}

#[test]
fn show_omits_agent_minimums() {
    let out = loaded().show(None, &|_| true);
    assert!(!out.contains("min_version"));
    assert!(!out.lines().any(|l| l.starts_with("agents")));
}

#[test]
fn minimum_agent_versions_match_premises() {
    let p = std::fs::read_to_string(root().join("docs/premises.md")).unwrap();
    let pin = |name: &str| -> Version {
        let l = p.lines().find_map(|l| l.strip_prefix(name)).unwrap();
        Version::parse(l.trim()).unwrap()
    };
    for a in Agent::ALL {
        assert_eq!(min_agent_version(a), pin(&format!("{}: ", a.spec().name)), "{a}");
    }
    assert!(min_agent_version(Agent::ClaudeCode) >= Version::new(2, 1, 323));
}

#[test]
fn builtin_layer_has_the_act101_marketplace() {
    let m = defaults().marketplaces;
    assert_eq!(m.len(), 1);
    let a = &m[0];
    assert_eq!((a.name.as_str(), a.source.as_str()), ("act101", "github:act101-ai/act101"));
    assert_eq!(a.default_on, ["act-lite"]);
    assert_eq!(a.git_ref, "v2.9.2");
    assert_eq!(a.commit.as_deref(), Some("9d82bd9714927ac713212c07b60d87fd73a7500f"));
}

#[test]
fn load_returns_the_builtin_marketplace() {
    let m = loaded();
    let names: Vec<&str> = m.config.marketplaces.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["act101"]);
    assert_eq!(m.config.marketplaces[0].source, "github:act101-ai/act101");
    assert_eq!(m.source_of("marketplace.act101"), Layer::Builtin);
}
