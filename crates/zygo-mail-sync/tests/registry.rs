//! MS-6: registry and ack files, member cache (mail sync design S§6.1, S§7.3, S§7.4, S-C5, S-C8; S§19 #12).
//! No test uses wall-clock time; mode and link tests are cfg(unix).

use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use ulid::Ulid;
use zygo_core::paths::Paths;
use zygo_mail_sync::{address::Address, registry::*, state::SyncPaths};

const ID: &str = "0123456789abcdef";
const IDN: &str = "0123456789abcdef0123456789abcdef";

fn paths(p: &Path) -> Paths {
    Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    }
}

fn setup() -> (TempDir, SyncPaths) {
    let t = TempDir::new().unwrap();
    let sp = SyncPaths::new(&paths(t.path()), ID).unwrap();
    (t, sp)
}

fn u(n: u64) -> Ulid {
    Ulid::from_parts(n, n as u128)
}

fn j(v: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap()
}

fn reg(host: &str, members: &[String]) -> Vec<u8> {
    j(json!({"version":1,"host":host,"identity":IDN,"members":members}))
}

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("m{i}")).collect()
}

fn pr(fh: &str, b: &[u8]) -> Result<Registry, Check> {
    parse_registry(fh, "100644", b.len() as u64, b)
}

#[test]
fn registry_valid_file_parses() {
    let m = vec!["alice".to_string(), "bob".into()];
    assert_eq!(
        pr("b", &reg("b", &m)),
        Ok(Registry {
            host: "b".into(),
            identity: IDN.into(),
            members: m
        })
    );
}

#[test]
fn registry_refused_for_a_mode_other_than_100644() {
    let b = reg("b", &names(1));
    for m in ["100755", "120000", "160000", "040000", ""] {
        assert_eq!(
            parse_registry("b", m, b.len() as u64, &b),
            Err(Check("mode"))
        );
    }
}

#[test]
fn registry_refused_over_64_kib() {
    let mut b = reg("b", &names(1));
    assert_eq!(parse_registry("b", "100644", 65537, &b), Err(Check("size")));
    b.resize(65536, b' ');
    assert!(pr("b", &b).is_ok());
    b.push(b' ');
    // the bytes, not only the argument
    assert_eq!(parse_registry("b", "100644", 10, &b), Err(Check("size")));
}

#[test]
fn registry_refused_for_bad_json() {
    for b in [
        b"{".to_vec(),
        b"\xff".to_vec(),
        j(json!({"version":1,"host":"b","identity":IDN,"members":"alice"})),
        j(json!({"version":1,"host":"b","identity":IDN})),
    ] {
        assert_eq!(pr("b", &b), Err(Check("json")));
    }
}

#[test]
fn registry_refused_when_version_is_not_1() {
    for v in [json!(2), json!("1"), json!(0)] {
        assert_eq!(
            pr(
                "b",
                &j(json!({"version":v,"host":"b","identity":IDN,"members":[]}))
            ),
            Err(Check("version"))
        );
    }
    assert_eq!(pr("b", b"[]"), Err(Check("version")));
}

#[test]
fn registry_refused_when_host_is_not_the_file_name_or_form() {
    assert_eq!(pr("b", &reg("c", &names(1))), Err(Check("host")));
    for h in ["-b", "B", &"a".repeat(33)] {
        assert_eq!(pr(h, &reg(h, &names(1))), Err(Check("host")));
    }
}

#[test]
fn registry_refused_for_a_bad_identity() {
    for id in [
        IDN.to_uppercase(),
        IDN[..31].to_string(),
        format!("{IDN}0"),
        "g".repeat(32),
    ] {
        assert_eq!(
            pr(
                "b",
                &j(json!({"version":1,"host":"b","identity":id,"members":[]}))
            ),
            Err(Check("identity"))
        );
    }
}

#[test]
fn registry_holds_256_members_and_refuses_257() {
    assert_eq!(MEMBERS_MAX, 256);
    assert_eq!(pr("b", &reg("b", &names(256))).unwrap().members.len(), 256);
    assert_eq!(pr("b", &reg("b", &names(257))), Err(Check("members")));
}

#[test]
fn registry_refused_for_a_member_name_outside_m6() {
    for n in [
        "all",
        "cli",
        "zygo",
        "kabcdefgh",
        "Alice",
        "1a",
        "",
        &"a".repeat(33),
    ] {
        assert_eq!(
            pr("b", &reg("b", &[n.to_string()])),
            Err(Check("member")),
            "{n}"
        );
    }
}

