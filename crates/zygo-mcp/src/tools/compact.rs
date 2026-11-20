use serde_json::{json, Value};

use crate::tool::{Tool, ToolDef, ToolError};

pub const DESCRIPTION: &str = "Compact this conversation now. Call it at a boundary of your work, when what came before is no longer needed, for example before starting the next unit of a plan.";

pub struct Compact;

impl Tool for Compact {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "compact".into(),
            description: DESCRIPTION.into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
        }
    }
    // No side effect: the proxy sees the tool call (§10.2.3 trigger 2).
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        if args.as_object().is_none_or(|o| !o.is_empty()) {
            return Err(ToolError::InvalidParams("Invalid params: compact takes no arguments".into()));
        }
        Ok(Value::String("compaction requested".into()))
    }
}
