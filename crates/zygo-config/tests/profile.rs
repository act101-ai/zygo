use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use zygo_config::profile::*;
use zygo_config::*;
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

fn tmpdir(name: &str) -> Tmp {
    Tmp(tempfile::Builder::new().prefix(&format!("zygo-n6-{name}-")).tempdir().unwrap())
}

static CTX_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn ctx(user: &str, team: Option<&str>) -> Cx {
    let n = CTX_N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = tmpdir(&format!("ctx{n}"));
    let config_file = d.join("config.toml");
    std::fs::write(&config_file, user).unwrap();
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
    Cx { c: LoadContext { paths, repo: None, team: team.map(|t| toml::from_str(t).unwrap()) }, _d: d.0 }
}

fn merged(user: &str) -> Merged {
    load(&ctx(user, None)).unwrap()
}

// ---- step 1: selection ----

const U: &str = "[user]\ndefault_profile = \"a\"\n[profile.a]\nprompt = \"A\"\n[profile.b]\nprompt = \"B\"\n";
#[test] fn flag_wins_over_default() { assert_eq!(select(&merged(U), Some("b")).unwrap().name, "b"); }
#[test] fn default_used_without_flag() { assert_eq!(select(&merged(U), None).unwrap().name, "a"); }
#[test] fn flag_none_disables() { assert!(select(&merged(U), Some("none")).is_none()); assert_eq!(requested(&merged(U), Some("none")), None); }
#[test] fn default_none_disables() { assert!(select(&merged("[user]\ndefault_profile = \"none\"\n[profile.a]\nprompt = \"A\"\n"), None).is_none()); }
#[test] fn nothing_selected() { assert!(select(&merged("[profile.a]\nprompt = \"A\"\n"), None).is_none()); }
#[test] fn unknown_name_is_requested_not_selected() {
    assert!(select(&merged(U), Some("zz")).is_none());
    assert_eq!(requested(&merged(U), Some("zz")).as_deref(), Some("zz"));
}
#[test] fn user_profile_shadows_team_preset() {
    let m = load(&ctx("[profile.rust]\nprompt = \"u\"\n", Some("[profile.rust]\ndisabled = [\"design\"]\nprompt = \"t\"\n"))).unwrap();
    let p = select(&m, Some("rust")).unwrap();
    assert_eq!(p.prompt.as_deref(), Some("u")); assert!(p.disabled.is_empty());
}
#[test] fn all_parts_copied_and_json_round_trips() {
    let p = select(&merged("[profile.x]\noverrides = { review = \"m1\" }\ndisabled = [\"design\"]\nprompt = \"p\"\nplugins = { \"team:skills\" = true }\nfeatures = { work_units = false }\n[profile.x.roles.bench]\nmodel = \"m1\"\ndescription = \"d\"\nprompt = \"q\"\nwrites = false\n"), Some("x")).unwrap();
    assert_eq!((p.overrides["review"].as_str(), p.disabled.clone(), p.plugins["team:skills"]), ("m1", vec!["design".to_string()], true));
    assert!(p.features.is_some() && !p.roles["bench"].writes);
    assert_eq!(serde_json::from_str::<Profile>(&serde_json::to_string(&p).unwrap()).unwrap(), p);
}

// ---- validate helpers ----

fn roles() -> BTreeMap<String, Role> {
    ["lead", "plan", "triage", "review", "design", "code", "read"].iter().map(|r| (r.to_string(), Role::default())).collect()
}
fn cat() -> CatalogView {
    let mv = |d: &[Dialect]| ModelView { dialects: d.iter().copied().collect(), ..Default::default() };
    CatalogView {
        models: BTreeMap::from([
            ("m1".to_string(), mv(&[Dialect::Anthropic, Dialect::Openai])),
            ("m2".to_string(), mv(&[Dialect::Openai])),
        ]),
    }
}
fn plugins() -> BTreeSet<String> { ["team:skills", "team:other"].iter().map(|s| s.to_string()).collect() }
fn ok() -> Profile { Profile { name: "rust".into(), overrides: BTreeMap::from([("review".to_string(), "m1".to_string())]), ..Default::default() } }
fn crole(f: impl FnOnce(&mut CustomRole)) -> CustomRole {
    let mut r = CustomRole { model: "m1".into(), description: "d".into(), prompt: "q".into(), writes: true };
    f(&mut r);
    r
}
fn run(p: Profile) -> Result<(), Violation> { validate(&p, &roles(), "lead", &cat(), Dialect::Anthropic, &plugins()) }
fn bad(f: impl FnOnce(&mut Profile)) -> String { let mut p = ok(); f(&mut p); run(p).unwrap_err().to_string() }
fn good(f: impl FnOnce(&mut Profile)) { let mut p = ok(); f(&mut p); assert!(run(p).is_ok()); }

