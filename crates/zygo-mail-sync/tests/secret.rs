//! MS-7: the secret pass on one message (S§8.6, S-G4, S§19 #4 and #5). No test depends on wall-clock time.
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use zygo_config::{builtin_patterns, Action, RedactionConfig, RedactionPattern};
use zygo_mail_sync::secret::{check_with, Outcome};
use zygo_redact::act_lite_store::{store_path, Lookup};
use zygo_redact::DeadlinePolicy;

/// An hour of budget: a test not about time never times out under machine load.
fn far() -> DeadlinePolicy {
    DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() }
}

fn aws() -> String {
    ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat()
}

fn builtin() -> RedactionConfig {
    RedactionConfig { pattern: builtin_patterns(), ..Default::default() }
}

/// An M§5 message object: six string fields.
fn msg(body: &str) -> Value {
    json!({"version": 1, "id": "01JD0000000000000000000000", "from": "lead", "to": "dev",
           "sent_at": "2026-11-24T10:00:00Z", "subject": "status", "body": body})
}

/// A main worktree and a store folder in one temporary directory; `ACT_CONFIG_DIR` points at `<tmp>/act`.
fn fixture() -> (TempDir, PathBuf, Lookup) {
    let t = TempDir::new().unwrap();
    let root = t.path().join("repo");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let act: OsString = t.path().join("act").into_os_string();
    let lookup = Lookup { env: Box::new(move |k| (k == "ACT_CONFIG_DIR").then(|| act.clone())), ..Lookup::system() };
    (t, root, lookup)
}

fn write_store(root: &Path, lookup: &Lookup, text: &str) -> PathBuf {
    let p = store_path(root, lookup);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

#[test]
fn aws_key_is_a_finding_with_its_pattern_id() {
    let (_t, root, lookup) = fixture();
    let m = msg(&format!("the key is {}", aws()));
    let o = check_with(&m, &builtin(), &root, &far(), &lookup);
    assert_eq!(o, Outcome::Finding(vec!["aws_access_key_id".to_string()]));
    assert!(!format!("{o:?}").contains(&aws()));
}

#[test]
fn clean_message_is_clear_and_unchanged() {
    let (_t, root, lookup) = fixture();
    let m = msg("the build is green");
    let before = m.clone();
    assert_eq!(check_with(&m, &builtin(), &root, &far(), &lookup), Outcome::Clear);
    assert_eq!(m, before);
}

#[test]
fn finding_leaves_the_input_unchanged() {
    let (_t, root, lookup) = fixture();
    let m = msg(&format!("the key is {}", aws()));
    let before = m.clone();
    let _ = check_with(&m, &builtin(), &root, &far(), &lookup);
    assert_eq!(m, before);
}

#[test]
fn expire_after_zero_scans_could_not_run() {
    let (_t, root, lookup) = fixture();
    let p = DeadlinePolicy { base: Duration::from_secs(3600), expire_after_scans: Some(0), ..Default::default() };
    assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &p, &lookup), Outcome::CouldNotRun("secret_timeout"));
}

#[test]
fn store_that_does_not_parse_could_not_run() {
    let (_t, root, lookup) = fixture();
    write_store(&root, &lookup, "not an assignment\n");
    assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::CouldNotRun("act_lite_store"));
}

#[test]
fn store_that_cannot_be_read_could_not_run() {
    let (_t, root, lookup) = fixture();
    std::fs::create_dir_all(store_path(&root, &lookup)).unwrap(); // a folder where the file goes
    assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::CouldNotRun("act_lite_store"));
}

#[test]
fn no_store_file_and_clean_message_is_clear() {
    let (_t, root, lookup) = fixture();
    assert!(!store_path(&root, &lookup).exists());
    assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::Clear);
}

#[test]
fn no_store_file_still_finds_regex_patterns() {
    let (_t, root, lookup) = fixture();
    let m = msg(&format!("the key is {}", aws()));
    assert_eq!(check_with(&m, &builtin(), &root, &far(), &lookup), Outcome::Finding(vec!["aws_access_key_id".to_string()]));
}

#[test]
fn store_value_is_a_finding_without_its_text() {
    let (_t, root, lookup) = fixture();
    let value = ["s3cr3t", "-value-", "4417"].concat();
    write_store(&root, &lookup, &format!("TOKEN={value}\n"));
    let o = check_with(&msg(&format!("use {value} now")), &builtin(), &root, &far(), &lookup);
    assert_eq!(o, Outcome::Finding(vec!["act_lite_secret".to_string()]));
    assert!(!format!("{o:?}").contains(&value));
}

