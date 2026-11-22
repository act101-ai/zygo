//! HUD-4: the price function, the long-context tier, the cache-write TTL split and price sanity
//! (H§7.3 steps 1-2, AU5, AU32, AU33; H§14 #11 and the `zygo-config` part of #12).

use zygo_config::builtin::defaults;
use zygo_config::*;
use zygo_core::paths::Paths;

fn builtin_prices(id: &str) -> Prices {
    let d = defaults();
    let row = d.catalog.get(id).unwrap_or_else(|| panic!("no built-in row {id}"));
    row.provider[0].price_usd_per_mtok.clone().unwrap_or_else(|| panic!("{id} has no price"))
}

fn flat(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Prices {
    Prices { input, output, cache_read, cache_write, ..Default::default() }
}

#[track_caller]
fn close(got: Option<f64>, want: f64) {
    let got = got.expect("priced");
    assert!((got - want).abs() < 1e-9, "got {got}, want {want}");
}

// ---- H§14 #11: the long-context tier ----

#[test]
fn haiku_prompt_at_the_threshold_uses_base_rates() {
    let p = builtin_prices("claude-haiku-5-5");
    // prompt = 60,000 + 30,000 + 10,000 = 100,000: not above `long_context.above`.
    let u = TokenUsage { input: 60_000, output: 2_000, cache_read: 30_000, cache_write: 10_000, ..Default::default() };
    // (60,000 x 0.10 + 2,000 x 0.50 + 30,000 x 0.01 + 10,000 x 0.20) / 1e6
    close(p.cost(&u), 0.0093);
}

#[test]
fn haiku_prompt_one_token_above_uses_all_four_long_rates() {
    let p = builtin_prices("claude-haiku-5-5");
    // prompt = 60,001 + 30,000 + 10,000 = 100,001.
    let u = TokenUsage { input: 60_001, output: 2_000, cache_read: 30_000, cache_write: 10_000, ..Default::default() };
    // (60,001 x 0.50 + 2,000 x 2.50 + 30,000 x 0.05 + 10,000 x 1.00) / 1e6
    close(p.cost(&u), 0.0465005);
}

#[test]
fn each_long_rate_is_used_alone() {
    let p = Prices {
        long_context: Some(LongContextPrices { above: 10, input: 1000.0, output: 100.0, cache_read: 10.0, cache_write: 1.0, cache_write_5m: None }),
        ..flat(8.0, 4.0, 2.0, 1.0e-3)
    };
    let u = |input, output, cache_read, cache_write| TokenUsage { input, output, cache_read, cache_write, ..Default::default() };
    close(p.cost(&u(11, 0, 0, 0)), 11.0 * 1000.0 / 1e6);
    close(p.cost(&u(0, 7, 11, 0)), (7.0 * 100.0 + 11.0 * 10.0) / 1e6);
    close(p.cost(&u(0, 0, 0, 11)), 11.0 * 1.0 / 1e6);
    // The output tokens are not part of the prompt: a large output on a short prompt keeps the base rates.
    close(p.cost(&u(10, 1_000_000, 0, 0)), (10.0 * 8.0 + 1_000_000.0 * 4.0) / 1e6);
}

#[test]
fn a_table_without_a_tier_keeps_its_rates_for_any_prompt() {
    let p = builtin_prices("claude-opus-5-5");
    assert_eq!(p.long_context, None);
    let u = TokenUsage { input: 900_000, output: 1_000, cache_read: 50_000, cache_write: 0, ..Default::default() };
    // (900,000 x 4 + 1,000 x 20 + 50,000 x 0.20) / 1e6
    close(p.cost(&u), 3.63);
}

#[test]
fn zero_usage_costs_zero() {
    assert_eq!(builtin_prices("claude-haiku-5-5").cost(&TokenUsage::default()), Some(0.0));
}

// ---- AU33: the cache-write TTL split ----

#[test]
fn split_cache_writes_use_the_two_rates() {
    let p = builtin_prices("claude-opus-5-5");
    let u = TokenUsage { cache_write: 2_000, cache_write_5m: Some(1_000), cache_write_1h: Some(1_000), ..Default::default() };
    // (1,000 x 5 + 1,000 x 8) / 1e6
    close(p.cost(&u), 0.013);
}

#[test]
fn cache_writes_without_the_split_use_cache_write() {
    let p = builtin_prices("claude-opus-5-5");
    let u = TokenUsage { cache_write: 2_000, ..Default::default() };
    close(p.cost(&u), 0.016);
}

#[test]
fn a_table_without_cache_write_5m_prices_the_split_at_cache_write() {
    let p = flat(4.0, 20.0, 0.2, 8.0);
    let u = TokenUsage { cache_write: 2_000, cache_write_5m: Some(1_000), cache_write_1h: Some(1_000), ..Default::default() };
    close(p.cost(&u), 0.016);
}

#[test]
fn write_tokens_outside_the_split_use_cache_write() {
    let p = builtin_prices("claude-opus-5-5");
    // 3,000 written, 1,000 of them reported as 5-minute, none as 1-hour: the other 2,000 have no TTL.
    let u = TokenUsage { cache_write: 3_000, cache_write_5m: Some(1_000), ..Default::default() };
    close(p.cost(&u), (1_000.0 * 5.0 + 2_000.0 * 8.0) / 1e6);
    // Only the 1-hour part is reported.
    let u = TokenUsage { cache_write: 3_000, cache_write_1h: Some(1_000), ..Default::default() };
    close(p.cost(&u), 3_000.0 * 8.0 / 1e6);
}

#[test]
fn a_split_larger_than_the_total_is_priced_as_reported() {
    let p = builtin_prices("claude-opus-5-5");
    let u = TokenUsage { cache_write: 0, cache_write_5m: Some(1_000), cache_write_1h: Some(1_000), ..Default::default() };
    close(p.cost(&u), 0.013);
    // The reported writes count as prompt tokens for the tier test.
    let h = builtin_prices("claude-haiku-5-5");
    let u = TokenUsage { input: 100_000, cache_write: 0, cache_write_5m: Some(1), ..Default::default() };
    close(h.cost(&u), (100_000.0 * 0.50 + 1.0 * 0.625) / 1e6);
}

#[test]
fn the_long_tier_has_its_own_5_minute_rate() {
    let p = builtin_prices("claude-haiku-5-5");
    let u = TokenUsage { input: 100_000, cache_write: 2_000, cache_write_5m: Some(1_000), cache_write_1h: Some(1_000), ..Default::default() };
    // (100,000 x 0.50 + 1,000 x 0.625 + 1,000 x 1.00) / 1e6
    close(p.cost(&u), 0.051625);
    // Below the threshold: (90,000 x 0.10 + 1,000 x 0.125 + 1,000 x 0.20) / 1e6
    close(p.cost(&TokenUsage { input: 90_000, ..u }), 0.009325);
}

#[test]
fn a_long_tier_without_a_5_minute_rate_uses_its_own_cache_write() {
    let p = Prices {
        cache_write_5m: Some(1.0),
        long_context: Some(LongContextPrices { above: 10, input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 64.0, cache_write_5m: None }),
        ..flat(0.0, 0.0, 0.0, 2.0)
    };
    let u = TokenUsage { cache_write: 1_000, cache_write_5m: Some(1_000), ..Default::default() };
    close(p.cost(&u), 1_000.0 * 64.0 / 1e6);
}

// ---- AU32: a non-finite cost is not priced ----

#[test]
fn a_cost_that_overflows_is_not_priced() {
    let p = flat(f64::MAX, f64::MAX, 0.0, 0.0);
    assert_eq!(p.cost(&TokenUsage { input: 10, output: 10, ..Default::default() }), None);
}

#[test]
fn nan_and_infinite_rates_are_not_priced() {
    let u = TokenUsage { input: 1, output: 1, cache_read: 1, cache_write: 1, ..Default::default() };
    assert_eq!(flat(f64::NAN, 1.0, 1.0, 1.0).cost(&u), None);
    assert_eq!(flat(1.0, f64::INFINITY, 1.0, 1.0).cost(&u), None);
    assert_eq!(flat(1.0, 1.0, f64::NEG_INFINITY, 1.0).cost(&u), None);
    // inf x 0 tokens is NaN, not 0.
    assert_eq!(flat(f64::INFINITY, 1.0, 1.0, 1.0).cost(&TokenUsage::default()), None);
}

#[test]
fn token_counts_near_u64_max_do_not_wrap() {
    let p = flat(1.0, 1.0, 1.0, 1.0);
    let u = TokenUsage { input: u64::MAX, cache_read: u64::MAX, cache_write: u64::MAX, cache_write_5m: Some(u64::MAX), cache_write_1h: Some(u64::MAX), ..Default::default() };
    let c = p.cost(&u).expect("finite");
    assert!(c > 5.5e13 && c.is_finite(), "{c}");
    // The prompt sum saturates, so the long tier applies.
    let p = Prices { long_context: Some(LongContextPrices { above: u64::MAX - 1, input: 2.0, output: 2.0, cache_read: 2.0, cache_write: 2.0, cache_write_5m: None }), ..p };
    assert!(p.cost(&u).expect("finite") > 1.1e14);
}

// ---- AU32: `config check` ----

fn checked(user: &str) -> Vec<Violation> {
    let d = tempfile::Builder::new().prefix("zygo-hud4-").tempdir().unwrap();
    let p = d.path();
    let paths = Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    };
    std::fs::write(&paths.config_file, user).unwrap();
    let m = load_with_builtin(&LoadContext { paths, repo: None, team: None }, toml::Table::new()).unwrap();
    check(&m.config)
}