// ---- step 2: name rule and "no part set" ----

#[test] fn name_rule() {
    for n in ["", "Rust", "a_b", "a b", &"a".repeat(33)] {
        assert_eq!(bad(|p| p.name = n.into()), format!("profile.{n}: name must be 1-32 characters of a-z, 0-9 and -"));
    }
    good(|p| p.name = "a".repeat(32)); good(|p| p.name = "0-9".into()); good(|p| p.name = "a".into());
}
#[test] fn name_none_is_reserved() { assert_eq!(bad(|p| p.name = "none".into()), "profile.none: name is reserved; none disables the profile"); }
#[test] fn no_part_is_invalid() { assert_eq!(bad(|p| *p = Profile { name: "rust".into(), ..Default::default() }), "profile.rust: sets no part"); }
#[test] fn each_part_alone_is_enough() {
    let only = |f: fn(&mut Profile)| { let mut p = Profile { name: "rust".into(), ..Default::default() }; f(&mut p); p };
    assert!(run(only(|p| { p.overrides.insert("review".into(), "m1".into()); })).is_ok());
    assert!(run(only(|p| p.disabled = vec!["design".into()])).is_ok());
    assert!(run(only(|p| p.prompt = Some("x".into()))).is_ok());
    assert!(run(only(|p| { p.plugins.insert("team:skills".into(), true); })).is_ok());
    assert!(run(only(|p| p.features = Some("results = false".parse().unwrap()))).is_ok());
    assert!(run(only(|p| { p.roles.insert("bench".into(), crole(|_| {})); })).is_ok());
    assert!(run(Profile { name: "rust".into(), prompt: Some(String::new()), ..Default::default() }).is_err());
}

// ---- step 3: text fields ----

#[test] fn profile_prompt_limit_is_2048_bytes() {
    assert_eq!(bad(|p| p.prompt = Some("x".repeat(2049))), "profile.rust.prompt: 2049 bytes; the limit is 2048");
    good(|p| p.prompt = Some("x".repeat(2048)));
    assert_eq!(bad(|p| p.prompt = Some("é".repeat(1025))), "profile.rust.prompt: 2050 bytes; the limit is 2048");
}
#[test] fn profile_prompt_allows_braces() { good(|p| p.prompt = Some("use {x}".into())); }
#[test] fn control_chars_refused_newline_and_tab_allowed() {
    assert_eq!(bad(|p| p.prompt = Some("a\u{7}b".into())), "profile.rust.prompt: contains a control character");
    good(|p| p.prompt = Some("a\nb\tc".into()));
    assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.prompt = "a\u{1b}".into())); }), "profile.rust.roles.bench.prompt: contains a control character");
    assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.description = "a\u{0}".into())); }), "profile.rust.roles.bench.description: contains a control character");
}
#[test] fn role_description_rules() {
    let d = |s: String| bad(move |p| { p.roles.insert("bench".into(), crole(|r| r.description = s)); });
    assert_eq!(d(String::new()), "profile.rust.roles.bench.description: must not be empty");
    assert_eq!(d("x".repeat(257)), "profile.rust.roles.bench.description: 257 bytes; the limit is 256");
    good(|p| { p.roles.insert("bench".into(), crole(|r| r.description = "x".repeat(256))); });
    assert_eq!(d("a\nb".into()), "profile.rust.roles.bench.description: must be one line");
    for s in ["a{b", "a}b"] { assert_eq!(d(s.into()), "profile.rust.roles.bench.description: must not contain { or }"); }
}
#[test] fn role_prompt_rules() {
    let q = |s: String| bad(move |p| { p.roles.insert("bench".into(), crole(|r| r.prompt = s)); });
    assert_eq!(q(String::new()), "profile.rust.roles.bench.prompt: must not be empty");
    assert_eq!(q("x".repeat(2049)), "profile.rust.roles.bench.prompt: 2049 bytes; the limit is 2048");
    good(|p| { p.roles.insert("bench".into(), crole(|r| r.prompt = "x".repeat(2048))); });
    assert_eq!(q("a{b".into()), "profile.rust.roles.bench.prompt: must not contain { or }");
}

