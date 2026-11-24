//! Content pass (§16.2). Every flagged word here is an invented tracer: from a test `deny` list,
//! or from a test trie given to the classifier through `filter_with_trie`. No real word list.

use rustrict::{Trie, Type};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;
use zygo_config::{ContentConfig, RedactionConfig};
use zygo_core::Agent;
use zygo_redact::cache::{CachePool, SESSION_CAP_BYTES};
use zygo_redact::content::{admits, builtin_classifier_calls, filter, filter_with_trie, set_lookup_delay, ContentFindings, Layout, Shape, CLASSIFIER};
use zygo_redact::{compile, Compiled, SessionCache};

const R: &str = "🤬";
const CC: Shape = Shape { layout: Layout::Messages, agent: Agent::ClaudeCode };
const OC: Shape = Shape { layout: Layout::Messages, agent: Agent::OpenCode };
const RESP: Shape = Shape { layout: Layout::Input, agent: Agent::ClaudeCode };
const LONG: Duration = Duration::from_secs(10);

/// Invented words with chosen classifier categories and severities.
static TRIE: LazyLock<Trie> = LazyLock::new(|| {
    Trie::from_iter([
        ("zorblax", Type::PROFANE & Type::MODERATE),
        ("quibbet", Type::PROFANE & Type::MILD),
        ("vexmoor", Type::SEXUAL & Type::MODERATE),
        ("glimfen", Type::SEXUAL & Type::SEVERE),
        ("drabwick", Type::OFFENSIVE & Type::MODERATE),
        ("zib", Type::PROFANE & Type::MODERATE),
        ("zak", Type::PROFANE & Type::SEVERE),
    ])
});

fn compiled(deny: &[&str], allow: &[&str], replacement: &str) -> Compiled {
    let words = |l: &[&str]| l.iter().map(|w| w.to_string()).collect();
    let content = ContentConfig { replacement: replacement.into(), allow: words(allow), deny: words(deny) };
    compile(&RedactionConfig { pattern: vec![], content }).unwrap()
}
fn cache() -> SessionCache {
    SessionCache::new(CachePool::new(256 << 20, SESSION_CAP_BYTES))
}
fn run(body: &mut Value, shape: Shape, c: &Compiled) -> ContentFindings {
    filter(body, shape, c, &mut cache(), LONG)
}
fn user(content: Value) -> Value {
    json!({"messages": [{"role": "user", "content": content}]})
}
fn counts(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
    pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
}
fn fixture(rel: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}
/// A user message built from a q6 capture's `user_blocks`.
fn q6(name: &str) -> Value {
    let f = fixture(&format!("claude-code/q6/{name}.json"));
    let blocks: Vec<Value> = f["user_blocks"].as_array().unwrap().iter().map(|t| json!({"type": "text", "text": t})).collect();
    user(Value::Array(blocks))
}

#[test]
fn gate_admits_main_compaction_auxiliary_and_absent() {
    for class in [None, Some("main"), Some("compaction"), Some("auxiliary")] {
        assert!(admits(class), "{class:?}");
    }
    for class in [Some("subagent"), Some("other")] {
        assert!(!admits(class), "{class:?}");
    }
}

#[test]
fn messages_shape_filters_user_text_only() {
    let c = compiled(&["plimsoq"], &[], R);
    let orig = json!({
        "system": "plimsoq",
        "tools": [{"name": "t", "description": "plimsoq"}],
        "messages": [
            {"role": "user", "content": "say plimsoq"},
            {"role": "assistant", "content": [{"type": "text", "text": "plimsoq"}]},
            {"role": "user", "content": [
                {"type": "text", "text": "a plimsoq b"},
                {"type": "tool_result", "tool_use_id": "x", "content": [{"type": "text", "text": "plimsoq"}]},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "plimsoq"}},
                {"type": "thinking", "thinking": "plimsoq", "signature": "s"}
            ]},
            {"role": "system", "content": "plimsoq"}
        ]
    });
    let mut b = orig.clone();
    let f = run(&mut b, CC, &c);
    let mut want = orig.clone();
    want["messages"][0]["content"] = json!("say 🤬");
    want["messages"][2]["content"][0]["text"] = json!("a 🤬 b");
    assert_eq!(b, want);
    assert_eq!(f, ContentFindings { counts: counts(&[("content_profane", 2)]), changed: true });
}

