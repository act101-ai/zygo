//! Stage 2, resolution (§7.4): name → role, custom role or catalog id; profile; candidate providers.

use http::StatusCode;
use serde_json::Value;
use zygo_config::{Dialect, Merged, ModelRow, Provider};

use crate::errors::ZygoError;
use crate::pipeline::{Adjustment, RequestCtx, Stage};
use crate::provider::Candidate;
use crate::session::SessionState;

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The name without `[1m]`.
    pub requested: String,
    pub one_m: bool,
    /// Role, custom role, or "direct".
    pub role: String,
    pub model: String,
    /// Providers that serve `model` on the dialect, config order, the pinned one first.
    pub candidates: Vec<Provider>,
    pub adjustments: Vec<Adjustment>,
}

/// `cfg` is the serving config at request time (a stale override is seen after a reload, §11.3).
pub fn resolve(name: &str, dialect: Dialect, s: &SessionState, cfg: &Merged) -> Result<Resolved, ZygoError> {
    let (base, one_m) = match name.strip_suffix("[1m]") {
        Some(b) => (b, true),
        None => (name, false),
    };
    let c = &cfg.config;
    let p = s.profile.as_ref();
    // Disabled providers count (§7.3 note: `zygo_no_provider` = no provider for the dialect).
    let serves = |row: &ModelRow| row.provider.iter().any(|pr| upstream_dialect(pr, dialect).is_some());
    let unknown = || ZygoError::new(StatusCode::BAD_REQUEST, "zygo_unknown_model", "unknown model name");
    let mut adjustments = Vec::new();
    let (role, model) = if let Some(r) = c.role.get(base) {
        if p.is_some_and(|p| p.disabled.iter().any(|d| d == base)) {
            return Err(ZygoError::new(StatusCode::BAD_REQUEST, "zygo_role_disabled", "role disabled by the profile"));
        }
        let mut model = r.model.clone();
        if let Some(o) = p.and_then(|p| p.overrides.get(base)) {
            if c.catalog.get(o).is_some_and(serves) {
                model = o.clone();
            } else {
                adjustments.push(Adjustment("profile_override_stale".into()));
            }
        }
        (base.to_string(), model)
    } else if let Some(cr) = p.and_then(|p| p.roles.get(base)) {
        if c.catalog.get(&cr.model).is_none() {
            return Err(unknown());
        }
        (base.to_string(), cr.model.clone())
    } else if c.catalog.get(base).is_some() {
        ("direct".to_string(), base.to_string())
    } else {
        return Err(unknown());
    };
    let row = c.catalog.get(&model).ok_or_else(unknown)?;
    if !serves(row) {
        return Err(ZygoError::new(StatusCode::BAD_REQUEST, "zygo_no_provider", "no provider serves this model"));
    }
    let mut candidates: Vec<Provider> =
        row.provider.iter().filter(|pr| upstream_dialect(pr, dialect).is_some()).cloned().collect();
    // §10.4: the pinned provider first.
    let pinned = s.pins.lock().unwrap_or_else(|e| e.into_inner()).get(&model).cloned();
    if let Some(i) = pinned.and_then(|n| candidates.iter().position(|pr| pr.name == n)) {
        let pr = candidates.remove(i);
        candidates.insert(0, pr);
    }
    Ok(Resolved { requested: base.to_string(), one_m, role, model, candidates, adjustments })
}

/// The dialect sent upstream: the client's when the provider speaks it, else a v1 translation pair (§7.6):
/// `anthropic` → `openai`, else `openai-chat`.
pub(crate) fn upstream_dialect(p: &Provider, client: Dialect) -> Option<Dialect> {
    if p.dialects.contains(&client) {
        return Some(client);
    }
    if client != Dialect::Anthropic {
        return None;
    }
    [Dialect::Openai, Dialect::OpenaiChat].into_iter().find(|d| p.dialects.contains(d))
}

/// Stage 2 (§7.3).
pub struct ResolveStage;

impl Stage for ResolveStage {
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        let name = ctx
            .body
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| ZygoError::bad_request("model must be a string"))?;
        let r = resolve(name, ctx.dialect, &ctx.session, &ctx.cfg)?;
        ctx.candidates = r
            .candidates
            .iter()
            .filter_map(|p| {
                let dialect = upstream_dialect(p, ctx.dialect)?;
                Some(Candidate { provider: p.clone(), dialect, key: None })
            })
            .collect();
        ctx.adjustments.extend(r.adjustments.iter().cloned());
        ctx.resolved = Some(r);
        Ok(())
    }
}
