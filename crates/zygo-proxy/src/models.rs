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

/// Roles (disabled ones absent, overrides annotated), then custom roles: (name, description).
fn roles(s: &SessionState) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, r) in &s.config.roles {
        let mut description = r.description.clone();
        if let Some(p) = s.profile.as_ref().filter(|p| p.overrides.contains_key(name)) {
            description = format!("{description} · profile {}: {}", p.name, r.model);
        }
        out.push((name.clone(), description));
    }
    for (name, cr) in &s.config.custom_roles {
        out.push((name.clone(), cr.description.clone()));
    }
    out
}

/// Roles (disabled ones absent, overrides annotated), custom roles, then catalog ids reachable on `d`; the Anthropic
/// list shape.
pub(crate) fn list(s: &SessionState, cfg: &Merged, d: Dialect) -> Value {
    let mut data: Vec<Value> = roles(s).iter().map(|(name, description)| entry(name, description)).collect();
    for (id, v) in cfg.config.catalog.view().models {
        if v.dialects.contains(&d) {
            data.push(entry(&id, ""));
        }
    }
    let first = data.first().map(|m| m["id"].clone()).unwrap_or(Value::Null);
    let last = data.last().map(|m| m["id"].clone()).unwrap_or(Value::Null);
    json!({ "data": data, "has_more": false, "first_id": first, "last_id": last })
}

/// Empty: the proxy holds no client's base prompt, and Codex reads this value in place of its own. The Codex launch
/// supplies it through the catalog file (K§6.3.1) and does not rely on this route (K§6.4.1; RS-8 Planning notes).
const BASE_INSTRUCTIONS: &str = "";

/// One entry of the model-catalog shape Codex lists (PC7, fixtures/codex/models/catalog-shape.json); no
/// `apply_patch_tool_type` (K§6.4.1).
fn catalog_entry(slug: &str, description: &str) -> Value {
    json!({
        "slug": slug,
        "display_name": slug,
        "description": description,
        "supported_reasoning_levels": [],
        "default_reasoning_level": null,
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "support_verbosity": false,
        "truncation_policy": { "mode": "bytes", "limit": 10000 },
        "experimental_supported_tools": [],
        "base_instructions": BASE_INSTRUCTIONS,
    })
}

/// The session's roles and custom roles in the model-catalog shape Codex lists (K§6.4.1; PC7,
/// fixtures/codex/models/catalog-shape.json); no catalog ids.
pub(crate) fn list_openai(s: &SessionState) -> Value {
    let models: Vec<Value> = roles(s).iter().map(|(name, description)| catalog_entry(name, description)).collect();
    json!({ "models": models })
}
