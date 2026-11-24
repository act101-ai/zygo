//! DR-10: one agent record change against D§6.6 (D§6.3, D§9.4 step 9, D-C4, D-C5).
use std::collections::BTreeSet;
use zygo_work::decisions::record::parse_record;
use zygo_work::gate::transition::{agent_change, RangeCtx, Version};

const ID: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TV";
const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
/// A commit in the node range.
const IN: &str = "1111111111111111111111111111111111111111";
/// A commit outside the node range.
const OUT: &str = "2222222222222222222222222222222222222222";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";

/// A valid record `ID`: an open `decision` of class `none`, then each `(key, value)` of `set` replaces or adds that key.
/// Values are TOML strings, except `round`, which is an integer.
fn rec(set: &[(&str, &str)]) -> Version {
    let mut ks: Vec<(String, String)> = [("id", ID), ("kind", "decision"), ("status", "open"), ("class", "none"),
        ("title", "Store the pane name"), ("unit", "phase-3:N12"), ("round", "2"), ("role", "code"),
        ("model", "claude-sonnet-4-5"), ("commit", C40), ("date", "2026-11-24"), ("reverse_cost", "low")]
        .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for (k, v) in set { ks.retain(|(n, _)| n != k); ks.push((k.to_string(), v.to_string())); }
    let mut s = String::from("+++\ndepends_on = []\n");
    for (k, v) in &ks { s += &if k == "round" { format!("{k} = {v}\n") } else { format!("{k} = \"{v}\"\n") }; }
    s += "+++\n";
    s += BODY;
    Version::Valid(parse_record(ID, s.as_bytes()).unwrap())
}
fn open() -> Version { rec(&[]) }
fn fixed() -> Version { rec(&[("status", "fixed"), ("fixed_by", IN)]) }
fn superseded() -> Version { rec(&[("status", "superseded")]) }
fn overturned() -> Version { rec(&[("status", "overturned"), ("reviewed", "2026-11-25"), ("review_note", "Wrong layer.")]) }
const PROMOTED: [(&str, &str); 5] = [("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", ""),
                                      ("promoted_to", "rule"), ("promotion", "Add to the rules.")];
/// `ctx.range` = {IN}; `ctx.superseding` = the given ids.
fn ctx(superseding: &[&str]) -> RangeCtx {
    RangeCtx { range: BTreeSet::from([IN.to_string()]), superseding: superseding.iter().map(|s| s.to_string()).collect() }
}
fn chg(old: &Version, new: &Version) -> Option<String> { agent_change(ID, old, new, &ctx(&[])) }
fn p(s: &str) -> Option<String> { Some(format!("{ID}: {s}")) }

#[test] fn allow_none_to_open() { assert_eq!(chg(&Version::Absent, &open()), None); }
#[test] fn allow_open_to_open_with_non_frozen_keys() {
    assert_eq!(chg(&open(), &rec(&[("title", "Keep the pane name"), ("reverse_cost", "medium")])), None);
    assert_eq!(chg(&open(), &rec(&[("model", "unknown")])), p("frozen key model changed"));
}
#[test] fn allow_open_to_fixed_with_fixed_by_in_range() { assert_eq!(chg(&open(), &fixed()), None); }
#[test] fn allow_open_to_superseded_named_by_an_added_record() {
    assert_eq!(agent_change(ID, &open(), &superseded(), &ctx(&[ID])), None);
}
#[test] fn allow_overturned_to_fixed() {
    let new = rec(&[("status", "fixed"), ("fixed_by", IN), ("reviewed", "2026-11-25"), ("review_note", "Wrong layer.")]);
    assert_eq!(chg(&overturned(), &new), None);
}
#[test] fn allow_promoted_ratified_gains_promoted_by() {
    let mut set = PROMOTED.to_vec(); set.push(("promoted_by", IN));
    assert_eq!(chg(&rec(&PROMOTED), &rec(&set)), None);
}