#[test]
fn responses_shape_filters_user_input_only() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut b = json!({"input": "plimsoq"});
    run(&mut b, RESP, &c);
    assert_eq!(b, json!({"input": "🤬"}));
    let orig = json!({
        "instructions": "plimsoq",
        "input": [
            {"role": "user", "content": "plimsoq one"},
            {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "two plimsoq"},
                {"type": "input_image", "image_url": "plimsoq"}
            ]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "plimsoq"}]},
            {"type": "function_call_output", "call_id": "c", "output": "plimsoq"}
        ]
    });
    let mut b = orig.clone();
    let f = run(&mut b, RESP, &c);
    let mut want = orig.clone();
    want["input"][0]["content"] = json!("🤬 one");
    want["input"][1]["content"][0]["text"] = json!("two 🤬");
    assert_eq!(b, want);
    assert_eq!(f.counts, counts(&[("content_profane", 2)]));
}

#[test]
fn token_rule_letters_three_to_forty_outside_long_runs() {
    let c = compiled(&["plimsoq", "zov"], &[], R);
    let in64 = format!("{}/plimsoq", "a".repeat(56));
    let in65 = format!("{}/plimsoq", "a".repeat(57));
    assert_eq!((in64.len(), in65.len()), (64, 65));
    for (text, want) in [
        ("Plimsoq PLIMSOQ plimsoq.", "🤬 🤬 🤬."),
        ("zov", "🤬"),
        ("x-plimsoq-y path/plimsoq", "x-🤬-y path/🤬"),
        ("éplimsoq\tplimsoq\n", "é🤬\t🤬\n"),
        ("plimsoq_x plimsoq1 xplimsoq", "plimsoq_x plimsoq1 xplimsoq"),
        (in64.as_str(), &format!("{}/🤬", "a".repeat(56))),
        (in65.as_str(), in65.as_str()),
    ] {
        let mut b = user(json!(text));
        run(&mut b, CC, &c);
        assert_eq!(b["messages"][0]["content"], want, "{text}");
    }
    let c = compiled(&["plimsoq"], &[], "[x]");
    let mut b = user(json!("plimsoq"));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"], "[x]");
}

#[test]
fn harness_blocks_are_skipped() {
    let c = compiled(&["plimsoq"], &[], R);
    let prefixes = ["<system-reminder>", "<bash-stdout>", "<bash-stderr>", "<system-update>", "Stop hook feedback:", "SubagentStop hook feedback:"];
    let mut blocks: Vec<Value> = prefixes.iter().map(|p| json!({"type": "text", "text": format!("{p} plimsoq")})).collect();
    blocks.push(json!({"type": "text", "text": " <system-reminder> plimsoq"}));
    let mut b = user(Value::Array(blocks));
    let f = run(&mut b, CC, &c);
    for i in 0..prefixes.len() {
        assert!(b["messages"][0]["content"][i]["text"].as_str().unwrap().ends_with(" plimsoq"), "{i}");
    }
    assert_eq!(b["messages"][0]["content"][prefixes.len()]["text"], " <system-reminder> 🤬");
    assert_eq!(f.counts, counts(&[("content_profane", 1)]));
    let mut b = user(json!("<system-reminder>plimsoq"));
    assert!(!run(&mut b, CC, &c).changed);
    let mut b = json!({"input": "Stop hook feedback: plimsoq"});
    assert!(!run(&mut b, RESP, &c).changed);
}

#[test]
fn command_name_element_is_skipped() {
    let c = compiled(&["echo", "plimsoq"], &[], R);
    let mut b = q6("tracer-one");
    run(&mut b, CC, &c);
    assert_eq!(
        b["messages"][0]["content"][0]["text"],
        "<command-message>🤬-a</command-message>\n<command-name>/echo-a</command-name>\n<command-args>tracer one</command-args>\n"
    );
    // Outside a `<command-message>` block the element is typed text.
    let mut b = user(json!("<command-name>plimsoq</command-name> plimsoq"));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"], "<command-name>🤬</command-name> 🤬");
    let mut b = user(json!([{"type": "text", "text": "hi <command-name>plimsoq</command-name>"}]));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][0]["text"], "hi <command-name>🤬</command-name>");
}

