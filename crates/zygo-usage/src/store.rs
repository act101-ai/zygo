use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use rusqlite::{named_params, Connection, ErrorCode, OpenFlags, Transaction, TransactionBehavior};

use crate::schema::*;

pub struct Store {
    pub(crate) conn: Connection,
}

const INSERT_REQUEST: &str = "INSERT INTO requests (ts, session, token_label, agent, agent_version, agent_id, agent_type, request_class, compaction, route, role_requested, role_served, model, provider, profile, passthrough, status, latency_ms, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, estimated_usd, adjustments, redactions, repo, work_item) VALUES (:ts, :session, :token_label, :agent, :agent_version, :agent_id, :agent_type, :request_class, :compaction, :route, :role_requested, :role_served, :model, :provider, :profile, :passthrough, :status, :latency_ms, :input_tokens, :output_tokens, :cache_read_tokens, :cache_write_tokens, :estimated_usd, :adjustments, :redactions, :repo, :work_item)";

fn bind_request(stmt: &mut rusqlite::Statement<'_>, r: &RequestRow) -> Result<()> {
    if !r.estimated_usd.is_finite() {
        return Err(anyhow!("estimated_usd is not finite: {}", r.estimated_usd));
    }
    stmt.execute(named_params! {
        ":ts": r.ts,
        ":session": r.session,
        ":token_label": r.token_label,
        ":agent": r.agent,
        ":agent_version": r.agent_version,
        ":agent_id": r.agent_id,
        ":agent_type": r.agent_type,
        ":request_class": r.request_class,
        ":compaction": r.compaction,
        ":route": r.route,
        ":role_requested": r.role_requested,
        ":role_served": r.role_served,
        ":model": r.model,
        ":provider": r.provider,
        ":profile": r.profile,
        ":passthrough": r.passthrough,
        ":status": r.status,
        ":latency_ms": i64::try_from(r.latency_ms)?,
        ":input_tokens": i64::try_from(r.input_tokens)?,
        ":output_tokens": i64::try_from(r.output_tokens)?,
        ":cache_read_tokens": i64::try_from(r.cache_read_tokens)?,
        ":cache_write_tokens": i64::try_from(r.cache_write_tokens)?,
        ":estimated_usd": r.estimated_usd,
        ":adjustments": serde_json::to_string(&r.adjustments)?,
        ":redactions": serde_json::to_string(&r.redactions)?,
        ":repo": r.repo,
        ":work_item": r.work_item,
    })?;
    Ok(())
}

/// One `upstream_errors` row on `conn` (a connection or an open transaction).
fn insert_error(conn: &Connection, e: &UpstreamErrorRow) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO upstream_errors (ts, session, request_class, model, status, error_type, error_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?.execute(rusqlite::params![e.ts, e.session, e.request_class, e.model, e.status, e.error_type, e.error_message])?;
    Ok(())
}

/// The schema version this build writes to `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 1;

