//! O§6.2-O§6.6: the net savings of trims less read-backs, each priced by the requests of its carry context.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use rusqlite::Transaction;
use zygo_core::{tokens::BYTES_PER_TOKEN, Agent};

use crate::{carry::{self, CtxRequest, Rates}, query::{summary_in, u64_at, Filter, Summary}, store::Store};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Savings {
    pub bytes_read_back: u64,
    pub kept_out_bytes: i64,
    pub kept_out_tokens: i64,
    pub spend_usd: f64,
    /// Distinct `requests` rowids that carried an event but had no price.
    pub unpriced: u64,
    pub views: u64,
    pub readbacks: u64,
    /// Largest `kept_out_bytes` first.
    pub by_key: Vec<SavingsKey>,
}

/// One O§6.6 key; `command` and `shaper` are `"-"` for null, all three for a read-back with no trim row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavingsKey {
    pub tool: String,
    pub command: String,
    pub shaper: String,
    pub views: u64,
    pub readbacks: u64,
    pub kept_out_bytes: i64,
    pub spend_usd: f64,
}

/// `(session, agent_id)`: `None` is the main loop.
type Ctx = (String, Option<String>);
type Key = (String, String, String);

/// A trim (`bytes` > 0 or 0) or a read-back (`bytes` < 0, `readback`).
struct Event {
    ts: i64,
    bytes: i64,
    readback: bool,
    ctx: Ctx,
    key: Key,
}

/// The first trim row of a `result_id`: `(session, agent_id, tool, command, shaper)`.
type Trim = (String, Option<String>, String, Option<String>, Option<String>);

fn dash(v: Option<String>) -> String { v.unwrap_or_else(|| "-".into()) }

fn has_table(tx: &Transaction<'_>, name: &str) -> Result<bool> {
    Ok(tx.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [name],
        |r| r.get::<_, i64>(0))? == 1)
}