#[test]
fn stop_hook_feedback_in_q1_capture_is_skipped() {
    let c = compiled(&["tracer"], &[], R);
    let orig = fixture("claude-code/requests/q1-continued-subagent-request.json")["body"].clone();
    let mut b = orig.clone();
    run(&mut b, CC, &c);
    let mut want = orig.clone();
    want["messages"][0]["content"][2]["text"] = json!("🤬-q1-sub report back");
    assert_eq!(b, want);
}

#[test]
fn command_arguments_filtered_in_q6_captures() {
    let c = compiled(&["tracer", "body"], &[], R);
    let mut b = q6("tracer-one");
    let f = run(&mut b, CC, &c);
    let m = &b["messages"][0]["content"];
    assert_eq!(m[0]["text"], "<command-message>echo-a</command-message>\n<command-name>/echo-a</command-name>\n<command-args>🤬 one</command-args>\n");
    assert_eq!(m[1]["text"], "Command A body. Arguments here: 🤬 one. End of A.\n");
    assert_eq!(f.counts, counts(&[("content_profane", 2)]));
    let mut b = q6("tracer-two");
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][1]["text"], "Command B body without a placeholder.\n\n\nARGUMENTS: 🤬 two");
    // Only the block directly after `<command-message>` is an expanded command file.
    let mut b = q6("tracer-one");
    b["messages"][0]["content"].as_array_mut().unwrap().push(json!({"type": "text", "text": "body"}));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][2]["text"], "🤬");
    // Only the block directly after: a harness block there leaves the next block plain user text.
    let mut b = q6("tracer-one");
    b["messages"][0]["content"].as_array_mut().unwrap().insert(1, json!({"type": "text", "text": "<system-reminder>body</system-reminder>"}));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][1]["text"], "<system-reminder>body</system-reminder>");
    assert_eq!(b["messages"][0]["content"][2]["text"], "Command A 🤬. Arguments here: 🤬 one. End of A.\n");
    // The title request (auxiliary) carries the typed command line as plain user text.
    let mut b = q6("tracer-three");
    run(&mut b, CC, &c);
    assert!(b["messages"][0]["content"][0]["text"].as_str().unwrap().starts_with("<session>\n/echo-a 🤬 three\n"));
}

#[test]
fn positional_arguments_are_found_by_whole_token() {
    // Command file "Use $1, then $2. Body $1s." with arguments `plimsoq zorp`.
    let c = compiled(&["plimsoq", "body"], &[], R);
    let mut b = user(json!([
        {"type": "text", "text": "<command-message>pos</command-message>\n<command-name>/pos</command-name>\n<command-args>plimsoq zorp</command-args>\n"},
        {"type": "text", "text": "Use plimsoq, then zorp. Body plimsoqs."}
    ]));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][1]["text"], "Use 🤬, then zorp. Body plimsoqs.");
}

fn command(args: &str, expanded: &str) -> Value {
    user(json!([
        {"type": "text", "text": format!("<command-message>x</command-message>\n<command-args>{args}</command-args>")},
        {"type": "text", "text": expanded}
    ]))
}

#[test]
fn whole_arguments_and_positional_copies_are_both_filtered() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut b = command("plimsoq zorp", "All: plimsoq zorp. First: plimsoq.");
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][1]["text"], "All: 🤬 zorp. First: 🤬.");
}

#[test]
fn argument_tokens_lose_one_layer_of_shell_quotes() {
    let c = compiled(&["plimsoq"], &[], R);
    for args in ["\"plimsoq zorp\" y", "'plimsoq zorp' y", "\"plimsoq\" \"\" ''"] {
        let mut b = command(args, "First: plimsoq zorp.");
        run(&mut b, CC, &c);
        assert_eq!(b["messages"][0]["content"][1]["text"], "First: 🤬 zorp.", "{args}");
    }
}

#[test]
fn arguments_line_counts_only_when_it_holds_the_typed_arguments() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut b = command("zorp", "Body.\nARGUMENTS: plimsoq");
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][1]["text"], "Body.\nARGUMENTS: plimsoq");
    let mut b = command(" plimsoq ", "Body.\nARGUMENTS: plimsoq");
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][1]["text"], "Body.\nARGUMENTS: 🤬");
}