const VERDICTS: [&str; 3] = ["keep", "discard", "rework"];

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let start = Instant::now();
        loop {
            let attempt = (|| -> rusqlite::Result<i64> {
                let mode: String =
                    conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
                if mode != "wal" {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "journal_mode is {mode}"
                    )));
                }
                let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                if version > SCHEMA_VERSION {
                    return Ok(version);
                }
                conn.execute_batch(SCHEMA)?;
                if version < SCHEMA_VERSION {
                    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                }
                Ok(SCHEMA_VERSION)
            })();
            match attempt {
                Ok(version) if version > SCHEMA_VERSION => {
                    return Err(anyhow!(
                        "usage database schema version {version} is newer than supported version {SCHEMA_VERSION}"
                    ));
                }
                Ok(_) => break,
                Err(e) if is_busy(&e) && start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(e.into()),
            }
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Store { conn })
    }

    /// For readers on a short deadline (the status line): no directory or file is created, no migration, no busy
    /// wait. A database of another schema version is an error.
    pub fn open_read_only(path: &Path) -> Result<Store> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::ZERO)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(anyhow!(
                "usage database schema version {version} is not {SCHEMA_VERSION}"
            ));
        }
        Ok(Store { conn })
    }

    #[doc(hidden)]
    pub fn busy_timeout_ms(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))?)
    }

    pub fn insert_request(&self, r: &RequestRow) -> Result<()> {
        bind_request(&mut *self.conn.prepare_cached(INSERT_REQUEST)?, r)
    }

    /// One `upstream_errors` row (§14, FIX-10).
    pub fn insert_upstream_error(&self, row: &UpstreamErrorRow) -> Result<()> {
        insert_error(&self.conn, row)
    }

    /// The request row and its `upstream_errors` row in one `BEGIN IMMEDIATE` transaction: both or neither (FIX-10 M3).
    pub fn insert_request_and_error(&self, r: &RequestRow, e: &UpstreamErrorRow) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        bind_request(&mut *tx.prepare_cached(INSERT_REQUEST)?, r)?;
        insert_error(&tx, e)?;
        tx.commit()?;
        Ok(())
    }

    /// The `upstream_errors` rows of `session`, by `ts`, then insertion order.
    pub fn upstream_errors(&self, session: &str) -> Result<Vec<UpstreamErrorRow>> {
        let mut q = self.conn.prepare(
            "SELECT ts, session, request_class, model, status, error_type, error_message FROM upstream_errors WHERE session = ?1 ORDER BY ts, rowid",
        )?;
        let rows = q.query_map([session], |r| {
            Ok(UpstreamErrorRow { ts: r.get(0)?, session: r.get(1)?, request_class: r.get(2)?, model: r.get(3)?, status: r.get(4)?,
                error_type: r.get(5)?, error_message: r.get(6)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Serve-mode replay: inserts every row in one `BEGIN IMMEDIATE` transaction, or none
    /// when the session already has any `requests` row. All rows must share one session.
    /// Returns the number of rows inserted.
    pub fn insert_requests(&self, rows: &[RequestRow]) -> Result<usize> {
        let Some(first) = rows.first() else { return Ok(0) };
        if let Some(r) = rows.iter().find(|r| r.session != first.session) {
            return Err(anyhow!(
                "insert_requests: rows span sessions {:?} and {:?}",
                first.session,
                r.session
            ));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let stored: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM requests WHERE session = ?1)",
            [&first.session],
            |r| r.get(0),
        )?;
        if stored {
            return Ok(0);
        }
        {
            let mut stmt = tx.prepare_cached(INSERT_REQUEST)?;
            for r in rows {
                bind_request(&mut stmt, r)?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// A serve session whose server rows are not (all) local yet; `zygo usage --sync` retries it (§14).
    pub fn mark_sync_pending(&self, session: &str) -> Result<()> {
        self.conn.execute("INSERT OR IGNORE INTO sync_pending (session) VALUES (?1)", [session])?;
        Ok(())
    }

    pub fn clear_sync_pending(&self, session: &str) -> Result<()> {
        self.conn.execute("DELETE FROM sync_pending WHERE session = ?1", [session])?;
        Ok(())
    }

    /// `zygo usage --sync`: the session's rows become `rows` (delete, then insert, in one `BEGIN IMMEDIATE`); every row
    /// must carry `session`.
    pub fn replace_requests(&self, session: &str, rows: &[RequestRow]) -> Result<usize> {
        if let Some(r) = rows.iter().find(|r| r.session != session) {
            return Err(anyhow!("replace_requests: a row of session {:?} for session {session:?}", r.session));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM requests WHERE session = ?1", [session])?;
        tx.execute("DELETE FROM upstream_errors WHERE session = ?1", [session])?;
        {
            let mut stmt = tx.prepare_cached(INSERT_REQUEST)?;
            for r in rows {
                bind_request(&mut stmt, r)?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    pub fn insert_trim(&self, t: &TrimRow) -> Result<()> {
        self.conn.prepare_cached(
            "INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?.execute((
            t.ts,
            &t.session,
            &t.agent_id,
            &t.tool,
            &t.kind,
            i64::try_from(t.stored_bytes)?,
            i64::try_from(t.view_bytes)?,
        ))?;
        Ok(())
    }

    pub fn insert_work_unit(&self, u: &WorkUnitRow) -> Result<()> {
        if let Some(v) = u.verdict.as_deref().filter(|v| !VERDICTS.contains(v)) {
            return Err(anyhow!("invalid verdict {v:?}: expected keep, discard or rework"));
        }
        self.conn.prepare_cached(
            "INSERT INTO work_units (unit, round, session, agent_id, agent_type, began_at, ended_at, tool_calls, failed_calls, check_ran, check_passed, guard_events, levels, verdict) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) ON CONFLICT(session, agent_id, unit, round, began_at) DO UPDATE SET agent_type = excluded.agent_type, ended_at = excluded.ended_at, tool_calls = excluded.tool_calls, failed_calls = excluded.failed_calls, check_ran = excluded.check_ran, check_passed = excluded.check_passed, guard_events = excluded.guard_events, levels = excluded.levels, verdict = COALESCE(excluded.verdict, work_units.verdict)",
        )?.execute(rusqlite::params![
            u.unit,
            u.round,
            u.session,
            u.agent_id,
            u.agent_type,
            u.began_at,
            u.ended_at,
            u.tool_calls,
            u.failed_calls,
            u.check_ran,
            u.check_passed,
            serde_json::to_string(&u.guard_events)?,
            serde_json::to_string(&u.levels)?,
            u.verdict,
        ])?;
        Ok(())
    }

    pub fn prune(&self, retain_days: u32) -> Result<()> {
        if retain_days == 0 {
            return Ok(());
        }
        let cutoff = time::OffsetDateTime::now_utc().unix_timestamp() - i64::from(retain_days) * 86_400;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM requests WHERE ts < ?1", [cutoff])?;
        tx.execute("DELETE FROM upstream_errors WHERE ts < ?1", [cutoff])?;
        tx.execute("DELETE FROM trims WHERE ts < ?1", [cutoff])?;
        tx.execute("DELETE FROM work_units WHERE began_at < ?1", [cutoff])?;
        tx.commit()?;
        Ok(())
    }

    /// Records the verdict on every row of the unit and round (all agents).
    /// Returns true when at least one row changed.
    pub fn set_verdict(&self, session: &str, unit: &str, round: u32, verdict: &str) -> Result<bool> {
        if !VERDICTS.contains(&verdict) {
            return Err(anyhow!("invalid verdict {verdict:?}: expected keep, discard or rework"));
        }
        let n = self.conn.execute(
            "UPDATE work_units SET verdict = ?1 WHERE session = ?2 AND unit = ?3 AND round = ?4",
            rusqlite::params![verdict, session, unit, round],
        )?;
        Ok(n > 0)
    }
}
