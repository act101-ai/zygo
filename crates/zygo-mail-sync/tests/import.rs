//! MS-12: import (mail sync design S§8.5, S§6.2, S-C5, S-C8; S§19 #12, #13, #14, #15). No test depends on wall-clock time.
#![cfg(unix)]

mod common;

use common::*;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use ulid::Ulid;
use zygo_core::paths::Paths;
use zygo_mail_sync::branch::ensure_worktree;
use zygo_mail_sync::env::{Crash, NoSeam, Seam};
use zygo_mail_sync::import::*;
use zygo_mail_sync::state::{imported_append, imported_load, SyncPaths};

const KEY: &str = "kbbbbbbbb"; // bob's pane key
fn u(n: u64) -> Ulid { Ulid::from_parts(1_764_000_000_000 + n, n as u128) }
fn file(id: Ulid) -> String { format!("{id}.json") }
/// A valid message from alice@a.
fn msg(id: Ulid, to: &str) -> Vec<u8> {
    format!(r#"{{"version":1,"id":"{id}","from":"alice@a","to":"{to}","sent_at":"2026-11-25T10:00:00Z","subject":"s","body":"b"}}"#).into_bytes()
}
/// Odd key order, whitespace, an unknown key and a final newline.
fn odd(id: Ulid) -> Vec<u8> {
    format!("{{ \"body\" : \"b\",\n  \"to\":\"bob@b\", \"x-extra\": [1, {{\"k\": null}}],\t\"version\": 1, \"subject\":\"s\",\"id\":\"{id}\",\"from\":\"alice@a\",\"sent_at\":\"2026-11-25T10:00:00Z\" }}\n").into_bytes()
}
/// check_message for a 100644 blob under bob@b named by `id`, with its real size.
fn check(id: Ulid, bytes: &[u8]) -> Result<(), &'static str> {
    check_message("bob@b", &file(id), "100644", "blob", bytes.len() as u64, bytes)
}
/// msg(u(1), "bob@b") with `field` set to `value` (serde_json in the test only).
fn with(field: &str, value: serde_json::Value) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_slice(&msg(u(1), "bob@b")).unwrap();
    v[field] = value;
    serde_json::to_vec(&v).unwrap()
}

#[test]
fn constants_have_the_spec_values() {
    assert_eq!(CANDIDATE_CAP, 500);
    assert_eq!(MESSAGE_MAX, 8388608);
    assert_eq!(MESSAGE_MAX, zygo_workspace::mail::store::FILE_MAX);
}

#[test]
fn check_message_accepts_any_key_order_and_unknown_keys() {
    assert_eq!(check(u(1), &msg(u(1), "bob@b")), Ok(()));
    assert_eq!(check(u(1), &odd(u(1))), Ok(()));
}

#[test]
fn check_message_names_the_first_failed_check() {
    let m = msg(u(1), "bob@b");
    let f = file(u(1));
    let n = m.len() as u64;
    assert_eq!(check_message("bob@b", &f, "120000", "blob", n, &m), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "160000", "commit", u64::MAX, b""), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "040000", "tree", u64::MAX, b""), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "100755", "blob", n, &m), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "100644", "tree", n, &m), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "120000", "blob", n, b"{bad"), Err("mode")); // first failure wins
    for bad in ["..", "x.json", "01arz3ndektsv4rrffq69g5fav.json", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "../01ARZ3NDEKTSV4RRFFQ69G5FAV.json"] {
        assert_eq!(check_message("bob@b", bad, "100644", "blob", n, &m), Err("name"), "{bad:?}");
    }
    assert_eq!(check_message("bob@b", &f, "100644", "blob", 8388609, &m), Err("size"));
    assert_eq!(check_message("bob@b", &f, "100644", "blob", 8388608, &m), Ok(()));
    for bad in [&b"{bad"[..], b"", b"[1]", b"null",
        br#"[1,"01ARZ3NDEKTSV4RRFFQ69G5FAV","alice@a","bob@b","2026-11-25T10:00:00Z","s","b"]"#] {
        assert_eq!(check(u(1), bad), Err("json"), "{:?}", String::from_utf8_lossy(bad));
    }
    let mut missing: serde_json::Value = serde_json::from_slice(&m).unwrap();
    missing.as_object_mut().unwrap().remove("body");
    assert_eq!(check(u(1), &serde_json::to_vec(&missing).unwrap()), Err("json"));
    let dup = format!(r#"{{"version":1,"id":"{}","id":"{}","from":"alice@a","to":"bob@b","sent_at":"2026-11-25T10:00:00Z","subject":"s","body":"b"}}"#, u(1), u(1));
    assert_eq!(check(u(1), dup.as_bytes()), Err("json"));
    assert_eq!(check(u(1), &with("version", 2.into())), Err("version"));
    assert_eq!(check(u(1), &with("version", "1".into())), Err("json"));
    assert_eq!(check(u(1), &with("id", u(2).to_string().into())), Err("id"));
    assert_eq!(check(u(1), &with("to", "eve@b".into())), Err("to"));
    assert_eq!(check(u(1), &with("to", "bob".into())), Err("to"));
    for bad in ["alice", "cli@a", "zygo@a", "alice@A", "@a", "alice@"] {
        assert_eq!(check(u(1), &with("from", bad.into())), Err("from"), "{bad}");
    }
    assert_eq!(check(u(1), &with("sent_at", "yesterday".into())), Err("sent-at"));
}

#[test]
fn check_message_limits_are_bytes_not_characters() {
    assert_eq!(check(u(1), &with("subject", "a".repeat(200).into())), Ok(()));
    assert_eq!(check(u(1), &with("subject", "é".repeat(100).into())), Ok(())); // 200 bytes
    assert_eq!(check(u(1), &with("subject", "é".repeat(101).into())), Err("subject")); // 101 chars, 202 bytes
    assert_eq!(check(u(1), &with("body", "é".repeat(524288).into())), Ok(())); // 1048576 bytes
    assert_eq!(check(u(1), &with("body", format!("{}a", "é".repeat(524288)).into())), Err("body"));
}

#[test]
fn check_message_refuses_a_hostile_sent_at() {
    for good in ["2026-11-25T10:00:00Z", "2026-11-25t10:00:00z", "2026-11-25T10:00:00.123456789+05:30",
        "2024-02-29T23:59:60-00:00", "2000-02-29T00:00:00Z"] {
        assert_eq!(check(u(1), &with("sent_at", good.into())), Ok(()), "{good}");
    }
    for bad in ["", "2026-11-25T10:00:00", "2026-11-25 10:00:00Z", "2026-02-29T10:00:00Z", "1900-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z", "2026-00-10T00:00:00Z", "2026-11-31T00:00:00Z", "2026-11-00T00:00:00Z",
        "2026-11-25T24:00:00Z", "2026-11-25T10:60:00Z", "2026-11-25T10:00:61Z", "2026-11-25T10:00:00+24:00",
        "2026-11-25T10:00:00+05:60", "2026-11-25T10:00:00.Z", "+2026-11-25T10:00:00Z", "２026-11-25T10:00:00Z",
        "2026-11-25T10:00:00Z\n", "2026-11-25T10:00:00ZZ", "26-11-25T10:00:00Z"] {
        assert_eq!(check(u(1), &with("sent_at", bad.into())), Err("sent-at"), "{bad:?}");
    }
}