#[test] fn refuse_kind_change() { assert_eq!(chg(&open(), &rec(&[("kind", "assumption")])), p("frozen key kind changed")); }
#[test] fn refuse_class_change_to_none() { // graph review focus 40
    assert_eq!(chg(&rec(&[("class", "security")]), &open()), p("frozen key class changed"));
}
#[test] fn refuse_unit_change() { assert_eq!(chg(&open(), &rec(&[("unit", "phase-3:N13")])), p("frozen key unit changed")); }
#[test] fn refuse_round_change() { assert_eq!(chg(&open(), &rec(&[("round", "3")])), p("frozen key round changed")); }
#[test] fn refuse_role_change() { assert_eq!(chg(&open(), &rec(&[("role", "lead")])), p("frozen key role changed")); }
#[test] fn refuse_agent_writes_ratified() {
    assert_eq!(chg(&open(), &rec(&[("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", "")])), p("agent writes status ratified"));
}
#[test] fn refuse_agent_writes_overturned() {
    assert_eq!(chg(&open(), &rec(&[("status", "overturned"), ("reviewed", "2026-11-25"), ("review_note", "No.")])), p("agent writes status overturned"));
}
#[test] fn refuse_agent_writes_reviewed() { assert_eq!(chg(&open(), &rec(&[("reviewed", "2026-11-25")])), p("agent writes operator key reviewed")); }
#[test] fn refuse_agent_writes_review_note() { assert_eq!(chg(&open(), &rec(&[("review_note", "x")])), p("agent writes operator key review_note")); }
#[test] fn refuse_agent_writes_promoted_to() {
    assert_eq!(chg(&open(), &rec(&[("promoted_to", "rule"), ("promotion", "x")])), p("agent writes operator key promoted_to"));
}
#[test] fn refuse_agent_writes_promotion() { assert_eq!(chg(&open(), &rec(&[("promotion", "x")])), p("agent writes operator key promotion")); }
#[test] fn refuse_fixed_by_outside_range() {
    assert_eq!(chg(&open(), &rec(&[("status", "fixed"), ("fixed_by", OUT)])), p(&format!("fixed_by {OUT} is not in the node range")));
}
#[test] fn refuse_promoted_by_outside_range() {
    let mut set = PROMOTED.to_vec(); set.push(("promoted_by", OUT));
    assert_eq!(chg(&rec(&PROMOTED), &rec(&set)), p(&format!("promoted_by {OUT} is not in the node range")));
}
#[test] fn refuse_superseded_that_no_added_record_names() {
    assert_eq!(chg(&open(), &superseded()), p("superseded but no record added in the node range names it"));
}
#[test] fn refuse_change_of_fixed() {
    assert_eq!(chg(&fixed(), &rec(&[("status", "fixed"), ("fixed_by", IN), ("title", "Other")])), p("fixed is final"));
}
#[test] fn refuse_change_of_superseded() { assert_eq!(chg(&superseded(), &open()), p("superseded is final")); }
#[test] fn refuse_deleted_record() { assert_eq!(chg(&open(), &Version::Absent), p("record deleted")); }
#[test] fn refuse_malformed_new_version() { assert_eq!(chg(&open(), &Version::Malformed), p("malformed record")); }
#[test] fn refuse_new_record_not_open() {
    for (s, new) in [("fixed", fixed()), ("superseded", superseded()), ("ratified", rec(&PROMOTED)), ("overturned", overturned())] {
        assert_eq!(chg(&Version::Absent, &new), p(&format!("new record with status {s}")), "{s}");
    }
}
#[test] fn refuse_new_record_with_operator_key() {
    for (k, set) in [("reviewed", vec![("reviewed", "2026-11-25")]), ("review_note", vec![("review_note", "")]),
                     ("promoted_to", vec![("promoted_to", "adr"), ("promotion", "x")]), ("promotion", vec![("promotion", "x")])] {
        assert_eq!(chg(&Version::Absent, &rec(&set)), p(&format!("new record writes operator key {k}")), "{k}");
    }
}

#[test] fn refuse_new_record_with_fixed_by_or_promoted_by() { // D§6.6 row "none → open"
    assert_eq!(chg(&Version::Absent, &rec(&[("fixed_by", IN)])), p("new record writes fixed_by"));
    assert_eq!(chg(&Version::Absent, &rec(&[("promoted_by", IN)])), p("new record writes promoted_by"));
}
#[test] fn record_names() { // D§6.8, D§9.4 step 9
    assert_eq!(agent_change("DDR-bad", &open(), &open(), &ctx(&[])), Some("DDR-bad: malformed record name".into()));
    assert_eq!(agent_change("README", &Version::Absent, &Version::Malformed, &ctx(&[])), None);
}