#[test]
fn block_pattern_is_a_finding() {
    let (_t, root, lookup) = fixture();
    let cfg = RedactionConfig {
        pattern: vec![RedactionPattern { id: "ship_word".into(), pattern: "zz-forbidden-zz".into(), action: Action::Block, description: "d".into() }],
        ..Default::default()
    };
    assert_eq!(check_with(&msg("say zz-forbidden-zz"), &cfg, &root, &far(), &lookup), Outcome::Finding(vec!["ship_word".to_string()]));
}

#[test]
fn pattern_that_does_not_compile_could_not_run() {
    let (_t, root, lookup) = fixture();
    let cfg = RedactionConfig {
        pattern: vec![RedactionPattern { id: "bad".into(), pattern: "(".into(), action: Action::Redact, description: "d".into() }],
        ..Default::default()
    };
    assert_eq!(check_with(&msg("the build is green"), &cfg, &root, &far(), &lookup), Outcome::CouldNotRun("redaction_patterns"));
}

fn aws_finding() -> Outcome {
    Outcome::Finding(vec!["aws_access_key_id".to_string()])
}

fn check_value(v: &Value) -> Outcome {
    let (_t, root, lookup) = fixture();
    check_with(v, &builtin(), &root, &far(), &lookup)
}

#[test]
fn secret_in_subject_is_a_finding() {
    let mut m = msg("hi");
    m["subject"] = Value::String(format!("key {}", aws()));
    assert_eq!(check_value(&m), aws_finding());
}

#[test]
fn secret_in_unknown_root_field_is_a_finding() {
    let mut m = msg("hi");
    m["x_extra"] = Value::String(aws());
    assert_eq!(check_value(&m), aws_finding());
}

#[test]
fn secret_as_object_key_is_a_finding() {
    let mut map = serde_json::Map::new();
    map.insert(aws(), json!("x"));
    map.insert("body".into(), json!("hi"));
    assert_eq!(check_value(&Value::Object(map)), aws_finding());
}

#[test]
fn secret_in_input_reasoning_path_is_a_finding() {
    let v = json!({"input": [{"type": "reasoning", "summary": aws()}]});
    assert_eq!(check_value(&v), aws_finding());
}

#[test]
fn secret_in_messages_thinking_path_is_a_finding() {
    let v = json!({"messages": [{"content": [{"type": "thinking", "thinking": aws(), "signature": "s"}]}]});
    assert_eq!(check_value(&v), aws_finding());
}

#[test]
fn secret_nested_in_array_of_objects_is_a_finding() {
    let v = json!({"body": "hi", "parts": [{"a": "x"}, {"b": {"c": [{"d": aws()}]}}]});
    assert_eq!(check_value(&v), aws_finding());
}

#[test]
fn block_and_redact_matches_report_both_ids() {
    let (_t, root, lookup) = fixture();
    let mut cfg = builtin();
    cfg.pattern.push(RedactionPattern { id: "ship_word".into(), pattern: "zz-forbidden-zz".into(), action: Action::Block, description: "d".into() });
    let m = msg(&format!("say zz-forbidden-zz with {}", aws()));
    assert_eq!(
        check_with(&m, &cfg, &root, &far(), &lookup),
        Outcome::Finding(vec!["aws_access_key_id".to_string(), "ship_word".to_string()])
    );
}

/// A dangling symlink at the store path fails closed: an implementation that follows the link would see "no store".
#[cfg(unix)]
#[test]
fn dangling_store_symlink_could_not_run() {
    let (t, root, lookup) = fixture();
    let p = store_path(&root, &lookup);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(t.path().join("missing-target"), &p).unwrap();
    assert!(!p.exists());
    assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::CouldNotRun("act_lite_store"));
}

/// 65535 config patterns compile alone; the store pattern makes 65536, over zygo-redact's u16 limit,
/// so the failure is the store's (S§8.6 check name `act_lite_store`), not the patterns'.
#[test]
fn compile_failure_caused_by_the_store_is_a_store_failure() {
    let (_t, root, lookup) = fixture();
    write_store(&root, &lookup, "TOKEN=some-stored-value-1\n");
    let cfg = RedactionConfig {
        pattern: (0..65535).map(|i| RedactionPattern { id: format!("p{i}"), pattern: "zq".into(), action: Action::Redact, description: String::new() }).collect(),
        ..Default::default()
    };
    assert_eq!(check_with(&msg("the build is green"), &cfg, &root, &far(), &lookup), Outcome::CouldNotRun("act_lite_store"));
}
