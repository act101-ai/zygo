use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::repo::{checkout, Checkout};
use zygo_workspace::identity::*;

#[test] // W§5: first 16 hex characters of SHA-256 of the path bytes
fn workspace_id_is_16_hex_of_sha256() {
    assert_eq!(workspace_id(Path::new("/srv/repo/.git")), "4b45de8d0544bbd0");
    assert_ne!(workspace_id(Path::new("/srv/repo2/.git")), "4b45de8d0544bbd0");
}

#[test] // AU2 slug rule, W§18 #2
fn slug_follows_au2() {
    assert_eq!(slug("My Repo!!"), "my-repo");
    assert_eq!(slug("---"), "ws");
    assert_eq!(slug(""), "ws");
    assert_eq!(slug("a__b..c"), "a-b-c");
    assert_eq!(slug("\u{212A}"), "ws"); // KELVIN SIGN: only ASCII letters are lower-cased
    assert_eq!(slug(&"a".repeat(50)), "a".repeat(32));
    // the cut at 32 ends on `-`, which is trimmed again
    assert_eq!(slug(&format!("{}-{}", "a".repeat(31), "b".repeat(18))), "a".repeat(31));
}

#[test] // AU2 collision suffix, W§18 #2
fn unique_name_adds_a_suffix_within_32() {
    assert_eq!(unique_name("app", &|_| false), "app");
    assert_eq!(unique_name("app", &|n| n == "app"), "app-2");
    assert_eq!(unique_name("app", &|n| n == "app" || n == "app-2"), "app-3");
    let long = "a".repeat(32);
    assert_eq!(unique_name(&long, &|n| n == long), format!("{}-2", "a".repeat(30)));
    assert_eq!(unique_name(&long, &|n| !n.ends_with("-10")), format!("{}-10", "a".repeat(29)));
    // the cut slug part ends on `-`: trimmed before the suffix
    let dash = format!("{}-bb", "a".repeat(29));
    assert_eq!(unique_name(&dash, &|n| n == dash), format!("{}-2", "a".repeat(29)));
}
