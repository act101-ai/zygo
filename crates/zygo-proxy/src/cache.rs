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