// ---- step 4: overrides and custom roles ----

#[test] fn override_key_must_be_bundle_role() { assert_eq!(bad(|p| { p.overrides.insert("nope".into(), "m1".into()); }), "profile.rust.overrides.nope: not a bundle role"); }
#[test] fn override_model_in_catalog() { assert_eq!(bad(|p| { p.overrides.insert("review".into(), "ghost".into()); }), "profile.rust.overrides.review: model is not in the catalog"); }
#[test] fn override_model_servable_on_dialect() {
    assert_eq!(bad(|p| { p.overrides.insert("review".into(), "m2".into()); }), "profile.rust.overrides.review: model cannot be served on the anthropic dialect");
    let mut p = ok(); p.overrides.insert("review".into(), "m2".into());
    assert!(validate(&p, &roles(), "lead", &cat(), Dialect::Openai, &plugins()).is_ok());
}
#[test] fn custom_role_count_boundary() {
    let n = |k: usize| move |p: &mut Profile| for i in 0..k { p.roles.insert(format!("r{i}"), crole(|_| {})); };
    good(n(8)); assert_eq!(bad(n(9)), "profile.rust.roles: 9 custom roles; the limit is 8");
}
#[test] fn custom_role_name_rules() {
    let named = |n: &str| { let n = n.to_string(); move |p: &mut Profile| { p.roles.insert(n, crole(|_| {})); } };
    assert_eq!(bad(named("Bad")), "profile.rust.roles.Bad: name must be 1-32 characters of a-z, 0-9 and -");
    assert_eq!(bad(named("")), "profile.rust.roles.: name must be 1-32 characters of a-z, 0-9 and -");
    assert_eq!(bad(named(&"a".repeat(33))), format!("profile.rust.roles.{}: name must be 1-32 characters of a-z, 0-9 and -", "a".repeat(33)));
    good(named(&"a".repeat(32)));
    assert_eq!(bad(named("lead")), "profile.rust.roles.lead: name is a bundle role");
    assert_eq!(bad(named("m1")), "profile.rust.roles.m1: name is a catalog model id");
    for w in ["direct", "subscription", "fable", "best", "opus", "opusplan", "sonnet", "haiku"] {
        assert_eq!(bad(named(w)), format!("profile.rust.roles.{w}: name is reserved"));
    }
}
#[test] fn custom_role_model_checked_like_override() {
    assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.model = "ghost".into())); }), "profile.rust.roles.bench.model: model is not in the catalog");
    assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.model = "m2".into())); }), "profile.rust.roles.bench.model: model cannot be served on the anthropic dialect");
}
#[test] fn order_prompt_before_overrides_and_overrides_before_roles() {
    assert_eq!(bad(|p| { p.prompt = Some("x".repeat(3000)); p.overrides.insert("nope".into(), "m1".into()); }), "profile.rust.prompt: 3000 bytes; the limit is 2048");
    assert_eq!(bad(|p| { p.overrides.insert("nope".into(), "m1".into()); p.roles.insert("lead".into(), crole(|_| {})); }), "profile.rust.overrides.nope: not a bundle role");
}

// ---- step 5: disabled, plugins, features ----

