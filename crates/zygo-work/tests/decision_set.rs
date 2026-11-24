//! DR-2: the record set, the set checks and the dependency graph (D§6.8, D§7, D§5, D§9.5, D§11.2).
use std::fs;
use std::path::{Path, PathBuf};
use zygo_work::decisions::graph::*;
use zygo_work::decisions::record::{Class, Kind, Record, Status};
// use zygo_work::decisions::set::*;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";

fn id(n: u32) -> String { format!("DDR-01JDQ2Y7Z8K4M5N6P7Q8R{n:05}") }
fn rec(n: u32, status: Status, deps: &[u32], supersedes: Option<u32>) -> Record {
    Record { id: id(n), kind: Kind::Decision, status, class: Class::None, title: "T".into(), unit: "phase-3:N12".into(),
             round: 1, role: "code".into(), model: "unknown".into(), commit: C40.into(), date: "2026-11-24".into(),
             depends_on: deps.iter().map(|d| id(*d)).collect(), reverse_cost: "low".into(), supersedes: supersedes.map(id),
             fixed_by: None, promoted_by: None, reviewed: None, review_note: None, promoted_to: None, promotion: None,
             body: BODY.into() }
}
fn open(n: u32, deps: &[u32]) -> Record { rec(n, Status::Open, deps, None) }

#[test] fn dependents_breadth_first_once_at_smallest_depth() { // D§7, D§17 #4
    let mut x = open(1, &[]); x.kind = Kind::Assumption;
    let rs = vec![x, open(2, &[1]), open(3, &[2]), open(4, &[3]), open(5, &[4, 3])]; // 5: depth 3 via 3, depth 4 via 4
    assert_eq!(dependents(&rs, &id(1)), Dependents { rows: vec![(1, id(2)), (2, id(3)), (3, id(4)), (3, id(5))], cycle_at: None });
    assert_eq!(dependents(&rs, &id(4)), Dependents { rows: vec![(1, id(5))], cycle_at: None });
    assert_eq!(dependents(&rs, &id(9)), Dependents { rows: vec![], cycle_at: None });
}
#[test] fn dependents_cut_cycle_sets_cycle_at() { // D§7
    let rs = vec![open(1, &[]), open(2, &[1, 3]), open(3, &[2])];
    assert_eq!(dependents(&rs, &id(1)), Dependents { rows: vec![(1, id(2)), (2, id(3))], cycle_at: Some(id(2)) });
    assert_eq!(dependents(&rs, &id(2)), Dependents { rows: vec![(1, id(3))], cycle_at: Some(id(2)) });
}
#[test] fn dependent_count_transitive_and_skip_superseded() { // D§5, D§11.2 step 2
    let rs = vec![open(1, &[]), open(2, &[1]), rec(3, Status::Superseded, &[1], None), open(4, &[3])];
    assert_eq!(dependent_count(&rs, &id(1), false), 3);
    assert_eq!(dependent_count(&rs, &id(1), true), 1);
    assert_eq!(dependent_count(&rs, &id(2), false), 0);
}
#[test] fn deepest_chain_cases() { // D§9.5
    assert_eq!(deepest_chain(&[]), 0);
    assert_eq!(deepest_chain(&[open(1, &[])]), 1);
    assert_eq!(deepest_chain(&[open(1, &[]), open(2, &[1])]), 2);
    assert_eq!(deepest_chain(&[open(1, &[]), open(2, &[1]), open(3, &[2]), open(4, &[1])]), 3);
    assert_eq!(deepest_chain(&[open(1, &[2]), open(2, &[1])]), 2); // cut at the first repeat
    assert_eq!(deepest_chain(&[open(1, &[9])]), 1);                // a missing target ends the path
}