#[test]
fn only_the_block_directly_after_command_message_is_expanded() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut b = user(json!([
        {"type": "text", "text": "<command-message>x</command-message>\n<command-args>zorp</command-args>"},
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}},
        {"type": "text", "text": "plimsoq zorp"}
    ]));
    run(&mut b, CC, &c);
    assert_eq!(b["messages"][0]["content"][2]["text"], "🤬 zorp");
}

#[test]
#[ignore = "timing: asserts the pass ends within 1500 ms of wall-clock time"]
fn huge_argument_string_times_out_instead_of_overrunning() {
    let c = compiled(&["plimsoq"], &[], R);
    let args: String = (0..200_000u32).map(|i| format!("t{i} ")).collect();
    let mut k = cache();
    // The `<command-message>` block (plain text, no classifier work) is cached first.
    let mut first = user(json!([{"type": "text", "text": format!("<command-message>x</command-message>\n<command-args>{args}</command-args>")}]));
    filter(&mut first, CC, &c, &mut k, LONG);
    let mut b = command(&args, "Use t1 and t2. plimsoq");
    let orig = b.clone();
    let t = std::time::Instant::now();
    let f = filter(&mut b, CC, &c, &mut k, Duration::ZERO);
    let took = t.elapsed();
    assert_eq!((b == orig, f.counts), (true, counts(&[("content_timeout", 1)])));
    // Cutting the deadline charge for the argument words makes this take seconds.
    assert!(took < Duration::from_millis(1500), "{took:?}");
}

#[test]
fn a_new_string_repeated_in_one_body_is_classified_once() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut k = cache();
    let mut b = user(json!([{"type": "text", "text": "a plimsoq"}, {"type": "text", "text": "a plimsoq"}]));
    let f = filter(&mut b, CC, &c, &mut k, LONG);
    assert_eq!(k.scan_count(), 1);
    assert_eq!(b["messages"][0]["content"][1]["text"], "a 🤬");
    assert_eq!(f.counts, counts(&[("content_profane", 2)]));
}

#[test]
fn written_bytes_count_toward_the_64_kib_check() {
    // About 9 KiB scanned, 70 KiB written: only the written bytes cross a check.
    let long = "x".repeat(64);
    let c = compiled(&["plimsoq"], &[], &long);
    let orig = user(json!("plimsoq ".repeat(1100)));
    let mut b = orig.clone();
    let f = filter(&mut b, CC, &c, &mut cache(), Duration::ZERO);
    assert_eq!((b == orig, f.counts), (true, counts(&[("content_timeout", 1)])));
}

#[test]
fn runs_are_measured_across_range_edges() {
    let c = compiled(&["plimsoq"], &[], R);
    let cmd = json!({"type": "text", "text": "<command-message>x</command-message>\n<command-args>plimsoq</command-args>"});
    for (pad, want) in [(56, "🤬"), (57, "plimsoq")] {
        let text = format!("Args: {}/plimsoq", "a".repeat(pad));
        let mut b = user(json!([cmd.clone(), {"type": "text", "text": text}]));
        run(&mut b, CC, &c);
        assert_eq!(b["messages"][0]["content"][1]["text"], format!("Args: {}/{want}", "a".repeat(pad)), "{pad}");
    }
}

#[test]
fn opencode_file_and_shell_spans_are_skipped_for_opencode_only() {
    let c = compiled(&["plimsoq"], &[], R);
    let text = "see <file path=\"a.rs\">plimsoq</file> and plimsoq <shell cmd=\"ls\">plimsoq\n</shell> <file a> plimsoq";
    let mut b = user(json!([{"type": "text", "text": text}]));
    run(&mut b, OC, &c);
    assert_eq!(
        b["messages"][0]["content"][0]["text"],
        "see <file path=\"a.rs\">plimsoq</file> and 🤬 <shell cmd=\"ls\">plimsoq\n</shell> <file a> 🤬"
    );
    let mut b = user(json!([{"type": "text", "text": text}]));
    let f = run(&mut b, CC, &c);
    assert_eq!(f.counts, counts(&[("content_profane", 4)]));
    // A second open tag before the close restarts the span.
    let mut b = user(json!("<file a> plimsoq <file b>plimsoq</file> <shell x><shell y>plimsoq</shell> plimsoq"));
    run(&mut b, OC, &c);
    assert_eq!(b["messages"][0]["content"], "<file a> 🤬 <file b>plimsoq</file> <shell x><shell y>plimsoq</shell> 🤬");
}