#[test] fn disabled_rules() {
    assert_eq!(bad(|p| p.disabled = vec!["nope".into()]), "profile.rust.disabled[0]: not a bundle role");
    assert_eq!(bad(|p| p.disabled = vec!["design".into(), "design".into()]), "profile.rust.disabled[1]: listed twice");
    assert_eq!(bad(|p| p.disabled = vec!["lead".into()]), "profile.rust.disabled[0]: is the main role and cannot be disabled");
    let mut p = ok(); p.disabled = vec!["lead".into()];
    assert!(validate(&p, &roles(), "plan", &cat(), Dialect::Anthropic, &plugins()).is_ok());
}
#[test] fn plugin_count_boundary() {
    let known: BTreeSet<String> = (0..33).map(|i| format!("team:p{i}")).collect();
    let n = |k: usize| move |p: &mut Profile| for i in 0..k { p.plugins.insert(format!("team:p{i}"), true); };
    let r = |k| { let mut p = ok(); n(k)(&mut p); validate(&p, &roles(), "lead", &cat(), Dialect::Anthropic, &known) };
    assert!(r(32).is_ok());
    assert_eq!(r(33).unwrap_err().to_string(), "profile.rust.plugins: 33 plugins; the limit is 32");
}
#[test] fn plugin_form_and_existence() {
    for s in ["noplugin", ":x", "x:", "a:b:c", "Team:x"] {
        assert_eq!(bad(|p| { p.plugins.insert(s.into(), true); }), format!("profile.rust.plugins.{s}: not of the form <marketplace>:<plugin>"));
    }
    assert_eq!(bad(|p| { p.plugins.insert("team:ghost".into(), true); }), "profile.rust.plugins.team:ghost: not in a configured marketplace");
    good(|p| { p.plugins.insert("team:skills".into(), false); });
}
fn feat(t: &str) -> impl FnOnce(&mut Profile) + '_ { move |p| p.features = Some(t.parse().unwrap()) }
#[test] fn feature_keys_and_types() {
    for t in ["results = false", "work_units = false", "compact_tool = false", "redaction = { content = false }"] { good(feat(t)); }
    assert_eq!(bad(feat("bogus = true")), "profile.rust.features.bogus: not a feature key");
    assert_eq!(bad(feat("redaction = { bogus = true }")), "profile.rust.features.redaction.bogus: not a feature key");
    assert_eq!(bad(feat("work_units = 1")), "profile.rust.features.work_units: must be true or false");
    assert_eq!(bad(feat("redaction = true")), "profile.rust.features.redaction: must be a table");
    assert_eq!(bad(feat("redaction = { content = 1 }")), "profile.rust.features.redaction.content: must be true or false");
}
#[test] fn feature_secrets_refused_with_the_layers_text() {
    for t in ["redaction = { secrets = false }", "redaction = { secrets = true }"] {
        assert_eq!(bad(feat(t)), "profile.rust.features.redaction.secrets: features: redaction.secrets cannot be set by a profile");
    }
}
#[test] fn order_disabled_plugins_features() {
    assert_eq!(bad(|p| { p.features = Some("bogus = 1".parse().unwrap()); p.plugins.insert("team:ghost".into(), true); p.disabled = vec!["nope".into()]; }), "profile.rust.disabled[0]: not a bundle role");
    assert_eq!(bad(|p| { p.features = Some("bogus = 1".parse().unwrap()); p.plugins.insert("team:ghost".into(), true); }), "profile.rust.plugins.team:ghost: not in a configured marketplace");
}

// ---- step 6: set_default_profile ----