fn model(prices: &str) -> String {
    format!(
        "main_role = \"lead\"\n[role.lead]\nmodel = \"m\"\n[[model]]\nid = \"m\"\n[[model.provider]]\nname = \"p\"\nbase_url = \"http://127.0.0.1:1\"\nupstream_model = \"up\"\n[model.provider.price_usd_per_mtok]\n{prices}\n"
    )
}

const PRICE: &str = "model[0].provider[0].price_usd_per_mtok";

#[track_caller]
fn rejected(prices: &str, key: &str, message: &str) {
    let v = checked(&model(prices));
    let want = Violation { severity: Severity::Error, path: format!("{PRICE}.{key}"), message: message.into() };
    assert_eq!(v, vec![want], "{prices}");
}

const BAD_RATE: &str = "must be a finite number >= 0";

#[test]
fn check_accepts_sane_prices() {
    assert_eq!(checked(&model("input = 4.0\noutput = 20.0\ncache_read = 0.2\ncache_write = 8.0")), vec![]);
    assert_eq!(checked(&model("input = 0.0\noutput = 0\ncache_read = 0.0\ncache_write = 0.0\ncache_write_5m = 0.0")), vec![]);
    let long = "input = 0.1\noutput = 0.5\ncache_read = 0.01\ncache_write = 0.2\ncache_write_5m = 0.125\n\
                long_context = { above = 1, input = 0.5, output = 2.5, cache_read = 0.05, cache_write = 1.0, cache_write_5m = 0.625 }";
    assert_eq!(checked(&model(long)), vec![]);
}

