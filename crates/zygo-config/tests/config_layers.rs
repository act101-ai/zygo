use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use zygo_config::*;
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;

// ---- helpers ----

#[must_use = "the directory is removed when this value drops; bind it"]
struct Tmp(tempfile::TempDir);
impl std::ops::Deref for Tmp {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.0.path()
    }
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

fn tmp(name: &str) -> Tmp {
    Tmp(tempfile::Builder::new().prefix(&format!("zygo-n3-{name}-")).tempdir().unwrap())
}

static CTX_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn ctx(user: Option<&str>, repo: Option<PathBuf>, team: Option<&str>) -> Cx {
    let n = CTX_N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = tmp(&format!("ctx{n}"));
    let config_file = d.join("config.toml");
    if let Some(u) = user {
        std::fs::write(&config_file, u).unwrap();
    }
    let paths = Paths {
        config_file,
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    };
    Cx { c: LoadContext { paths, repo, team: team.map(|t| toml::from_str(t).unwrap()) }, _d: d.0 }
}

fn loaded(user: &str) -> Merged {
    load_with_builtin(&ctx(Some(user), None, None), toml::Table::new()).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn model(id: &str) -> String {
    format!(
        "[[model]]\nid = \"{id}\"\n[model.facts]\nreasoning = \"always\"\neffort_levels = [\"low\",\"high\"]\ncontext_window = 200000\nmax_output_tokens = 64000\n[[model.provider]]\nname = \"p\"\nbase_url = \"https://p.example.invalid\"\nupstream_model = \"u\"\ndialects = [\"anthropic\"]\nkey = \"env:ZYGO_N3_TEST_KEY\"\n"
    )
}

fn base() -> String {
    model("a") + "[role.lead]\nmodel = \"a\"\ndescription = \"d\"\nprompt = \"p\"\n"
}

fn cfg(text: &str) -> Config {
    toml::from_str::<Config>(text).unwrap()
}

// ---- step 1: schema ----

#[test]
fn defaults_match_spec() {
    let c = Config::default();
    assert_eq!(c.main_role, "lead");
    assert_eq!((c.results.trim_threshold_bytes, c.results.head_lines, c.results.tail_lines), (16384, 60, 20));
    assert!(c.features.results && c.features.work_units && c.features.compact_tool);
    assert!(c.features.redaction.secrets && c.features.redaction.content);
    assert_eq!(c.guards.trailers, Level::Notify);
    for l in [c.guards.min_level, c.guards.claims, c.guards.scope, c.guards.navigation, c.guards.output, c.guards.repeat] {
        assert_eq!(l, Level::Monitor);
    }
    assert_eq!(c.navigation.lines, 400);
    assert_eq!(c.limits.subagent_max_turns, 0);
    assert_eq!(c.work.sources, ["work-graph", "beads", "github", "branch"]);
    assert_eq!(c.usage.retain_days, 90);
    assert!(!c.proxy.require_zdr && !c.proxy.strict_schemas);
    assert_eq!((c.proxy.max_inflight_mib, c.proxy.redaction_cache_mib), (512, 256));
    assert_eq!(
        c.cache.insert_markers,
        BTreeMap::from([("claude-code".to_string(), false), ("opencode".to_string(), true)])
    );
    assert_eq!(c.redaction.content.replacement, "🤬");
    assert!(Effort::None < Effort::Minimal);
    assert!(Effort::Minimal < Effort::Low);
    assert!(Effort::Low < Effort::Medium);
    assert!(Effort::Medium < Effort::High);
    assert!(Effort::High < Effort::Xhigh);
    assert!(Effort::Xhigh < Effort::Max);
    assert!(Level::Monitor < Level::Notify && Level::Notify < Level::Enforce);
}

#[test]
fn defaults_when_keys_omitted() {
    assert!(cfg(&model("a")).catalog.0[0].provider[0].enabled);
    let c = cfg("[profile.x.roles.r]\nmodel = \"a\"\ndescription = \"d\"\nprompt = \"p\"");
    assert!(c.profile["x"].roles["r"].writes);
    let c = cfg("[[redaction.pattern]]\nid = \"t\"\npattern = \"x\"\ndescription = \"d\"");
    assert_eq!(c.redaction.pattern[0].action, Action::Redact);
    let c = cfg("[features]\nresults = false");
    assert!(!c.features.results);
    assert!(c.features.work_units && c.features.compact_tool);
    assert!(c.features.redaction.secrets && c.features.redaction.content);
    let c = cfg("[results]\nhead_lines = 5");
    assert_eq!((c.results.head_lines, c.results.trim_threshold_bytes, c.results.tail_lines), (5, 16384, 20));
    assert_eq!(cfg("[proxy]\nrequire_zdr = true").proxy.max_inflight_mib, 512);
    assert_eq!(cfg("[guards]\nclaims = \"notify\"").guards.trailers, Level::Notify);
}

#[test]
fn workspace_tool_defaults_off_and_only_the_config_sets_it() {
    assert!(!Features::default().workspace_tool);
    assert!(!Config::default().features.workspace_tool);
    assert!(!cfg("[features]\nresults = false").features.workspace_tool);
    assert!(!loaded("").config.features.workspace_tool);
    let m = loaded("[features]\nworkspace_tool = true\n");
    assert!(m.config.features.workspace_tool);
    assert_eq!(m.source_of("features.workspace_tool"), Layer::User);
    // A profile cannot set it (AU65): profile validation refuses the key.
    let m = loaded("[profile.p]\nfeatures = { workspace_tool = true }\n");
    let p = zygo_config::profile::select(&m, Some("p")).unwrap();
    let c = &m.config;
    let v = zygo_config::profile::validate(&p, &c.role, &c.main_role, &c.catalog.view(), Dialect::Anthropic, &BTreeSet::new())
        .unwrap_err();
    assert_eq!(v.to_string(), "profile.p.features.workspace_tool: not a feature key");
}

const SPEC_6_3: &str = r#"
[[model]]
id = "claude-opus-5-5"
soft_context_window = 400000
  [model.facts]
  reasoning = "always"
  effort_levels = ["low", "medium", "high", "xhigh", "max"]
  max_output_tokens = 128000
  context_window = 1000000
  sampling = false
  vision = true
  structured_output = "native"
  forced_tool_choice = false
  verified_on = 2026-11-18
  source = "https://example.invalid/facts"
  [[model.provider]]
  name = "anthropic"
  base_url = "https://api.anthropic.com"
  dialects = ["anthropic"]
  upstream_model = "claude-opus-5-5"
  key = "env:ANTHROPIC_API_KEY"
  enabled = true
  [model.provider.price_usd_per_mtok]
  input = 4.0
  output = 20.0
  cache_read = 0.20
  cache_write = 8.0
  [model.provider.flags]
  zdr = false
"#;

const SPEC_6_4: &str = r#"
main_role = "lead"
[role.lead]
model = "claude-opus-5-5"
description = "Orchestrates the session: plans, briefs subagents, checks their work."
prompt = "x"
writes = false
"#;

const SPEC_11_1: &str = r#"
[profile.rust]
overrides = { review = "gpt-6-sol", code = "claude-sonnet-5-5" }
disabled = ["design"]
prompt = "Prefer small, reviewable commits."
plugins = { "team:my-skills" = true, "team:noisy-plugin" = false }
features = { work_units = false }
  [profile.rust.roles.bench]
  model = "claude-haiku-5-5"
  description = "Runs and summarizes benchmarks."
  prompt = "You run the benchmark you are given and report the numbers."
  writes = false
[user]
prompt = "Answer in British English."
default_profile = "rust"
"#;

const SPEC_15_1: &str = r#"
[[marketplace]]
name = "team"
source = "github:example/agent-plugins"
ref = "v1.4.0"
commit = "4f2c"
"#;

const SPEC_REST: &str = r#"
[[schema_rule]]
family = "openai"
code = "optional_properties_max"
severity = "refuse"
limit = 100
[[redaction.pattern]]
id = "t"
pattern = "x{3}"
action = "block"
description = "d"
[server]
url = "https://zygo.example.invalid"
token = "env:ZYGO_TEAM_TOKEN"
[serve]
tokens = [{ label = "alice-laptop", sha256 = "00" }]
"#;

#[test]
fn parses_spec_examples() {
    let c = cfg(&[SPEC_6_3, SPEC_6_4, SPEC_11_1, SPEC_15_1, SPEC_REST].concat());
    let m = c.catalog.get("claude-opus-5-5").unwrap();
    assert_eq!(m.soft_context_window, Some(400000));
    let f = m.facts.as_ref().unwrap();
    assert_eq!(f.context_window, 1000000);
    assert_eq!(f.effort_levels.len(), 5);
    assert_eq!(f.verified_on.as_deref(), Some("2026-11-18"));
    let p = &m.provider[0];
    assert_eq!(p.name, "anthropic");
    assert_eq!(p.key, Some(KeyRef::Env("ANTHROPIC_API_KEY".into())));
    assert_eq!(p.price_usd_per_mtok.as_ref().unwrap().cache_write, 8.0);
    assert!(!p.flags.zdr);
    assert!(!c.role["lead"].writes);
    let r = &c.profile["rust"];
    assert_eq!(r.overrides["review"], "gpt-6-sol");
    assert_eq!(r.disabled, ["design"]);
    assert!(!r.plugins["team:noisy-plugin"]);
    assert!(!r.roles["bench"].writes);
    assert_eq!(r.features.as_ref().unwrap()["work_units"], toml::Value::Boolean(false));
    assert_eq!(c.user.default_profile.as_deref(), Some("rust"));
    assert_eq!(c.marketplaces[0].git_ref, "v1.4.0");
    assert_eq!(c.schema_rule[0].limit, Some(100));
    assert_eq!(c.redaction.pattern[0].action, Action::Block);
    assert_eq!(c.server.token, Some(KeyRef::Env("ZYGO_TEAM_TOKEN".into())));
    assert_eq!(c.serve.tokens[0].label, "alice-laptop");
}

#[test]
fn keyref_forms() {
    #[derive(serde::Deserialize)]
    struct K {
        k: KeyRef,
    }
    assert_eq!(toml::from_str::<K>("k = \"env:X\"").unwrap().k, KeyRef::Env("X".into()));
    assert_eq!(toml::from_str::<K>("k = \"keychain:anthropic\"").unwrap().k, KeyRef::Keychain("anthropic".into()));
    assert!(toml::from_str::<K>("k = \"ANTHROPIC_API_KEY\"").is_err());
    // An empty name loads; `check` reports it (round 2).
    assert_eq!(toml::from_str::<K>("k = \"env:\"").unwrap().k, KeyRef::Env(String::new()));
}

#[test]
fn catalog_view_dialects() {
    let c = cfg(r#"
[[model]]
id = "a"
  [[model.provider]]
  name = "o"
  dialects = ["openai"]
  [[model.provider]]
  name = "n"
  dialects = ["anthropic"]
  enabled = false
[[model]]
id = "b"
  [[model.provider]]
  name = "n"
  dialects = ["openai"]
  enabled = false
[[model]]
id = "c"
dialects = ["anthropic"]
"#);
    let v = c.catalog.view();
    assert_eq!(v.models["a"].dialects, BTreeSet::from([Dialect::Openai, Dialect::Anthropic]));
    assert!(v.models["b"].dialects.is_empty());
    assert_eq!(v.models["c"].dialects, BTreeSet::from([Dialect::Anthropic]));
}

// ---- step 5: load ----

fn team_ctx(team: &str, user: &str) -> Cx {
    ctx(Some(user), None, Some(team))
}

#[test]
fn user_over_builtin_table_by_table() {
    let m = loaded("[results]\ntrim_threshold_bytes = 1000");
    assert_eq!(m.config.results.trim_threshold_bytes, 1000);
    assert_eq!(m.config.results.head_lines, 60);
    assert_eq!(m.source_of("results.trim_threshold_bytes"), Layer::User);
    assert_eq!(m.source_of("results.head_lines"), Layer::Builtin);
}

#[test]
fn missing_user_file_is_empty_layer() {
    let m = load_with_builtin(&ctx(None, None, None), toml::Table::new()).unwrap();
    assert_eq!(m.config, Config::default());
}

#[test]
fn bad_toml_is_parse_error() {
    assert!(matches!(load(&ctx(Some("[results"), None, None)), Err(ConfigError::Parse { layer: "user", .. })));
}

#[test]
fn duplicates_refused_per_layer() {
    let c = ctx(Some(&(model("a") + &model("a"))), None, None);
    let e = load(&c).unwrap_err();
    assert!(matches!(e, ConfigError::Duplicate { .. }));
    assert_eq!(e.to_string(), format!("user config {}: model[0] and model[1] have the same id", c.paths.config_file.display()));
    let rule = "[[schema_rule]]\nfamily = \"openai\"\ncode = \"one_of\"\nseverity = \"refuse\"\n";
    let c = ctx(Some(&rule.repeat(2)), None, None);
    let e = load(&c).unwrap_err();
    assert_eq!(e.to_string(), format!("user config {}: schema_rule[0] and schema_rule[1] have the same family and code", c.paths.config_file.display()));
    let b: toml::Table = toml::from_str(&(model("a") + &model("a"))).unwrap();
    let e = load_with_builtin(&ctx(None, None, None), b).unwrap_err();
    assert_eq!(e.to_string(), "builtin config (built-in): model[0] and model[1] have the same id");
}

#[test]
fn keyed_arrays_merge_by_key() {
    let team = "[[marketplace]]\nname = \"team\"\nref = \"v1\"\n[[marketplace]]\nname = \"other\"\n";
    let user = "[[marketplace]]\nname = \"team\"\nref = \"v2\"\n[[marketplace]]\nname = \"mine\"\n";
    let m = load_with_builtin(&team_ctx(team, user), toml::Table::new()).unwrap();
    let names: Vec<_> = m.config.marketplaces.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["team", "other", "mine"]);
    assert_eq!(m.config.marketplaces[0].git_ref, "v2");
    assert_eq!(m.source_of("marketplace.team"), Layer::User);
    assert_eq!(m.source_of("marketplace.other"), Layer::Team);
    let team = "[[redaction.pattern]]\nid = \"t1\"\npattern = \"a\"\ndescription = \"d\"\n";
    let user = "[[redaction.pattern]]\nid = \"t1\"\npattern = \"b\"\naction = \"block\"\ndescription = \"d\"\n";
    let m = load_with_builtin(&team_ctx(team, user), toml::Table::new()).unwrap();
    assert_eq!(m.config.redaction.pattern.len(), 1);
    assert_eq!(m.config.redaction.pattern[0].action, Action::Block);
}

#[test]
fn marketplace_keeps_team_required() {
    let team = "[[marketplace]]\nname = \"team\"\nref = \"v1\"\nrequired = [\"p\"]\napprovals = [{ plugin = \"q\", digest = \"d\" }]\n";
    let user = "[[marketplace]]\nname = \"team\"\nref = \"v2\"\n";
    let m = load_with_builtin(&team_ctx(team, user), toml::Table::new()).unwrap();
    let t = &m.config.marketplaces[0];
    assert_eq!(t.git_ref, "v2");
    assert_eq!(t.required, ["p"]);
    assert_eq!(t.approvals[0].plugin, "q");
}

#[test]
fn builtin_marketplace_source_fixed() {
    let b: toml::Table = toml::from_str(
        "[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\nref = \"v1\"\ndefault_on = [\"act-lite\"]\n",
    )
    .unwrap();
    let user = "[[marketplace]]\nname = \"act101\"\nsource = \"github:other/x\"\nref = \"v9\"\n";
    let m = load_with_builtin(&ctx(Some(user), None, None), b.clone()).unwrap();
    let a = &m.config.marketplaces[0];
    assert_eq!((a.source.as_str(), a.git_ref.as_str()), ("github:act101-ai/act101", "v1"));
    assert_eq!(m.warnings, ["config: user marketplace[0]: a user entry cannot change the built-in source; ignored"]);
    let user = "[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\nref = \"v2\"\ncommit = \"c\"\n";
    let m = load_with_builtin(&ctx(Some(user), None, None), b).unwrap();
    let a = &m.config.marketplaces[0];
    assert_eq!(a.git_ref, "v2");
    assert_eq!(a.commit.as_deref(), Some("c"));
    assert_eq!(a.default_on, ["act-lite"]);
    assert!(m.warnings.is_empty());
}

#[test]
fn user_ref_without_commit_clears_the_builtin_commit() {
    let b: toml::Table = toml::from_str(&format!(
        "[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\nref = \"v2.9.2\"\ncommit = \"{}\"\ndefault_on = [\"act-lite\"]\n",
        "a".repeat(40)
    ))
    .unwrap();
    // (a) a user ref alone: the built-in commit does not stay.
    let user = "[[marketplace]]\nname = \"act101\"\nref = \"v3.0.0\"\n";
    let m = load_with_builtin(&ctx(Some(user), None, None), b.clone()).unwrap();
    let a = &m.config.marketplaces[0];
    assert_eq!(a.git_ref, "v3.0.0");
    assert_eq!(a.commit, None);
    assert_eq!(a.source, "github:act101-ai/act101");
    assert_eq!(a.default_on, ["act-lite"]);
    assert_eq!(m.source_of("marketplace.act101"), Layer::Builtin);
    assert!(m.warnings.is_empty());
    // (b) a user ref and commit: both kept.
    let user = format!("[[marketplace]]\nname = \"act101\"\nref = \"v3.0.0\"\ncommit = \"{}\"\n", "b".repeat(40));
    let m = load_with_builtin(&ctx(Some(&user), None, None), b.clone()).unwrap();
    let a = &m.config.marketplaces[0];
    assert_eq!(a.git_ref, "v3.0.0");
    assert_eq!(a.commit.as_deref(), Some(&*"b".repeat(40)));
    // (c) a user commit alone keeps the built-in ref.
    let user = format!("[[marketplace]]\nname = \"act101\"\ncommit = \"{}\"\n", "c".repeat(40));
    let m = load_with_builtin(&ctx(Some(&user), None, None), b.clone()).unwrap();
    let a = &m.config.marketplaces[0];
    assert_eq!(a.git_ref, "v2.9.2");
    assert_eq!(a.commit.as_deref(), Some(&*"c".repeat(40)));
    // (d) no user file.
    let m = load_with_builtin(&ctx(None, None, None), b).unwrap();
    let a = &m.config.marketplaces[0];
    assert_eq!(a.git_ref, "v2.9.2");
    assert_eq!(a.commit.as_deref(), Some(&*"a".repeat(40)));
}

#[test]
fn profile_shadows_team_preset() {
    let m = load(&team_ctx("[profile.rust]\ndisabled = [\"design\"]\nprompt = \"t\"", "[profile.rust]\nprompt = \"u\"")).unwrap();
    let p = &m.config.profile["rust"];
    assert_eq!(p.prompt.as_deref(), Some("u"));
    assert!(p.disabled.is_empty());
    assert_eq!(m.source_of("profile.rust"), Layer::User);
}

#[test]
fn serve_mode_catalog_is_team_only() {
    let b: toml::Table = toml::from_str(&model("z")).unwrap();
    let c = team_ctx("[[model]]\nid = \"a\"\ndialects = [\"anthropic\"]", &model("b"));
    let m = load_with_builtin(&c, b).unwrap();
    let ids: Vec<_> = m.config.catalog.0.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["a"]);
    assert!(m.config.catalog.0[0].from_team);
    assert_eq!(m.warnings, ["config: [[model]] in user config ignored; the server's catalog is authoritative"]);
}