#[test] fn insert_into_existing_user_table_is_byte_exact() {
    let d = tmpdir("a"); let f = d.join("config.toml");
    let before = "# top\n[results]\ntrim_threshold_bytes = 1000 # keep\n\n[user]\nprompt = \"Hi\"\n\n[guards]\nclaims = \"warn\"\n";
    std::fs::write(&f, before).unwrap(); set_default_profile(&f, "rust").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), before.replace("prompt = \"Hi\"\n", "prompt = \"Hi\"\ndefault_profile = \"rust\"\n"));
}
#[test] fn change_existing_value_keeps_its_comment() {
    let d = tmpdir("b"); let f = d.join("config.toml");
    std::fs::write(&f, "[user]\ndefault_profile = \"a\" # mine\n").unwrap(); set_default_profile(&f, "b").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "[user]\ndefault_profile = \"b\" # mine\n");
}
#[test] fn file_without_user_table_gets_one_appended() {
    let d = tmpdir("c"); let f = d.join("config.toml");
    let before = "[results]\ntrim_threshold_bytes = 1000\n";
    std::fs::write(&f, before).unwrap(); set_default_profile(&f, "none").unwrap();
    let after = std::fs::read_to_string(&f).unwrap();
    assert!(after.starts_with(before));
    assert_eq!(after[before.len()..].trim(), "[user]\ndefault_profile = \"none\"");
}
#[test] fn missing_file_and_directory_are_created() {
    let d = tmpdir("d"); let f = d.join("sub/config.toml"); set_default_profile(&f, "rust").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap().parse::<toml::Table>().unwrap()["user"]["default_profile"].as_str(), Some("rust"));
}
#[test] fn user_not_a_table_and_bad_toml_are_errors_and_leave_the_file() {
    for text in ["user = 1\n", "[user\n"] {
        let d = tmpdir("e"); let f = d.join("config.toml"); std::fs::write(&f, text).unwrap();
        assert!(matches!(set_default_profile(&f, "rust"), Err(ConfigError::Parse { layer: "user", .. })));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text);
    }
}
#[cfg(unix)] #[test] fn symlinked_config_stays_a_symlink_and_target_is_written() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let d = tmpdir("f"); let target = d.join("real.toml"); let link = d.join("config.toml");
    std::fs::write(&target, "[user]\nprompt = \"Hi\"\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &link).unwrap();
    set_default_profile(&link, "rust").unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert!(std::fs::read_to_string(&target).unwrap().contains("default_profile = \"rust\""));
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
}

// ---- round 2 ----