#[test]
fn registry_to_bytes_round_trips() {
    let r = Registry {
        host: "b".into(),
        identity: IDN.into(),
        members: vec!["alice".into(), "bob".into()],
    };
    assert_eq!(
        r.to_bytes(),
        format!(r#"{{"version":1,"host":"b","identity":"{IDN}","members":["alice","bob"]}}"#)
            .into_bytes()
    );
    assert_eq!(pr("b", &r.to_bytes()), Ok(r));
}

fn pa(fh: &str, b: &[u8]) -> Result<Ack, Check> {
    parse_ack(fh, "100644", b.len() as u64, b)
}

fn ack(host: &str, ids: &[String]) -> Vec<u8> {
    j(json!({"version":1,"host":host,"identity":IDN,"imported":ids}))
}

fn ids(n: u64) -> Vec<String> {
    (1..=n).map(|i| u(i).to_string()).collect()
}

#[test]
fn ack_valid_file_parses() {
    let a = pa(
        "b",
        &ack("b", &[u(2).to_string(), u(1).to_string(), u(1).to_string()]),
    )
    .unwrap();
    assert_eq!((a.host.as_str(), a.identity.as_str()), ("b", IDN));
    assert_eq!(a.imported, BTreeSet::from([u(1), u(2)]));
}

#[test]
fn ack_refused_as_the_registry() {
    let b = ack("b", &ids(1));
    assert_eq!(
        parse_ack("b", "120000", b.len() as u64, &b),
        Err(Check("mode"))
    );
    assert_eq!(pa("b", b"{"), Err(Check("json")));
    assert_eq!(
        pa(
            "b",
            &j(json!({"version":2,"host":"b","identity":IDN,"imported":[]}))
        ),
        Err(Check("version"))
    );
    assert_eq!(pa("c", &b), Err(Check("host")));
    assert_eq!(
        pa(
            "b",
            &j(json!({"version":1,"host":"b","identity":"x","imported":[]}))
        ),
        Err(Check("identity"))
    );
}

#[test]
fn ack_refused_over_8_mib() {
    let b = ack("b", &ids(1));
    assert_eq!(parse_ack("b", "100644", 8388609, &b), Err(Check("size")));
    assert!(parse_ack("b", "100644", 8388608, &b).is_ok());
}

#[test]
fn ack_holds_200000_entries_and_refuses_200001() {
    assert_eq!(ACK_ENTRIES_MAX, 200000);
    assert_eq!(
        pa("b", &ack("b", &ids(200000))).unwrap().imported.len(),
        200000
    );
    assert_eq!(pa("b", &ack("b", &ids(200001))), Err(Check("entries")));
}

#[test]
fn ack_refused_for_an_entry_that_is_not_a_ulid() {
    // `…FAVX` is 27 characters; `…FAU` has the excluded letter `U`.
    let lower = "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_lowercase();
    for e in [
        "not-a-ulid",
        lower.as_str(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAVX",
        "01ARZ3NDEKTSV4RRFFQ69G5FAU",
    ] {
        assert_eq!(
            pa("b", &ack("b", &[e.to_string()])),
            Err(Check("entry")),
            "{e}"
        );
    }
}

#[test]
fn ack_to_bytes_round_trips() {
    let a = Ack {
        host: "b".into(),
        identity: IDN.into(),
        imported: BTreeSet::from([u(2), u(1)]),
    };
    assert_eq!(
        a.to_bytes(),
        format!(
            r#"{{"version":1,"host":"b","identity":"{IDN}","imported":["{}","{}"]}}"#,
            u(1),
            u(2)
        )
        .into_bytes()
    );
    assert_eq!(pa("b", &a.to_bytes()), Ok(a));
}

fn cache() -> MemberCache {
    MemberCache {
        hosts: BTreeMap::from([
            ("b".into(), vec!["bob".into(), "carol".into()]),
            ("c".into(), vec!["dave".into()]),
        ]),
    }
}

#[cfg(unix)]
#[test]
fn write_cache_is_0600_atomic_and_names_only() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, sp) = setup();
    write_cache(&sp, &MemberCache::default()).unwrap();
    write_cache(&sp, &cache()).unwrap();
    assert_eq!(
        fs::read(&sp.members).unwrap(),
        br#"{"hosts":{"b":["bob","carol"],"c":["dave"]}}"#
    );
    assert_eq!(
        fs::metadata(&sp.members).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let left: Vec<_> = fs::read_dir(&sp.sync)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["members.json"]); // no temp file left
}

