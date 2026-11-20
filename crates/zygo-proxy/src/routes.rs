//! The router: session API (§7.2, §10.2.4, §17.1) and fallbacks (§7.3).

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use bytes::{Bytes, BytesMut};
use http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use zygo_core::overrides::Overrides;

use zygo_config::Dialect;
use zygo_usage::{RequestRow, Store};

use crate::errors::ZygoError;
use crate::forward;
use crate::identity::{authenticate, model_identity, team_label};
use crate::models;
use crate::pipeline::ModelRoute;
use crate::server::App;
use crate::session::{SessionRequest, SessionSettings, SessionState, WindowArg, WINDOW_MAX, WINDOW_MIN};
use crate::team::{PluginsReply, TeamConfigReply};

/// Request body limit (§7.3).
const BODY_LIMIT: usize = 32 << 20;

type Reply = Result<Response, ZygoError>;

pub(crate) fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/zygo/sessions", post(create))
        .route("/v1/zygo/sessions/{id}", delete(end))
        .route("/v1/zygo/sessions/{id}/heartbeat", post(heartbeat))
        .route("/v1/zygo/sessions/{id}/compact", post(set_compact).delete(clear_compact))
        .route("/v1/zygo/sessions/{id}/status", get(status))
        .route("/v1/zygo/sessions/{id}/settings", patch(settings))
        .route("/v1/zygo/sessions/{id}/requests", get(requests))
        .route("/v1/zygo/config", get(team_config))
        .route("/v1/zygo/plugins", get(plugins))
        .route("/anthropic/v1/messages", post(messages))
        .route("/anthropic/v1/messages/count_tokens", post(count_tokens))
        .route("/anthropic/v1/models", get(models))
        .route_layer(middleware::from_fn_with_state(app.clone(), sweep))
        .route("/openai/v1/responses", post(not_in_v1))
        .route("/openai/v1/chat/completions", post(not_in_v1))
        .route("/openai/v1/models", get(not_in_v1))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(app)
}

/// §7.2 note (N10 review): every session-API request first removes the expired sessions.
async fn sweep(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    app.sessions.sweep();
    next.run(req).await
}

async fn healthz() -> &'static str {
    "ok"
}

async fn not_found() -> ZygoError {
    ZygoError::new(StatusCode::NOT_FOUND, "zygo_not_found", "no such route")
}

/// The v2 routes (§7.6), with no identity check.
async fn not_in_v1() -> Response {
    let e = ZygoError::new(StatusCode::NOT_FOUND, "zygo_route_not_in_v1", "this route is not in zygo v1");
    (e.status, Json(e.body(Dialect::Openai))).into_response()
}

async fn method_not_allowed() -> ZygoError {
    ZygoError::new(StatusCode::METHOD_NOT_ALLOWED, "zygo_method_not_allowed", "method not allowed")
}

/// Reads the body after stage 1 (the handlers authenticate first), under `BODY_LIMIT`. `app.body_idle` bounds each wait
/// for a chunk (§7 note 2026-11-19, N60 review); a stall is an unreadable body.
async fn read_body(app: &App, req: Request) -> Result<Bytes, ZygoError> {
    let mut buf = BytesMut::new();
    let mut s = req.into_body().into_data_stream();
    while let Some(c) = forward::next_within(&mut s, app.body_idle).await {
        let c = c.map_err(|_| ZygoError::bad_request("unreadable body"))?;
        if buf.len() + c.len() > BODY_LIMIT {
            return Err(ZygoError::new(StatusCode::PAYLOAD_TOO_LARGE, "zygo_body_too_large", "request body over 32 MiB"));
        }
        buf.extend_from_slice(&c);
    }
    Ok(buf.freeze())
}

/// The `{id}` path segment; a rejection (e.g. not UTF-8) uses the §7.3 error shape.
fn path_id(p: Result<Path<String>, PathRejection>) -> Result<String, ZygoError> {
    p.map(|Path(id)| id).map_err(|_| ZygoError::bad_request("invalid session id"))
}