fn one_line_clean(m: &str) { assert!(!m.chars().any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}'), "{m:?}"); }
#[test] fn untrusted_strings_are_escaped_in_messages() {
    type Edit = Box<dyn FnOnce(&mut Profile)>;
    let cases: Vec<(Edit, &str)> = vec![
        (Box::new(|p| p.name = "a\nb".into()), "profile.a\\nb: name must be 1-32 characters of a-z, 0-9 and -"),
        (Box::new(|p| p.name = "\x1b[2J".into()), "profile.\\u{1b}[2J: name must be 1-32 characters of a-z, 0-9 and -"),
        (Box::new(|p| { p.overrides.insert("a\nb".into(), "m1".into()); }), "profile.rust.overrides.a\\nb: not a bundle role"),
        (Box::new(|p| { p.overrides.insert("review".into(), "\x1b[2J".into()); }), "profile.rust.overrides.review: model is not in the catalog"),
        (Box::new(|p| { p.roles.insert("\x1b[2J".into(), crole(|_| {})); }), "profile.rust.roles.\\u{1b}[2J: name must be 1-32 characters of a-z, 0-9 and -"),
        (Box::new(|p| p.disabled = vec!["a\nb".into()]), "profile.rust.disabled[0]: not a bundle role"),
        (Box::new(|p| { p.plugins.insert("\x1b[2J:x".into(), true); }), "profile.rust.plugins.\\u{1b}[2J:x: not of the form <marketplace>:<plugin>"),
        (Box::new(|p| { p.features = Some(toml::Table::from_iter([("a\nb".to_string(), toml::Value::Boolean(true))])); }), "profile.rust.features.a\\nb: not a feature key"),
        (Box::new(|p| { p.features = Some(toml::Table::from_iter([("redaction".to_string(), toml::Value::Table(toml::Table::from_iter([("\x1b[2J".to_string(), toml::Value::Boolean(true))])))])); }), "profile.rust.features.redaction.\\u{1b}[2J: not a feature key"),
    ];
    for (f, want) in cases {
        let mut p = ok(); f(&mut p);
        let m = run(p).unwrap_err().to_string();
        one_line_clean(&m);
        assert_eq!(m, want);
    }
}
#[test] fn whitespace_only_text_counts_as_empty() {
    assert_eq!(bad(|p| *p = Profile { name: "rust".into(), prompt: Some(" \n\t ".into()), ..Default::default() }), "profile.rust: sets no part");
    assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.description = "   ".into())); }), "profile.rust.roles.bench.description: must not be empty");
    assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.prompt = " \n ".into())); }), "profile.rust.roles.bench.prompt: must not be empty");
}
#[test] fn line_separators_break_one_line() {
    for s in ["a\u{2028}b", "a\u{2029}b"] {
        assert_eq!(bad(|p| { p.roles.insert("bench".into(), crole(|r| r.description = s.into())); }), "profile.rust.roles.bench.description: must be one line");
    }
}
#[test] fn plugin_parts_follow_the_plugin_name_rule() {
    let long = format!("team:{}", "a".repeat(65));
    for s in ["-a:x", "a-:x", "a--b:x", "team:-x", "team:x-", "team:x--y", long.as_str()] {
        assert_eq!(bad(|p| { p.plugins.insert(s.into(), true); }), format!("profile.rust.plugins.{s}: not of the form <marketplace>:<plugin>"));
    }
    let ok64 = format!("a-b:{}", "a".repeat(64));
    assert_eq!(bad(|p| { p.plugins.insert(ok64.clone(), true); }), format!("profile.rust.plugins.{ok64}: not in a configured marketplace"));
}
#[test] fn user_inline_table_is_edited_in_place() {
    let d = tmpdir("g"); let f = d.join("config.toml");
    let before = "# top\nuser = { prompt = \"Hi\" }\n[results]\ntrim_threshold_bytes = 1000 # keep\n";
    std::fs::write(&f, before).unwrap(); set_default_profile(&f, "rust").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), before.replace("{ prompt = \"Hi\" }", "{ prompt = \"Hi\", default_profile = \"rust\" }"));
    set_default_profile(&f, "b").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), before.replace("{ prompt = \"Hi\" }", "{ prompt = \"Hi\", default_profile = \"b\" }"));
}
#[cfg(unix)] #[test] fn dangling_symlink_writes_its_target() {
    use std::os::unix::fs::symlink;
    let d = tmpdir("h"); std::fs::create_dir_all(d.join("real")).unwrap();
    let link = d.join("config.toml");
    symlink("real/zygo.toml", &link).unwrap();   // relative, dangling
    set_default_profile(&link, "rust").unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    let t: toml::Table = std::fs::read_to_string(d.join("real/zygo.toml")).unwrap().parse().unwrap();
    assert_eq!(t["user"]["default_profile"].as_str(), Some("rust"));
}
#[test] fn concurrent_writers_leave_one_parsable_file_and_no_temp_files() {
    let d = tmpdir("i"); let f = d.join("config.toml");
    std::fs::write(&f, "[results]\ntrim_threshold_bytes = 1000\n").unwrap();
    let names: Vec<String> = (0..16).map(|i| format!("p{i}")).collect();
    std::thread::scope(|s| for n in &names { let f = &f; s.spawn(move || set_default_profile(f, n).unwrap()); });
    let t: toml::Table = std::fs::read_to_string(&f).unwrap().parse().unwrap();
    assert!(names.iter().any(|n| t["user"]["default_profile"].as_str() == Some(n)));
    assert_eq!(t["results"]["trim_threshold_bytes"].as_integer(), Some(1000));
    let left: Vec<_> = std::fs::read_dir(&*d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.ends_with(".tmp")).collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(d.join("config.toml.lock").exists());
}
#[test] fn bad_toml_error_does_not_echo_the_source() {
    let value = format!("sk-ant-{}", ulid::Ulid::generate());
    for text in [format!("[user]\nkey = {value}\n"), format!("[user]\nkey = \"{value}\n")] {
        let d = tmpdir("r3"); let f = d.join("config.toml"); std::fs::write(&f, &text).unwrap();
        let e = set_default_profile(&f, "rust").unwrap_err().to_string();
        assert!(e.contains("TOML parse error at line 2, column ") && !e.contains(&value) && !e.contains("sk-ant"), "{e}");
    }
}