#[test]
fn turn_cap_lower_non_zero() {
    for (t, u, v, l) in [(0, 5, 5, Layer::User), (10, 5, 5, Layer::User), (10, 0, 10, Layer::Team), (10, 20, 10, Layer::Team)] {
        let m = load(&team_ctx(&format!("[limits]\nsubagent_max_turns = {t}"), &format!("[limits]\nsubagent_max_turns = {u}")))
            .unwrap();
        assert_eq!(m.config.limits.subagent_max_turns, v, "({t},{u})");
        assert_eq!(m.source_of("limits.subagent_max_turns"), l, "({t},{u})");
    }
}

#[test]
fn dotted_id_source_key() {
    let m = loaded(&model("gpt-4.1"));
    assert_eq!(m.source_of("model.\"gpt-4.1\""), Layer::User);
    assert_eq!(m.source_of("model.\"gpt-4.1\".provider"), Layer::User);
    assert_eq!(m.source_of("model.gpt-4.1"), Layer::Builtin);
}

#[test]
fn team_keys_recorded() {
    let m = load(&team_ctx("[guards]\nmin_level = \"notify\"", "")).unwrap();
    assert_eq!(m.team_keys, BTreeSet::from(["guards.min_level".to_string()]));
}

// ---- step 9: repository file ----

