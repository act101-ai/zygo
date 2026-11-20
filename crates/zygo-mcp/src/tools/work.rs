use std::path::PathBuf;

use serde_json::{json, Value};
use zygo_core::paths::Paths;
use zygo_usage::Store;
use zygo_work::hooks::{self, BeginArgs};

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

/// `work_begin` and `work_verdict` (§13.3), inside a session only.
pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>> {
    let Some(sd) = ctx.session_dir.as_ref() else { return Vec::new() };
    vec![Box::new(WorkBegin), Box::new(WorkVerdict { db: Paths::from_env().usage_db, session: hooks::session_id(sd) })]
}

const VERDICTS: [&str; 3] = ["keep", "discard", "rework"];

/// The reply only: the `work` hook (`--event begin`) records the unit, because only the hook sees `agent_id`.
pub(crate) struct WorkBegin;

impl Tool for WorkBegin {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "work_begin".into(),
            description: "Record the unit of work you are starting: its id, its round, the one command that proves it done and the files it may change. Call it once, first.".into(),
            input_schema: json!({"type": "object", "properties": {
                "unit": {"type": "string"},
                "round": {"type": "integer", "minimum": 1},
                "check": {"type": "string", "maxLength": 1024},
                "files": {"type": "array", "items": {"type": "string"}, "maxItems": 64}
            }, "required": ["unit", "round"]}),
        }
    }
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a = BeginArgs::parse(&args).map_err(ToolError::InvalidParams)?;
        Ok(Value::String(format!("noted {} round {}", a.unit, a.round)))
    }
}

pub(crate) struct WorkVerdict {
    db: PathBuf,
    session: String,
}

impl Tool for WorkVerdict {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "work_verdict".into(),
            description: "Lead only. Record your verdict on a unit of work and round after its subagent has stopped: keep, discard or rework, with your findings.".into(),
            input_schema: json!({"type": "object", "properties": {
                "unit": {"type": "string"},
                "round": {"type": "integer", "minimum": 1},
                "verdict": {"type": "string", "enum": VERDICTS},
                "findings": {"type": "string"}
            }, "required": ["unit", "round", "verdict"]}),
        }
    }
    /// `findings` is accepted, not stored (§14 stores no free text).
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let bad = |m: &str| ToolError::InvalidParams(m.into());
        let unit = args.get("unit").and_then(Value::as_str).filter(|u| hooks::valid_unit(u)).ok_or_else(|| bad("unit must be a work unit id"))?;
        let round = args
            .get("round")
            .and_then(Value::as_u64)
            .filter(|r| *r >= 1)
            .and_then(|r| u32::try_from(r).ok())
            .ok_or_else(|| bad("round must be an integer from 1"))?;
        let verdict = args
            .get("verdict")
            .and_then(Value::as_str)
            .filter(|v| VERDICTS.contains(v))
            .ok_or_else(|| bad("verdict must be keep, discard or rework"))?;
        if args.get("findings").is_some_and(|f| !f.is_string()) {
            return Err(bad("findings must be a string"));
        }
        if !Store::open(&self.db)?.set_verdict(&self.session, unit, round, verdict)? {
            return Err(ToolError::Failed(format!("no folded row for {unit} round {round} in this session yet")));
        }
        Ok(Value::String(format!("recorded {verdict} for {unit} round {round}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zygo_config::Features;
    use zygo_core::session_dir::SessionDir;
    use zygo_core::Agent;
    use zygo_usage::{Filter, Store, WorkUnitRow};

    #[test]
    fn definitions() {
        let d = WorkBegin.def();
        assert_eq!(d.name, "work_begin");
        assert_eq!(d.description, "Record the unit of work you are starting: its id, its round, the one command that proves it done and the files it may change. Call it once, first.");
        assert_eq!(d.input_schema["required"], json!(["unit", "round"]));
        let v = WorkVerdict { db: "unused.db".into(), session: "S1".into() }.def();
        assert_eq!(v.name, "work_verdict");
        assert_eq!(v.input_schema["properties"]["verdict"]["enum"], json!(["keep", "discard", "rework"]));
        assert_eq!(v.input_schema["required"], json!(["unit", "round", "verdict"]));
    }

    #[test]
    fn begin_replies_noted() {
        assert_eq!(WorkBegin.call(json!({"unit": "work-graph:zygo-v1-N12", "round": 2, "check": "", "files": []})),
                   Ok(json!("noted work-graph:zygo-v1-N12 round 2")));
        assert_eq!(WorkBegin.call(json!({"unit": "beads:bd-a1b2", "round": 1, "future_arg": 1})), Ok(json!("noted beads:bd-a1b2 round 1")));
        for bad in [json!({"round": 1}), json!({"unit": "beads", "round": 1}), json!({"unit": "beads:bd-a1b2", "round": 0}),
                    json!({"unit": "beads:bd-a1b2", "round": 1, "check": "x".repeat(1025)})] {
            assert!(matches!(WorkBegin.call(bad), Err(ToolError::InvalidParams(_))));
        }
        assert_eq!(WorkBegin.call(json!({"unit": "beads:bd-a1b2", "round": 1, "files": ["p".repeat(4097)]})),
                   Err(ToolError::InvalidParams("`files` has a path over 4,096 bytes".into())));
        let files: Vec<String> = (0..64).map(|i| format!("src/f{i}.rs")).collect();
        assert_eq!(WorkBegin.call(json!({"unit": "beads:bd-a1b2", "round": 1, "check": "c".repeat(1024), "files": files})),
                   Ok(json!("noted beads:bd-a1b2 round 1")));
    }

    #[test]
    fn verdict_is_recorded_on_the_row() {
        let t = tempfile::tempdir().unwrap();
        let db = t.path().join("usage.db");
        Store::open(&db).unwrap().insert_work_unit(&WorkUnitRow { unit: "beads:bd-a1b2".into(), round: 1, session: "S1".into(),
            agent_id: "a1".into(), began_at: 100, ..Default::default() }).unwrap();
        let tool = WorkVerdict { db: db.clone(), session: "S1".into() };
        assert_eq!(tool.call(json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "keep", "findings": "check passes"})),
                   Ok(json!("recorded keep for beads:bd-a1b2 round 1")));
        assert_eq!(Store::open(&db).unwrap().work_units(&Filter::default()).unwrap()[0].verdict.as_deref(), Some("keep"));
        assert!(matches!(tool.call(json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "maybe"})), Err(ToolError::InvalidParams(_))));
        assert!(matches!(tool.call(json!({"unit": "beads:bd-a1b2", "round": 9, "verdict": "keep"})), Err(ToolError::Failed(_))));
    }

    #[test]
    fn tools_need_a_session() {
        let t = tempfile::tempdir().unwrap();
        let mut c = McpCtx { agent: Agent::ClaudeCode, features: Features::default(), session_dir: None };
        assert!(tools(&c).is_empty());
        c.session_dir = Some(SessionDir::new(t.path().join("s")));
        let names: Vec<String> = tools(&c).iter().map(|t| t.def().name).collect();
        assert_eq!(names, ["work_begin", "work_verdict"]);
    }
}
