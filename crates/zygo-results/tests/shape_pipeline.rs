use zygo_results::Kind;
use zygo_results::shape::{self, clean::clean, command::parse, ShapeInput, Shaped, Shaper, Step, SHAPE_MAX_BYTES};
use Step::{ColorCodesRemoved as Color, ProgressLinesRemoved as Progress, RepeatedLinesCounted as Counted};

const T: u64 = 16384; // `trim_threshold_bytes` default
const DIFF: &str = "\x1b[1mdiff --git a/f b/f\x1b[0m\n--- a/f\n+++ b/f\n@@ -1 +1,3 @@\n-old\r\n\x1b[32m+x\x1b[0m\n\x1b[32m+x\x1b[0m\n\x1b[32m+x\x1b[0m\n";
const DIFF_PLAIN: &str = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,3 @@\n-old\r\n+x\n+x\n+x\n";
const JSON: &str = "[\n  \x1b[0;39m1\x1b[0m,\n  1,\n  1,\n  1\n]\n";
const JSON_PLAIN: &str = "[\n  1,\n  1,\n  1,\n  1\n]\n";
const JSONL: &str = "\x1b[1m{\"a\":1}\x1b[0m\r\n{\"a\":1}\n{\"a\":1}\n";
const JSONL_PLAIN: &str = "{\"a\":1}\r\n{\"a\":1}\n{\"a\":1}\n";

fn input(cmd: Option<&str>, kind: Kind, threshold: u64) -> ShapeInput {
    ShapeInput { parsed: cmd.map(parse), kind, threshold }
}

/// The reuse decision of `shape::run` (O§7.3).
fn reuse_of(inp: &ShapeInput) -> bool {
    inp.parsed.as_ref().is_some_and(|p| p.reuse) || matches!(inp.kind, Kind::Diff | Kind::Json | Kind::Jsonl)
}

/// O-C9 line walk, in order, one cursor over the source lines. The source is
/// `clean(raw, reuse)` when it changes the text, else `raw`; `raw` when over `SHAPE_MAX_BYTES`.
/// Each output line is the source line at the cursor (advance 1), or
/// `<line> [repeated N times]` where the N source lines from the cursor equal `<line>` (advance N).
/// At the end the cursor is at the end of the source.
fn walk_ok(raw: &str, reuse: bool, out: &str) -> bool {
    let cleaned = (raw.len() <= SHAPE_MAX_BYTES).then(|| clean(raw, reuse)).flatten().map(|c| c.0);
    let src: Vec<&str> = cleaned.as_deref().unwrap_or(raw).split('\n').collect();
    let mut i = 0;
    for line in out.split('\n') {
        if src.get(i) == Some(&line) {
            i += 1;
            continue;
        }
        let counted = line.strip_suffix(" times]").and_then(|l| l.rsplit_once(" [repeated "))
            .and_then(|(b, d)| Some((b, d.parse::<usize>().ok()?)));
        match counted {
            Some((base, n)) if n > 0 && src.get(i..i + n).is_some_and(|run| run.iter().all(|l| *l == base)) => i += n,
            _ => return false,
        }
    }
    i == src.len()
}

/// Runs the pipeline, then asserts the O-C9 line walk on its output.
fn shape(raw: &str, inp: &ShapeInput) -> Shaped {
    let out = shape::run(raw, inp);
    assert!(walk_ok(raw, reuse_of(inp), &out.text), "{:?}", out.text);
    out
}

#[test]
fn line_walk_rejects_wrong_outputs() {
    for wrong in [
        "diff --git a/f b/f",
        "",
        "diff --git a/f b/f\n+x [repeated 99 times]\n",
        "+x\n--- a/f\ndiff --git a/f b/f\n",
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,3 @@\n-old\n+x [repeated 3 times]\n",
    ] {
        assert!(!walk_ok(DIFF, true, wrong), "{wrong:?}");
    }
    assert!(walk_ok(DIFF, true, DIFF_PLAIN));
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
        (JSONL, JSONL_PLAIN, input(None, Kind::Jsonl, T)),
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
    let n = SHAPE_MAX_BYTES / unit.len();
    assert_eq!(out.text, format!("x [repeated {n} times]\n{}", "y".repeat(SHAPE_MAX_BYTES - n * unit.len())));
    assert_eq!(out.steps, vec![Color, Counted]);
    assert_eq!(out.shaper, Some(Shaper::Dedup));
    assert!(!out.size_view);
}

#[test]
fn raw_over_max_bytes_under_threshold_is_untouched() {
    let unit = "\x1b[31mx\x1b[0m\n";
    let mut raw = unit.repeat(SHAPE_MAX_BYTES / unit.len() + 1);
    raw.truncate(SHAPE_MAX_BYTES + 2);
    assert_eq!(raw.len(), SHAPE_MAX_BYTES + 2);
    let out = shape(&raw, &input(None, Kind::Text, u64::MAX));
    assert_eq!(out, Shaped { text: raw.clone(), steps: vec![], shaper: None, size_view: false });
}

#[test]
fn threshold_edges() {
    // one byte over
    let out = shape("ab", &input(None, Kind::Text, 1));
    assert_eq!(out.shaper, Some(Shaper::Size));
    assert!(out.size_view);
    // threshold 0: empty text is not over; one byte is
    let out = shape("", &input(None, Kind::Text, 0));
    assert_eq!(out.shaper, None);
    assert!(!out.size_view);
    let out = shape("x", &input(None, Kind::Text, 0));
    assert_eq!(out.shaper, Some(Shaper::Size));
    assert!(out.size_view);
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
