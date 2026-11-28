//! CC-4: the content pass skips the harness spans that the client's descriptor names (K§6.1.2 concern 9).

use zygo_core::Agent;
use zygo_redact::harness::skipped_spans;

#[test]
fn skipped_spans_take_the_descriptor_tags() {
    let oc: Agent = "opencode".parse().unwrap();
    let cc: Agent = "claude-code".parse().unwrap();
    let s = r#"<file path="a">x</file> <shell>y</shell> z"#;
    assert_eq!(skipped_spans(s, oc.spec().redact_skip_tags), vec![0..23, 24..40]);
    assert!(skipped_spans(s, cc.spec().redact_skip_tags).is_empty());
    let cmd = "<command-message>m</command-message><command-name>/x</command-name>";
    for a in [oc, cc] {
        assert_eq!(skipped_spans(cmd, a.spec().redact_skip_tags), vec![36..67], "{a}");
    }
}
