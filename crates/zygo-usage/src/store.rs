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

const INSERT_TRIM: &str = "INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes, result_id, command, shaper) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";
const INSERT_TRIM_SEVEN: &str = "INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

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

/// The longest wait of each usage-store statement in the trim hook and the MCP server's read-back write
/// (O§7.11, O-C10).
pub const HOOK_DB_BOUND: Duration = Duration::from_millis(100);

/// The `trims` columns the full open adds to an older table (O§6.5 item 3), in order.
const TRIM_COLUMNS: [(&str, &str); 3] = [("result_id", "TEXT"), ("command", "TEXT"), ("shaper", "TEXT")];
/// The database paths whose `trims` columns this process has checked (O§6.5 item 3: one time per process).
static TRIM_COLUMNS_CHECKED: std::sync::Mutex<std::collections::BTreeSet<std::path::PathBuf>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// The message of an SQLite failure, when there is one.
fn sqlite_message(e: &rusqlite::Error) -> Option<&str> {
    match e { rusqlite::Error::SqliteFailure(_, Some(m)) => Some(m), _ => None }
}

/// The `TRIM_COLUMNS` entries that `PRAGMA table_xinfo(trims)` lacks (`table_info` hides generated columns), plus each name in `report_missing` (the O§16
/// item 15 seam), in order.
fn missing_trim_columns(conn: &Connection, report_missing: &[&str]) -> rusqlite::Result<Vec<(&'static str, &'static str)>> {
    let have: Vec<String> = conn.prepare("PRAGMA table_xinfo(trims)")?
        .query_map([], |r| r.get(1))?.collect::<rusqlite::Result<_>>()?;
    Ok(TRIM_COLUMNS.into_iter().filter(|(name, _)| !have.iter().any(|h| h == name) || report_missing.contains(name)).collect())
}

/// Adds each column of `missing_trim_columns`. "duplicate column name" means another process added it first: success.
fn add_trim_columns(conn: &Connection, report_missing: &[&str]) -> rusqlite::Result<()> {
    for (name, ty) in missing_trim_columns(conn, report_missing)? {
        match conn.execute_batch(&format!("ALTER TABLE trims ADD COLUMN {name} {ty}")) {
            Err(e) if !sqlite_message(&e).is_some_and(|m| m.starts_with("duplicate column name")) => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

const VERDICTS: [&str; 3] = ["keep", "discard", "rework"];

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

impl Store {
    /// The full open: creates the file and the schema, and adds each missing `trims` column one time per process for
    /// this path (O§6.5 item 3). The hook and the MCP server's read-back write move to the bounded open (O§7.11); until
    /// then they call this open.
    pub fn open(path: &Path) -> Result<Store> {
        let checked = TRIM_COLUMNS_CHECKED.lock().unwrap_or_else(|e| e.into_inner()).contains(path);
        Store::open_full(path, if checked { None } else { Some(&[] as &[&str]) })
    }

    /// `open`, with the column check always run and each name of `report_missing` treated as missing (O§16 item 15).
    #[doc(hidden)]
    pub fn open_with_column_check(path: &Path, report_missing: &[&str]) -> Result<Store> {
        Store::open_full(path, Some(report_missing))
    }

    fn open_full(path: &Path, column_check: Option<&[&str]>) -> Result<Store> {
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
                if let Some(report_missing) = column_check {
                    add_trim_columns(&conn, report_missing)?;
                }
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
        if column_check.is_some() {
            TRIM_COLUMNS_CHECKED.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf());
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

    /// The bounded open of the trim hook and the MCP server's read-back write (O§7.11): busy timeout `bound` for each
    /// statement and no retry loop; no directory, file, schema or column is created (O§6.5 item 3), so the open takes no
    /// write lock. A database of another schema version is an error.
    pub fn open_bounded(path: &Path, bound: Duration) -> Result<Store> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(bound)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(anyhow!(
                "usage database schema version {version} is not {SCHEMA_VERSION}"
            ));
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
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

    /// Ten columns; on a table without the new columns, the seven old ones (O§6.5 item 4).
    pub fn insert_trim(&self, t: &TrimRow) -> Result<()> {
        let (stored, view) = (i64::try_from(t.stored_bytes)?, i64::try_from(t.view_bytes)?);
        match self.conn.prepare_cached(INSERT_TRIM) {
            Ok(mut st) => {
                st.execute(rusqlite::params![t.ts, t.session, t.agent_id, t.tool, t.kind, stored, view, t.result_id, t.command, t.shaper])?;
            }
            // O§6.5 item 4: a table no new full open has touched yet. Only a missing column falls back.
            Err(e) if sqlite_message(&e).is_some_and(|m| m.starts_with("table trims has no column named ") || m.starts_with("no such column")) => {
                self.conn.prepare_cached(INSERT_TRIM_SEVEN)?.execute(rusqlite::params![t.ts, t.session, t.agent_id, t.tool, t.kind, stored, view])?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// One `readbacks` row (O§6.4). The MCP server writes it through the bounded open (OS-5).
    pub fn insert_readback(&self, r: &ReadbackRow) -> Result<()> {
        self.conn.prepare_cached(
            "INSERT INTO readbacks (ts, session, read_in, result_id, tool, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?.execute(rusqlite::params![r.ts, r.session, r.read_in, r.result_id, r.tool, i64::try_from(r.bytes)?])?;
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
        tx.execute("DELETE FROM readbacks WHERE ts < ?1", [cutoff])?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // review round 1 F3: `PRAGMA table_info` hides a generated column; the check must still see it
    fn readbacks_schema_missing_columns_see_generated_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE trims (ts INTEGER NOT NULL, result_id TEXT, command TEXT, shaper TEXT GENERATED ALWAYS AS ('x') VIRTUAL)").unwrap();
        assert_eq!(missing_trim_columns(&conn, &[]).unwrap(), []);
        assert_eq!(missing_trim_columns(&conn, &["shaper"]).unwrap(), [("shaper", "TEXT")]);
    }
}
