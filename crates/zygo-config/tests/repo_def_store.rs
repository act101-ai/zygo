//! WD-3: workspace approvals in approvals.toml (R§7.2, R§11, R§18 item 8).

use std::fs;
use std::path::{Path, PathBuf};

use zygo_config::repo_def::{
    self, Files, Stored, approve_workspace_in as approve, is_workspace_approved_in as approved,
};

const ID: &str = "3f9a1c0d5e7b2a46";
const ID2: &str = "0123456789abcdef";

fn d(text: &str) -> String {
    repo_def::digest(&Files {
        full_ref: None,
        commit: "a".repeat(40),
        workspace: Some(text.as_bytes().to_vec()),
        profiles: Default::default(),
        ignored: vec![],
    })
}

fn d1() -> String {
    d("version = 1\n")
}

fn d2() -> String {
    d("version = 1\n# 2\n")
}

fn entry(id: &str, digest: &str) -> String {
    format!("[[workspace]]\nid = \"{id}\"\ndigest = \"{digest}\"\n")
}

/// Sorted file names in `dir`.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn store(t: &tempfile::TempDir) -> PathBuf {
    t.path().join("approvals.toml")
}

#[test]
fn approve_writes_one_entry_to_a_new_store() {
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("sub/approvals.toml");
    let dg = d1();
    assert_eq!(approve(&f, ID, &dg).unwrap(), Stored::Added);
    assert_eq!(fs::read_to_string(&f).unwrap(), entry(ID, &dg));
    assert!(approved(&f, ID, &dg));
    // The lock file stays; no temp file.
    assert_eq!(
        names(f.parent().unwrap()),
        ["approvals.toml", "approvals.toml.lock"]
    );
}

#[test]
fn approve_keeps_every_earlier_byte() {
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let (dg, dg2) = (d1(), d2());
    // No final newline.
    let before = format!(
        "# mine\n[[approval]]\nmarketplace = \"m\"\nplugin = \"p\"\ndigest = \"{}\"\n[[workspace]]\nid = \"{ID2}\"\ndigest = \"{dg}\"",
        "a".repeat(64)
    );
    fs::write(&f, &before).unwrap();
    assert_eq!(approve(&f, ID, &dg2).unwrap(), Stored::Added);
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        format!("{before}\n\n{}", entry(ID, &dg2))
    );
    // The earlier entry stays (R§7.2 "never removed").
    assert!(approved(&f, ID2, &dg) && approved(&f, ID, &dg2));
}

#[test]
fn second_approve_is_already_present_and_byte_equal() {
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let dg = d1();
    assert_eq!(approve(&f, ID, &dg).unwrap(), Stored::Added);
    let b = fs::read(&f).unwrap();
    assert_eq!(approve(&f, ID, &dg).unwrap(), Stored::AlreadyPresent);
    assert_eq!(fs::read(&f).unwrap(), b);
}

#[test]
fn approval_is_keyed_by_id_and_digest() {
    // R§18 item 8, review focus 32.
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let (dg, dg2) = (d1(), d2());
    approve(&f, ID, &dg).unwrap();
    assert!(!approved(&f, ID2, &dg)); // same digest, another workspace
    assert!(!approved(&f, ID, &dg2)); // same workspace, another digest
    assert_eq!(approve(&f, ID2, &dg).unwrap(), Stored::Added); // a second entry, not AlreadyPresent
}

#[test]
fn missing_store_approves_nothing() {
    let t = tempfile::tempdir().unwrap();
    assert!(!approved(&t.path().join("none.toml"), ID, &d1()));
}

#[test]
fn invalid_store_approves_nothing_and_is_left_as_it_is() {
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let dg = d1();
    for text in [
        format!("{}[x\n", entry(ID, &dg)),                     // not TOML
        "workspace = 1\n".to_string(),                         // not an array of tables
        format!("[[workspace]]\nid = 1\ndigest = \"{dg}\"\n"), // id not a string
        format!("[[workspace]]\nid = \"{ID}\"\n"),             // digest missing
    ] {
        fs::write(&f, &text).unwrap();
        assert!(!approved(&f, ID, &dg));
        let e = approve(&f, ID, &dg).unwrap_err();
        assert!(format!("{e:#}").contains("invalid approvals file"), "{e:#}");
        assert_eq!(fs::read_to_string(&f).unwrap(), text);
        assert_eq!(names(t.path()), ["approvals.toml", "approvals.toml.lock"]);
    }
}

#[test]
fn malformed_ids_and_digests_are_refused() {
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let dg = d1();
    for (id, digest) in [
        ("3F9A1C0D5E7B2A46", dg.clone()),
        ("3f9a1c0d5e7b2a4", dg.clone()),
        ("3f9a1c0d5e7b2a46x", dg.clone()),
        (ID, dg[..63].to_string()),
        (ID, dg.to_uppercase()),
    ] {
        assert!(approve(&f, id, &digest).is_err());
        assert!(!f.exists());
    }
    fs::write(&f, entry("3F9A1C0D5E7B2A46", &dg)).unwrap();
    assert!(!approved(&f, "3F9A1C0D5E7B2A46", &dg));
}

#[cfg(unix)]
#[test]
fn failed_write_leaves_no_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    // Root writes in a read-only folder, so the case is skipped there.
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped the read-only folder case: the test runs as root");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let dg = d1();
    approve(&f, ID, &dg).unwrap(); // file and lock file exist
    let b = fs::read(&f).unwrap();
    let n = names(t.path());
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let r = approve(&f, ID2, &dg);
    // Restored before the asserts that can fail.
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(r.is_err());
    assert_eq!(fs::read(&f).unwrap(), b);
    assert_eq!(names(t.path()), n); // no `.approvals.toml.*.tmp`
    assert!(!approved(&f, ID2, &dg));
}

#[test]
fn parallel_approves_all_land() {
    let t = tempfile::tempdir().unwrap();
    let f = store(&t);
    let dg = d1();
    let ids: Vec<String> = (0..4).map(|i| format!("000000000000000{i}")).collect();
    let handles: Vec<_> = ids
        .iter()
        .cloned()
        .map(|id| {
            let (f, dg) = (f.clone(), dg.clone());
            std::thread::spawn(move || approve(&f, &id, &dg).unwrap())
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), Stored::Added);
    }
    for id in &ids {
        assert!(approved(&f, id, &dg));
    }
    let v: toml::Value = toml::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(v["workspace"].as_array().unwrap().len(), 4);
}
