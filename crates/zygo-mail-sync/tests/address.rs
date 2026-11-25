//! MS-4: the address grammar (mail sync design S§5, S§7.2; S§16 M§6) and the ULID file name form
//! (S§8.5 "Checks"). Pure functions: no file, no clock.

use ulid::Ulid;
use zygo_core::ids::RESERVED_MEMBER_NAMES;
use zygo_mail_sync::address::*;

fn own() -> Vec<String> {
    vec!["studio-2".into(), "old-host".into()]
}

fn remote(name: &str, host: &str) -> Result<To, AddrError> {
    Ok(To::Remote(Address { name: name.into(), host: host.into() }))
}

fn local(name: &str) -> Result<To, AddrError> {
    Ok(To::Local(name.into()))
}

const GOOD: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn reserved_names_are_all_cli_and_zygo() {
    assert_eq!(RESERVED, ["all", "cli", "zygo"]);
    assert_eq!(RESERVED, RESERVED_MEMBER_NAMES);
}

#[test]
fn a_bare_name_is_local() {
    assert_eq!(resolve_to("bob", &own()), local("bob"));
    assert_eq!(resolve_to("bob", &[]), local("bob"));
    let max = format!("a{}", "b".repeat(31));
    assert_eq!(resolve_to(&max, &own()), local(&max));
    // M§8: a bare pane key is a local `to`
    assert_eq!(resolve_to("kabcdefgh", &own()), local("kabcdefgh"));
}

#[test]
fn an_own_host_address_is_local() {
    assert_eq!(resolve_to("bob@studio-2", &own()), local("bob"));
    assert_eq!(resolve_to("bob@old-host", &own()), local("bob")); // an older own host (S§7.5)
}

#[test]
fn another_host_address_is_remote() {
    assert_eq!(resolve_to("bob@b", &own()), remote("bob", "b"));
    assert_eq!(resolve_to("bob@studio-2", &[]), remote("bob", "studio-2"));
    let name = format!("a{}", "b".repeat(31));
    let host = "0".repeat(32);
    assert_eq!(resolve_to(&format!("{name}@{host}"), &own()), remote(&name, &host));
}

#[test]
fn a_pane_key_with_a_host_is_bad_form() {
    assert_eq!(resolve_to("kabcdefgh@studio-2", &own()), Err(AddrError::BadForm));
}

#[test]
fn a_name_that_is_also_an_own_host_is_local() {
    let own = vec!["b".to_string()];
    assert_eq!(resolve_to("b@b", &own), local("b"));
}

#[test]
fn all_is_all() {
    assert_eq!(resolve_to("all", &own()), Ok(To::All));
    assert_eq!(resolve_to("all", &[]), Ok(To::All));
}

#[test]
fn reserved_names_are_refused() {
    for s in ["cli@b", "zygo@b", "cli@studio-2", "zygo@studio-2", "all@b", "all@studio-2", "cli", "zygo"] {
        assert_eq!(resolve_to(s, &own()), Err(AddrError::Reserved), "{s:?}");
    }
}

#[test]
fn a_name_or_host_outside_its_form_is_bad_form() {
    let long_name = format!("a{}", "b".repeat(32));
    let long_host = "a".repeat(33);
    let bad = [
        String::new(), "Bob".into(), "1bob".into(), "-bob".into(), "a_b".into(), "bob ".into(), long_name.clone(),
        "@b".into(), "bob@".into(), "@".into(), "Bob@b".into(), "1bob@b".into(), "kabcdefgh@b".into(),
        format!("{long_name}@b"), "bob@B".into(), "bob@-b".into(), "bob@a_b".into(), "bob@a.b".into(),
        "bob@b/c".into(), "bob@b@c".into(), "bob@ b".into(), "bob @b".into(), "bob@b\n".into(),
        format!("bob@{long_host}"), "cli@B".into(),
    ];
    for s in &bad {
        assert_eq!(resolve_to(s, &own()), Err(AddrError::BadForm), "{s:?}");
    }
}

#[test]
fn parse_takes_the_address_form() {
    assert_eq!(parse("bob@b"), Some(Address { name: "bob".into(), host: "b".into() }));
    assert_eq!(parse("x9-y@studio-2"), Some(Address { name: "x9-y".into(), host: "studio-2".into() }));
}

#[test]
fn parse_refuses_others() {
    for s in ["bob", "all", "", "@", "@b", "bob@", "all@b", "cli@b", "zygo@b", "Bob@b", "bob@B", "bob@b@c",
        "kabcdefgh@b", "bob@b/c", "bob@b\n"] {
        assert_eq!(parse(s), None, "{s:?}");
    }
}

#[test]
fn ulid_file_takes_crockford_ulid_names() {
    assert_eq!(ulid_file(&format!("{GOOD}.json")), Some(Ulid::from_string(GOOD).unwrap()));
    assert_eq!(ulid_file("00000000000000000000000000.json"), Some(Ulid(0)));
    assert_eq!(ulid_file("7ZZZZZZZZZZZZZZZZZZZZZZZZZ.json"), Some(Ulid(u128::MAX)));
    // every allowed character
    assert!(ulid_file("0123456789ABCDEFGHJKMNPQRS.json").is_some());
    assert!(ulid_file("00000000TVWXYZ000000000000.json").is_some());
}

#[test]
fn ulid_file_refuses_other_names() {
    let lower = GOOD.to_lowercase();
    let one_lower = format!("{}v", &GOOD[..25]);
    let bad = [
        format!("{lower}.json"), format!("{one_lower}.json"),
        format!("{}I.json", &GOOD[..25]), format!("{}L.json", &GOOD[..25]),
        format!("{}O.json", &GOOD[..25]), format!("{}U.json", &GOOD[..25]),
        format!("{}.json", &GOOD[..25]), format!("{GOOD}0.json"),
        GOOD.to_string(), format!("{GOOD}.JSON"), format!("{GOOD}.json.tmp"), format!("{GOOD}.txt"),
        format!("{GOOD}json"), format!("x{GOOD}.json"), format!("{GOOD}.json\n"), format!("{GOOD}.json/"),
        format!("../{GOOD}.json"), String::new(), ".json".into(),
        "80000000000000000000000000.json".into(), "ZZZZZZZZZZZZZZZZZZZZZZZZZZ.json".into(),
        format!("{}é.json", &GOOD[..24]),
    ];
    for s in &bad {
        assert_eq!(ulid_file(s), None, "{s:?}");
    }
}