#[test]
fn captured_requests_change_only_the_typed_text() {
    let c = compiled(&["answer", "tracer"], &[], R);
    let orig = fixture("claude-code/requests/p2-direct-claude-haiku-4-5.json")["body"].clone();
    let mut b = orig.clone();
    let f = run(&mut b, CC, &c);
    let mut want = orig.clone();
    want["messages"][0]["content"][8]["text"] = json!("🤬-p2-direct-claude-haiku-4-5");
    assert_eq!(b, want);
    assert_eq!(f.counts, counts(&[("content_profane", 1)]));
    let orig = fixture("claude-code/requests/smoke.json")["body"].clone();
    let mut b = orig.clone();
    let f = run(&mut b, CC, &c);
    assert_eq!((b, f), (orig, ContentFindings::default()));
}

#[test]
fn classifier_thresholds_whole_word_and_categories() {
    let c = compiled(&[], &[], R);
    let mut b = user(json!("zorblax quibbet vexmoor glimfen drabwick zib zak zorblaxer Zorblax hello"));
    let f = filter_with_trie(&mut b, CC, &c, &mut cache(), LONG, &TRIE);
    assert_eq!(b["messages"][0]["content"], "🤬 quibbet vexmoor 🤬 🤬 zib 🤬 zorblaxer 🤬 hello");
    assert_eq!(f.counts, counts(&[("content_offensive", 1), ("content_profane", 3), ("content_sexual", 1)]));
}

#[test]
fn deny_then_allow_then_classifier() {
    let c = compiled(&["quibbet"], &["zorblax"], R);
    let mut b = user(json!("zorblax quibbet glimfen"));
    filter_with_trie(&mut b, CC, &c, &mut cache(), LONG, &TRIE);
    assert_eq!(b["messages"][0]["content"], "zorblax 🤬 🤬");
}

#[test]
fn built_in_classifier_passes_ordinary_text() {
    let c = compiled(&[], &[], R);
    let mut b = user(json!("please refactor the parser, then run the tests and commit"));
    assert!(!run(&mut b, CC, &c).changed);
}

#[test]
fn built_in_classifier_called_for_new_tokens_only() {
    let c = compiled(&["plimsoq"], &["refactor"], R);
    let mut k = cache();
    let orig = user(json!("please refactor plimsoq parser parser"));
    let n0 = builtin_classifier_calls();
    filter(&mut orig.clone(), CC, &c, &mut k, LONG);
    // "please" and "parser" once each; not the deny word, the allow word or a repeat.
    assert_eq!(builtin_classifier_calls() - n0, 2);
    filter(&mut orig.clone(), CC, &c, &mut k, LONG);
    assert_eq!(builtin_classifier_calls() - n0, 2);
}

#[test]
fn classifier_version_matches_lockfile() {
    let lock = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock")).unwrap();
    let v = CLASSIFIER.strip_prefix("rustrict ").unwrap();
    assert!(lock.contains(&format!("name = \"rustrict\"\nversion = \"{v}\"\n")), "update content::CLASSIFIER");
}

#[test]
fn session_cache_reuses_results_byte_for_byte() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut k = cache();
    let orig = user(json!([{"type": "text", "text": "a plimsoq"}, {"type": "text", "text": "clean text"}]));
    let (mut b1, mut b2) = (orig.clone(), orig.clone());
    let f1 = filter(&mut b1, CC, &c, &mut k, LONG);
    assert_eq!(k.scan_count(), 2);
    let f2 = filter(&mut b2, CC, &c, &mut k, LONG);
    assert_eq!(k.scan_count(), 2);
    assert_eq!(serde_json::to_vec(&b1).unwrap(), serde_json::to_vec(&b2).unwrap());
    assert_eq!(f1, f2);
    let mut b3 = user(json!("clean text"));
    assert!(!filter(&mut b3, CC, &c, &mut k, LONG).changed);
    assert_eq!(k.scan_count(), 2);
    let c2 = compiled(&["clean"], &[], R);
    let mut b4 = orig.clone();
    filter(&mut b4, CC, &c2, &mut k, LONG);
    assert_eq!(k.scan_count(), 4);
    assert_eq!(b4["messages"][0]["content"][1]["text"], "🤬 text");
}

