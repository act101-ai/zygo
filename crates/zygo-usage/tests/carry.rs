use std::cell::RefCell;
use zygo_usage::{carry::carry, Carry, CtxRequest, Rates};

const R: Rates = Rates { input: 4.0, cache_read: 0.4, write: 5.0 };    // O§6.3 test row: cache_write_5m 5, cache_write 8
fn flat(_: &str, _: &str, _: u64) -> Option<Rates> { Some(R) }
fn row(rowid: i64, ts: i64, cr: u64, cw: u64) -> CtxRequest {
    CtxRequest { rowid, ts, model: "m1".into(), provider: "p1".into(), input_tokens: 100, cache_read_tokens: cr,
        cache_write_tokens: cw, ..Default::default() }
}
/// O§6.3 worked example: one row before the trim (ts 100), requests 1-7 at ts 110..170, 7 the compaction, one row after.
fn example() -> Vec<CtxRequest> {
    let mut q = vec![row(1, 90, 10_000, 2_000), row(2, 110, 10_000, 12_000), row(3, 120, 22_000, 1_000),
        row(4, 130, 23_000, 1_000), row(5, 140, 24_000, 1_000), row(6, 150, 25_000, 1_000), row(7, 160, 0, 27_000),
        row(8, 170, 27_000, 500), row(9, 180, 27_500, 100)];
    q[7].compaction = true;
    q
}
fn near(a: f64, b: f64) -> bool { (a - b).abs() < 1e-12 }

#[test]
fn worked_example_trim_is_0_12() {
    let c = carry(100, &example(), false, &flat);
    assert!(near(c.usd_per_token * 10_000.0, 0.12), "{}", c.usd_per_token * 10_000.0);
    assert!(c.unpriced.is_empty());
}

#[test]
fn worked_example_read_back_after_request_5_is_0_0054() {
    let c = carry(155, &example(), false, &flat);
    assert!(near(c.usd_per_token * 1_000.0, 0.0054), "{}", c.usd_per_token * 1_000.0);
}

#[test]
fn growing_context_reads_after_the_first_row() {     // cache write > cache read on each row, cache read never falls
    let q = [row(1, 10, 100, 1_000), row(2, 20, 1_100, 2_000), row(3, 30, 3_100, 4_000), row(4, 40, 7_100, 8_000)];
    assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 5.0 + 3.0 * 0.4));
}

#[test]
fn cut_before_the_first_row_gives_an_empty_carry() {
    let q = [row(1, 10, 40_000, 10_000), row(2, 30, 0, 20_000), row(3, 40, 20_000, 1_000)];   // 20_100 < 50_100 / 2
    assert_eq!(carry(20, &q, false, &flat), Carry::default());
}

#[test]
fn cut_compares_with_the_row_just_before() {
    // before the event: prompts 100_100 then 40_100; first row 30_100 >= 40_100 / 2 (not cut against 100_100)
    // then 100_100, then 45_100 < 100_100 / 2 (cut against the row just before, not against the first row 30_100)
    let q = [row(1, 10, 90_000, 10_000), row(2, 20, 0, 40_000), row(3, 40, 0, 30_000), row(4, 50, 30_000, 70_000),
        row(5, 60, 45_000, 0)];
    assert!(near(carry(30, &q, false, &flat).usd_per_token * 1e6, 5.0 + 0.4));
}

#[test]
fn compaction_row_is_taken_and_ends_the_carry() {
    let mut q = vec![row(1, 10, 1_000, 1_000), row(2, 20, 2_000, 1_000), row(3, 30, 3_000, 1_000)];
    q[0].compaction = true;
    assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 5.0));
}

#[test]
fn event_in_the_same_second_as_a_starts_at_a() {
    let q = [row(1, 100, 1_000, 1_000), row(2, 110, 2_000, 1_000)];
    assert!(near(carry(100, &q, false, &flat).usd_per_token * 1e6, 5.0 + 0.4));
    assert!(near(carry(101, &q, false, &flat).usd_per_token * 1e6, 5.0));
}

#[test]
fn first_only_takes_one_row() {
    let q = [row(1, 10, 100, 1_000), row(2, 20, 1_100, 2_000), row(3, 30, 3_100, 4_000)];
    assert!(near(carry(5, &q, true, &flat).usd_per_token * 1e6, 5.0));
}

#[test]
fn row_with_no_cache_tokens_is_priced_at_input() {
    let q = [row(1, 10, 0, 0), row(2, 20, 0, 0)];
    assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 4.0 + 4.0));
}

