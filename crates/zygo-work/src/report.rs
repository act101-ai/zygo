//! `zygo work report [<unit>] [--since <date>]` (§13.6).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use zygo_usage::{Filter, Store, WorkUnitRow};

use crate::decisions::graph::deepest_chain;
use crate::decisions::record::{name_kind, NameKind, Record, Status};
use crate::decisions::set::{read_dir, DirError, RecordSet};
use crate::join::{join, Joined};

const NONE: &str = "no work units";

/// The report of the units (and their `:<part>` units) begun at or after `since`; creates no database.
/// `records_in` is the repository top whose `docs/decisions/` gives the record lines (D§9.5); `None` gives none.
pub fn report(db: &Path, records_in: Option<&Path>, unit: Option<&str>, since: Option<i64>) -> anyhow::Result<String> {
    let read = records_in.map(read_dir);
    let valid: &[Record] = match &read {
        Some(Ok(s)) => &s.valid,
        _ => &[],
    };
    let body = blocks(db, valid, unit, since)?;
    Ok(match read.as_ref().and_then(records_line) {
        Some(l) => format!("{body}\n\n{l}"),
        None => body,
    })
}

/// `u` is `unit` or `unit:<part>`.
fn under(u: &str, unit: &str) -> bool {
    u == unit || u.strip_prefix(unit).is_some_and(|r| r.starts_with(':'))
}

/// The `decision records:` line (D§9.5, D§6.8); `None` when the directory holds no record name.
fn records_line(read: &Result<RecordSet, DirError>) -> Option<String> {
    let set = match read {
        Err(DirError::NotDirectory) => return Some("decision records: docs/decisions is not a directory".into()),
        Err(DirError::TooMany) => return Some("decision records: over 10,000 files".into()),
        Ok(set) => set,
    };
    let named = set.malformed.iter().any(|(n, _)| matches!(name_kind(n), NameKind::Record(_)));
    if set.valid.is_empty() && !named {
        return None;
    }
    let open = set.valid.iter().filter(|r| r.status == Status::Open).count();
    let mut l = format!(
        "decision records: {open} open, {} total, deepest chain {}",
        set.valid.len(),
        deepest_chain(&set.valid)
    );
    if !set.malformed.is_empty() {
        let _ = write!(l, ", {} malformed", set.malformed.len());
    }
    Some(l)
}

fn blocks(db: &Path, records: &[Record], unit: Option<&str>, since: Option<i64>) -> anyhow::Result<String> {
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
    joined.retain(|j| unit.is_none_or(|u| under(&j.row.unit, u)));
    Ok(if joined.is_empty() { NONE.into() } else { render(&joined, records) })
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

/// One block per unit, by earliest `began_at`, then unit; blocks joined by one blank line. A block with records of
/// its unit (or its `:<part>` units) gains the `decisions:` line (D§9.5).
pub fn render(joined: &[Joined], records: &[Record]) -> String {
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
        let mine: Vec<&Record> = records.iter().filter(|r| under(&r.unit, unit)).collect();
        if !mine.is_empty() {
            let open = mine.iter().filter(|r| r.status == Status::Open).count();
            let _ = writeln!(out, "  decisions: {open} open, {} total", mine.len());
        }
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
