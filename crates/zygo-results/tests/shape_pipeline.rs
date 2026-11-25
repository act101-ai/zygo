use std::collections::HashSet;
use zygo_results::Kind;
use zygo_results::shape::{self, clean::clean, command::parse, ShapeInput, Shaped, Shaper, Step, SHAPE_MAX_BYTES};
use Step::{ColorCodesRemoved as Color, ProgressLinesRemoved as Progress, RepeatedLinesCounted as Counted};

const T: u64 = 16384; // `trim_threshold_bytes` default
const DIFF: &str = "\x1b[1mdiff --git a/f b/f\x1b[0m\n--- a/f\n+++ b/f\n@@ -1 +1,3 @@\n-old\r\n\x1b[32m+x\x1b[0m\n\x1b[32m+x\x1b[0m\n\x1b[32m+x\x1b[0m\n";
const DIFF_PLAIN: &str = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,3 @@\n-old\r\n+x\n+x\n+x\n";
const JSON: &str = "[\n  \x1b[0;39m1\x1b[0m,\n  1,\n  1,\n  1\n]\n";
const JSON_PLAIN: &str = "[\n  1,\n  1,\n  1,\n  1\n]\n";

fn input(cmd: Option<&str>, kind: Kind, threshold: u64) -> ShapeInput {
    ShapeInput { parsed: cmd.map(parse), kind, threshold }
}

/// Runs the pipeline, then the O-C9 line walk: each output line is an input
/// line, a cleaned input line, or one of those plus ` [repeated <n> times]`, n >= 3.
fn shape(raw: &str, inp: &ShapeInput) -> Shaped {
    let out = shape::run(raw, inp);
    let cleaned: Vec<String> = [false, true].into_iter().filter_map(|r| clean(raw, r).map(|c| c.0)).collect();
    let mut known: HashSet<&str> = raw.split('\n').collect();
    cleaned.iter().for_each(|c| known.extend(c.split('\n')));
    for line in out.text.split('\n') {
        let base = line.strip_suffix(" times]").and_then(|l| l.rsplit_once(" [repeated "))
            .filter(|(_, n)| n.parse::<usize>().is_ok_and(|n| n >= 3)).map(|(b, _)| b);
        assert!(known.contains(line) || base.is_some_and(|b| known.contains(b)), "{line:?}");
    }
    out
}

#[test]
fn colored_repeated_text_is_cleaned_then_counted() {
    let raw = "\x1b[33mwarning: x\x1b[0m\n".repeat(3) + "done\n";
    let out = shape(&raw, &input(Some("cargo build"), Kind::Text, T));
    assert_eq!(out.text, "warning: x [repeated 3 times]\ndone\n");
    assert_eq!(out.steps, vec![Color, Counted]);
    assert_eq!(out.shaper, Some(Shaper::Dedup));
    assert!(!out.size_view);
}

#[test]
fn reuse_text_differs_from_raw_only_by_removed_escapes() {
    // O§16 item 9: by command, by sniffed kind with no command, json, jsonl
    for (raw, want, inp) in [
        (DIFF, DIFF_PLAIN, input(Some("git diff"), Kind::Text, T)),
        (DIFF, DIFF_PLAIN, input(None, Kind::Diff, T)),
        (JSON, JSON_PLAIN, input(None, Kind::Json, T)),
        (JSON, JSON_PLAIN, input(None, Kind::Jsonl, T)),
    ] {
        let out = shape(raw, &inp);
        assert_eq!(out.text, want, "{inp:?}");
        assert_eq!(out.steps, vec![Color], "{inp:?}");
        assert_eq!(out.shaper, Some(Shaper::Clean), "{inp:?}");
        assert!(!out.size_view, "{inp:?}");
    }
}

#[test]
fn same_diff_without_reuse_gets_steps_1_and_2_in_full() {
    let out = shape(DIFF, &input(Some("cargo build"), Kind::Text, T));
    assert_eq!(out.text, "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,3 @@\n-old\n+x [repeated 3 times]\n");
    assert_eq!(out.steps, vec![Color, Progress, Counted]);
    assert_eq!(out.shaper, Some(Shaper::Dedup));
}

#[test]
fn raw_over_max_bytes_gets_only_the_size_view() {
    let unit = "\x1b[31mx\x1b[0m\n";
    let raw = unit.repeat(SHAPE_MAX_BYTES / unit.len() + 1);
    assert!(raw.len() > SHAPE_MAX_BYTES);
    let out = shape(&raw, &input(None, Kind::Text, T));
    assert_eq!(out.text, raw);
    assert!(out.steps.is_empty());
    assert!(out.size_view);
    assert_eq!(out.shaper, Some(Shaper::Size));
}

#[test]
fn raw_of_exactly_max_bytes_is_shaped() {
    let unit = "\x1b[31mx\x1b[0m\n";
    let mut raw = unit.repeat(SHAPE_MAX_BYTES / unit.len());
    raw.push_str(&"y".repeat(SHAPE_MAX_BYTES - raw.len()));
    assert_eq!(raw.len(), SHAPE_MAX_BYTES);
    let out = shape(&raw, &input(None, Kind::Text, T));
    assert_eq!(out.steps, vec![Color, Counted]);
    assert!(!out.size_view);
}

#[test]
fn text_over_threshold_after_steps_1_2_needs_the_size_view() {
    let raw: String = (0..50).map(|i| format!("\x1b[2mline {i}\x1b[0m\n")).collect();
    let plain: String = (0..50).map(|i| format!("line {i}\n")).collect();
    let out = shape(&raw, &input(None, Kind::Text, 100));
    assert_eq!(out.text, plain);
    assert_eq!(out.steps, vec![Color]);
    assert_eq!(out.shaper, Some(Shaper::Clean));
    assert!(out.size_view);
    // at the threshold: not over
    assert!(!shape(&raw, &input(None, Kind::Text, plain.len() as u64)).size_view);
}

#[test]
fn a_step_with_no_change_lets_the_pipeline_go_on() {
    // O§7.10 "(operator)": dedup gives no change on a counted line; clean still applies
    let out = shape("\x1b[1mx [repeated 4 times]\x1b[0m\ny\ny\ny\n", &input(None, Kind::Text, T));
    assert_eq!(out.text, "x [repeated 4 times]\ny\ny\ny\n");
    assert_eq!(out.steps, vec![Color]);
    assert_eq!(out.shaper, Some(Shaper::Clean));
}

#[test]
fn unchanged_text_has_no_shaper_unless_over_threshold() {
    let out = shape("ok\n", &input(None, Kind::Text, T));
    assert_eq!(out, Shaped { text: "ok\n".into(), steps: vec![], shaper: None, size_view: false });
    let raw: String = (0..50).map(|i| format!("line {i}\n")).collect();
    let out = shape(&raw, &input(None, Kind::Text, 100));
    assert_eq!(out, Shaped { text: raw.clone(), steps: vec![], shaper: Some(Shaper::Size), size_view: true });
}