#[test]
fn check_rejects_a_negative_input_price() {
    rejected("input = -1\noutput = 1.0\ncache_read = 1.0\ncache_write = 1.0", "input", BAD_RATE);
    rejected("input = -1.0", "input", BAD_RATE);
}

#[test]
fn check_rejects_a_nan_output_price() {
    rejected("input = 1.0\noutput = nan\ncache_read = 1.0\ncache_write = 1.0", "output", BAD_RATE);
}

#[test]
fn check_rejects_an_infinite_cache_read_price() {
    rejected("input = 1.0\noutput = 1.0\ncache_read = inf\ncache_write = 1.0", "cache_read", BAD_RATE);
    rejected("cache_read = -inf", "cache_read", BAD_RATE);
}

#[test]
fn check_rejects_bad_cache_write_prices() {
    rejected("cache_write = -0.5", "cache_write", BAD_RATE);
    rejected("cache_write_5m = nan", "cache_write_5m", BAD_RATE);
}

#[test]
fn check_rejects_long_context_above_zero() {
    rejected("input = 1.0\nlong_context = { above = 0, input = 1.0, output = 1.0, cache_read = 1.0, cache_write = 1.0 }", "long_context.above", "must be > 0");
    // `above` absent is 0.
    rejected("long_context = { input = 1.0 }", "long_context.above", "must be > 0");
}

#[test]
fn check_rejects_bad_long_context_prices() {
    for key in ["input", "output", "cache_read", "cache_write", "cache_write_5m"] {
        for bad in ["-1.0", "nan", "inf"] {
            rejected(&format!("long_context = {{ above = 5, {key} = {bad} }}"), &format!("long_context.{key}"), BAD_RATE);
        }
    }
}

#[test]
fn check_reports_each_bad_price() {
    let v = checked(&model("input = -1\noutput = nan\ncache_read = inf\ncache_write = 1.0\nlong_context = { above = 0 }"));
    let paths: Vec<&str> = v.iter().map(|x| x.path.as_str()).collect();
    let want: Vec<String> = ["input", "output", "cache_read", "long_context.above"].iter().map(|k| format!("{PRICE}.{k}")).collect();
    assert_eq!(paths, want);
    assert!(v.iter().all(|x| x.severity == Severity::Error));
}

// ---- schema and catalog data ----

