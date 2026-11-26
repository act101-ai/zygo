//! Stage 4, cache markers (§10.4). Provider pinning is applied in stage 2 (`resolve`).

use serde_json::{json, Value};
use zygo_config::Dialect;

use crate::errors::ZygoError;
use crate::pipeline::{ModelRoute, RequestCtx, Stage};

pub struct CacheStage;

/// A `cache_control` key at any depth.
fn has_marker(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.iter().any(|(k, v)| k == "cache_control" || has_marker(v)),
        Value::Array(a) => a.iter().any(has_marker),
        _ => false,
    }
}

/// Appends the marker to the last element of the array `field` when that element is an object.
fn mark_last(body: &mut Value, field: &str) -> bool {
    let Some(last) = body.get_mut(field).and_then(Value::as_array_mut).and_then(|a| a.last_mut()).and_then(Value::as_object_mut)
    else {
        return false;
    };
    last.insert("cache_control".into(), json!({ "type": "ephemeral", "ttl": "1h" }));
    true
}

/// A Messages body built for an Anthropic upstream (K§6.4.2, base §10.4): with no `cache_control` key anywhere, a
/// string `system` becomes one text block, then the last `system` block and the last tool get
/// `{"type":"ephemeral","ttl":"1h"}`. `true` when it added a marker.
pub fn mark_upstream(body: &mut Value) -> bool {
    if has_marker(body) {
        return false;
    }
    if let Some(system) = body.get_mut("system").filter(|s| s.is_string()) {
        let text = system.take();
        *system = json!([{ "type": "text", "text": text }]);
    }
    let system = mark_last(body, "system");
    let tools = mark_last(body, "tools");
    system || tools
}

impl Stage for CacheStage {
    /// Messages route, Anthropic client dialect, `[cache] insert_markers.<agent>`, no `cache_control` anywhere: the
    /// last `system` block (only an array) and the last tool definition get the marker; every other byte as received.
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        let on = ctx.cfg.config.cache.insert_markers.get(ctx.session.agent.as_str()).copied().unwrap_or(false);
        if ctx.route != ModelRoute::Messages || ctx.dialect != Dialect::Anthropic || !on || has_marker(&ctx.body) {
            return Ok(());
        }
        let system = mark_last(&mut ctx.body, "system");
        let tools = mark_last(&mut ctx.body, "tools");
        ctx.changed |= system || tools;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark() -> Value {
        json!({"type":"ephemeral","ttl":"1h"})
    }

    #[test]
    fn mark_upstream_wraps_string_system_and_marks_last_tool() {
        let mut b = json!({"system":"S","tools":[{"name":"a"},{"name":"b"}],"messages":[]});
        assert!(mark_upstream(&mut b));
        assert_eq!(
            b,
            json!({"system":[{"type":"text","text":"S","cache_control":mark()}],
                   "tools":[{"name":"a"},{"name":"b","cache_control":mark()}],"messages":[]})
        );
    }

    #[test]
    fn mark_upstream_marks_last_block_of_array_system() {
        let mut b = json!({"system":[{"type":"text","text":"a"},{"type":"text","text":"b"}]});
        assert!(mark_upstream(&mut b));
        assert_eq!(b, json!({"system":[{"type":"text","text":"a"},{"type":"text","text":"b","cache_control":mark()}]}));
    }

    #[test]
    fn mark_upstream_leaves_a_marked_body() {
        let before = json!({"system":"S","tools":[{"name":"a"}],
            "messages":[{"role":"user","content":[{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]}]});
        let mut b = before.clone();
        assert!(!mark_upstream(&mut b));
        assert_eq!(b, before);
    }

    #[test]
    fn mark_upstream_without_system_or_tools_is_false() {
        let before = json!({"messages":[{"role":"user","content":"hi"}]});
        let mut b = before.clone();
        assert!(!mark_upstream(&mut b));
        assert_eq!(b, before);
    }
}
