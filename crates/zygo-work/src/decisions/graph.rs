//! The dependency graph of the valid records (D§7, D§5, D§9.5, D§11.2). Computed at each call (D-C3).
//! Every walk uses an explicit stack or queue: a 10,000-record chain must not overflow a thread's stack.
use std::collections::{HashMap, HashSet, VecDeque};

use super::record::{Record, Status};

/// The records that depend on one record, directly or through others (D§7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependents {
    /// `(depth, id)`, each record once at its smallest depth, sorted by depth then id.
    pub rows: Vec<(u32, String)>,
    /// The first record where the walk met a cycle, if any.
    pub cycle_at: Option<String>,
}

/// The ids of the nodes that close a cycle: an iterative three-color DFS from each start in order, children in
/// listed order; each edge to a node on the current path adds that node's id.
pub(crate) fn back_edges(next: &HashMap<&str, Vec<&str>>, starts: &[&str]) -> Vec<String> {
    const GREY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color: HashMap<&str, u8> = HashMap::new();
    let mut found = Vec::new();
    for &start in starts {
        if color.contains_key(start) {
            continue;
        }
        color.insert(start, GREY);
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        while let Some(top) = stack.last_mut() {
            let (node, i) = *top;
            match next.get(node).and_then(|c| c.get(i)) {
                Some(&child) => {
                    top.1 += 1;
                    match color.get(child) {
                        Some(&GREY) => found.push(child.to_string()),
                        Some(_) => {}
                        None => {
                            color.insert(child, GREY);
                            stack.push((child, 0));
                        }
                    }
                }
                None => {
                    color.insert(node, BLACK);
                    stack.pop();
                }
            }
        }
    }
    found
}

/// The inverse edges: for each `d` in `r.depends_on`, an edge `d → r.id`; each child list sorted and deduplicated.
fn inverse<'a>(records: impl Iterator<Item = &'a Record>) -> HashMap<&'a str, Vec<&'a str>> {
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in records {
        for d in &r.depends_on {
            next.entry(d.as_str()).or_default().push(r.id.as_str());
        }
    }
    for children in next.values_mut() {
        children.sort_unstable();
        children.dedup();
    }
    next
}

fn walk<'a>(records: impl Iterator<Item = &'a Record>, id: &str) -> Dependents {
    let next = inverse(records);
    let mut seen: HashSet<&str> = HashSet::from([id]);
    let mut queue: VecDeque<(&str, u32)> = VecDeque::from([(id, 0)]);
    let mut rows = Vec::new();
    while let Some((node, depth)) = queue.pop_front() {
        for &child in next.get(node).map(Vec::as_slice).unwrap_or_default() {
            if seen.insert(child) {
                rows.push((depth + 1, child.to_string()));
                queue.push_back((child, depth + 1));
            }
        }
    }
    rows.sort();
    let cycle_at = back_edges(&next, &[id]).into_iter().next();
    Dependents { rows, cycle_at }
}

/// The dependents of `id`, breadth first (D§7).
pub fn dependents(records: &[Record], id: &str) -> Dependents {
    walk(records.iter(), id)
}

/// The number of dependents of `id` (D§5). With `skip_superseded`, `superseded` records leave the graph before the
/// walk: they are not counted and not walked through (D§11.2 step 2).
pub fn dependent_count(records: &[Record], id: &str, skip_superseded: bool) -> usize {
    let kept = records.iter().filter(|r| !(skip_superseded && r.status == Status::Superseded));
    walk(kept, id).rows.len()
}

/// The number of records on the longest `depends_on` path (D§9.5); 0 for no records. A missing target ends the
/// path; an edge back to a record on the current path counts 0.
pub fn deepest_chain(records: &[Record]) -> usize {
    let known: HashSet<&str> = records.iter().map(|r| r.id.as_str()).collect();
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in records {
        let children = next.entry(r.id.as_str()).or_default();
        children.extend(r.depends_on.iter().map(String::as_str).filter(|d| known.contains(d)));
    }
    let mut memo: HashMap<&str, usize> = HashMap::new();
    let mut on_path: HashSet<&str> = HashSet::new();
    for r in records {
        let start = r.id.as_str();
        if memo.contains_key(start) {
            continue;
        }
        on_path.insert(start);
        // (node, next child index, longest child path so far)
        let mut stack: Vec<(&str, usize, usize)> = vec![(start, 0, 0)];
        while let Some(top) = stack.last_mut() {
            let (node, i, best) = *top;
            match next.get(node).and_then(|c| c.get(i)) {
                Some(&child) => {
                    top.1 += 1;
                    if let Some(&m) = memo.get(child) {
                        top.2 = best.max(m);
                    } else if !on_path.contains(child) {
                        on_path.insert(child);
                        stack.push((child, 0, 0));
                    }
                }
                None => {
                    memo.insert(node, best + 1);
                    on_path.remove(node);
                    stack.pop();
                    if let Some(parent) = stack.last_mut() {
                        parent.2 = parent.2.max(best + 1);
                    }
                }
            }
        }
    }
    memo.values().copied().max().unwrap_or(0)
}