#[test]
fn harness_context_is_part_of_the_cache_key() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut k = cache();
    let mut b = user(json!([{"type": "text", "text": "<command-message>x</command-message>"}, {"type": "text", "text": "Body plimsoq"}]));
    filter(&mut b, CC, &c, &mut k, LONG);
    assert_eq!(b["messages"][0]["content"][1]["text"], "Body plimsoq");
    let mut b = user(json!([{"type": "text", "text": "Body plimsoq"}]));
    filter(&mut b, CC, &c, &mut k, LONG);
    assert_eq!(b["messages"][0]["content"][0]["text"], "Body 🤬");
    assert_eq!(k.scan_count(), 3);
}

#[test]
fn timeout_at_64_kib_mid_body_fails_open_and_is_not_cached() {
    let c = compiled(&["plimsoq"], &[], R);
    let big = format!("plimsoq {}", "word ".repeat(14_000));
    let orig = json!({"messages": [{"role": "user", "content": "first plimsoq"}, {"role": "user", "content": big}]});
    let mut k = cache();
    let mut b = orig.clone();
    let f = filter(&mut b, CC, &c, &mut k, Duration::ZERO);
    assert_eq!(b["messages"][0]["content"], "first 🤬");
    assert_eq!(b["messages"][1], orig["messages"][1]);
    assert_eq!(f, ContentFindings { counts: counts(&[("content_profane", 1), ("content_timeout", 1)]), changed: true });
    assert_eq!(k.scan_count(), 2);
    let mut b = orig.clone();
    let f = filter(&mut b, CC, &c, &mut k, LONG);
    assert_eq!(k.scan_count(), 3);
    assert!(b["messages"][1]["content"].as_str().unwrap().starts_with("🤬 word word "));
    assert_eq!(f.counts, counts(&[("content_profane", 2)]));
}

#[test]
#[ignore = "timing: a 64 KiB filter must finish inside a 200 ms wall-clock budget"]
fn budget_clock_starts_after_the_cache_lookup() {
    let c = compiled(&["plimsoq"], &[], R);
    let mut k = cache();
    let msgs: Vec<Value> = (0..64).map(|i| json!({"role": "user", "content": format!("{i} {}", "lorem ipsum ".repeat(340))})).collect();
    let cached = json!({"messages": msgs});
    filter(&mut cached.clone(), CC, &c, &mut k, LONG);
    // A new string that crosses one 64 KiB check; the lookup phase alone outlasts the budget.
    let mut b = cached.clone();
    b["messages"].as_array_mut().unwrap().push(json!({"role": "user", "content": format!("plimsoq {}", "word ".repeat(14_000))}));
    set_lookup_delay(Duration::from_millis(400));
    let f = filter(&mut b, CC, &c, &mut k, Duration::from_millis(200));
    set_lookup_delay(Duration::ZERO);
    assert_eq!(f.counts, counts(&[("content_profane", 1)]));
    assert!(b["messages"][64]["content"].as_str().unwrap().starts_with("🤬 word "));
}

#[test]
fn timeout_after_256_classifications() {
    let c = compiled(&["plimsoq"], &[], R);
    let words: String = (0..300u32).map(|i| format!("qa{}{} ", char::from(b'a' + (i / 26) as u8), char::from(b'a' + (i % 26) as u8))).collect();
    let orig = user(json!(format!("{words}plimsoq")));
    let mut k = cache();
    for n in 1..=2 {
        let mut b = orig.clone();
        let f = filter(&mut b, CC, &c, &mut k, Duration::ZERO);
        assert_eq!((b == orig, f), (true, ContentFindings { counts: counts(&[("content_timeout", 1)]), changed: false }));
        assert_eq!(k.scan_count(), n);
    }
    let mut b = orig.clone();
    let f = filter(&mut b, CC, &c, &mut k, LONG);
    assert_eq!(b["messages"][0]["content"], format!("{words}🤬"));
    assert_eq!(f.counts, counts(&[("content_profane", 1)]));
    // The check runs at the 256th classification, not before: 255 new words finish under a zero budget, 256 do not.
    for (n, done) in [(255u32, true), (256, false)] {
        let words: String = (0..n).map(|i| format!("qb{}{} ", char::from(b'a' + (i / 26) as u8), char::from(b'a' + (i % 26) as u8))).collect();
        let mut b = user(json!(format!("{words}plimsoq")));
        let f = filter(&mut b, CC, &c, &mut cache(), Duration::ZERO);
        assert_eq!(f.counts.contains_key("content_timeout"), !done, "{n}");
        assert_eq!(b["messages"][0]["content"] == format!("{words}🤬"), done, "{n}");
    }
}