/// Refuses a key of a JSON object outside `allowed`, naming it (§7.3 note: a field name, never a value).
fn known_fields(v: &Value, allowed: &[&str]) -> Result<(), ZygoError> {
    match v.as_object().and_then(|o| o.keys().find(|k| !allowed.contains(&k.as_str()))) {
        // Escaped so the message stays one line.
        Some(key) => Err(ZygoError::bad_request(format!("unknown field `{}`", key.escape_debug()))),
        None => Ok(()),
    }
}

/// Session-token routes: the header id must equal the path id (§7.2 note), then stage 1.
fn session(app: &App, h: &HeaderMap, id: &str) -> Result<Arc<SessionState>, ZygoError> {
    if h.get("x-zygo-session").and_then(|v| v.to_str().ok()) != Some(id) {
        return Err(ZygoError::token_invalid());
    }
    authenticate(h, &app.sessions)
}

fn label(app: &App, h: &HeaderMap) -> Result<String, ZygoError> {
    team_label(h, app.served().team.as_deref())
}

/// `SessionRequest` fields over HTTP (§7.2).
const SESSION_REQUEST_FIELDS: &[&str] = &["agent", "agent_version", "profile", "passthrough", "repo", "work"];

async fn create(State(app): State<Arc<App>>, req: Request) -> Reply {
    let snap = app.served();
    let label = team_label(req.headers(), snap.team.as_deref())?;
    let headers = req.headers().clone();
    let b = read_body(&app, req).await?;
    let bad = || ZygoError::bad_request("invalid session request");
    let v: Value = serde_json::from_slice(&b).map_err(|_| bad())?;
    known_fields(&v, SESSION_REQUEST_FIELDS)?;
    let mut r: SessionRequest = serde_json::from_value(v).map_err(|_| bad())?;
    // §20 #26: only a local proxy sees the client's login.
    if r.passthrough {
        return Err(ZygoError::new(StatusCode::FORBIDDEN, "zygo_passthrough_local_only", "passthrough is local mode only"));
    }
    // Serve mode: the server's own plugin list (N10 review).
    r.known_plugins = snap.known_plugins.clone();
    // A reload during the body read can revoke the token: checked again under the session lock (N16 round 2).
    let still_valid = || team_label(&headers, app.served().team.as_deref()).is_ok_and(|l| l == label);
    Ok(Json(app.sessions.create(r, Some(label.clone()), &snap.cfg, &still_valid)?).into_response())
}

/// §7.2 team layer; no provider data.
async fn team_config(State(app): State<Arc<App>>, h: HeaderMap) -> Reply {
    let snap = app.served();
    team_label(&h, snap.team.as_deref())?;
    Ok(Json(TeamConfigReply::from_merged(&snap.cfg)).into_response())
}

/// §15.3.
async fn plugins(State(app): State<Arc<App>>, h: HeaderMap) -> Reply {
    let snap = app.served();
    team_label(&h, snap.team.as_deref())?;
    Ok(Json(PluginsReply::from_merged(&snap.cfg)).into_response())
}

