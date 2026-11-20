//! Fold an open unit's log span into its `work_units` row (§13.3).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use zygo_usage::WorkUnitRow;

use crate::log::{self, Begin, CheckResult, Kind, LogLine};

pub fn fold(dir: &Path, agent: &str) -> Option<WorkUnitRow> {
    fold_begin(dir, agent, &log::open_unit(dir, agent)?)
}

/// Reads from the unit's `begin` offset to the end of the log, bounded by the 4,000-line × 1 KiB cap (§13.3 note).
/// `None` when the line at the offset is not this unit's `begin`.
pub fn fold_begin(dir: &Path, agent: &str, b: &Begin) -> Option<WorkUnitRow> {
    let mut f = std::fs::File::open(log::log_path(dir, agent)).ok()?;
    f.seek(SeekFrom::Start(b.offset)).ok()?;
    let mut buf = Vec::new();
    f.take(u64::from(log::MAX_LINES) * log::MAX_LINE_BYTES as u64).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut raw = text.lines();
    let first: LogLine = serde_json::from_str(raw.next()?).ok()?;
    let lines = raw.filter_map(|l| serde_json::from_str::<LogLine>(l).ok());
    match &first.kind {
        Kind::Begin { unit, round } if *unit == b.unit && *round == b.round => {}
        _ => return None,
    }
    let mut row = WorkUnitRow {
        unit: b.unit.clone(),
        round: b.round,
        session: b.session.clone(),
        agent_id: agent.to_string(),
        agent_type: b.agent_type.clone(),
        began_at: b.began_at,
        ended_at: Some(first.ts),
        ..Default::default()
    };
    let mut check: Option<bool> = None;
    for l in lines {
        if matches!(l.kind, Kind::Begin { .. }) {
            break;
        }
        row.ended_at = Some(l.ts);
        match l.kind {
            Kind::End { .. } => break,
            Kind::Call { .. } => row.tool_calls += 1,
            Kind::Failed { .. } => {
                row.tool_calls += 1;
                row.failed_calls += 1;
            }
            Kind::Edit { .. } => check = None,
            Kind::Check { result } => check = Some(result == CheckResult::Passed),
            Kind::Guard { guard, level } => *row.guard_events.entry(guard).or_default().entry(level).or_default() += 1,
            Kind::Levels { levels } => row.levels = levels,
            _ => {}
        }
    }
    row.check_ran = check.is_some();
    row.check_passed = check == Some(true);
    Some(row)
}