fn repo_with(name: &str, text: &str) -> Tmp {
    let d = tmp(name);
    git(&d, &["init", "-b", "main"]);
    std::fs::write(d.join(".zygo.toml"), text).unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "x"]);
    d
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_file_from_default_branch() {
    let d = repo_with("rf1", "version = 1\n[guards]\nclaims = \"enforce\"\n[proxy]\nrequire_zdr = true\n");
    std::fs::write(d.join(".zygo.toml"), "version = 1\n[guards]\nclaims = \"monitor\"\n").unwrap();
    match repo_file::read_default_branch(&d) {
        RepoFile::Found { branch, table, ignored } => {
            assert_eq!(branch, "main");
            assert_eq!(table["guards"]["claims"].as_str(), Some("enforce"));
            assert_eq!(ignored, ["proxy"]);
        }
        other => panic!("{other:?}"),
    }
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.config.guards.claims, Level::Enforce);
    assert_eq!(m.source_of("guards.claims"), Layer::Repository);
    assert!(!m.config.proxy.require_zdr);
    assert_eq!(
        m.warnings,
        [".zygo.toml: [proxy] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]
    );
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_file_origin_head() {
    let root = tmp("rf2");
    let up = root.join("up");
    std::fs::create_dir_all(&up).unwrap();
    git(&up, &["init", "-b", "trunk"]);
    std::fs::write(up.join(".zygo.toml"), "[navigation]\nlines = 50\n").unwrap();
    git(&up, &["add", "."]);
    git(&up, &["commit", "-m", "x"]);
    git(&root, &["clone", "up", "down"]);
    let down = root.join("down");
    git(&down, &["checkout", "-b", "feature"]);
    std::fs::write(down.join(".zygo.toml"), "[navigation]\nlines = 10\n").unwrap();
    git(&down, &["commit", "-am", "y"]);
    match repo_file::read_default_branch(&down) {
        RepoFile::Found { branch, table, .. } => {
            assert_eq!(branch, "origin/trunk");
            assert_eq!(table["navigation"]["lines"].as_integer(), Some(50));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_file_absent_and_invalid() {
    assert_eq!(repo_file::read_default_branch(&tmp("rf3")), RepoFile::Absent);
    let d = repo_with("rf4", "version = 2\n");
    match repo_file::read_default_branch(&d) {
        RepoFile::Invalid { why, .. } => assert!(why.contains("version"), "{why}"),
        other => panic!("{other:?}"),
    }
    let d = repo_with("rf5", "[guards]\nclaims = \"loud\"\n");
    assert!(matches!(repo_file::read_default_branch(&d), RepoFile::Invalid { .. }));
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.config.guards.claims, Level::Monitor);
    assert_eq!(m.warnings.len(), 1);
    assert!(m.warnings[0].starts_with(".zygo.toml: invalid ("), "{}", m.warnings[0]);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn min_level_floor() {
    let d = repo_with("rf6", "[guards]\nclaims = \"monitor\"\nmin_level = \"monitor\"\n[features]\nresults = false\n");
    let m = load(&ctx(None, Some(d.to_path_buf()), Some("[guards]\nmin_level = \"notify\""))).unwrap();
    assert_eq!(m.config.guards.min_level, Level::Notify);
    assert_eq!(m.config.guards.claims, Level::Notify);
    assert_eq!(m.source_of("guards.claims"), Layer::Team);
    assert!(m.config.features.results);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_turn_cap() {
    let d = repo_with("rf7", "[limits]\nsubagent_max_turns = 12\n");
    let m = load(&ctx(Some("[limits]\nsubagent_max_turns = 30"), Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.config.limits.subagent_max_turns, 12);
    assert_eq!(m.source_of("limits.subagent_max_turns"), Layer::Repository);
}

// ---- step 13: config check ----

fn one(text: &str) -> Violation {
    let v = check(&loaded(text).config);
    assert_eq!(v.len(), 1, "{v:?}");
    v.into_iter().next().unwrap()
}

fn err(text: &str, msg: &str) {
    let v = one(text);
    assert_eq!((v.severity, v.to_string()), (Severity::Error, msg.to_string()));
}

fn none(text: &str) {
    assert_eq!(check(&loaded(text).config), vec![]);
}

/// `base()` with `from` replaced by `to` (once).
fn with(from: &str, to: &str) -> String {
    let b = base();
    assert!(b.contains(from), "{from}");
    b.replacen(from, to, 1)
}

#[test]
fn check_base_is_clean() {
    none(&base());
}

#[test]
fn check_needs_provider() {
    err("[[model]]\nid = \"b\"", "model[0]: has no provider");
    err("[[model]]\nid = \"c\"\ndialects = [\"anthropic\"]", "model[0]: has no provider");
    let team = "[[model]]\nid = \"c\"\ndialects = [\"anthropic\"]\n[role.lead]\nmodel = \"c\"\ndescription = \"d\"\nprompt = \"p\"";
    assert_eq!(check(&load_with_builtin(&ctx(None, None, Some(team)), toml::Table::new()).unwrap().config), vec![]);
}

#[test]
fn check_empty_fields() {
    err(&(base() + &model("")), "model[1]: id must not be empty");
    err(&with("name = \"p\"", "name = \"\""), "model[0].provider[0]: name must not be empty");
    err(&with("base_url = \"https://p.example.invalid\"", "base_url = \"\""), "model[0].provider[0].base_url: must not be empty");
    err(&with("upstream_model = \"u\"", "upstream_model = \"\""), "model[0].provider[0].upstream_model: must not be empty");
}

#[test]
fn check_soft_window() {
    let soft = |n: u64| with("id = \"a\"\n", &format!("id = \"a\"\nsoft_context_window = {n}\n"));
    err(&soft(0), "model[0].soft_context_window: must be > 0");
    err(&soft(300000), "model[0].soft_context_window: exceeds facts.context_window");
    let no_facts = "[[model]]\nid = \"a\"\nsoft_context_window = 1000\n[[model.provider]]\nname = \"p\"\nbase_url = \"https://p.example.invalid\"\nupstream_model = \"u\"\n";
    err(no_facts, "model[0].soft_context_window: needs [model.facts]");
    none(&soft(200000));
}

#[test]
fn check_effort_levels() {
    let lv = "effort_levels = [\"low\",\"high\"]";
    err(&with(lv, "effort_levels = []"), "model[0].facts.effort_levels: empty only with reasoning = \"none\"");
    none(&with(lv, "effort_levels = []").replacen("reasoning = \"always\"", "reasoning = \"none\"", 1));
    let order = "model[0].facts.effort_levels: must follow the ladder order without repeats";
    err(&with(lv, "effort_levels = [\"high\",\"low\"]"), order);
    err(&with(lv, "effort_levels = [\"low\",\"low\"]"), order);
    let bad = with(lv, "effort_levels = [\"lowest\"]");
    assert!(matches!(load(&ctx(Some(&bad), None, None)), Err(ConfigError::Parse { .. })));
}

#[test]
fn check_reasoning_default_on() {
    let on = with("reasoning = \"always\"", "reasoning = \"always\"\nreasoning_default_on = true");
    err(&on, "model[0].facts.reasoning_default_on: only with reasoning = \"switchable\"");
    none(&on.replacen("reasoning = \"always\"", "reasoning = \"switchable\"", 1));
}

#[test]
fn check_role_model_exists() {
    err(&with("model = \"a\"", "model = \"nope\""), "role.lead.model: unknown model");
}

#[test]
fn check_schema_rules() {
    let rule = |body: &str| base() + "[[schema_rule]]\nfamily = \"openai\"\nseverity = \"refuse\"\n" + body;
    err(&rule("code = \"\""), "schema_rule[0].code: must be 1-64 of [a-z0-9_]");
    err(&rule("code = \"one_of\"\nlimit = 0"), "schema_rule[0].limit: must be > 0");
    err(&rule(&format!("code = \"{}\"", secret())), "schema_rule[0].code: must be 1-64 of [a-z0-9_]");
    let gemini = base() + "[[schema_rule]]\nfamily = \"gemini\"\ncode = \"x\"\nseverity = \"refuse\"\n";
    assert!(matches!(load(&ctx(Some(&gemini), None, None)), Err(ConfigError::Parse { .. })));
}

#[test]
fn check_unknown_flag_is_warning() {
    let v = one(&(base() + "[model.provider.flags]\nzdr = true\nfast = true\n"));
    assert_eq!((v.severity, v.to_string()), (Severity::Warning, "model[0].provider[0].flags.fast: unknown flag".into()));
}

#[test]
fn check_patterns_compile() {
    // `load` refuses a bad pattern (round 2), so `check` is run on the parsed text.
    let v = check(&cfg(&(base() + "[[redaction.pattern]]\nid = \"t\"\npattern = \"(\"\ndescription = \"d\"\n")));
    assert_eq!(v.len(), 1, "{v:?}");
    let v = v.into_iter().next().unwrap();
    assert_eq!(v.severity, Severity::Error);
    assert!(v.to_string().starts_with("redaction.pattern[0].pattern: does not compile:"), "{v}");
}

#[test]
fn check_content_lists() {
    let content = |body: &str| base() + "[redaction.content]\n" + body;
    err(&content("replacement = \"\""), "redaction.content.replacement: must be 1-64 bytes");
    err(&content(&format!("replacement = \"{}\"", "x".repeat(65))), "redaction.content.replacement: must be 1-64 bytes");
    err(&content("allow = [\"ab\"]"), "redaction.content.allow[0]: must be 3-40 lower-case letters");
    err(&content("deny = [\"Abc\"]"), "redaction.content.deny[0]: must be 3-40 lower-case letters");
    err(&content("allow = [\"zzq\"]\ndeny = [\"zzq\"]"), "redaction.content.allow[0]: is also in redaction.content.deny");
}

// ---- step 17: layers 5–7 ----

fn guards(pairs: &[(&str, &str)]) -> Overrides {
    Overrides { guards: pairs.iter().map(|(g, l)| (g.to_string(), l.to_string())).collect(), ..Overrides::default() }
}

#[test]
fn apply_profile_features() {
    let m = loaded("[profile.p]\nfeatures = { work_units = false }\n[profile.s]\nfeatures = { redaction = { secrets = false } }");
    let p = m.apply_profile("p").unwrap();
    assert!(!p.config.features.work_units);
    assert_eq!(p.source_of("features.work_units"), Layer::Profile);
    assert!(m.config.features.work_units);
    assert_eq!(m.source_of("features.work_units"), Layer::Builtin);
    match m.apply_profile("q") {
        Err(ConfigError::Profile { name, message }) => assert_eq!((name.as_str(), message.as_str()), ("q", "not found")),
        other => panic!("{other:?}"),
    }
    match m.apply_profile("s") {
        Err(ConfigError::Profile { message, .. }) => {
            assert_eq!(message, "features: redaction.secrets cannot be set by a profile")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn apply_flag_sets_source() {
    let m = loaded("");
    let f = m.apply_flag("results.trim_threshold_bytes", 4096.into());
    assert_eq!(f.config.results.trim_threshold_bytes, 4096);
    assert_eq!(f.source_of("results.trim_threshold_bytes"), Layer::Flags);
    let f = m.apply_flag("results.trim_threshold_bytes", "x".into());
    assert_eq!(f.config, m.config);
    assert_eq!(f.source_of("results.trim_threshold_bytes"), Layer::Builtin);
    assert_eq!(f.warnings.len(), 1);
    assert!(f.warnings[0].starts_with("flag results.trim_threshold_bytes:"), "{}", f.warnings[0]);
}

#[test]
fn overrides_apply() {
    let m = loaded("");
    let o = Overrides {
        results: Some(false),
        results_threshold: Some(4096),
        redaction_content: Some(false),
        work_units: Some(false),
        guards: BTreeMap::from([("scope".to_string(), "enforce".to_string())]),
        ..Default::default()
    };
    let s = m.with_overrides(&o).unwrap();
    assert!(!s.config.features.results);
    assert_eq!(s.config.results.trim_threshold_bytes, 4096);
    assert!(!s.config.features.redaction.content);
    assert!(!s.config.features.work_units);
    assert_eq!(s.config.guards.scope, Level::Enforce);
    assert_eq!(s.source_of("guards.scope"), Layer::Session);
    assert_eq!(m.config, Config::default());
}

#[test]
fn override_team_min_level_only() {
    let m = load(&ctx(None, None, Some("[guards]\nmin_level = \"notify\""))).unwrap();
    assert_eq!(m.with_overrides(&guards(&[("claims", "enforce")])).unwrap().config.guards.claims, Level::Enforce);
    let e = m.with_overrides(&guards(&[("claims", "monitor")])).unwrap_err();
    assert_eq!(e, Refusal::BelowFloor { guard: "claims".into(), min: Level::Notify });
    assert_eq!(e.to_string(), "refused: guard claims cannot go below min_level notify");
}

#[test]
fn override_team_set_key_refused() {
    let m = load(&ctx(Some("[features]\nwork_units = false"), None, Some("[features]\nwork_units = true"))).unwrap();
    let e = m.with_overrides(&Overrides { work_units: Some(true), ..Overrides::default() }).unwrap_err();
    assert_eq!(e, Refusal::TeamSet { key: "features.work_units".into() });
    assert_eq!(e.to_string(), "refused: `features.work_units` is set by the team config");
}

#[test]
fn override_unknown_refused() {
    let m = loaded("");
    assert!(matches!(m.with_overrides(&guards(&[("lead", "monitor")])), Err(Refusal::Unknown { what: "guard", .. })));
    assert!(matches!(m.with_overrides(&guards(&[("scope", "loud")])), Err(Refusal::Unknown { what: "level", .. })));
}

// ---- step 21: show ----

#[test]
fn show_user_layer_exact() {
    let m = loaded("[results]\ntrim_threshold_bytes = 1000\n[work]\nsources = [\"branch\"]");
    assert_eq!(
        m.show(Some(Layer::User), &|_| true),
        "results.trim_threshold_bytes = 1000  # user\nwork.sources = [\"branch\"]  # user\n"
    );
}

#[test]
fn show_all_and_keys() {
    let m = loaded(&model("gpt-4.1"));
    let all = m.show(None, &|_| true);
    assert!(all.contains("results.head_lines = 60  # builtin\n"), "{all}");
    assert!(all.contains("model.\"gpt-4.1\".provider.p.key = \"env:ZYGO_N3_TEST_KEY\" <set>  # user\n"), "{all}");
    let unset = m.show(None, &|_| false);
    assert!(unset.contains("model.\"gpt-4.1\".provider.p.key = \"env:ZYGO_N3_TEST_KEY\" <unset>  # user\n"), "{unset}");
}

#[test]
fn show_after_profile_and_override() {
    let p = loaded("[profile.p]\nfeatures = { work_units = false }").apply_profile("p").unwrap();
    assert!(p.show(None, &|_| true).contains("features.work_units = false  # profile\n"));
    let s = p.with_overrides(&Overrides { results: Some(false), ..Overrides::default() }).unwrap();
    assert!(s.show(None, &|_| true).contains("features.results = false  # session\n"));
}

#[test]
fn show_escapes_keys_and_values() {
    let m = loaded("[profile.\"x\\u001b[2K\\nforged\"]\nprompt = \"A\\nresults.head_lines = 1  # team\"\n");
    let all = m.show(None, &|_| true);
    assert!(all.contains("profile.\"x\\u{1b}[2K\\nforged\".prompt = "), "{all}");
    assert!(all.contains("\\nresults.head_lines = 1  # team"), "{all}");
    assert!(!all.contains('\u{1b}'), "{all}");
    assert!(!all.lines().any(|l| l.starts_with("forged") || l.starts_with("results.head_lines = 1")), "{all}");
}

// ---- round 2 ----

fn round2_builtin() -> toml::Table {
    toml::from_str(
        "[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\nref = \"v1\"\n[[redaction.pattern]]\nid = \"aws\"\npattern = \"AKIA[0-9A-Z]{16}\"\ndescription = \"d\"\n",
    )
    .unwrap()
}

#[test]
fn empty_keyed_array_adds_nothing() {
    let team = "[[marketplace]]\nname = \"team\"\nsource = \"github:t/t\"\nref = \"v1\"\nrequired = [\"p\"]\n[[redaction.pattern]]\nid = \"t1\"\npattern = \"a\"\ndescription = \"d\"\n";
    let user = "marketplace = []\n[redaction]\npattern = []\n";
    let m = load_with_builtin(&team_ctx(team, user), round2_builtin()).unwrap();
    let names: Vec<_> = m.config.marketplaces.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["act101", "team"]);
    assert_eq!(m.config.marketplaces[1].required, ["p"]);
    let ids: Vec<_> = m.config.redaction.pattern.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["aws", "t1"]);
}

fn dup_err(c: &LoadContext) -> String {
    match load(c) {
        Err(e @ ConfigError::Duplicate { .. }) => e.to_string(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn duplicates_refused_every_keyed_array() {
    let provider = "[[model.provider]]\nname = \"p\"\nbase_url = \"https://p.example.invalid\"\nupstream_model = \"u\"\n";
    let cases = [
        (model("a") + provider, "model[0].provider[0] and model[0].provider[1] have the same name"),
        ("[[redaction.pattern]]\nid = \"t\"\npattern = \"a\"\n".repeat(2), "redaction.pattern[0] and redaction.pattern[1] have the same id"),
        ("[[marketplace]]\nname = \"m\"\n".repeat(2), "marketplace[0] and marketplace[1] have the same name"),
        ("[[serve.tokens]]\nlabel = \"alice\"\nsha256 = \"00\"\n".repeat(2), "serve.tokens[0] and serve.tokens[1] have the same label"),
    ];
    for (text, what) in &cases {
        let c = ctx(Some(text), None, None);
        assert_eq!(dup_err(&c), format!("user config {}: {what}", c.paths.config_file.display()));
    }
    let c = ctx(None, None, Some(&"[[marketplace]]\nname = \"m\"\n".repeat(2)));
    assert_eq!(dup_err(&c), "team config (team): marketplace[0] and marketplace[1] have the same name");
}

#[test]
fn load_refuses_bad_pattern() {
    let c = ctx(Some("[[redaction.pattern]]\nid = \"t\"\npattern = \"(\"\ndescription = \"d\"\n"), None, None);
    let e = load(&c).unwrap_err().to_string();
    let head = format!("user config {}: redaction.pattern[0].pattern: does not compile:", c.paths.config_file.display());
    assert!(e.starts_with(&head), "{e}");
    assert!(!e.contains('\n'), "{e}");
}

/// A fake `git` that exits at once but leaves a child holding stdout open for 6 s.
#[cfg(unix)]
#[test]
#[ignore = "timing: fake git sleeps 6 s against the 2 s bound; asserts elapsed < 3 s"]
fn git_bound_is_total() {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("ZYGO_N3_FAKE_GIT").is_some() {
        let t = std::time::Instant::now();
        let r = repo_file::read_default_branch(&std::env::temp_dir());
        let took = t.elapsed();
        assert_eq!(r, RepoFile::Unread { why: "git symbolic-ref: timed out".into() });
        assert!(took < std::time::Duration::from_secs(3), "took {took:?}");
        return;
    }
    // PATH is changed only in a child test process: other tests run git in parallel.
    let d = tmp("fakegit");
    let g = d.join("git");
    std::fs::write(&g, "#!/bin/sh\nsleep 6 &\nexit 0\n").unwrap();
    std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", d.display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["git_bound_is_total", "--exact", "--test-threads=1"])
        .env("ZYGO_N3_FAKE_GIT", "1")
        .env("PATH", path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && stdout.contains("1 passed"), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
}

/// N53: a non-zero git exit is an absence only for these messages (git 2.50.1, `LC_ALL=C`); any other is a failed read.
#[test]
fn git_absent_matches_only_absent_messages() {
    for e in [
        "fatal: not a git repository (or any of the parent directories): .git",
        "fatal: not a git repository (or any parent up to mount point /Volumes/x)",
        "fatal: ref refs/remotes/origin/HEAD is not a symbolic ref",
        "fatal: invalid object name 'main'.",
        "fatal: path '.zygo.toml' does not exist in 'main'",
        "fatal: path '.zygo.toml' exists on disk, but not in 'main'",
        "fatal: invalid object name 'main'.\n",
        // Round 3: git stops at a filesystem boundary with exactly this pair.
        "fatal: not a git repository (or any parent up to mount point /Volumes)\nStopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).\n",
    ] {
        assert!(repo_file::git_absent(e), "{e}");
    }
    for e in [
        "fatal: detected dubious ownership in repository at '/r'",
        "fatal: not a git repository: /nowhere/x",
        "fatal: bad object main:.zygo.toml",
        "error: inflate: data stream error (incorrect header check)\nfatal: loose object 0123 (stored in .git/objects/01/23) is corrupt",
        "fatal: cannot change to '/missing': No such file or directory",
        "",
        // Round 2: only exactly one absent message line is an absence; any other line makes it a failed read.
        "warning: ignoring broken ref refs/heads/main\nfatal: invalid object name 'main'.\n",
        "fatal: invalid object name 'main'.\nfatal: invalid object name 'main'.\n",
        "fatal: not a git repository (or any parent up to mount point /Volumes)\nStopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).\nwarning: x\n",
        "fatal: invalid object name 'main'.\nStopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).\n",
    ] {
        assert!(!repo_file::git_absent(e), "{e}");
    }
}

#[test]
fn apply_flag_keeps_floor_and_cap() {
    let team = "[guards]\nmin_level = \"notify\"\n[limits]\nsubagent_max_turns = 10\n";
    let m = load(&ctx(None, None, Some(team))).unwrap();
    let f = m.apply_flag("guards.scope", "monitor".into());
    assert_eq!(f.config.guards.scope, Level::Notify);
    let f = m.apply_flag("limits.subagent_max_turns", 50.into());
    assert_eq!(f.config.limits.subagent_max_turns, 10);
    let f = m.apply_flag("limits.subagent_max_turns", 5.into());
    assert_eq!(f.config.limits.subagent_max_turns, 5);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_bad_min_level_dropped() {
    let d = repo_with("rf8", "[guards]\nmin_level = \"loud\"\nclaims = \"enforce\"\n");
    assert!(matches!(repo_file::read_default_branch(&d), RepoFile::Found { .. }));
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.config.guards.claims, Level::Enforce);
    assert_eq!(m.warnings, [".zygo.toml: guards.min_level ignored; only user or team config may set it"]);
}

#[test]
fn provider_key_optional() {
    let keyless = base().replace("key = \"env:ZYGO_N3_TEST_KEY\"\n", "");
    let m = loaded(&keyless);
    assert_eq!(m.config.catalog.0[0].provider[0].key, None);
    assert!(check(&m.config).is_empty());
    assert!(!m.show(None, &|_| true).contains(".key"));
    err(&with("env:ZYGO_N3_TEST_KEY", "env:"), "model[0].provider[0].key: name must not be empty");
    err(&with("env:ZYGO_N3_TEST_KEY", "keychain:"), "model[0].provider[0].key: name must not be empty");
    err(&(base() + "[server]\ntoken = \"env:\"\n"), "server.token: name must not be empty");
}

#[test]
fn show_kept_team_fields_are_team() {
    let team = "[[marketplace]]\nname = \"team\"\nref = \"v1\"\nrequired = [\"p\"]\napprovals = [{ plugin = \"q\", digest = \"d\" }]\n";
    let m = load(&team_ctx(team, "[[marketplace]]\nname = \"team\"\nref = \"v2\"\n")).unwrap();
    let all = m.show(None, &|_| true);
    assert!(all.contains("marketplace.team.ref = \"v2\"  # user\n"), "{all}");
    assert!(all.contains("marketplace.team.required = [\"p\"]  # team\n"), "{all}");
    let approvals = all.lines().find(|l| l.starts_with("marketplace.team.approvals = ")).unwrap();
    assert!(approvals.ends_with("  # team"), "{approvals}");
}

#[test]
fn errors_are_one_line() {
    let e = load(&ctx(Some("[results"), None, None)).unwrap_err().to_string();
    assert!(!e.contains('\n'), "{e}");
    let e = load(&ctx(Some("[results]\nhead_lines = \"x\"\n"), None, None)).unwrap_err().to_string();
    assert!(!e.contains('\n'), "{e}");
    let e = loaded("[profile.p]\nfeatures = { results = 3 }").apply_profile("p").unwrap_err().to_string();
    assert!(!e.contains('\n'), "{e}");
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_foreign_tables_one_line() {
    let d = repo_with("rf9", "[guards]\nclaims = \"enforce\"\n[proxy]\nrequire_zdr = true\n[server]\nurl = \"x\"\n");
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(
        m.warnings,
        [".zygo.toml: [proxy], [server] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]
    );
}

#[test]
fn load_and_profile_errors_escape_names() {
    let e = load(&ctx(Some("[profile.\"x\\u001b[2K\"]\nprompt = 1\n"), None, None)).unwrap_err().to_string();
    assert!(e.ends_with("profile.\"x\\u{1b}[2K\".prompt at line 2: expected a string, found an integer"), "{e}");
    assert_eq!(loaded("").apply_profile("z\u{1b}[2K").unwrap_err().to_string(), "profile z\\u{1b}[2K: not found");
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_ignored_table_names_are_escaped() {
    let d = repo_with("rf10", "[guards]\nclaims = \"enforce\"\n[\"x\\u001b[2K\\nforged\"]\na = 1\n");
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.warnings, [".zygo.toml: [x\\u{1b}[2K\\nforged] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]);
}

#[test]
fn real_builtin_layer_resolves_role_and_model() {
    let m = load(&ctx(None, None, None)).unwrap();
    assert_eq!(m.config.role["lead"].model, "claude-opus-5-5");
    let row = m.config.catalog.get("claude-opus-5-5").expect("built-in model row");
    assert!(!row.provider.is_empty());
}

// ---- N35 round 3: a TOML syntax error names the place, never the source text (C3) ----

/// Two files whose line 2 holds a credential-shaped value built at run time: unquoted, and with no closing quote.
fn bad_key_files() -> (String, [String; 2]) {
    let value = format!("sk-ant-{}", ulid::Ulid::generate());
    (value.clone(), [format!("[user]\nkey = {value}\n"), format!("[user]\nkey = \"{value}\n")])
}

#[test]
fn toml_syntax_error_does_not_echo_the_source() {
    let (value, files) = bad_key_files();
    for text in files {
        let Err(e) = load(&ctx(Some(&text), None, None)) else { panic!("{text} loaded") };
        let e = e.to_string();
        assert!(e.contains("TOML parse error at line 2, column "), "{e}");
        assert!(!e.contains(&value) && !e.contains("sk-ant"), "{e}");
    }
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_file_syntax_error_does_not_echo_the_source() {
    let (value, files) = bad_key_files();
    for (i, text) in files.iter().enumerate() {
        let d = repo_with(&format!("rf-r3-{i}"), &format!("version = 1\n{}", text.trim_start_matches("[user]\n")));
        match repo_file::read_default_branch(&d) {
            RepoFile::Invalid { why, .. } => {
                assert!(why.contains("TOML parse error at line 2, column "), "{why}");
                assert!(!why.contains(&value) && !why.contains("sk-ant"), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }
}

// ---- N54: a type or pattern error names the key, never the value (C3) ----

fn secret() -> String {
    format!("sk-ant-{}", ulid::Ulid::generate())
}

/// The `load` error of a user file holding `text`, checked free of `value`, and the file's path.
fn user_err(text: &str, value: &str) -> (String, String) {
    let c = ctx(Some(text), None, None);
    let e = load_with_builtin(&c, toml::Table::new()).unwrap_err().to_string();
    assert!(!e.contains(value) && !e.contains("sk-ant"), "{e}");
    (e, c.paths.config_file.display().to_string())
}

#[test]
fn type_error_names_the_key_not_the_value() {
    let v = secret();
    let cases = [
        (format!("[[model]]\nid = \"a\"\n[model.facts]\ncontext_window = \"{v}\"\n"), "model[0].facts.context_window at line 4: expected u64, found a string"),
        (format!("[proxy]\nrequire_zdr = \"{v}\"\n"), "proxy.require_zdr at line 2: expected a boolean, found a string"),
        (format!("[guards]\nscope = \"{v}\"\n"), "guards.scope at line 2: expected one of `monitor`, `notify`, `enforce`, found another string"),
        (format!("[[model]]\nid = \"a\"\ndialects = [\"{v}\"]\n"), "model[0].dialects[0] at line 3: expected one of `anthropic`, `openai`, `openai-chat`, found another string"),
        (format!("proxy = \"{v}\"\n"), "proxy at line 1: expected struct ProxyConfig, found a string"),
        (format!("[server]\ntoken = \"{v}\"\n"), "server.token at line 2: expected env:<NAME> or keychain:<name>, found a string that is not valid"),
    ];
    for (text, want) in cases {
        let (e, path) = user_err(&text, &v);
        assert_eq!(e, format!("user config {path}: {want}"));
    }
}

#[test]
fn type_error_out_of_range_integer() {
    let n = i64::from(u32::MAX) + 1 + (ulid::Ulid::generate().random() % 1000) as i64;
    let (e, path) = user_err(&format!("[navigation]\nlines = {n}\n"), &n.to_string());
    assert_eq!(e, format!("user config {path}: navigation.lines at line 2: expected u32, found an integer that is not valid"));
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn type_error_in_team_profile_flag_and_repo_file() {
    let v = secret();
    let team = ctx(None, None, Some(&format!("[proxy]\nrequire_zdr = \"{v}\"\n")));
    let e = load_with_builtin(&team, toml::Table::new()).unwrap_err().to_string();
    assert_eq!(e, "team config (team): proxy.require_zdr: expected a boolean, found a string");
    let e = loaded(&format!("[profile.p]\nfeatures = {{ results = \"{v}\" }}")).apply_profile("p").unwrap_err().to_string();
    assert_eq!(e, "profile p: features.results: expected a boolean, found a string");
    let f = loaded("").apply_flag("results.trim_threshold_bytes", v.clone().into());
    assert_eq!(f.warnings, ["flag results.trim_threshold_bytes: results.trim_threshold_bytes: expected u64, found a string; ignored"]);
    let files = [
        (format!("version = \"{v}\"\n"), "version is not 1"),
        (format!("version = 1\n[navigation]\nlines = \"{v}\"\n"), "navigation.lines at line 3: expected u32, found a string"),
    ];
    for (i, (text, want)) in files.into_iter().enumerate() {
        match repo_file::read_default_branch(&repo_with(&format!("n54-{i}"), &text)) {
            RepoFile::Invalid { why, .. } => assert_eq!(why, want),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn bad_pattern_names_the_key_not_the_pattern() {
    let v = secret();
    let text = format!("[[redaction.pattern]]\nid = \"t\"\npattern = \"({v}\"\ndescription = \"d\"\n");
    let want = "redaction.pattern[0].pattern: does not compile: unclosed group at pattern line 1, column 1";
    let (e, path) = user_err(&text, &v);
    assert_eq!(e, format!("user config {path}: {want}"));
    let found: Vec<String> = check(&cfg(&text)).iter().map(ToString::to_string).collect();
    assert_eq!(found, [want]);
}
#[test]
fn keyed_ids_are_not_echoed() {
    let v = secret();
    let row = model(&v);
    // duplicate: model[0] and model[1] share the id `v`
    let c = ctx(Some(&(row.clone() + &row)), None, None);
    let e = dup_err(&c);
    assert_eq!(e, format!("user config {}: model[0] and model[1] have the same id", c.paths.config_file.display()));
    // config check: the provider path names the model by index
    let text = row.replace("base_url = \"https://p.example.invalid\"", "base_url = \"\"") + "[role.lead]\nmodel = \"x\"\ndescription = \"d\"\nprompt = \"p\"\n";
    let found: Vec<String> = check(&loaded(&text).config).iter().map(ToString::to_string).collect();
    assert_eq!(found, ["model[0].provider[0].base_url: must not be empty", "role.lead.model: unknown model"]);
    // a pattern id: load and check name the pattern by index
    let pat = format!("[[redaction.pattern]]\nid = \"{v}\"\npattern = \"(\"\ndescription = \"d\"\n");
    let (e, path) = user_err(&pat, &v);
    assert_eq!(e, format!("user config {path}: redaction.pattern[0].pattern: does not compile: unclosed group at pattern line 1, column 1"));
    let found: Vec<String> = check(&cfg(&pat)).iter().map(ToString::to_string).collect();
    assert_eq!(found, ["redaction.pattern[0].pattern: does not compile: unclosed group at pattern line 1, column 1"]);
    // a user entry for the built-in marketplace names it by index
    let b: toml::Table = toml::from_str(&format!("[[marketplace]]\nname = \"{v}\"\nsource = \"github:a/b\"\nref = \"v1\"\n")).unwrap();
    let m = load_with_builtin(&ctx(Some(&format!("[[marketplace]]\nname = \"{v}\"\nsource = \"github:c/d\"\n")), None, None), b).unwrap();
    assert_eq!(m.warnings, ["config: user marketplace[0]: a user entry cannot change the built-in source; ignored"]);
}

#[test]
fn enum_as_one_key_table_loads_like_toml() {
    for v in ["{ notify = {} }", "{ enforce = [] }"] {
        let m = loaded(&format!("[guards]\nscope = {v}\n"));
        assert_ne!(m.config.guards.scope, schema::Level::Monitor, "{v}");
    }
    let cases = [
        ("{ notify = 1 }", "guards.scope.notify at line 2: expected an empty table or array, found an integer"),
        ("{ notify = {}, enforce = {} }", "guards.scope at line 2: expected a table with one key, found 2 elements"),
    ];
    for (v, want) in cases {
        let (e, path) = user_err(&format!("[guards]\nscope = {v}\n"), "sk-ant");
        assert_eq!(e, format!("user config {path}: {want}"));
    }
    // A variant table that holds a secret: the message names the path, never the value.
    let s = secret();
    let (e, _) = user_err(&format!("[guards]\nscope = {{ notify = {{ k = \"{s}\" }} }}\n"), &s);
    assert!(e.contains("guards.scope.notify at line 2: "), "{e}");
}

#[test]
fn type_error_noun_is_the_node_kind() {
    let n = 1_000_000_000_000 + (ulid::Ulid::generate().random() % 1_000_000) as i64;
    let (e, path) = user_err(&format!("[[model]]\nid = \"a\"\n[model.facts]\nverified_on = {n}\n"), &n.to_string());
    assert_eq!(e, format!("user config {path}: model[0].facts.verified_on at line 4: expected a date or a string, found an integer"));
    let (e, path) = user_err(&format!("[server]\ntoken = {n}.5\n"), &n.to_string());
    assert_eq!(e, format!("user config {path}: server.token at line 2: expected a string, found a float"));
    let v = secret();
    let (e, path) = user_err(&format!("[[model]]\nid = \"a\"\n[[model.provider]]\nname = \"p\"\nkey = \"{v}\"\n"), &v);
    assert_eq!(e, format!("user config {path}: model[0].provider[0].key at line 5: expected env:<NAME> or keychain:<name>, found a string that is not valid"));
}

// ---- N55: output drops marketplace source userinfo ----

fn userinfo_config(t: &str) -> String {
    format!("[[marketplace]]\nname = \"a\"\nsource = \"git+https://bob:{t}@h.example/o/r.git\"\nref = \"v1.0.0\"\n[[marketplace]]\nname = \"b\"\nsource = \"https://{t}@h.example/o/r\"\nref = \"v1.0.0\"\n")
}

#[test]
fn show_prints_a_marketplace_source_without_userinfo() {
    let t = format!("tk{}", zygo_core::ids::ulid());
    let m = loaded(&userinfo_config(&t));
    for text in [m.show(None, &|_| true), m.show(Some(Layer::User), &|_| true)] {
        assert!(text.contains("marketplace.a.source = \"git+https://h.example/o/r.git\"  # user\n"), "{text}");
        assert!(text.contains("marketplace.b.source = \"https://h.example/o/r\"  # user\n"), "{text}");
        assert!(!text.contains(&t), "{text}");
    }
}

#[test]
fn show_keeps_the_config_source_unchanged() {
    let t = format!("tk{}", zygo_core::ids::ulid());
    let m = loaded(&userinfo_config(&t));
    let a = m.config.marketplaces.iter().find(|x| x.name == "a").unwrap();
    assert_eq!(a.source, format!("git+https://bob:{t}@h.example/o/r.git"));
}

// ---- N75: team floors (§6.1 note 2026-11-20) ----

const FLOORS: &str = "[guards]\nmin_level = \"enforce\"\n[features.redaction]\nsecrets = true\n[[marketplace]]\nname = \"team\"\nref = \"v1\"\nrequired = [\"p\"]\n";

/// The three team floors of `FLOORS` hold, each with the source `team`.
#[track_caller]
fn floors(m: &Merged) {
    assert_eq!(m.config.guards.min_level, Level::Enforce);
    assert_eq!(m.source_of("guards.min_level"), Layer::Team);
    assert!(m.config.features.redaction.secrets);
    assert_eq!(m.source_of("features.redaction.secrets"), Layer::Team);
    let t = m.config.marketplaces.iter().find(|x| x.name == "team").expect("team marketplace kept");
    assert_eq!(t.required, ["p"]);
    assert_eq!(m.source_of("marketplace.team.required"), Layer::Team);
}

#[test]
fn team_floors_hold_over_user() {
    let user = "[guards]\nmin_level = \"monitor\"\nclaims = \"monitor\"\n[features.redaction]\nsecrets = false\n\
                [[marketplace]]\nname = \"team\"\nref = \"v2\"\nrequired = []\n[plugins]\ndisable = [\"team:p\"]\n";
    let m = load_with_builtin(&team_ctx(FLOORS, user), toml::Table::new()).unwrap();
    floors(&m);
    assert_eq!(m.config.guards.claims, Level::Enforce);
    assert_eq!(m.source_of("guards.claims"), Layer::Team);
    assert_eq!(m.config.marketplaces[0].git_ref, "v2");
    assert_eq!(m.source_of("marketplace.team.ref"), Layer::User);
    let all = m.show(None, &|_| true);
    assert!(all.contains("guards.min_level = \"enforce\"  # team\n"), "{all}");
    assert!(all.contains("features.redaction.secrets = true  # team\n"), "{all}");
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn team_floors_hold_over_repository() {
    let d = repo_with(
        "n75r",
        "version = 1\n[guards]\nmin_level = \"monitor\"\nclaims = \"monitor\"\n[navigation]\nlines = 50\n\
         [features.redaction]\nsecrets = false\n[[marketplace]]\nname = \"team\"\nref = \"v3\"\nrequired = []\n",
    );
    let team = format!("[navigation]\nlines = 100\n{FLOORS}");
    let m = load_with_builtin(&ctx(None, Some(d.to_path_buf()), Some(&team)), toml::Table::new()).unwrap();
    floors(&m);
    assert_eq!(m.config.guards.claims, Level::Enforce);
    assert_eq!(m.config.marketplaces[0].git_ref, "v1");
    assert_eq!(m.config.navigation.lines, 50);
    assert_eq!(m.source_of("navigation.lines"), Layer::Repository);
    assert_eq!(
        m.warnings,
        [
            ".zygo.toml: [features], [marketplace] ignored; the repository file may set only [guards], [navigation], [limits], [work]",
            ".zygo.toml: guards.min_level ignored; only user or team config may set it",
        ]
    );
}

#[test]
fn team_floors_hold_over_profile() {
    let user = "[profile.p]\nplugins = { \"team:p\" = false }\nfeatures = { work_units = false }\n\
                [profile.s]\nfeatures = { redaction = { secrets = false } }\n";
    let team = format!("[features]\nwork_units = true\n{FLOORS}");
    let m = load_with_builtin(&team_ctx(&team, user), toml::Table::new()).unwrap();
    let p = m.apply_profile("p").unwrap();
    floors(&p);
    assert!(!p.config.features.work_units);
    assert_eq!(p.source_of("features.work_units"), Layer::Profile);
    match m.apply_profile("s") {
        Err(ConfigError::Profile { message, .. }) => {
            assert_eq!(message, "features: redaction.secrets cannot be set by a profile")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn team_floors_hold_over_flags() {
    let team = format!("[results]\ntrim_threshold_bytes = 1000\n{FLOORS}");
    let m = load_with_builtin(&team_ctx(&team, ""), toml::Table::new()).unwrap();
    let own: toml::Table = toml::from_str("name = \"team\"\nref = \"v9\"\nrequired = []\n").unwrap();
    for (key, v) in [
        ("guards.min_level", toml::Value::from("monitor")),
        ("features.redaction.secrets", toml::Value::from(false)),
        ("marketplace", toml::Value::Array(vec![])),
        ("marketplace", toml::Value::Array(vec![toml::Value::Table(own)])),
        ("plugins.disable", toml::Value::Array(vec!["team:p".into()])),
    ] {
        let f = m.apply_flag(key, v);
        assert!(f.warnings.is_empty(), "{key}: {:?}", f.warnings);
        floors(&f);
    }
    // The flag's own `team` entry keeps its other keys; only `required` is the team's.
    let own: toml::Table = toml::from_str("name = \"team\"\nref = \"v9\"\nrequired = []\n").unwrap();
    let f = m.apply_flag("marketplace", toml::Value::Array(vec![toml::Value::Table(own)]));
    assert_eq!(f.config.marketplaces[0].git_ref, "v9");
    assert_eq!(f.source_of("marketplace.team.ref"), Layer::Flags);
    // A wrong-typed marketplace is refused as today: input kept, one warning.
    // So are `required` and `approvals` contents that are not their types (round 3, R2-B1).
    let mut bad = vec![toml::Value::Array(vec!["x".into()])];
    for e in [
        "name = \"team\"\nrequired = \"p\"\n",
        "name = \"team\"\nrequired = [1]\n",
        "name = \"team\"\nrequired = [[\"x\"]]\n",
        "name = \"team\"\napprovals = [1]\n",
        "name = \"team\"\napprovals = [{ plugin = 1 }]\n",
    ] {
        bad.push(toml::Value::Array(vec![toml::Value::Table(toml::from_str(e).unwrap())]));
    }
    for v in bad {
        let f = m.apply_flag("marketplace", v);
        assert_eq!(f.config, m.config, "wrong-typed marketplace kept its warning");
        assert_eq!(f.warnings.len(), 1, "wrong-typed marketplace kept its warning");
        assert!(f.warnings[0].starts_with("flag marketplace:"), "{}", f.warnings[0]);
    }
    // A guards that is not a table is refused as today, not replaced by the floor (round 2, B1).
    for v in [toml::Value::from("x"), toml::Value::Integer(3)] {
        let f = m.apply_flag("guards", v);
        assert_eq!(f.config, m.config, "wrong-typed guards kept its warning");
        assert_eq!(f.warnings.len(), 1, "wrong-typed guards kept its warning");
        assert!(f.warnings[0].starts_with("flag guards:"), "{}", f.warnings[0]);
    }
    // An invalid min_level is refused as today, not floored: team notify, user enforce.
    let m2 = load_with_builtin(&team_ctx("[guards]\nmin_level = \"notify\"\n", "[guards]\nmin_level = \"enforce\"\n"), toml::Table::new()).unwrap();
    let f = m2.apply_flag("guards.min_level", "loud".into());
    assert_eq!(f.config.guards.min_level, Level::Enforce, "invalid min_level kept its warning");
    assert_eq!(f.source_of("guards.min_level"), Layer::User, "invalid min_level kept its warning");
    assert_eq!(f.warnings.len(), 1, "invalid min_level kept its warning");
    assert!(f.warnings[0].starts_with("flag guards.min_level:"), "{}", f.warnings[0]);
    let f = m.apply_flag("guards.scope", "monitor".into());
    assert_eq!(f.config.guards.scope, Level::Enforce);
    let f = m.apply_flag("results.trim_threshold_bytes", 4096.into());
    assert_eq!(f.config.results.trim_threshold_bytes, 4096);
    assert_eq!(f.source_of("results.trim_threshold_bytes"), Layer::Flags);
}

/// Every flag entry with a team marketplace's name gets the team's `required` and `approvals` (round 2, M1, M2).
#[test]
fn team_marketplace_fields_hold_on_every_flag_entry() {
    let team = "[[marketplace]]\nname = \"team\"\nref = \"v1\"\nrequired = [\"p\"]\napprovals = [{ plugin = \"q\", digest = \"d\" }]\n";
    let m = load_with_builtin(&team_ctx(team, ""), toml::Table::new()).unwrap();
    let held = |f: &Merged, n: usize| {
        let ts: Vec<_> = f.config.marketplaces.iter().filter(|x| x.name == "team").collect();
        assert_eq!(ts.len(), n);
        for t in ts {
            assert_eq!(t.required, ["p"]);
            assert_eq!(t.approvals, m.config.marketplaces[0].approvals);
        }
        assert_eq!(f.source_of("marketplace.team.required"), Layer::Team);
        assert_eq!(f.source_of("marketplace.team.approvals"), Layer::Team);
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    };
    for e in ["name = \"team\"\nref = \"v9\"\nrequired = []\napprovals = []\n", "name = \"team\"\nref = \"v9\"\n"] {
        let e: toml::Table = toml::from_str(e).unwrap();
        let f = m.apply_flag("marketplace", toml::Value::Array(vec![toml::Value::Table(e)]));
        held(&f, 1);
        assert_eq!(f.config.marketplaces[0].git_ref, "v9");
    }
    let e: toml::Table = toml::from_str("name = \"team\"\nrequired = []\napprovals = []\n").unwrap();
    let f = m.apply_flag("marketplace", toml::Value::Array(vec![toml::Value::Table(e.clone()), toml::Value::Table(e)]));
    held(&f, 2);
    // An entry already equal to the team's values is recorded as team too (round 3, R2-M1).
    let e: toml::Table = toml::from_str(&format!("ref = \"v9\"\n{}", team.trim_start_matches("[[marketplace]]\n").replace("ref = \"v1\"\n", ""))).unwrap();
    let f = m.apply_flag("marketplace", toml::Value::Array(vec![toml::Value::Table(e)]));
    held(&f, 1);
    assert_eq!(f.source_of("marketplace.team.ref"), Layer::Flags);
}

/// A team marketplace with no `required` keeps its `approvals` on a flag entry of its name, as for the user layer;
/// a flag may still remove it (round 3, R2-M2).
#[test]
fn team_marketplace_without_required_keeps_approvals() {
    let team = "[[marketplace]]\nname = \"t2\"\nref = \"v1\"\napprovals = [{ plugin = \"p\", digest = \"d\" }]\n";
    let m = load_with_builtin(&team_ctx(team, ""), toml::Table::new()).unwrap();
    let e: toml::Table = toml::from_str("name = \"t2\"\nref = \"v2\"\napprovals = []\n").unwrap();
    let f = m.apply_flag("marketplace", toml::Value::Array(vec![toml::Value::Table(e)]));
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    assert_eq!(f.config.marketplaces[0].approvals, m.config.marketplaces[0].approvals);
    assert_eq!(f.source_of("marketplace.t2.approvals"), Layer::Team);
    assert_eq!(f.config.marketplaces[0].git_ref, "v2");
    let f = m.apply_flag("marketplace", toml::Value::Array(vec![]));
    assert!(f.config.marketplaces.is_empty());
}

#[test]
fn team_floors_hold_over_session() {
    let m = load_with_builtin(&team_ctx(FLOORS, ""), toml::Table::new()).unwrap();
    let e = m.with_overrides(&guards(&[("claims", "notify")])).unwrap_err();
    assert_eq!(e, Refusal::BelowFloor { guard: "claims".into(), min: Level::Enforce });
    let s = m.with_overrides(&Overrides { redaction_content: Some(false), ..Overrides::default() }).unwrap();
    floors(&s);
    assert!(!s.config.features.redaction.content);
}

#[test]
fn team_key_outside_floors_stays_replaceable() {
    let team = "[guards]\nmin_level = \"notify\"\nclaims = \"enforce\"\n[features]\nwork_units = true\n\
                [features.redaction]\nsecrets = false\n";
    let user = "[guards]\nmin_level = \"enforce\"\nclaims = \"notify\"\n[features]\nwork_units = false\n\
                [features.redaction]\nsecrets = false\n";
    let m = load_with_builtin(&team_ctx(team, user), toml::Table::new()).unwrap();
    assert_eq!(m.config.guards.claims, Level::Enforce);
    assert_eq!(m.source_of("guards.claims"), Layer::User);
    assert!(!m.config.features.work_units);
    assert_eq!(m.source_of("features.work_units"), Layer::User);
    assert_eq!(m.config.guards.min_level, Level::Enforce);
    assert_eq!(m.source_of("guards.min_level"), Layer::User);
    assert!(!m.config.features.redaction.secrets);
    assert_eq!(m.source_of("features.redaction.secrets"), Layer::User);
}

// ---- HUD-3: [statusline] and [update], user config only (H§8, OP7, AU1; auto-update "Opt-out") ----

#[test]
fn statusline_and_update_defaults() {
    let c = Config::default();
    assert_eq!(c.statusline.style, "compact");
    assert!(c.update.check);
    assert_eq!(cfg(""), c);
    assert_eq!(cfg("[statusline]\n").statusline, StatuslineConfig { style: "compact".into() });
    assert_eq!(cfg("[update]\n").update, UpdateConfig { check: true });
    let m = load(&ctx(None, None, None)).unwrap();
    assert_eq!((m.config.statusline.style.as_str(), m.config.update.check), ("compact", true));
    assert_eq!(m.source_of("statusline.style"), Layer::Builtin);
    assert_eq!(m.source_of("update.check"), Layer::Builtin);
}

#[test]
fn statusline_and_update_from_user_config() {
    let m = load(&ctx(Some("[statusline]\nstyle = \"verbose\"\n[update]\ncheck = false\n"), None, None)).unwrap();
    assert_eq!(m.config.statusline.style, "verbose");
    assert!(!m.config.update.check);
    assert_eq!(m.source_of("statusline.style"), Layer::User);
    assert_eq!(m.source_of("update.check"), Layer::User);
    assert_eq!(m.warnings, Vec::<String>::new());
}

#[test]
fn team_layer_drops_statusline_and_update_with_one_warning() {
    let team = "[statusline]\nstyle = \"verbose\"\n[update]\ncheck = false\n[guards]\nmin_level = \"notify\"\n";
    let m = load(&team_ctx(team, "")).unwrap();
    assert_eq!(m.config.statusline.style, "compact");
    assert!(m.config.update.check);
    assert_eq!(m.source_of("statusline.style"), Layer::Builtin);
    assert_eq!(m.source_of("update.check"), Layer::Builtin);
    assert_eq!(m.team_keys, BTreeSet::from(["guards.min_level".to_string()]));
    assert_eq!(m.warnings, ["config: [statusline], [update] in team config ignored; only user config may set them"]);
    let all = m.show(None, &|_| true);
    assert!(all.contains("statusline.style = \"compact\"  # builtin\n"), "{all}");
    assert!(all.contains("update.check = true  # builtin\n"), "{all}");
}

#[test]
fn team_layer_drop_names_only_the_tables_present() {
    let m = load(&team_ctx("[update]\ncheck = false\n", "")).unwrap();
    assert!(m.config.update.check);
    assert_eq!(m.warnings, ["config: [update] in team config ignored; only user config may set them"]);
    let m = load(&team_ctx("[guards]\nmin_level = \"notify\"\n", "")).unwrap();
    assert_eq!(m.warnings, Vec::<String>::new());
}

#[test]
fn user_statusline_and_update_hold_in_a_team_session() {
    let team = "[statusline]\nstyle = \"compact\"\n[update]\ncheck = true\n";
    let m = load(&team_ctx(team, "[statusline]\nstyle = \"verbose\"\n[update]\ncheck = false\n")).unwrap();
    assert_eq!(m.config.statusline.style, "verbose");
    assert!(!m.config.update.check);
    assert_eq!(m.source_of("statusline.style"), Layer::User);
    assert_eq!(m.source_of("update.check"), Layer::User);
    // A user-only key the team sent is not a team key: a session override of it is not refused as team-set.
    assert_eq!(m.team_keys, BTreeSet::new());
}

#[test]
fn dropped_team_tables_do_not_refuse_the_load() {
    let m = load(&team_ctx("statusline = 3\n[update]\ncheck = \"no\"\n", "")).unwrap();
    assert_eq!((m.config.statusline.style.as_str(), m.config.update.check), ("compact", true));
    assert_eq!(m.warnings, ["config: [statusline], [update] in team config ignored; only user config may set them"]);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn repo_file_drops_statusline_and_update() {
    let d = repo_with("hud3", "version = 1\n[statusline]\nstyle = \"verbose\"\n[update]\ncheck = false\n");
    match repo_file::read_default_branch(&d) {
        RepoFile::Found { table, ignored, .. } => {
            assert_eq!(ignored, ["statusline", "update"]);
            assert!(!table.contains_key("statusline") && !table.contains_key("update"));
        }
        other => panic!("{other:?}"),
    }
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.config.statusline.style, "compact");
    assert!(m.config.update.check);
    assert_eq!(m.source_of("statusline.style"), Layer::Builtin);
    assert_eq!(
        m.warnings,
        [".zygo.toml: [statusline], [update] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]
    );
}

#[test]
fn check_statusline_style() {
    none(&(base() + "[statusline]\nstyle = \"compact\"\n"));
    none(&(base() + "[statusline]\nstyle = \"verbose\"\n"));
    for bad in ["wide", "", "Compact", "verbose "] {
        err(&(base() + &format!("[statusline]\nstyle = \"{bad}\"\n")), "statusline.style: must be \"compact\" or \"verbose\"");
    }
}

// ---- HUD-9: H§14 #34, config part ----

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn h14_34_repo_file_statusline_is_ignored_with_one_note() { // H§14 #34 (config part): H§8, the `KEPT` filter
    let d = repo_with("hud9", "[statusline]\nstyle = \"verbose\"\n");
    match repo_file::read_default_branch(&d) {
        RepoFile::Found { table, ignored, .. } => {
            assert_eq!(ignored, ["statusline"]);
            assert!(table.is_empty(), "{table:?}");
        }
        other => panic!("{other:?}"),
    }
    // The one note: the launcher prints each warning as one stderr line.
    let note = ".zygo.toml: [statusline] ignored; the repository file may set only [guards], [navigation], [limits], [work]";
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.warnings, [note]);
    assert!(!note.contains('\n'));
    assert_eq!(m.config.statusline.style, "compact");
    assert_eq!(m.source_of("statusline.style"), Layer::Builtin);
    // A kept table of the same file still applies; the user's own style holds over the repository file.
    let d = repo_with("hud9b", "[statusline]\nstyle = \"compact\"\n[navigation]\nlines = 50\n");
    let m = load(&ctx(Some("[statusline]\nstyle = \"verbose\"\n"), Some(d.to_path_buf()), None)).unwrap();
    assert_eq!(m.warnings, [note]);
    assert_eq!((m.config.statusline.style.as_str(), m.source_of("statusline.style")), ("verbose", Layer::User));
    assert_eq!(m.source_of("navigation.lines"), Layer::Repository);
}

// ---- W5: [workspace] and [loadout], user config only (W§11, AU10, AU22) ----

#[test]
fn parse_duration_forms() { // W§11 `drain_timeout`: <n>s | <n>m | <n>h
    use std::time::Duration;
    assert_eq!(parse_duration("90s"), Some(Duration::from_secs(90)));
    assert_eq!(parse_duration("10m"), Some(Duration::from_secs(600)));
    assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(parse_duration("0s"), Some(Duration::ZERO));
    assert_eq!(parse_duration("007m"), Some(Duration::from_secs(420)));
    assert_eq!(parse_duration("18446744073709551615s"), Some(Duration::from_secs(u64::MAX)));
    for bad in [
        "", "s", "10", "10D", "10ms", "10M", "-5m", "+5m", "1.5h", " 10m", "10m ", "10 m", "1h30m", "١٠m",
        "18446744073709551616s", "18446744073709551615m", "5124095576030432h", "213503982334602d",
    ] {
        assert_eq!(parse_duration(bad), None, "{bad:?}");
    }
}

const W11: &str = r#"
[workspace]
default_loadout = "solo"        # built-in
drain_block_dispatch = true     # OP6
drain_timeout = "10m"           # AU4; <n>s | <n>m | <n>h

[loadout.pair]
[[loadout.pair.window]]
name = "agents"
layout = "main-vertical"
[[loadout.pair.window.pane]]
kind = "claude-code"
profile = "cheap"
args = ["--model", "lead"]
yolo = false
passthrough = false
server = "https://zygo.example"
compact = "600k"
config_dir = "~/.claude-work"
[[loadout.pair.window.pane]]
kind = "shell"
cwd = "branch:feature-x"
"#;

#[test]
fn workspace_and_loadout_defaults() {
    let c = Config::default();
    assert_eq!(c.workspace, WorkspaceConfig { default_loadout: "solo".into(), drain_block_dispatch: true, drain_timeout: "10m".into(), pool: None });
    assert_eq!(parse_duration(&c.workspace.drain_timeout), Some(std::time::Duration::from_secs(600)));
    assert!(c.loadout.is_empty());
    assert_eq!((BUILTIN_LOADOUT, LOADOUT_KINDS), ("solo", ["claude-code", "opencode", "shell"]));
    assert_eq!(cfg(""), c);
    assert_eq!(cfg("[workspace]\n").workspace, c.workspace);
    assert_eq!(cfg("[workspace]\ndrain_timeout = \"30s\"\n").workspace, WorkspaceConfig { drain_timeout: "30s".into(), ..c.workspace.clone() });
    assert_eq!(
        LoadoutPane::default(),
        LoadoutPane { kind: String::new(), name: None, job: None, profile: None, args: vec![], yolo: false, passthrough: false, server: None, compact: None, config_dir: None, pool: false, cwd: None }
    );
    assert_eq!(LoadoutWindow::default(), LoadoutWindow { name: None, layout: None, pane: vec![] });
    let m = load(&ctx(None, None, None)).unwrap();
    assert_eq!(m.config.workspace, c.workspace);
    assert!(m.config.loadout.is_empty());
    assert_eq!(m.source_of("workspace.default_loadout"), Layer::Builtin);
    assert!(m.show(None, &|_| true).contains("workspace.drain_timeout = \"10m\"  # builtin\n"));
}

#[test]
fn loadout_parses_as_w11_writes_it() {
    let m = load(&ctx(Some(W11), None, None)).unwrap();
    assert_eq!(m.warnings, Vec::<String>::new());
    assert_eq!(m.config.workspace, WorkspaceConfig::default());
    assert_eq!(m.config.loadout.keys().collect::<Vec<_>>(), ["pair"]);
    let agent = LoadoutPane {
        kind: "claude-code".into(), name: None, job: None, profile: Some("cheap".into()), args: vec!["--model".into(), "lead".into()], yolo: false,
        passthrough: false, server: Some("https://zygo.example".into()), compact: Some("600k".into()),
        config_dir: Some("~/.claude-work".into()), pool: false, cwd: None,
    };
    let shell = LoadoutPane { kind: "shell".into(), cwd: Some("branch:feature-x".into()), ..LoadoutPane::default() };
    let window = LoadoutWindow { name: Some("agents".into()), layout: Some("main-vertical".into()), pane: vec![agent, shell] };
    assert_eq!(m.config.loadout["pair"], Loadout { window: vec![window] });
    assert_eq!(m.source_of("workspace.drain_timeout"), Layer::User);
    assert_eq!(m.source_of("loadout.pair.window"), Layer::User);
    let all = m.show(None, &|_| true);
    assert!(all.contains("workspace.default_loadout = \"solo\"  # user\n"), "{all}");
    assert!(all.lines().any(|l| l.starts_with("loadout.pair.window = [") && l.ends_with("  # user")), "{all}");
    for bad in ["[loadout.x]\nwindow = 3\n", "[workspace]\ndrain_block_dispatch = \"yes\"\n", "[[loadout.x.window]]\npane = [3]\n"] {
        assert!(matches!(load(&ctx(Some(bad), None, None)), Err(ConfigError::Parse { .. })), "{bad}");
    }
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn w18_62_repo_file_loadout_is_ignored_with_the_existing_warning() { // W§18 #62, repository part
    let d = repo_with("w5", "version = 1\n[loadout.x]\n[[loadout.x.window]]\n[[loadout.x.window.pane]]\nkind = \"shell\"\n");
    match repo_file::read_default_branch(&d) {
        RepoFile::Found { table, ignored, .. } => {
            assert_eq!(ignored, ["loadout"]);
            assert!(!table.contains_key("loadout"));
        }
        other => panic!("{other:?}"),
    }
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert!(m.config.loadout.is_empty());
    assert_eq!(m.warnings, [".zygo.toml: [loadout] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]);
}

#[test]
fn w18_62_team_layer_drops_loadout_and_workspace_with_the_au22_warning() { // W§18 #62, team part; the launcher adds `zygo: `
    const AU22: &str = "[loadout] and [workspace] are read from user config only; ignored";
    assert_eq!(layers::WORKSPACE_ONLY_IGNORED, AU22);
    let lo = "[loadout.evil]\n[[loadout.evil.window]]\n[[loadout.evil.window.pane]]\nkind = \"shell\"\nargs = [\"-c\", \"id\"]\n";
    let team = format!("[workspace]\ndefault_loadout = \"evil\"\n{lo}[guards]\nmin_level = \"notify\"\n");
    let m = load(&team_ctx(&team, "")).unwrap();
    assert_eq!(m.config.workspace, WorkspaceConfig::default());
    assert!(m.config.loadout.is_empty());
    assert_eq!(m.source_of("workspace.default_loadout"), Layer::Builtin);
    assert_eq!(m.team_keys, BTreeSet::from(["guards.min_level".to_string()]));
    assert_eq!(m.warnings, [AU22]);
    // one table, or a value of the wrong type (dropped before the layer is parsed): the same one line
    for one in ["[workspace]\ndrain_timeout = \"1s\"\n", "[loadout.x]\n", "loadout = 3\nworkspace = \"x\"\n"] {
        assert_eq!(load(&team_ctx(one, "")).unwrap().warnings, [AU22], "{one}");
    }
    // with a HUD-3 table: the HUD-3 line first
    let m = load(&team_ctx("[statusline]\nstyle = \"verbose\"\n[loadout.x]\n", "")).unwrap();
    assert_eq!(m.warnings, ["config: [statusline] in team config ignored; only user config may set them", AU22]);
    // the user's tables hold in a team session
    let user = "[workspace]\ndrain_timeout = \"5m\"\n[loadout.mine]\n[[loadout.mine.window]]\n[[loadout.mine.window.pane]]\nkind = \"shell\"\n";
    let m = load(&team_ctx("[workspace]\ndrain_timeout = \"1s\"\n", user)).unwrap();
    assert_eq!((m.config.workspace.drain_timeout.as_str(), m.source_of("workspace.drain_timeout")), ("5m", Layer::User));
    assert_eq!(m.config.loadout.keys().collect::<Vec<_>>(), ["mine"]);
    assert_eq!(m.warnings, [AU22]);
    assert_eq!(load(&team_ctx("[guards]\nmin_level = \"notify\"\n", "")).unwrap().warnings, Vec::<String>::new());
}

const LO: &str = "[loadout.pair]\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\n";
const BAD_KIND: &str = "must be \"claude-code\", \"opencode\" or \"shell\"";

#[test]
fn check_loadout_structure() {
    none(&(base() + LO + "kind = \"claude-code\"\npassthrough = true\nconfig_dir = \"~/.claude-work\"\n"));
    none(&(base() + LO + "kind = \"opencode\"\npassthrough = false\n"));
    none(&(base() + LO + "kind = \"shell\"\ncwd = \"branch:x\"\n"));
    err(&(base() + LO + "kind = \"vim\"\n"), &format!("loadout.pair.window[0].pane[0].kind: {BAD_KIND}"));
    err(&(base() + LO + "yolo = true\n"), &format!("loadout.pair.window[0].pane[0].kind: {BAD_KIND}"));
    err(&(base() + LO + "kind = \"opencode\"\npassthrough = true\n"), "loadout.pair.window[0].pane[0].passthrough: only with kind = \"claude-code\"");
    err(&(base() + LO + "kind = \"shell\"\nconfig_dir = \"x\"\n"), "loadout.pair.window[0].pane[0].config_dir: only with kind = \"claude-code\"");
    err(&(base() + "[loadout.pair]\n[[loadout.pair.window]]\nname = \"w\"\n"), "loadout.pair.window[0]: must have at least one pane");
    err(&(base() + "[loadout.pair]\n"), "loadout.pair: must have at least one window");
    let two = "[loadout.\"my lo\"]\n[[loadout.\"my lo\".window]]\n[[loadout.\"my lo\".window.pane]]\nkind = \"shell\"\n[[loadout.\"my lo\".window]]\n[[loadout.\"my lo\".window.pane]]\nkind = \"shell\"\n[[loadout.\"my lo\".window.pane]]\nkind = \"vim\"\n";
    err(&(base() + two), &format!("loadout.\"my lo\".window[1].pane[1].kind: {BAD_KIND}"));
}

#[test]
fn check_default_loadout_names_a_loadout() {
    none(&(base() + "[workspace]\ndefault_loadout = \"solo\"\n"));
    none(&(base() + "[workspace]\ndefault_loadout = \"pair\"\n" + LO + "kind = \"shell\"\n"));
    err(&(base() + "[workspace]\ndefault_loadout = \"nope\"\n"), "workspace.default_loadout: unknown loadout");
}

#[test]
fn check_loadout_missing_profile_is_a_warning() {
    let v = one(&(base() + LO + "kind = \"claude-code\"\nprofile = \"cheap\"\n"));
    assert_eq!((v.severity, v.to_string()), (Severity::Warning, "loadout.pair.window[0].pane[0].profile: unknown profile".into()));
    none(&(base() + "[profile.cheap]\n" + LO + "kind = \"claude-code\"\nprofile = \"cheap\"\n"));
}

#[test]
fn w18_63_config_check_reports_kind_and_passthrough() { // W§18 #63; pane[2]'s `--settings` is not reported (W§11, AU59)
    let panes = "kind = \"vim\"\n[[loadout.pair.window.pane]]\nkind = \"opencode\"\npassthrough = true\n[[loadout.pair.window.pane]]\nkind = \"claude-code\"\nargs = [\"--settings\", \"x\"]\n";
    let found: Vec<String> = check(&loaded(&(base() + LO + panes)).config).iter().map(|v| format!("{:?} {v}", v.severity)).collect();
    assert_eq!(found, [
        format!("Error loadout.pair.window[0].pane[0].kind: {BAD_KIND}"),
        "Error loadout.pair.window[0].pane[1].passthrough: only with kind = \"claude-code\"".to_string(),
    ]);
}

// ---- ML-1: [mail], pane name, member-name grammar (M§6, M§8, M§11, M-C6) ----

const M11: &str = "[mail]\nkeep = \"30d\"\nmax_unread = 500\nwake_gap = \"30s\"\n\n[[loadout.team.window]]\n[[loadout.team.window.pane]]\nkind = \"claude-code\"\nname = \"elena\"\n";
const MAIL_TEAM: &str = "config: [mail] in team config ignored; only user config may set them";

#[test]
fn mail_defaults_match_m11() {
    let d = MailConfig { keep: "30d".into(), max_unread: 500, wake_gap: "30s".into(), host: None, sync_gap: "60s".into(), workspace: BTreeMap::new() };
    assert_eq!(MailConfig::default(), d);
    assert_eq!(Config::default().mail, d);
    assert_eq!(cfg("").mail, d);
    assert_eq!(cfg("[mail]\nmax_unread = 10\n").mail, MailConfig { max_unread: 10, ..d.clone() });
    assert!(Features::default().mail && Config::default().features.mail);
    assert!(!cfg("[features]\nmail = false\n").features.mail);
    assert!(cfg("[features]\nresults = false\n").features.mail);
    assert_eq!(LoadoutPane::default().name, None);
    let m = loaded("");
    assert_eq!((m.config.mail.clone(), m.source_of("mail.keep")), (d, Layer::Builtin));
}

#[test]
fn mail_m11_example_parses() {
    let m = load(&ctx(Some(M11), None, None)).unwrap();
    assert_eq!(m.warnings, Vec::<String>::new());
    assert_eq!(m.config.mail, MailConfig::default());
    assert_eq!(m.source_of("mail.wake_gap"), Layer::User);
    assert_eq!(m.config.loadout["team"].window[0].pane[0].name.as_deref(), Some("elena"));
    none(&(base() + M11));
}

#[test]
fn mail_parse_duration_takes_days() {
    use std::time::Duration;
    assert_eq!(parse_duration("30d"), Some(Duration::from_secs(2_592_000)));
    assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86_400)));
    assert_eq!(parse_duration("0d"), Some(Duration::ZERO));
    assert_eq!(parse_duration("213503982334601d"), Some(Duration::from_secs(213_503_982_334_601 * 86_400)));
    for bad in ["d", "30D", "213503982334602d", "1.5d", "-1d", "30 d"] {
        assert_eq!(parse_duration(bad), None, "{bad:?}");
    }
    assert_eq!(parse_duration(&MailConfig::default().keep), Some(Duration::from_secs(2_592_000)));
    assert_eq!(parse_duration(&MailConfig::default().wake_gap), Some(Duration::from_secs(30)));
}

#[test]
fn mail_team_layer_drops_mail_with_the_user_only_warning() { // M§11, M-C6
    assert_eq!(layers::USER_ONLY, ["statusline", "update", "mail"]);
    let m = load(&team_ctx("[mail]\nkeep = \"1d\"\nmax_unread = 1\n[guards]\nmin_level = \"notify\"\n", "")).unwrap();
    assert_eq!(m.config.mail, MailConfig::default());
    assert_eq!(m.source_of("mail.keep"), Layer::Builtin);
    assert_eq!(m.team_keys, BTreeSet::from(["guards.min_level".to_string()]));
    assert_eq!(m.warnings, [MAIL_TEAM]);
    // a value of the wrong type is dropped before the layer is parsed
    assert_eq!(load(&team_ctx("mail = 3\n", "")).unwrap().warnings, [MAIL_TEAM]);
    let all = "[statusline]\nstyle = \"verbose\"\n[update]\ncheck = false\n[mail]\nwake_gap = \"1s\"\n";
    assert_eq!(load(&team_ctx(all, "")).unwrap().warnings,
        ["config: [statusline], [update], [mail] in team config ignored; only user config may set them"]);
    // the user's [mail] holds in a team session
    let m = load(&team_ctx("[mail]\nkeep = \"1d\"\n", "[mail]\nkeep = \"7d\"\n")).unwrap();
    assert_eq!((m.config.mail.keep.as_str(), m.source_of("mail.keep")), ("7d", Layer::User));
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
fn mail_repo_file_table_is_dropped() { // M-C6: a cloned repository sets no [mail]
    let d = repo_with("ml1", "version = 1\n[mail]\nkeep = \"1d\"\n");
    match repo_file::read_default_branch(&d) {
        RepoFile::Found { table, ignored, .. } => {
            assert_eq!(ignored, ["mail"]);
            assert!(!table.contains_key("mail"));
        }
        other => panic!("{other:?}"),
    }
    let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
    assert_eq!((m.config.mail.clone(), m.source_of("mail.keep")), (MailConfig::default(), Layer::Builtin));
    assert_eq!(m.warnings,
        [".zygo.toml: [mail] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]);
}

const NAME_GRAMMAR: &str = "must be 1-32 of [a-z0-9-], starting with a letter";

#[test]
fn mail_check_refuses_bad_pane_names() { // M§6
    let p = "loadout.pair.window[0].pane[0].name";
    none(&(base() + LO + "kind = \"claude-code\"\nname = \"elena\"\n"));
    err(&(base() + LO + "kind = \"claude-code\"\nname = \"Elena\"\n"), &format!("{p}: {NAME_GRAMMAR}"));
    err(&(base() + LO + "kind = \"opencode\"\nname = \"\"\n"), &format!("{p}: {NAME_GRAMMAR}"));
    err(&(base() + LO + "kind = \"claude-code\"\nname = \"all\"\n"), &format!("{p}: is reserved"));
    err(&(base() + LO + "kind = \"opencode\"\nname = \"cli\"\n"), &format!("{p}: is reserved"));
    err(&(base() + LO + "kind = \"claude-code\"\nname = \"zygo\"\n"), &format!("{p}: is reserved")); // S§7.2, M§6
    // unique in the loadout, across windows; the later pane is reported
    let twice = "kind = \"claude-code\"\nname = \"elena\"\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\nkind = \"opencode\"\nname = \"elena\"\n";
    err(&(base() + LO + twice), "loadout.pair.window[1].pane[0].name: is used by another pane in the loadout");
    // the same name in two loadouts is allowed
    let other = "[loadout.solo2]\n[[loadout.solo2.window]]\n[[loadout.solo2.window.pane]]\nkind = \"opencode\"\nname = \"elena\"\n";
    none(&(base() + LO + "kind = \"claude-code\"\nname = \"elena\"\n" + other));
}

#[test]
fn mail_check_refuses_bad_durations() { // M§11; the forms of parse_duration
    const FORMS: &str = "must be <n>s, <n>m, <n>h or <n>d";
    none(&(base() + "[mail]\nkeep = \"12h\"\nwake_gap = \"0s\"\n"));
    err(&(base() + "[mail]\nkeep = \"30\"\n"), &format!("mail.keep: {FORMS}"));
    err(&(base() + "[mail]\nwake_gap = \"1w\"\n"), &format!("mail.wake_gap: {FORMS}"));
    let v: Vec<String> = check(&loaded(&(base() + "[mail]\nkeep = \"x\"\nwake_gap = \"\"\n")).config).iter().map(ToString::to_string).collect();
    assert_eq!(v, [format!("mail.keep: {FORMS}"), format!("mail.wake_gap: {FORMS}")]);
}

// ---- ML-1 review round 1 ----

#[test] // finding 2; kills: a name of pane-key form (W§5) accepted, so mail_send's `to` is ambiguous
fn mail_check_reserves_the_pane_key_form() {
    let p = "loadout.pair.window[0].pane[0].name";
    err(&(base() + LO + "kind = \"claude-code\"\nname = \"kabcdefgh\"\n"), &format!("{p}: is reserved"));
    none(&(base() + LO + "kind = \"claude-code\"\nname = \"kabcdefg1\"\n"));
}

#[test] // finding 3; kills: a name on a shell pane, which has no member record (W AU53), accepted
fn mail_check_refuses_a_name_on_a_shell_pane() {
    let p = "loadout.pair.window[0].pane[0].name";
    err(&(base() + LO + "kind = \"shell\"\nname = \"elena\"\n"), &format!("{p}: only with kind = \"claude-code\" or \"opencode\""));
    // one violation only: the kind rule is reported in place of the grammar
    err(&(base() + LO + "kind = \"shell\"\nname = \"\"\n"), &format!("{p}: only with kind = \"claude-code\" or \"opencode\""));
}

#[test] // finding 4; kills: `mail` added to the profile feature keys (only the user config sets it, M§8)
fn mail_feature_cannot_be_set_by_a_profile() {
    let m = loaded("[profile.p]\nfeatures = { mail = false }\n");
    let p = zygo_config::profile::select(&m, Some("p")).unwrap();
    let c = &m.config;
    let v = zygo_config::profile::validate(&p, &c.role, &c.main_role, &c.catalog.view(), Dialect::Anthropic, &BTreeSet::new())
        .unwrap_err();
    assert_eq!(v.to_string(), "profile.p.features.mail: not a feature key");
}

// ---- OS-9: `[results] shape` (output shaping O§9) ----

#[test]
fn results_shape_is_true_without_a_results_table() {
    assert!(Config::default().results.shape);
    assert!(cfg("").results.shape);
    let m = loaded("");
    assert!(m.config.results.shape);
    assert_eq!(m.source_of("results.shape"), Layer::Builtin);
}

#[test]
fn results_shape_false_in_the_user_file_reads_false() {
    let m = loaded("[results]\nshape = false\n");
    assert!(!m.config.results.shape);
    assert_eq!(m.source_of("results.shape"), Layer::User);
    let r = &m.config.results;
    assert_eq!((r.trim_threshold_bytes, r.head_lines, r.tail_lines), (16384, 60, 20));
}