/// §7.3, §14: the usage rows of a session of the caller's label, after its queued rows (at most `rows_wait`; Q2). No id can be discovered: a session or row of
/// another label, or an id with no live session and no rows, is `session_unknown`. Provider names and prices never
/// leave the server (§7.2): `provider` goes out empty and `estimated_usd` 0.
async fn requests(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, h: HeaderMap) -> Reply {
    let id = path_id(id)?;
    let label = team_label(&h, app.served().team.as_deref())?;
    let live = app.sessions.get(&id);
    if live.as_ref().is_some_and(|s| s.team_label.as_deref() != Some(label.as_str())) {
        return Err(ZygoError::session_unknown());
    }
    let unavailable = || ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_usage_unavailable", "usage database unavailable");
    let (db, usage, rows_wait) = (app.usage_db.clone(), app.usage.clone(), app.rows_wait);
    let (rows, complete) = tokio::task::spawn_blocking(move || {
        // Q2: the rows still queued for the writer, at most `rows_wait`; `complete` says whether none is left.
        let complete = usage.flush(&id, rows_wait);
        let rows = match db {
            None => Ok(Vec::new()),
            Some(p) => Store::open(&p).and_then(|s| s.requests_for_session(&id)),
        };
        rows.map(|r| (r, complete))
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| unavailable())?;
    if rows.iter().any(|r| r.token_label.as_deref() != Some(label.as_str())) || (live.is_none() && rows.is_empty()) {
        return Err(ZygoError::session_unknown());
    }
    let rows: Vec<RequestRow> = rows.into_iter().map(|r| RequestRow { provider: String::new(), estimated_usd: 0.0, ..r }).collect();
    Ok(Json(json!({ "requests": rows, "complete": complete })).into_response())
}

async fn heartbeat(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, h: HeaderMap) -> Reply {
    let id = path_id(id)?;
    let label = label(&app, &h)?;
    app.sessions.heartbeat(&id, &label)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn end(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, h: HeaderMap) -> Reply {
    let id = path_id(id)?;
    let label = label(&app, &h)?;
    app.sessions.delete(&id, &label)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn window_reply(w: Option<WindowArg>) -> Response {
    Json(json!({ "window": w })).into_response()
}

async fn set_compact(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, req: Request) -> Reply {
    let s = session(&app, req.headers(), &path_id(id)?)?;
    let b = read_body(&app, req).await?;
    let bad = || ZygoError::bad_request("window must be a number from 100000 to 10000000 or \"off\"");
    let v: Value = serde_json::from_slice(&b).map_err(|_| bad())?;
    known_fields(&v, &["window"])?;
    let w = v.get("window").ok_or_else(bad).and_then(|w| WindowArg::deserialize(w).map_err(|_| bad()))?;
    if let WindowArg::Tokens(n) = w
        && !(WINDOW_MIN..=WINDOW_MAX).contains(&n)
    {
        return Err(bad());
    }
    app.sessions.accept(&s)?;
    Ok(window_reply(s.update_compact(Some(w))))
}

async fn clear_compact(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, h: HeaderMap) -> Reply {
    let s = session(&app, &h, &path_id(id)?)?;
    app.sessions.accept(&s)?;
    Ok(window_reply(s.update_compact(None)))
}

async fn status(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, h: HeaderMap) -> Reply {
    let s = session(&app, &h, &path_id(id)?)?;
    app.sessions.accept(&s)?;
    Ok(Json(s.status()).into_response())
}

async fn settings(State(app): State<Arc<App>>, id: Result<Path<String>, PathRejection>, req: Request) -> Reply {
    let s = session(&app, req.headers(), &path_id(id)?)?;
    let b = read_body(&app, req).await?;
    let not_object = || ZygoError::bad_request("settings must be a JSON object");
    let v: Value = serde_json::from_slice(&b).map_err(|_| not_object())?;
    let Some(obj) = v.as_object() else { return Err(not_object()) };
    known_fields(&v, &["redaction_content"])?;
    // `null` is refused, not read as "absent".
    if obj.values().any(|x| !x.is_boolean()) {
        return Err(ZygoError::bad_request("redaction_content must be true or false"));
    }
    let new: SessionSettings =
        serde_json::from_value(v).map_err(|_| ZygoError::bad_request("redaction_content must be true or false"))?;
    // §6.1 layer 7: the limits of the serving config.
    app.served()
        .cfg
        .with_overrides(&Overrides { redaction_content: new.redaction_content, ..Default::default() })
        .map_err(|r| ZygoError::new(StatusCode::FORBIDDEN, "zygo_setting_refused", r.to_string()))?;
    app.sessions.accept(&s)?;
    Ok(Json(s.update_settings(&new)).into_response())
}

// ---- model routes (§7.3, §7.4) ----

/// Stage 1 before the body is read.
async fn model_route(app: Arc<App>, route: ModelRoute, req: Request) -> Response {
    match model_identity(req.headers(), &app.sessions) {
        Ok(s) => forward::handle(app, s, route, req).await,
        Err(e) => e.into_response(),
    }
}

async fn messages(State(app): State<Arc<App>>, req: Request) -> Response {
    model_route(app, ModelRoute::Messages, req).await
}

async fn count_tokens(State(app): State<Arc<App>>, req: Request) -> Response {
    model_route(app, ModelRoute::CountTokens, req).await
}

async fn models(State(app): State<Arc<App>>, h: HeaderMap) -> Reply {
    let s = model_identity(&h, &app.sessions)?;
    app.sessions.accept(&s)?;
    Ok(Json(models::list(&s, &app.served().cfg, Dialect::Anthropic)).into_response())
}
