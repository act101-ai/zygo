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

#[test] // AU84, with the hostile values of the spec
fn validators_follow_au84() {
    assert!(valid_pane_key("kabcd2345"));
    for bad in ["", "k$(x)", "#{pane_id}", "kabcd234", "kabcd23456", "xabcd2345", "kABCD2345", "kabcd0189", "kabcd2345\n", " kabcd2345"] {
        assert!(!valid_pane_key(bad), "{bad:?}");
    }
    assert!(valid_ws_id("0123456789abcdef"));
    for bad in ["", "abc", "0123456789abcde", "0123456789abcdef0", "0123456789ABCDEF", "0123456789abcdeg", "0123456789abcdef\n"] {
        assert!(!valid_ws_id(bad), "{bad:?}");
    }
    let (a32, a33) = ("a".repeat(32), "a".repeat(33));
    for good in ["a", "my-repo", "app-2", "-", a32.as_str()] {
        assert!(valid_name(good), "{good:?}");
    }
    for bad in ["", "a b", "A", "a_b", "a.b", "a/b", "a\n", "\u{e9}", a33.as_str()] {
        assert!(!valid_name(bad), "{bad:?}");
    }
    assert!(valid_tmux_session("zygo-my-repo", "my-repo"));
    assert!(valid_tmux_session(&format!("zygo-{a32}"), &a32));
    let s33 = format!("zygo-{a33}");
    for (s, name) in [("other", "my-repo"), ("zygo-other", "my-repo"), ("my-repo", "my-repo"), ("zygo-", ""), ("zygo-a b", "a b"),
        ("zygo-my-repo\n", "my-repo"), ("ZYGO-my-repo", "my-repo"), (s33.as_str(), a33.as_str())] {
        assert!(!valid_tmux_session(s, name), "{s:?} {name:?}");
    }
}

#[test] // W§5: `k` plus 8 random characters of lower-case base32
fn new_pane_key_is_valid_and_random() {
    let keys: std::collections::BTreeSet<String> = (0..64).map(|_| new_pane_key()).collect();
    assert!(keys.iter().all(|k| valid_pane_key(k)));
    assert_eq!(keys.len(), 64);
}
