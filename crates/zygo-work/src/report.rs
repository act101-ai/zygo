//! `zygo work report [<unit>] [--since <date>]` (§13.6).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use zygo_usage::{Filter, Store, WorkUnitRow};

use crate::join::{join, Joined};

const NONE: &str = "no work units";

/// The report of the units (and their `:<part>` units) begun at or after `since`; creates no database.
pub fn report(db: &Path, unit: Option<&str>, since: Option<i64>) -> anyhow::Result<String> {
    if !db.exists() {
        return Ok(NONE.into());
    }
    let store = Store::open(db)?;
    let rows = store.work_units(&Filter { session: None, since })?;
    let sessions: BTreeSet<&str> = rows.iter().map(|r| r.session.as_str()).collect();
    let mut joined = Vec::new();
    for s in sessions {
        let units: Vec<WorkUnitRow> = rows.iter().filter(|r| r.session == s).cloned().collect();
        joined.extend(join(&units, &store.requests_for_session(s)?));
    }
    joined.retain(|j| unit.is_none_or(|u| j.row.unit == u || j.row.unit.strip_prefix(u).is_some_and(|r| r.starts_with(':'))));
    Ok(if joined.is_empty() { NONE.into() } else { render(&joined) })
}

#[derive(Default)]
struct Tokens {
    requests: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    usd: f64,
}

/// One block per unit, by earliest `began_at`, then unit; blocks joined by one blank line.
pub fn render(joined: &[Joined]) -> String {
    let mut by_unit: BTreeMap<&str, Vec<&Joined>> = BTreeMap::new();
    for j in joined {
        by_unit.entry(j.row.unit.as_str()).or_default().push(j);
    }
    let mut blocks: Vec<(i64, &str, String)> = Vec::new();
    for (unit, mut js) in by_unit {
        js.sort_by(|a, b| (a.row.round, a.row.began_at, &a.row.agent_id).cmp(&(b.row.round, b.row.began_at, &b.row.agent_id)));
        let rounds: BTreeSet<u32> = js.iter().map(|j| j.row.round).collect();
        let mut out = format!("{unit}\n  rounds: {}\n", rounds.len());
        let yn = |b: bool| if b { "yes" } else { "no" };
        let mut guards: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
        let mut tokens: BTreeMap<(&str, &str), Tokens> = BTreeMap::new();
        for j in &js {
            let r = &j.row;
            let _ = writeln!(
                out,
                "  round {} {}: check ran {}, passed {}; verdict {}",
                r.round,
                r.agent_id,
                yn(r.check_ran),
                yn(r.check_passed),
                r.verdict.as_deref().unwrap_or("none")
            );
            for (g, levels) in &r.guard_events {
                for (l, n) in levels {
                    *guards.entry(g).or_default().entry(l).or_default() += u64::from(*n);
                }
            }
            for q in &j.requests {
                let t = tokens.entry((q.role_served.as_deref().unwrap_or("-"), q.model.as_str())).or_default();
                t.requests += 1;
                t.input += q.input_tokens;
                t.output += q.output_tokens;
                t.cache_read += q.cache_read_tokens;
                t.cache_write += q.cache_write_tokens;
                t.usd += q.estimated_usd;
            }
        }
        let events: Vec<String> =
            guards.iter().flat_map(|(g, ls)| ls.iter().map(move |(l, n)| format!("{g} {l} {n}"))).collect();
        let _ = writeln!(out, "  guard events: {}", if events.is_empty() { "none".into() } else { events.join(", ") });
        for ((role, model), t) in &tokens {
            let _ = writeln!(
                out,
                "  {role} {model}: {} requests, {} in, {} out, {} cache read, {} cache write, ${:.4}",
                t.requests, t.input, t.output, t.cache_read, t.cache_write, t.usd
            );
        }
        out.pop();
        let first = js.iter().map(|j| j.row.began_at).min().unwrap_or_default();
        blocks.push((first, unit, out));
    }
    blocks.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    blocks.into_iter().map(|b| b.2).collect::<Vec<_>>().join("\n\n")
}

/// `YYYY-MM-DD` → Unix seconds at 00:00 UTC.
pub fn parse_since(s: &str) -> anyhow::Result<i64> {
    let f = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")?;
    Ok(time::Date::parse(s, &f)?.midnight().assume_utc().unix_timestamp())
}