#[test] fn unchanged_record_passes() {
    assert_eq!(chg(&fixed(), &fixed()), None);
    assert_eq!(chg(&superseded(), &superseded()), None);
    assert_eq!(chg(&Version::Absent, &Version::Absent), None);
}
#[test] fn other_pairs_are_not_agent_transitions() {
    assert_eq!(chg(&overturned(), &rec(&[("status", "overturned"), ("reviewed", "2026-11-25"), ("review_note", "Wrong layer."), ("title", "T")])),
               p("not an agent transition: overturned to overturned"));
    assert_eq!(chg(&overturned(), &rec(&[("reviewed", "2026-11-25"), ("review_note", "Wrong layer.")])),
               p("not an agent transition: overturned to open"));
    let ratified = [("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", "")];
    let mut retitled = ratified.to_vec(); retitled.push(("title", "T"));
    assert_eq!(chg(&rec(&ratified), &rec(&retitled)), p("an agent changes a ratified record only to add promoted_by"));
    let mut set = PROMOTED.to_vec(); set.push(("promoted_by", IN)); set.push(("title", "T"));
    assert_eq!(chg(&rec(&PROMOTED), &rec(&set)), p("an agent changes a ratified record only to add promoted_by"));
}
#[test] fn several_faults_give_the_first_in_rule_order() {
    assert_eq!(chg(&fixed(), &rec(&[("status", "fixed"), ("fixed_by", IN), ("kind", "assumption")])), p("fixed is final"));
    assert_eq!(chg(&open(), &rec(&[("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", ""), ("class", "spec")])),
               p("frozen key class changed"));
    assert_eq!(chg(&open(), &rec(&[("kind", "assumption"), ("role", "lead")])), p("frozen key kind changed"));
    assert_eq!(chg(&open(), &rec(&[("reviewed", "2026-11-25"), ("fixed_by", OUT)])), p("agent writes operator key reviewed"));
    assert_eq!(chg(&open(), &rec(&[("status", "fixed"), ("fixed_by", OUT)])), p(&format!("fixed_by {OUT} is not in the node range")));
    assert_eq!(chg(&open(), &rec(&[("status", "superseded"), ("fixed_by", OUT)])), p("open to superseded writes fixed_by"));
}

// Review round 1.
const ID2: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TW";
/// `v` with its `Record` changed by `f`.
fn with(v: Version, f: impl FnOnce(&mut zygo_work::decisions::record::Record)) -> Version {
    match v { Version::Valid(mut r) => { f(&mut r); Version::Valid(r) } v => v }
}

