//! `GET /<dialect>/v1/models` (§7.4, §11.4).

use serde_json::{json, Map, Value};
use zygo_config::{Dialect, Merged};

use crate::session::SessionState;

fn entry(id: &str, description: &str) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!("model"));
    m.insert("id".into(), json!(id));
    m.insert("display_name".into(), json!(id));
    if !description.is_empty() {
        m.insert("description".into(), json!(description));
    }
    Value::Object(m)
}

/// Roles (disabled ones absent, overrides annotated), custom roles, then catalog ids reachable on `d`; the Anthropic
/// list shape.
pub(crate) fn list(s: &SessionState, cfg: &Merged, d: Dialect) -> Value {
    let mut data = Vec::new();
    for (name, r) in &s.config.roles {
        let mut description = r.description.clone();
        if let Some(p) = s.profile.as_ref().filter(|p| p.overrides.contains_key(name)) {
            description = format!("{description} · profile {}: {}", p.name, r.model);
        }
        data.push(entry(name, &description));
    }
    for (name, cr) in &s.config.custom_roles {
        data.push(entry(name, &cr.description));
    }
    for (id, v) in cfg.config.catalog.view().models {
        if v.dialects.contains(&d) {
            data.push(entry(&id, ""));
        }
    }
    let first = data.first().map(|m| m["id"].clone()).unwrap_or(Value::Null);
    let last = data.last().map(|m| m["id"].clone()).unwrap_or(Value::Null);
    json!({ "data": data, "has_more": false, "first_id": first, "last_id": last })
}
