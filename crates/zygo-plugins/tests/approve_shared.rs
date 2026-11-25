//! WD-3: plugin approvals beside [[workspace]] entries in one approvals.toml (R§7.2, R§18 item 21).

use std::fs;

use zygo_config::repo_def::{approve_workspace_in, is_workspace_approved_in};
use zygo_plugins::approve::{approve_in, is_approved_in};

const ID: &str = "3f9a1c0d5e7b2a46";
const ID2: &str = "0123456789abcdef";

#[test]
fn plugin_approvals_hold_beside_workspace_entries() {
    let (p1, p2, w) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("approvals.toml");
    approve_in(&f, "m1", "p1", &p1).unwrap();
    approve_workspace_in(&f, ID, &w).unwrap();
    approve_in(&f, "m2", "p2", &p2).unwrap();
    approve_workspace_in(&f, ID2, &w).unwrap();
    assert!(is_approved_in(&f, "m1", "p1", &p1) && is_approved_in(&f, "m2", "p2", &p2));
    assert!(!is_approved_in(&f, "m1", "p1", &p2));
    assert!(is_workspace_approved_in(&f, ID, &w) && is_workspace_approved_in(&f, ID2, &w));

    // A hand-written file whose `[[workspace]]` entries come first and carry an extra key.
    let g = t.path().join("hand.toml");
    fs::write(
        &g,
        format!(
            "[[workspace]]\nid = \"{ID}\"\ndigest = \"{w}\"\nnote = \"x\"\n\n[[approval]]\nmarketplace = \"m\"\nplugin = \"p\"\ndigest = \"{p1}\"\n"
        ),
    )
    .unwrap();
    assert!(is_approved_in(&g, "m", "p", &p1));
    assert!(is_workspace_approved_in(&g, ID, &w));
}

#[test]
fn plugin_approve_keeps_workspace_entries_byte_for_byte() {
    let (p1, w) = ("a".repeat(64), "c".repeat(64));
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("approvals.toml");
    approve_workspace_in(&f, ID, &w).unwrap();
    approve_workspace_in(&f, ID2, &w).unwrap();
    let before = fs::read_to_string(&f).unwrap();
    approve_in(&f, "m", "p", &p1).unwrap();
    let after = fs::read_to_string(&f).unwrap();
    assert!(after.starts_with(&before));
    assert_eq!(
        &after[before.len()..],
        format!("\n[[approval]]\nmarketplace = \"m\"\nplugin = \"p\"\ndigest = \"{p1}\"\n")
    );
    assert!(is_workspace_approved_in(&f, ID, &w) && is_workspace_approved_in(&f, ID2, &w));
}
