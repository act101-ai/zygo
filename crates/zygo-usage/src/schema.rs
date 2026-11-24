use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS requests (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    token_label TEXT,
    agent TEXT NOT NULL,
    agent_version TEXT,
    agent_id TEXT,
    agent_type TEXT,
    request_class TEXT,
    compaction INTEGER NOT NULL,
    route TEXT NOT NULL,
    role_requested TEXT,
    role_served TEXT,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    profile TEXT,
    passthrough INTEGER NOT NULL,
    status INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    cache_write_tokens INTEGER NOT NULL,
    estimated_usd REAL NOT NULL,
    adjustments TEXT NOT NULL,
    redactions TEXT NOT NULL,
    repo TEXT,
    work_item TEXT
);
CREATE TABLE IF NOT EXISTS trims (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    agent_id TEXT,
    tool TEXT NOT NULL,
    kind TEXT NOT NULL,
    stored_bytes INTEGER NOT NULL,
    view_bytes INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS work_units (
    unit TEXT NOT NULL,
    round INTEGER NOT NULL,
    session TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    agent_type TEXT,
    began_at INTEGER NOT NULL,
    ended_at INTEGER,
    tool_calls INTEGER NOT NULL,
    failed_calls INTEGER NOT NULL,
    check_ran INTEGER NOT NULL,
    check_passed INTEGER NOT NULL,
    guard_events TEXT NOT NULL,
    levels TEXT NOT NULL,
    verdict TEXT,
    PRIMARY KEY (session, agent_id, unit, round, began_at)
);
CREATE INDEX IF NOT EXISTS requests_session_ts ON requests(session, ts);
CREATE INDEX IF NOT EXISTS requests_ts ON requests(ts);
CREATE INDEX IF NOT EXISTS trims_session ON trims(session);
CREATE INDEX IF NOT EXISTS trims_ts ON trims(ts);
CREATE INDEX IF NOT EXISTS work_units_session ON work_units(session);
CREATE TABLE IF NOT EXISTS sync_pending (session TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS upstream_errors (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    request_class TEXT,
    model TEXT NOT NULL,
    status INTEGER NOT NULL,
    error_type TEXT NOT NULL,
    error_message TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS upstream_errors_session_ts ON upstream_errors(session, ts);
CREATE INDEX IF NOT EXISTS upstream_errors_ts ON upstream_errors(ts);
";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RedactionCount {
    pub id: String,
    pub count: u32,
}

/// One proxied request. Every timestamp is Unix seconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestRow {
    /// Unix seconds.
    pub ts: i64,
    pub session: String,
    pub token_label: Option<String>,
    pub agent: String,
    pub agent_version: Option<String>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub request_class: Option<String>,
    pub compaction: bool,
    pub route: String,
    pub role_requested: Option<String>,
    pub role_served: Option<String>,
    pub model: String,
    pub provider: String,
    pub profile: Option<String>,
    pub passthrough: bool,
    pub status: u16,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub estimated_usd: f64,
    pub adjustments: Vec<String>,
    pub redactions: Vec<RedactionCount>,
    pub repo: Option<String>,
    pub work_item: Option<String>,
}

/// One tool-output trim. Every timestamp is Unix seconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrimRow {
    /// Unix seconds.
    pub ts: i64,
    pub session: String,
    pub agent_id: Option<String>,
    pub tool: String,
    pub kind: String,
    pub stored_bytes: u64,
    pub view_bytes: u64,
}

/// One agent's span on a work unit and round. Every timestamp is Unix seconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkUnitRow {
    pub unit: String,
    pub round: u32,
    pub session: String,
    pub agent_id: String,
    pub agent_type: Option<String>,
    /// Unix seconds.
    pub began_at: i64,
    /// Unix seconds.
    pub ended_at: Option<i64>,
    pub tool_calls: u32,
    pub failed_calls: u32,
    pub check_ran: bool,
    pub check_passed: bool,
    pub guard_events: BTreeMap<String, BTreeMap<String, u32>>,
    pub levels: BTreeMap<String, String>,
    pub verdict: Option<String>,
}

/// The upstream error of one passthrough request whose relayed upstream status is not 2xx (§14, FIX-10). No request
/// byte: `error_type` and `error_message` come from the upstream body only, scrubbed and capped.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpstreamErrorRow {
    /// Unix seconds.
    pub ts: i64,
    pub session: String,
    pub request_class: Option<String>,
    pub model: String,
    pub status: u16,
    pub error_type: String,
    pub error_message: String,
}