#[cfg(unix)]
#[test]
fn write_cache_refuses_a_linked_mail_folder() {
    let (t, sp) = setup();
    let real = t.path().join("elsewhere");
    fs::create_dir(&real).unwrap();
    fs::create_dir_all(sp.mail.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &sp.mail).unwrap();
    assert_eq!(
        write_cache(&sp, &cache()).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn read_cache_has_no_age_filter() {
    let (_t, sp) = setup();
    write_cache(&sp, &cache()).unwrap();
    fs::File::options()
        .write(true)
        .open(&sp.members)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
    assert_eq!(read_cache(&sp), cache());
}

#[test]
fn read_cache_is_empty_for_a_missing_or_bad_file_and_drops_bad_names() {
    let (_t, sp) = setup();
    assert_eq!(read_cache(&sp), MemberCache::default());
    fs::create_dir_all(&sp.sync).unwrap();
    fs::write(&sp.members, b"{").unwrap();
    assert_eq!(read_cache(&sp), MemberCache::default());
    fs::write(
        &sp.members,
        br#"{"hosts":{"b":["bob","zygo","Eve"],"-x":["amy"]}}"#,
    )
    .unwrap();
    assert_eq!(
        read_cache(&sp).hosts,
        BTreeMap::from([("b".to_string(), vec!["bob".to_string()])])
    );
}

#[cfg(unix)]
#[test]
fn read_cache_refuses_a_link() {
    let (t, sp) = setup();
    let real = t.path().join("real.json");
    fs::write(&real, br#"{"hosts":{"b":["bob"]}}"#).unwrap();
    fs::create_dir_all(&sp.sync).unwrap();
    std::os::unix::fs::symlink(&real, &sp.members).unwrap();
    assert_eq!(read_cache(&sp), MemberCache::default());
}

#[test]
fn cache_addresses_and_knows() {
    let c = cache();
    assert_eq!(c.addresses(), ["bob@b", "carol@b", "dave@c"]);
    let a = |n: &str, h: &str| Address {
        name: n.into(),
        host: h.into(),
    };
    assert!(c.knows(&a("bob", "b")));
    assert!(!c.knows(&a("bob", "c")));
    assert!(!c.knows(&a("eve", "b")));
}

#[test]
fn registry_refused_when_the_bytes_are_over_64_kib_and_the_size_is_not() {
    let mut b = reg("b", &names(1));
    b.resize(65537, b' ');
    assert_eq!(parse_registry("b", "100644", 100, &b), Err(Check("size")));
}

/// `{..., "x":[0,0,...]}`: `base` (an object) with an unknown key that holds `n` zeros.
fn with_unknown_array(base: &[u8], n: usize) -> Vec<u8> {
    let mut b = base[..base.len() - 1].to_vec();
    b.extend_from_slice(br#","x":["#);
    for i in 0..n {
        if i > 0 {
            b.push(b',');
        }
        b.push(b'0');
    }
    b.extend_from_slice(b"]}");
    b
}

#[test]
fn ack_with_a_large_unknown_array_gives_the_same_result() {
    let n = 4_000_000; // about 8 MB, under ACK_MAX
    let good = ack("b", &ids(2));
    let big = with_unknown_array(&good, n);
    assert!(big.len() as u64 <= ACK_MAX);
    assert_eq!(pa("b", &big), pa("b", &good));
    assert!(pa("b", &big).is_ok());
    let bad = ack("b", &["x".into()]);
    assert_eq!(pa("b", &with_unknown_array(&bad, n)), Err(Check("entry")));
}

#[test]
fn version_is_checked_before_the_shape() {
    let other = j(json!({"version":2,"other":true}));
    assert_eq!(pr("b", &other), Err(Check("version")));
    assert_eq!(pa("b", &other), Err(Check("version")));
    // the array form of the fields is not an object with `version`
    let arr = j(json!([1, "b", IDN, []]));
    assert_eq!(pr("b", &arr), Err(Check("version")));
    assert_eq!(pa("b", &arr), Err(Check("version")));
    for v in [
        json!(1.0),
        json!([1]),
        json!({"v":1}),
        json!(null),
        json!(true),
    ] {
        let b = j(json!({"version":v,"host":"b","identity":IDN,"imported":[]}));
        assert_eq!(pa("b", &b), Err(Check("version")), "{v}");
    }
    let big = with_unknown_array(&j(json!({"host":"b"})), 1000);
    let big = String::from_utf8(big)
        .unwrap()
        .replace(r#""x":"#, r#""version":"#);
    assert_eq!(pa("b", big.as_bytes()), Err(Check("version")));
    // a missing field after a good version names `json`
    assert_eq!(
        pa("b", &j(json!({"version":1,"host":"b"}))),
        Err(Check("json"))
    );
}

#[cfg(unix)]
#[test]
fn write_cache_that_fails_leaves_the_old_file() {
    // `mail/sync` is a link to a folder that holds an old cache: the write fails at its folder check.
    let (t, sp) = setup();
    let real = t.path().join("elsewhere");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("members.json"), b"old").unwrap();
    fs::create_dir_all(&sp.mail).unwrap();
    std::os::unix::fs::symlink(&real, &sp.sync).unwrap();
    assert!(write_cache(&sp, &cache()).is_err());
    assert_eq!(fs::read(real.join("members.json")).unwrap(), b"old");
    assert_eq!(fs::read_dir(&real).unwrap().count(), 1); // no temp file left
}

#[cfg(unix)]
#[test]
fn read_cache_of_a_fifo_is_empty_without_a_wait() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    let c = std::ffi::CString::new(sp.members.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(read_cache(&sp), MemberCache::default());
}
