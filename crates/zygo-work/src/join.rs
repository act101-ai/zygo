//! Join usage rows to work units (§13.6).

use std::collections::{BTreeMap, BTreeSet};

use zygo_core::Agent;
use zygo_usage::{RequestRow, WorkUnitRow};

#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    pub row: WorkUnitRow,
    pub requests: Vec<RequestRow>,
}

/// Per session and agent (`main` = no `agent_id`; a session with a request of a client that sends no per-agent id,
/// `ClientSpec::agent_id_header` `None`: the whole session; an id of no client: per agent id), unit *k* owns the
/// requests with `began_at` of *k* < `ts` ≤ `began_at` of *k+1*. One `Joined` per unit, in input order; requests by `ts`.
pub fn join(units: &[WorkUnitRow], requests: &[RequestRow]) -> Vec<Joined> {
    let whole_session = |r: &RequestRow| r.agent.parse::<Agent>().is_ok_and(|a| a.spec().agent_id_header.is_none());
    let whole: BTreeSet<&str> = requests.iter().filter(|r| whole_session(r)).map(|r| r.session.as_str()).collect();
    let key = |session: &str, agent: &str| (session.to_string(), if whole.contains(session) { String::new() } else { agent.to_string() });
    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, u) in units.iter().enumerate() {
        groups.entry(key(&u.session, &u.agent_id)).or_default().push(i);
    }
    for g in groups.values_mut() {
        g.sort_by_key(|&i| (units[i].began_at, i));
    }
    let mut owned: Vec<Vec<RequestRow>> = vec![Vec::new(); units.len()];
    let mut order: Vec<usize> = (0..requests.len()).collect();
    order.sort_by_key(|&i| (requests[i].ts, i));
    for i in order {
        let r = &requests[i];
        let Some(g) = groups.get(&key(&r.session, r.agent_id.as_deref().unwrap_or("main"))) else { continue };
        let pos = g.partition_point(|&k| units[k].began_at < r.ts);
        if pos > 0 {
            owned[g[pos - 1]].push(r.clone());
        }
    }
    units.iter().zip(owned).map(|(u, requests)| Joined { row: u.clone(), requests }).collect()
}