/// O§6.5 item 5: the select expression of a `trims` column, `NULL` on a database without it.
fn trim_col(cols: &[String], name: &'static str) -> &'static str {
    if cols.iter().any(|c| c == name) { name } else { "NULL" }
}

/// O§6.3 step 1: `Q_C` in `ts`, `rowid` order, and the OpenCode flag of its session.
fn context(tx: &Transaction<'_>, ctx: &Ctx) -> Result<(Vec<CtxRequest>, bool)> {
    let mut s = tx.prepare_cached(
        "SELECT rowid, ts, model, provider, input_tokens, cache_read_tokens, cache_write_tokens, compaction FROM requests \
         WHERE session = ?1 AND status BETWEEN 200 AND 299 AND CASE WHEN ?2 IS NULL THEN agent_id IS NULL AND \
         (request_class IS NULL OR request_class IN ('main', 'compaction')) ELSE agent_id = ?2 END ORDER BY ts, rowid",
    )?;
    let q = s.query_map(rusqlite::params![ctx.0, ctx.1], |r| {
        Ok(CtxRequest { rowid: r.get(0)?, ts: r.get(1)?, model: r.get(2)?, provider: r.get(3)?, input_tokens: u64_at(r, 4)?,
            cache_read_tokens: u64_at(r, 5)?, cache_write_tokens: u64_at(r, 6)?, compaction: r.get(7)? })
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let opencode = tx.query_row("SELECT EXISTS (SELECT 1 FROM requests WHERE session = ?1 AND agent = ?2)",
        rusqlite::params![ctx.0, Agent::OpenCode.as_str()], |r| r.get(0))?;
    Ok((q, opencode))
}

fn trim_events(tx: &Transaction<'_>, f: &Filter, cols: &[String]) -> Result<Vec<Event>> {
    let sql = format!(
        "SELECT ts, session, agent_id, tool, MAX(stored_bytes - view_bytes, 0), {}, {} FROM trims \
         WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2) ORDER BY ts, rowid",
        trim_col(cols, "command"), trim_col(cols, "shaper"));
    let mut s = tx.prepare(&sql)?;
    let rows = s.query_map(rusqlite::params![f.session, f.since], |r| {
        Ok(Event { ts: r.get(0)?, bytes: r.get(4)?, readback: false, ctx: (r.get(1)?, r.get(2)?),
            key: (r.get(3)?, dash(r.get(5)?), dash(r.get(6)?)) })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// O§6.4: a read-back is carried in its reading session, by the trim's agent context when it reads its own session.
fn readback_events(tx: &Transaction<'_>, f: &Filter, cols: &[String]) -> Result<Vec<Event>> {
    let mut s = tx.prepare(
        "SELECT ts, read_in, result_id, bytes FROM readbacks WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2) \
         ORDER BY ts, rowid",
    )?;
    let rows = s.query_map(rusqlite::params![f.session, f.since], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    // One pass over `trims`: `result_id` has no index, so a query per read-back would scan the table each time.
    let mut trims: HashMap<String, (i64, Trim)> = HashMap::new();
    if cols.iter().any(|c| c == "result_id") {
        let sql = format!(
            "SELECT result_id, rowid, session, agent_id, tool, {}, {} FROM trims WHERE result_id IN \
             (SELECT result_id FROM readbacks WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2))",
            trim_col(cols, "command"), trim_col(cols, "shaper"));
        let mut s = tx.prepare(&sql)?;
        let found = s.query_map(rusqlite::params![f.session, f.since], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?,
                (r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
        })?;
        for row in found {
            let (rid, rowid, trim) = row?;
            match trims.get(&rid) {
                Some((lowest, _)) if *lowest <= rowid => {}
                _ => { trims.insert(rid, (rowid, trim)); }
            }
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    for (ts, read_in, rid, bytes) in rows {
        let trim = trims.get(&rid).map(|(_, t)| t.clone());
        let (ctx, key) = match trim {
            Some((session, agent_id, tool, command, shaper)) => {
                let agent_id = if session == read_in { agent_id } else { None };
                ((read_in, agent_id), (tool, dash(command), dash(shaper)))
            }
            None => ((read_in, None), ("-".into(), "-".into(), "-".into())),
        };
        out.push(Event { ts, bytes: bytes.saturating_neg(), readback: true, ctx, key });
    }
    Ok(out)
}

impl Store {
    /// O§6.6: the summary and the savings of `f` from one read snapshot (FX-8). Both values come from one read
    /// transaction so the figures describe one row set. Savings is read first so that `tests/report_snapshot.rs`
    /// can see a write made from the rates callback; do not split the transaction or reverse the order.
    pub fn report(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<(Summary, Savings)> {
        // One deferred read transaction. Savings first: `rates` runs inside it, so the summary reads after
        // the callback and only the shared snapshot keeps a concurrent write out of both.
        let tx = self.conn.unchecked_transaction()?;
        let v = savings_in(&tx, f, rates)?;
        let s = summary_in(&tx, f)?;
        tx.finish()?;
        Ok((s, v))
    }

    /// O§6.6: the filter selects events only; each event's `Q_C` is read without it.
    pub fn savings(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<Savings> {
        // One deferred read transaction: every figure comes from the same snapshot.
        let tx = self.conn.unchecked_transaction()?;
        savings_in(&tx, f, rates)
    }
}

/// O§6.6: the savings of `f`, read through `tx` (one snapshot); the filter selects events only.
fn savings_in(tx: &Transaction<'_>, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<Savings> {
    let cols = tx.prepare("PRAGMA table_info(trims)")?
        .query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut events = trim_events(tx, f, &cols)?;
    if has_table(tx, "readbacks")? {
        events.extend(readback_events(tx, f, &cols)?);
    }
    let mut v = Savings::default();
    let mut contexts: HashMap<Ctx, (Vec<CtxRequest>, bool)> = HashMap::new();
    let mut unpriced = BTreeSet::new();
    let mut keys: BTreeMap<Key, SavingsKey> = BTreeMap::new();
    for e in events {
        if !contexts.contains_key(&e.ctx) {
            let c = context(tx, &e.ctx)?;
            contexts.insert(e.ctx.clone(), c);
        }
        let (q, first_only) = &contexts[&e.ctx];
        let c = carry::carry(e.ts, q, *first_only, rates);
        unpriced.extend(c.unpriced);
        let spend = e.bytes as f64 / BYTES_PER_TOKEN as f64 * c.usd_per_token;
        let k = keys.entry(e.key.clone()).or_insert_with(|| SavingsKey {
            tool: e.key.0, command: e.key.1, shaper: e.key.2, ..Default::default()
        });
        if e.readback {
            v.readbacks += 1;
            k.readbacks += 1;
            v.bytes_read_back = v.bytes_read_back.saturating_add(e.bytes.unsigned_abs());
        } else {
            v.views += 1;
            k.views += 1;
        }
        k.kept_out_bytes = k.kept_out_bytes.saturating_add(e.bytes);
        k.spend_usd += spend;
        v.kept_out_bytes = v.kept_out_bytes.saturating_add(e.bytes);
        v.spend_usd += spend;
    }
    let n = v.kept_out_bytes;
    v.kept_out_tokens = n.signum() * n.unsigned_abs().div_ceil(BYTES_PER_TOKEN) as i64;
    v.unpriced = unpriced.len() as u64;
    v.by_key = keys.into_values().collect();
    v.by_key.sort_by_key(|k| std::cmp::Reverse(k.kept_out_bytes));
    Ok(v)
}