#[test] fn f1_refuse_change_of_malformed_record() {
    assert_eq!(chg(&Version::Malformed, &open()), p("changes a malformed record"));
    assert_eq!(chg(&Version::Malformed, &rec(&PROMOTED)), p("changes a malformed record"));
}
#[test] fn f2_refuse_self_supersede_on_creation() {
    assert_eq!(chg(&Version::Absent, &rec(&[("supersedes", ID)])), p("supersedes itself"));
}
#[test] fn f2_refuse_superseded_by_its_own_supersedes() {
    let new = rec(&[("status", "superseded"), ("supersedes", ID)]);
    assert_eq!(agent_change(ID, &rec(&[("supersedes", ID)]), &new, &ctx(&[ID])), p("supersedes itself"));
}
#[test] fn f3_refuse_open_to_open_change_of_creation_keys() {
    assert_eq!(chg(&open(), &rec(&[("commit", OUT)])), p("frozen key commit changed"));
    assert_eq!(chg(&open(), &rec(&[("date", "2026-11-25")])), p("frozen key date changed"));
    assert_eq!(chg(&open(), &rec(&[("supersedes", ID2)])), p("frozen key supersedes changed"));
    assert_eq!(chg(&rec(&[("supersedes", ID2)]), &open()), p("frozen key supersedes changed"));
}
#[test] fn f4_malformed_name_is_escaped() {
    let got = agent_change("DDR-\u{1b}x\ny", &open(), &open(), &ctx(&[])).unwrap();
    assert_eq!(got, "DDR-\\u{1b}x\\ny: malformed record name");
    assert!(!got.contains('\n') && !got.contains('\u{1b}'));
}
#[test] fn f5_refuse_open_to_open_adds_fixed_by_or_promoted_by() {
    assert_eq!(chg(&open(), &rec(&[("fixed_by", IN)])), p("open to open writes fixed_by"));
    assert_eq!(chg(&open(), &rec(&[("promoted_by", IN)])), p("open to open writes promoted_by"));
}
#[test] fn f5_refuse_unpromoted_ratified_gains_promoted_by() {
    let ratified = [("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", "")];
    let mut set = ratified.to_vec(); set.push(("promoted_by", IN));
    assert_eq!(chg(&rec(&ratified), &rec(&set)), p("an agent changes a ratified record only to add promoted_by"));
}
#[test] fn f5_refuse_promoted_by_replaced() {
    let mut a = PROMOTED.to_vec(); a.push(("promoted_by", IN));
    let c = agent_change(ID, &rec(&a), &with(rec(&a), |r| r.promoted_by = Some(C40.into())),
                         &RangeCtx { range: BTreeSet::from([IN.to_string(), C40.to_string()]), superseding: BTreeSet::new() });
    assert_eq!(c, p("an agent changes a ratified record only to add promoted_by"));
}
#[test] fn f6_fixed_and_superseded_rows_write_only_their_keys() {
    assert_eq!(chg(&open(), &rec(&[("status", "fixed"), ("fixed_by", IN), ("title", "T")])), p("open to fixed writes title"));
    assert_eq!(chg(&open(), &with(fixed(), |r| r.body.push_str("\nMore.\n"))), p("open to fixed writes sections"));
    let new = rec(&[("status", "fixed"), ("fixed_by", IN), ("reviewed", "2026-11-25"), ("review_note", "Wrong layer."), ("reverse_cost", "high")]);
    assert_eq!(chg(&overturned(), &new), p("overturned to fixed writes reverse_cost"));
    assert_eq!(agent_change(ID, &open(), &rec(&[("status", "superseded"), ("title", "T")]), &ctx(&[ID])), p("open to superseded writes title"));
    assert_eq!(agent_change(ID, &open(), &with(superseded(), |r| r.depends_on = vec![ID2.into()]), &ctx(&[ID])),
               p("open to superseded writes depends_on"));
}
#[test] fn f7_refuse_record_id_that_does_not_match_its_name() {
    let other = with(open(), |r| r.id = ID2.into());
    assert_eq!(chg(&Version::Absent, &other), p("record id does not match its name"));
    assert_eq!(chg(&other, &open()), p("record id does not match its name"));
    assert_eq!(chg(&open(), &with(rec(&[("kind", "assumption")]), |r| r.id = ID2.into())), p("record id does not match its name"));
}
#[test] fn f8_refuse_overturned_to_fixed_outside_range() {
    let new = rec(&[("status", "fixed"), ("fixed_by", OUT), ("reviewed", "2026-11-25"), ("review_note", "Wrong layer.")]);
    assert_eq!(chg(&overturned(), &new), p(&format!("fixed_by {OUT} is not in the node range")));
}
#[test] fn f8_refuse_superseded_named_only_for_another_id() {
    assert_eq!(agent_change(ID, &open(), &superseded(), &ctx(&[ID2])), p("superseded but no record added in the node range names it"));
}
#[test] fn f8_malformed_new_version_of_a_valid_name() {
    assert_eq!(chg(&Version::Absent, &Version::Malformed), p("malformed record"));
    assert_eq!(chg(&Version::Malformed, &Version::Malformed), p("malformed record"));
}
#[test] fn f8_refuse_open_to_fixed_with_kind_change() {
    assert_eq!(chg(&open(), &rec(&[("status", "fixed"), ("fixed_by", IN), ("kind", "assumption")])), p("frozen key kind changed"));
}
