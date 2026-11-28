//! The order of `zygo decision review` (D§11.2 steps 1, 2 and 4).
use std::collections::{HashMap, HashSet, VecDeque};

use super::graph::{count_in, dependents, inverse_of};
use super::record::{Kind, Record, Status};

/// The open records of one `zygo decision review` in the D§11.2 order.
pub struct Queue {
    /// The ids that `next` has not returned yet, front first.
    order: VecDeque<String>,
    /// Each id's step 2 position.
    rank: HashMap<String, usize>,
    /// Each id's dependent count (step 2).
    count: HashMap<String, usize>,
    /// id -> the overturned record that last moved it to the front.
    cause: HashMap<String, String>,
    /// The ids that `next` has returned.
    handed: HashSet<String>,
}

impl Queue {
    /// The `open` records of `records` of `kind` whose unit starts with `unit_prefix`, by dependent count over the
    /// records that are not `superseded`, largest first, then id (D§11.2 step 2).
    pub fn new(records: &[Record], kind: Option<Kind>, unit_prefix: Option<&str>) -> Queue {
        let inv = inverse_of(records, true);
        let mut pairs: Vec<(usize, String)> = records
            .iter()
            .filter(|r| {
                r.status == Status::Open
                    && kind.is_none_or(|k| r.kind == k)
                    && unit_prefix.is_none_or(|p| r.unit.starts_with(p))
            })
            .map(|r| (count_in(&inv, &r.id), r.id.clone()))
            .collect();
        pairs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let rank = pairs.iter().enumerate().map(|(i, (_, id))| (id.clone(), i)).collect();
        let count = pairs.iter().map(|(n, id)| (id.clone(), *n)).collect();
        let order = pairs.into_iter().map(|(_, id)| id).collect();
        Queue { order, rank, count, cause: HashMap::new(), handed: HashSet::new() }
    }

    /// The next id to show.
    #[allow(clippy::should_implement_trait)] // `next` is the Produces name; `Queue` is not an iterator
    pub fn next(&mut self) -> Option<String> {
        let id = self.order.pop_front()?;
        self.handed.insert(id.clone());
        Some(id)
    }

    /// Puts the open dependents of the overturned `id` (over the records that are not `superseded`) at the front, in
    /// step 2 order, each once (D§11.2 step 5). A dependent is one of this queue's records other than `id` that is
    /// `open` in `records`: the caller passes the statuses as written in this review, so a dependent that `next`
    /// returned and that is still `open` was skipped and is queued again.
    pub fn overturned(&mut self, id: &str, records: &[Record]) {
        let live: Vec<Record> = records.iter().filter(|r| r.status != Status::Superseded).cloned().collect();
        let open: HashSet<&str> = live.iter().filter(|r| r.status == Status::Open).map(|r| r.id.as_str()).collect();
        let deps: HashSet<String> = dependents(&live, id)
            .rows
            .into_iter()
            .map(|(_, d)| d)
            .filter(|d| d != id && open.contains(d.as_str()) && self.rank.contains_key(d))
            .collect();
        let (queued, rest): (Vec<String>, Vec<String>) = self.order.drain(..).partition(|q| deps.contains(q));
        let again = deps.iter().filter(|d| self.handed.contains(*d)).cloned();
        let mut moved: Vec<String> = queued.into_iter().chain(again).collect();
        // Ranks are distinct, so one id queued and handed out sits next to itself after the sort.
        moved.sort_by_key(|m| self.rank.get(m).copied().unwrap_or(usize::MAX));
        moved.dedup();
        for m in &moved {
            self.cause.insert(m.clone(), id.to_string());
        }
        self.order = moved.into_iter().chain(rest).collect();
    }

    /// The ids not yet returned (the `N` of `[i/N]` at the start).
    pub fn remaining(&self) -> usize {
        self.order.len()
    }

    /// The step 2 count of a queued id; 0 for another id.
    pub fn dependents(&self, id: &str) -> usize {
        self.count.get(id).copied().unwrap_or(0)
    }

    /// The overturned record that last moved `id` to the front.
    pub fn cause(&self, id: &str) -> Option<&str> {
        self.cause.get(id).map(String::as_str)
    }
}