#[test]
fn the_new_keys_parse_and_survive_the_layers() {
    let user = model(
        "input = 0.1\noutput = 0.5\ncache_read = 0.01\ncache_write = 0.2\ncache_write_5m = 0.125\n\
         [model.provider.price_usd_per_mtok.long_context]\nabove = 100000\ninput = 0.5\noutput = 2.5\ncache_read = 0.05\ncache_write = 1.0\ncache_write_5m = 0.625",
    );
    let d = tempfile::Builder::new().prefix("zygo-hud4-").tempdir().unwrap();
    let p = d.path();
    let paths = Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    };
    std::fs::write(&paths.config_file, &user).unwrap();
    let m = load_with_builtin(&LoadContext { paths, repo: None, team: None }, toml::Table::new()).unwrap();
    let got = m.config.catalog.get("m").unwrap().provider[0].price_usd_per_mtok.clone().unwrap();
    let want = Prices {
        input: 0.1,
        output: 0.5,
        cache_read: 0.01,
        cache_write: 0.2,
        cache_write_5m: Some(0.125),
        long_context: Some(LongContextPrices { above: 100_000, input: 0.5, output: 2.5, cache_read: 0.05, cache_write: 1.0, cache_write_5m: Some(0.625) }),
    };
    assert_eq!(got, want);
    assert_eq!(m.source_of("model.m.provider.p.price_usd_per_mtok.long_context.above"), Layer::User);
    let shown = m.show(None, &|_| true);
    for line in [
        "model.m.provider.p.price_usd_per_mtok.cache_write_5m = 0.125  # user",
        "model.m.provider.p.price_usd_per_mtok.long_context.above = 100000  # user",
        "model.m.provider.p.price_usd_per_mtok.long_context.cache_write_5m = 0.625  # user",
    ] {
        assert!(shown.lines().any(|l| l == line), "missing {line}\n{shown}");
    }
}

#[test]
fn absent_optional_prices_are_not_written() {
    let t = toml::to_string(&flat(4.0, 20.0, 0.2, 8.0)).unwrap();
    assert!(!t.contains("cache_write_5m") && !t.contains("long_context"), "{t}");
    let back: Prices = toml::from_str(&t).unwrap();
    assert_eq!(back, flat(4.0, 20.0, 0.2, 8.0));
    let h = builtin_prices("claude-haiku-5-5");
    let back: Prices = toml::from_str(&toml::to_string(&h).unwrap()).unwrap();
    assert_eq!(back, h);
}

#[test]
fn builtin_anthropic_rows_have_the_5_minute_rate() {
    // 1.25 x the input rate of each row (AU33), as listed on the row's source page.
    for (id, rate) in [("claude-fable-5-2", 10.0), ("claude-opus-5-5", 5.0), ("claude-sonnet-5-5", 2.5), ("claude-haiku-5-5", 0.125)] {
        let p = builtin_prices(id);
        assert_eq!(p.cache_write_5m, Some(rate), "{id}");
        assert_eq!(p.cache_write_5m, Some(p.input * 1.25), "{id}");
    }
    for id in ["gpt-6.2-astra", "gpt-6.2-sol"] {
        assert_eq!(builtin_prices(id).cache_write_5m, None, "{id}");
    }
}

#[test]
fn builtin_long_context_tier_is_the_haiku_row_only() {
    let want = LongContextPrices { above: 100_000, input: 0.50, output: 2.50, cache_read: 0.05, cache_write: 1.00, cache_write_5m: Some(0.625) };
    for row in &defaults().catalog.0 {
        let p = row.provider[0].price_usd_per_mtok.as_ref().unwrap();
        let tier = if row.id == "claude-haiku-5-5" { Some(want.clone()) } else { None };
        assert_eq!(p.long_context, tier, "{}", row.id);
    }
}

// ---- HUD-8: H§14 #12, price part ----

#[test]
fn h14_12_price_sanity_and_the_cache_write_split() { // H§14 #12 (price part): AU32, AU33
    // `config check` rejects each bad price.
    rejected("input = -1", "input", BAD_RATE);
    rejected("output = nan", "output", BAD_RATE);
    rejected("cache_read = inf", "cache_read", BAD_RATE);
    rejected("long_context = { above = 0, input = 1.0, output = 1.0, cache_read = 1.0, cache_write = 1.0 }", "long_context.above", "must be > 0");
    // A cost that overflows to infinity counts as not priced.
    let huge = flat(f64::MAX, f64::MAX, 0.0, 0.0);
    assert_eq!(huge.cost(&TokenUsage { input: 10, output: 10, ..Default::default() }), None);
    // AU33: 1,000 5-minute and 1,000 1-hour cache-write tokens on a table with `cache_write_5m` use the two rates.
    let p = Prices { cache_write_5m: Some(5.0), ..flat(4.0, 20.0, 0.4, 8.0) };
    let split = TokenUsage { cache_write: 2_000, cache_write_5m: Some(1_000), cache_write_1h: Some(1_000), ..Default::default() };
    close(p.cost(&split), (1_000.0 * 5.0 + 1_000.0 * 8.0) / 1e6);
    // Without the split all 2,000 use `cache_write`.
    close(p.cost(&TokenUsage { cache_write: 2_000, ..Default::default() }), 2_000.0 * 8.0 / 1e6);
}