#[test]
fn expanded_file_tests_only_words_of_the_typed_arguments() {
    // A token of the expanded file is tested only when it equals (any case) a token of the typed
    // arguments; quotes, escapes, punctuation and the `ARGUMENTS:` line do not matter.
    let c = compiled(&["zorp"], &[], "XXXX");
    for (args, text, want) in [
        ("x-y y-zorp", "Pre x-y-zorp post.", "Pre x-y-XXXX post."),
        ("-zorp foo", "run-zorp on foo.", "run-XXXX on foo."),
        ("zorp- foo", "use zorp-x and foo.", "use XXXX-x and foo."),
        ("zorp foo", "Use zorp here.\n\n\nARGUMENTS: zorp foo", "Use XXXX here.\n\n\nARGUMENTS: XXXX foo"),
        ("zorp\\ foo bar", "First: zorp foo. Then bar.", "First: XXXX foo. Then bar."),
        ("ZORP", "Use Zorp.", "Use XXXX."),
        ("zorp", "The zorpish file text.", "The zorpish file text."),
        ("foo", "The zorp file text, foo.", "The zorp file text, foo."),
    ] {
        let mut b = command(args, text);
        run(&mut b, CC, &c);
        assert_eq!(b["messages"][0]["content"][1]["text"], want, "{args:?} {text:?}");
    }
}

#[test]
fn a_command_message_block_is_never_an_expanded_file() {
    let c = compiled(&["zorp"], &[], "XXXX");
    let cm = |a: &str| json!({"type": "text", "text": format!("<command-message>{a}</command-message>\n<command-name>/{a}</command-name>\n<command-args>{a}</command-args>")});
    let mut b = user(json!([cm("foo"), cm("zorp"), {"type": "text", "text": "Body of b: zorp"}]));
    let f = run(&mut b, CC, &c);
    let m = &b["messages"][0]["content"];
    assert_eq!(m[1]["text"], "<command-message>XXXX</command-message>\n<command-name>/zorp</command-name>\n<command-args>XXXX</command-args>");
    assert_eq!(m[2]["text"], "Body of b: XXXX");
    assert_eq!(f.counts, counts(&[("content_profane", 3)]));
}

#[test]
#[ignore = "timing: asserts the pass ends within 3000 ms of wall-clock time"]
fn many_distinct_argument_lengths_stay_linear() {
    // 3000 distinct token lengths (about 4.5 MB of arguments) and an 8 MB file: the pass finishes
    // or times out at a short budget; it is never superlinear in the argument count.
    let c = compiled(&["zorp"], &[], R);
    let mut args: String = (0..3000).map(|i| format!("{} ", "b".repeat(3 + i))).collect();
    args.push_str("zorp");
    let mut b = command(&args, &format!("{}zorp", "! ".repeat(4_000_000)));
    let t = std::time::Instant::now();
    filter(&mut b, CC, &c, &mut cache(), Duration::from_millis(200));
    let took = t.elapsed();
    assert!(took < Duration::from_millis(3000), "{took:?}");
}

#[test] // N68 R4: the lookup-delay seams of both passes exist only with the `test-hooks` feature
fn lookup_delay_seams_are_test_hooks_only() {
    for file in ["content.rs", "secret.rs"] {
        let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)).unwrap();
        let lines: Vec<&str> = src.lines().collect();
        let mut seen = 0;
        for (i, l) in lines.iter().enumerate() {
            if l.contains("static LOOKUP_DELAY") || l.contains("fn set_lookup_delay") || l.contains("LOOKUP_DELAY.with(Cell::get)") {
                seen += 1;
                let gated = lines[i.saturating_sub(3)..i].iter().any(|p| p.contains(r#"#[cfg(feature = "test-hooks")]"#));
                assert!(gated, "{file}:{}: `{}` is not behind `test-hooks`", i + 1, l.trim());
            }
        }
        assert_eq!(seen, 3, "{file}");
    }
}