#[test]
fn unpriced_row_adds_zero_and_is_listed_once() {
    let mut q = vec![row(1, 10, 1_000, 1_000), row(42, 20, 2_000, 1_000), row(3, 30, 3_000, 1_000)];
    q[1].model = "unknown".into();
    let rates = |m: &str, _: &str, _: u64| (m == "m1").then_some(R);
    let c = carry(5, &q, false, &rates);
    assert!(near(c.usd_per_token * 1e6, 5.0 + 0.4));
    assert_eq!(c.unpriced, vec![42]);
}

#[test]
fn doubled_rates_give_twice_usd_per_token() {     // O-C5: rates come only from the closure
    let double = |_: &str, _: &str, _: u64| Some(Rates { input: 8.0, cache_read: 0.8, write: 10.0 });
    let (one, two) = (carry(100, &example(), false, &flat), carry(100, &example(), false, &double));
    assert!(near(two.usd_per_token * 1e6, 2.0 * one.usd_per_token * 1e6));
}

#[test]
fn closure_gets_model_provider_and_prompt_of_each_carrying_row() {
    let seen = RefCell::new(Vec::new());
    let rates = |m: &str, p: &str, prompt: u64| {
        seen.borrow_mut().push((m.to_string(), p.to_string(), prompt));
        Some(R)
    };
    carry(155, &example(), false, &rates);
    assert_eq!(*seen.borrow(),
        vec![("m1".to_string(), "p1".to_string(), 27_100u64), ("m1".to_string(), "p1".to_string(), 27_600)]);
}

#[test]
fn event_after_the_last_row_gives_an_empty_carry() {
    assert_eq!(carry(200, &example(), false, &flat), Carry::default());
    assert_eq!(carry(0, &[], false, &flat), Carry::default());
}

/// Row 42 is a carrying row priced at `cache_read`; `bad` is the rate the closure gives for it.
fn non_finite_rate_case(bad: f64) -> Carry {
    let mut q = vec![row(1, 10, 1_000, 1_000), row(42, 20, 2_000, 1_000), row(3, 30, 3_000, 1_000)];
    q[1].model = "bad".into();
    let rates = |m: &str, _: &str, _: u64| Some(if m == "bad" { Rates { cache_read: bad, ..R } } else { R });
    carry(5, &q, false, &rates)
}

#[test]
fn nan_rate_is_treated_as_unpriced() {     // O§6.3: rates follow `Prices::cost`, which drops a non-finite result
    let c = non_finite_rate_case(f64::NAN);
    assert!(near(c.usd_per_token * 1e6, 5.0 + 0.4), "{}", c.usd_per_token * 1e6);
    assert_eq!(c.unpriced, vec![42]);
}

#[test]
fn infinite_rate_is_treated_as_unpriced() {
    let c = non_finite_rate_case(f64::INFINITY);
    assert!(near(c.usd_per_token * 1e6, 5.0 + 0.4), "{}", c.usd_per_token * 1e6);
    assert_eq!(c.unpriced, vec![42]);
}

#[test]
fn equal_cache_read_is_not_an_expiry() {     // a context that only grows never looks like an expiry
    let q = [row(1, 10, 1_000, 1_000), row(2, 20, 1_000, 500)];
    assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 5.0 + 0.4));
}

#[test]
fn prompt_exactly_half_of_the_row_before_still_carries() {     // "less than half": 1_000 * 2 == 2_000 is not cut
    let q = [row(1, 10, 1_900, 0), row(2, 30, 0, 900)];
    assert!(near(carry(20, &q, false, &flat).usd_per_token * 1e6, 5.0));
}

#[test]
fn first_only_with_a_cut_first_row_gives_an_empty_carry() {
    let q = [row(1, 10, 40_000, 10_000), row(2, 30, 0, 20_000), row(3, 40, 20_000, 1_000)];   // 20_100 < 50_100 / 2
    assert_eq!(carry(20, &q, true, &flat), Carry::default());
}

#[test]
fn equal_ts_rows_start_at_the_first_in_slice_order() {     // rows 2 and 3 share ts 20, given in rowid order
    let q = [row(1, 10, 1_000, 1_000), row(2, 20, 2_000, 1_000), row(3, 20, 3_000, 1_000)];
    let seen = RefCell::new(Vec::new());
    let rates = |_: &str, _: &str, prompt: u64| {
        seen.borrow_mut().push(prompt);
        Some(R)
    };
    let c = carry(20, &q, false, &rates);
    assert_eq!(*seen.borrow(), vec![3_100, 4_100]);
    assert!(near(c.usd_per_token * 1e6, 5.0 + 0.4));
}
