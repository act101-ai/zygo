use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{schema::*, store::Store};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub session: Option<String>,
    pub since: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoleModel {
    pub role: String,
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub estimated_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_share: f64,
    pub estimated_usd: f64,
    pub bytes_trimmed: u64,
    pub compaction_signals: u64,
    pub compactions_observed: u64,
    pub by_role_model: Vec<RoleModel>,
    pub guard_events: BTreeMap<String, BTreeMap<String, u64>>,
}

/// Reads a stored INTEGER as u64; a negative value is a conversion error, not a wrap.
fn u64_at(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u64> {
    let v: i64 = r.get(i)?;
    u64::try_from(v).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Integer, Box::new(e))
    })
}

impl Store {
    /// The sessions marked by `mark_sync_pending`, sorted.
    pub fn sync_pending(&self) -> Result<Vec<String>> {
        let mut q = self.conn.prepare("SELECT session FROM sync_pending ORDER BY session")?;
        let ids = q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    pub fn requests_for_session(&self, id: &str) -> Result<Vec<RequestRow>> {
        let mut q = self.conn.prepare(
            "SELECT ts, session, token_label, agent, agent_version, agent_id, agent_type, request_class, compaction, route, role_requested, role_served, model, provider, profile, passthrough, status, latency_ms, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, estimated_usd, adjustments, redactions, repo, work_item FROM requests WHERE session = ?1 ORDER BY ts, rowid",
        )?;
        let rows = q.query_map([id], |r| {
            let adjustments: String = r.get(23)?;
            let redactions: String = r.get(24)?;
            let json = |e: serde_json::Error| {
                rusqlite::Error::FromSqlConversionFailure(
                    23,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            };
            Ok(RequestRow {
                ts: r.get(0)?,
                session: r.get(1)?,
                token_label: r.get(2)?,
                agent: r.get(3)?,
                agent_version: r.get(4)?,
                agent_id: r.get(5)?,
                agent_type: r.get(6)?,
                request_class: r.get(7)?,
                compaction: r.get(8)?,
                route: r.get(9)?,
                role_requested: r.get(10)?,
                role_served: r.get(11)?,
                model: r.get(12)?,
                provider: r.get(13)?,
                profile: r.get(14)?,
                passthrough: r.get(15)?,
                status: r.get(16)?,
                latency_ms: u64_at(r, 17)?,
                input_tokens: u64_at(r, 18)?,
                output_tokens: u64_at(r, 19)?,
                cache_read_tokens: u64_at(r, 20)?,
                cache_write_tokens: u64_at(r, 21)?,
                estimated_usd: r.get(22)?,
                adjustments: serde_json::from_str(&adjustments).map_err(json)?,
                redactions: serde_json::from_str(&redactions).map_err(json)?,
                repo: r.get(25)?,
                work_item: r.get(26)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Work-unit rows by session, `began_at` (≥ `since`), agent id.
    pub fn work_units(&self, f: &Filter) -> Result<Vec<WorkUnitRow>> {
        let mut q = self.conn.prepare(
            "SELECT unit, round, session, agent_id, agent_type, began_at, ended_at, tool_calls, failed_calls, check_ran, check_passed, guard_events, levels, verdict FROM work_units WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR began_at >= ?2) ORDER BY session, began_at, agent_id",
        )?;
        let rows = q.query_map(rusqlite::params![f.session, f.since], |r| {
            let guard_events: String = r.get(11)?;
            let levels: String = r.get(12)?;
            let json = |i: usize| {
                move |e: serde_json::Error| {
                    rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e))
                }
            };
            Ok(WorkUnitRow {
                unit: r.get(0)?,
                round: r.get(1)?,
                session: r.get(2)?,
                agent_id: r.get(3)?,
                agent_type: r.get(4)?,
                began_at: r.get(5)?,
                ended_at: r.get(6)?,
                tool_calls: r.get(7)?,
                failed_calls: r.get(8)?,
                check_ran: r.get(9)?,
                check_passed: r.get(10)?,
                guard_events: serde_json::from_str(&guard_events).map_err(json(11))?,
                levels: serde_json::from_str(&levels).map_err(json(12))?,
                verdict: r.get(13)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn summary(&self, f: &Filter) -> Result<Summary> {
        let p = rusqlite::params![f.session, f.since];
        let mut sum = Summary::default();
        // One deferred read transaction: every part below reads the same snapshot.
        let tx = self.conn.unchecked_transaction()?;
        tx.query_row(
            "SELECT count(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(estimated_usd),0.0), COALESCE(SUM(EXISTS (SELECT 1 FROM json_each(adjustments) WHERE value IN ('context_signal','context_safety','context_compact'))),0), COALESCE(SUM(compaction),0) FROM requests WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2)",
            p,
            |r| {
                sum.requests = u64_at(r, 0)?;
                sum.input_tokens = u64_at(r, 1)?;
                sum.output_tokens = u64_at(r, 2)?;
                sum.cache_read_tokens = u64_at(r, 3)?;
                sum.cache_write_tokens = u64_at(r, 4)?;
                sum.estimated_usd = r.get(5)?;
                sum.compaction_signals = u64_at(r, 6)?;
                sum.compactions_observed = u64_at(r, 7)?;
                Ok(())
            },
        )?;
        let denom = sum.input_tokens + sum.cache_read_tokens + sum.cache_write_tokens;
        if denom > 0 {
            sum.cache_read_share = sum.cache_read_tokens as f64 / denom as f64;
        }
        let mut groups = tx.prepare(
            "SELECT COALESCE(role_served,''), model, count(*), SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(estimated_usd) FROM requests WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2) GROUP BY COALESCE(role_served,''), model ORDER BY 1, 2",
        )?;
        sum.by_role_model = groups
            .query_map(p, |r| {
                Ok(RoleModel {
                    role: r.get(0)?,
                    model: r.get(1)?,
                    requests: u64_at(r, 2)?,
                    input_tokens: u64_at(r, 3)?,
                    output_tokens: u64_at(r, 4)?,
                    cache_read_tokens: u64_at(r, 5)?,
                    cache_write_tokens: u64_at(r, 6)?,
                    estimated_usd: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        sum.bytes_trimmed = tx.query_row(
            "SELECT COALESCE(SUM(MAX(stored_bytes - view_bytes, 0)), 0) FROM trims WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2)",
            p,
            |r| u64_at(r, 0),
        )?;
        let mut q = tx.prepare(
            "SELECT guard_events FROM work_units WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR began_at >= ?2)",
        )?;
        let blobs = q
            .query_map(p, |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop((groups, q));
        tx.finish()?;
        for b in blobs {
            let m: BTreeMap<String, BTreeMap<String, u64>> = serde_json::from_str(&b)?;
            for (guard, levels) in m {
                for (level, n) in levels {
                    *sum.guard_events.entry(guard.clone()).or_default().entry(level).or_default() += n;
                }
            }
        }
        Ok(sum)
    }
}
