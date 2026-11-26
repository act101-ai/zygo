//! The proxy step (§7.3): body limits and in-flight memory, the stages, model rewrite, forward, fallback, pinning,
//! response headers and the usage row (§7.7, §10.4, §14).

use std::collections::BTreeMap;
use std::pin::Pin;
use std::cell::Cell;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use http::header::{HeaderName, HeaderValue, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, RETRY_AFTER};
use http::{HeaderMap, StatusCode};
use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zygo_config::keys::Secret;
use zygo_config::{Catalog, Dialect, Facts, Family, Prices, Provider, SchemaRule, TokenUsage};
use zygo_core::tokens::BYTES_PER_TOKEN;
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RedactionCount, RequestRow};

use crate::cache;
use crate::errors::ZygoError;
use crate::passthrough::CredentialType;
use crate::pipeline::{Adjustment, ModelRoute, Pipeline, RequestCtx, ResponseCtx};
use crate::provider::{loopback, unavailable, Candidate};
use crate::resolve::Resolved;
use crate::server::App;
use crate::session::{InFlight, SessionState};
use crate::tee::{CacheSplit, ErrorCapture, SseLine, Tee, Ticket, Usage, UsageSink, COPY_MAX, ERROR_BODY_MAX, HOLD_MAX};
use crate::translate::schema::{NullMap, RestoreFilter};
use crate::translate::{
    facts, is_response, responses_client, message_events, reasoning, schema, scrub, translate_error, translate_request, translate_response, SseEvent, StreamMapper,
    Target,
};
use crate::window;
use crate::Url;

pub const BUSY_WAIT: Duration = Duration::from_secs(5); // §7.3
pub const INFLIGHT_FACTOR: u64 = 4; // §7.3
/// The `anthropic-version` sent for a client that sends none (K§6.4.3).
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Never forwarded upstream, whatever `forward_headers` says (§7.3 and its note; RFC 9110 §7.2, §7.6.1, §8.6;
/// RFC 9112 §6.1).
const NEVER: &[&str] = &[
    "authorization",
    "x-api-key",
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
    "cookie",
    "accept-encoding",
];
const NEVER_PREFIX: &[&str] = &["x-zygo-", "x-claude-code-", "proxy-"];

fn never(name: &str) -> bool {
    NEVER.contains(&name) || NEVER_PREFIX.iter().any(|p| name.starts_with(p))
}

/// A new header map for the upstream request (§7.3 and its note): the default list (`content-type`, `accept`, and
/// `anthropic-version`, `anthropic-beta` to an `anthropic` upstream) or the provider's `forward_headers`, minus the
/// never-list; then the provider's credential, marked sensitive.
pub fn outbound_headers(client: &HeaderMap, p: &Provider, upstream: Dialect, key: Option<&Secret>) -> HeaderMap {
    let mut list: Vec<String> = vec!["content-type".into(), "accept".into()];
    if upstream == Dialect::Anthropic {
        list.extend(["anthropic-version".into(), "anthropic-beta".into()]);
    }
    if let Some(f) = &p.forward_headers {
        list = f.iter().map(|n| n.to_ascii_lowercase()).collect();
    }
    let mut out = HeaderMap::new();
    for name in list.iter().filter(|n| !never(n)) {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else { continue };
        if out.contains_key(&name) {
            continue;
        }
        for v in client.get_all(&name) {
            out.append(name.clone(), v.clone());
        }
    }
    // Each vendor's scheme. A key that is no valid header value is not sent.
    if let Some(k) = key {
        let (name, value) = match upstream {
            Dialect::Anthropic => ("x-api-key", HeaderValue::from_str(k.expose())),
            Dialect::Openai | Dialect::OpenaiChat => ("authorization", HeaderValue::from_str(&format!("Bearer {}", k.expose()))),
        };
        if let Ok(mut v) = value {
            v.set_sensitive(true);
            out.insert(name, v);
        }
    }
    out
}

/// The base URL's path without a trailing "/" + `route_path`; query = the base URL's, then the client's.
pub fn upstream_url(base_url: &str, route_path: &str, query: Option<&str>) -> Result<Url, ZygoError> {
    let mut u = Url::parse(base_url)
        .map_err(|_| ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_provider_unavailable", "invalid provider URL"))?;
    let path = format!("{}{route_path}", u.path().trim_end_matches('/'));
    u.set_path(&path);
    let q = match (u.query().filter(|q| !q.is_empty()), query.filter(|q| !q.is_empty())) {
        (Some(a), Some(b)) => Some(format!("{a}&{b}")),
        (a, b) => a.or(b).map(str::to_string),
    };
    u.set_query(q.as_deref());
    Ok(u)
}

/// Hop-by-hop headers (RFC 9110 §7.6.1).
const HOP: &[&str] = &["connection", "proxy-connection", "keep-alive", "te", "trailer", "transfer-encoding", "upgrade"];

/// Upstream headers that can carry a provider URL or proxy credentials (§7.3 note: no provider URL reaches the
/// client, §7.2), or set a client cookie.
const PROVIDER_ONLY: &[&str] =
    &["location", "content-location", "link", "refresh", "proxy-authenticate", "proxy-authentication-info", "set-cookie"];

/// Upstream headers that name the provider account; serve mode drops them (§7.3 note (c), §7.2).
const ACCOUNT: &[&str] = &["openai-organization", "openai-project", "anthropic-organization-id"];

/// Upstream headers minus hop-by-hop, `PROVIDER_ONLY` and `x-zygo-*` (§7.3 note); `content-length` dropped when
/// `rewritten`: in line mode (§10.2.6) or on a translated response (§7.6); `serve`: `ACCOUNT` dropped too (§7.3 note (c)).
pub fn response_headers(upstream: &HeaderMap, rewritten: bool, serve: bool) -> HeaderMap {
    // Headers the `connection` header names are hop-by-hop too.
    let named: Vec<String> = upstream
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|n| n.trim().to_ascii_lowercase())
        .collect();
    let mut out = HeaderMap::new();
    for (name, v) in upstream {
        let n = name.as_str();
        let drop = HOP.contains(&n)
            || named.iter().any(|x| x == n)
            || PROVIDER_ONLY.contains(&n)
            || n.starts_with("x-zygo-")
            || (rewritten && n == "content-length")
            || (serve && ACCOUNT.contains(&n));
        if !drop {
            out.append(name.clone(), v.clone());
        }
    }
    out
}

/// Request body limit (§7.3).
const BODY_LIMIT: u64 = 32 << 20;

fn too_large() -> ZygoError {
    ZygoError::new(StatusCode::PAYLOAD_TOO_LARGE, "zygo_body_too_large", "request body over 32 MiB")
}

fn busy() -> ZygoError {
    ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_busy", "proxy busy: in-flight request memory is full")
}

impl ModelRoute {
    fn client_path(self) -> &'static str {
        match self {
            ModelRoute::Messages => "/anthropic/v1/messages",
            ModelRoute::CountTokens => "/anthropic/v1/messages/count_tokens",
            ModelRoute::Responses => "/openai/v1/responses",
        }
    }
    fn upstream_path(self) -> &'static str {
        match self {
            ModelRoute::Messages => "/v1/messages",
            ModelRoute::CountTokens => "/v1/messages/count_tokens",
            // Its path on an `anthropic` upstream: the Messages body the request side builds (K§6.4.3).
            ModelRoute::Responses => "/v1/messages",
        }
    }
}

/// The upstream path of `route` in dialect `d` (§7.6).
fn upstream_path(route: ModelRoute, d: Dialect) -> &'static str {
    match d {
        Dialect::Anthropic => route.upstream_path(),
        Dialect::Openai => "/v1/responses",
        Dialect::OpenaiChat => "/v1/chat/completions",
    }
}

fn upstream_too_large() -> ZygoError {
    ZygoError::new(StatusCode::BAD_GATEWAY, "zygo_upstream_too_large", "upstream response over 4 MiB")
}

/// An encoded upstream body with a provider key set: the key cannot be found in it (K§6.4.4). A non-2xx keeps the
/// upstream `status`; a 2xx is a 502.
fn upstream_encoded(status: StatusCode) -> ZygoError {
    let s = if status.is_success() { StatusCode::BAD_GATEWAY } else { status };
    ZygoError::new(s, "zygo_upstream_encoded", "upstream response body has a content-encoding")
}

fn upstream_bad_response() -> ZygoError {
    ZygoError::new(StatusCode::BAD_GATEWAY, "zygo_upstream_bad_response", "upstream response could not be translated")
}

/// The public list price of the catalog model `id` (AU14, H§7.3 step 5): the first price in that row of the built-in
/// catalog compiled into this binary. The serving config is not read, so a team's configured price is never the
/// result. `None`: the built-in catalog has no such row, or the row has no price.
fn list_prices(id: &str) -> Option<&'static Prices> {
    static BUILTIN: OnceLock<Catalog> = OnceLock::new();
    let catalog = BUILTIN.get_or_init(|| zygo_config::builtin::defaults().catalog);
    catalog.get(id)?.provider.iter().find_map(|p| p.price_usd_per_mtok.as_ref())
}

/// The usage row of one request (§14); written when dropped: the tokens and cost, `add_request`, then the row.
struct Pending {
    row: RequestRow,
    session: Arc<SessionState>,
    sink: Arc<UsageSink>,
    /// The served provider's price: the row's cost and the session spend.
    prices: Option<Prices>,
    /// Passthrough (H§7.3 step 4), where `prices` is `None`: the session spend only.
    notional_prices: Option<Prices>,
    usage: Usage,
    /// The 5-minute and 1-hour parts of `usage.cache_write` (AU33).
    split: Option<CacheSplit>,
    /// The rows reply waits for it (§14, Q2).
    ticket: Option<Ticket>,
    /// Messages only: the served row's windows from the serving config (AU29).
    windows: Option<window::RequestWindows>,
    /// Set when the request is accepted (AU91).
    in_flight: Option<InFlight>,
    /// Passthrough, an upstream status that is not 2xx: the capture for the `upstream_errors` row (§14, FIX-10).
    upstream_error: Option<ErrorCapture>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        let u = self.usage;
        let r = &mut self.row;
        (r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens) = (u.input, u.output, u.cache_read, u.cache_write);
        let tokens = TokenUsage {
            input: u.input,
            output: u.output,
            cache_read: u.cache_read,
            cache_write: u.cache_write,
            cache_write_5m: self.split.map(|s| s.m5),
            cache_write_1h: self.split.map(|s| s.h1),
        };
        // One price call (AU5, H§7.3 step 3): its result is the row's cost and the session's spend. A notional
        // price is the session's alone: the passthrough row costs 0 (§14).
        let (table, on_row) = match &self.prices {
            Some(p) => (Some(p), true),
            None => (self.notional_prices.as_ref(), false),
        };
        let cost = table.and_then(|p| p.cost(&tokens));
        r.estimated_usd = cost.filter(|_| on_row).unwrap_or(0.0);
        // A team session (AU14, H§7.3 step 5): the cost above is at the server's configured prices and stays in
        // the row. The session spend is a second price call, at the built-in list prices of the served model.
        let spend = match self.session.team_label {
            Some(_) => list_prices(&r.model).and_then(|p| p.cost(&tokens)),
            None => cost,
        };
        self.session.add_request(&tokens, spend);
        // Used context (§10.2, A-9): the upstream prompt of a main-loop (class `main` or absent) Messages 2xx.
        let prompt = window::prompt(u);
        if r.route == "/anthropic/v1/messages"
            && (200..300).contains(&r.status)
            && r.request_class.as_deref().is_none_or(|c| c == "main")
            && prompt > 0
        {
            self.session.set_main_loop(prompt, &r.model, self.windows.take());
        }
        // The request ended (AU91): before the row, so the count is final when the row is written.
        drop(self.in_flight.take());
        self.sink.send(std::mem::take(&mut self.row), self.upstream_error.take(), self.ticket.take());
    }
}

fn names(a: &[Adjustment]) -> Vec<String> {
    a.iter().map(|a| a.0.clone()).collect()
}

fn ms(app: &App, start: Instant) -> u64 {
    u64::try_from(app.clock.now().saturating_duration_since(start).as_millis()).unwrap_or(u64::MAX)
}

/// A model route after stage 1. Every request gets one usage row, refusals included (§14).
pub(crate) async fn handle(app: Arc<App>, s: Arc<SessionState>, route: ModelRoute, req: Request) -> Response {
    let start = app.clock.now();
    let h = req.headers();
    let hint = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    let row = RequestRow {
        ts: app.clock.utc().unix_timestamp(),
        session: s.id.clone(),
        token_label: s.team_label.clone(),
        agent: s.agent.as_str().into(),
        agent_version: Some(s.agent_version.clone()),
        agent_id: hint("x-claude-code-agent-id"),
        agent_type: hint("x-claude-code-agent-type"),
        request_class: hint("x-claude-code-request-class"),
        compaction: h.contains_key("x-claude-code-compaction"),
        route: route.client_path().into(),
        profile: s.profile.as_ref().map(|p| p.name.clone()),
        passthrough: s.passthrough,
        repo: s.repo.clone(),
        work_item: s.work.clone(),
        ..Default::default()
    };
    let ticket = Some(app.usage.begin(&s.id));
    let mut slot = Some(Pending { row, session: s.clone(), sink: app.usage.clone(), prices: None, notional_prices: None, usage: Usage::default(), split: None, ticket, windows: None, in_flight: None, upstream_error: None });
    match run(&app, &s, route, req, &mut slot, start).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(p) = slot.as_mut() {
                p.row.status = e.status.as_u16();
                p.row.latency_ms = ms(&app, start);
            }
            e.response(route.dialect())
        }
    }
}

fn pending(slot: &mut Option<Pending>) -> &mut Pending {
    slot.as_mut().expect("the usage row is pending until the response")
}

async fn run(
    app: &Arc<App>,
    s: &Arc<SessionState>,
    route: ModelRoute,
    req: Request,
    slot: &mut Option<Pending>,
    start: Instant,
) -> Result<Response, ZygoError> {
    let headers = req.headers().clone();
    let query = req.uri().query().map(str::to_string);
    let (raw, permit) = read_charged(app, req).await?;
    let repeated = key_scan(&raw)?;
    let body: Value = serde_json::from_slice(&raw).map_err(|_| ZygoError::bad_request("invalid JSON body"))?;
    // The Responses field lists before any stage (K§11 item 12).
    if route == ModelRoute::Responses {
        responses_client::check_fields(&body)?;
    }
    // The client's tools as received, for the call items of the answer (K§6.4.3 stream obligation 2).
    let calls = (route == ModelRoute::Responses).then(|| responses_client::Calls::of(&body));
    let name = body.get("model").and_then(Value::as_str).ok_or_else(|| ZygoError::bad_request("model must be a string"))?.to_string();
    // The name the client sent: the Response's `model` (stream obligation 1).
    let client_model = name.clone();
    let mut ctx = RequestCtx {
        session: s.clone(),
        cfg: app.served().cfg.clone(),
        route,
        dialect: route.dialect(),
        headers,
        raw,
        body,
        changed: repeated,
        resolved: None,
        candidates: Vec::new(),
        adjustments: Vec::new(),
        redactions: BTreeMap::new(),
        redacted: false,
        line_mode: false,
    };
    // The stages are CPU-bound (N12's scans): a blocking thread. The in-flight charge lives through them.
    let pipeline = app.pipeline.clone();
    let (mut ctx, r, permit, requested) = match tokio::task::spawn_blocking(move || {
        let r = pipeline.on_request(&mut ctx);
        let requested = requested_name(&ctx, &r, &name);
        if ctx.session.passthrough {
            passthrough_names(&mut ctx, requested.as_deref());
        }
        (ctx, r, permit, requested)
    })
    .await
    {
        Ok(x) => x,
        Err(e) => match e.try_into_panic() {
            Ok(p) => std::panic::resume_unwind(p),
            Err(_) => return Err(unavailable()),
        },
    };
    {
        let p = pending(slot);
        p.row.adjustments = names(&ctx.adjustments);
        p.row.redactions = ctx.redactions.iter().map(|(id, n)| RedactionCount { id: id.clone(), count: *n }).collect();
        if let Some(rs) = &ctx.resolved {
            p.row.role_served = Some(rs.role.clone());
            p.row.model = rs.model.clone();
        }
        p.row.role_requested = requested;
    }
    r?;
    // With an `openai` candidate, before any candidate is tried: one answer whichever candidate serves (K§6.4.3
    // Responses → Responses). With none, `include` is safe to ignore.
    if route == ModelRoute::Responses
        && ctx.candidates.iter().any(|c| c.dialect == Dialect::Openai)
        && ctx.body.get("include").is_some_and(|i| !i.is_null() && !i.is_array())
    {
        return Err(ZygoError::bad_request("include must be an array"));
    }
    app.sessions.accept(s)?;
    pending(slot).in_flight = Some(InFlight::begin(s, &app.clock));
    let resolved = ctx.resolved.clone().expect("stage 2 resolved the request");
    if route == ModelRoute::Messages {
        pending(slot).windows = ctx.cfg.config.catalog.get(&resolved.model)
            .map(|row| window::RequestWindows::of(row, window::agent_window_for(resolved.one_m, &ctx.headers, row)));
    }

    // count_tokens on a provider that does not speak `anthropic`: a local estimate (§7.3).
    if route == ModelRoute::CountTokens && ctx.candidates.first().is_some_and(|c| c.dialect != Dialect::Anthropic) {
        drop(permit);
        ctx.adjustments.push(Adjustment("count_estimated".into()));
        let p = pending(slot);
        p.row.status = 200;
        p.row.latency_ms = ms(app, start);
        p.row.adjustments = names(&ctx.adjustments);
        let mut resp = Json(json!({ "input_tokens": string_bytes(&ctx.body).div_ceil(BYTES_PER_TOKEN) })).into_response();
        zygo_headers(resp.headers_mut(), &resolved, &ctx.adjustments, ctx.redacted);
        return Ok(resp);
    }

    // Model facts and strict schemas (§7.6).
    let cfg = ctx.cfg.clone();
    let rw = Rewrite {
        facts: cfg.config.catalog.get(&resolved.model).and_then(|m| m.facts.as_ref()),
        rules: &cfg.config.schema_rule,
        strict: cfg.config.proxy.strict_schemas,
        markers: cfg.config.cache.insert_markers.get(s.agent.as_str()).copied().unwrap_or(false),
    };
    // Attempts (§7.3): fallback on a connect error, a 5xx or a 429.
    let mut attempted = false;
    let mut served = None;
    let mut last = None;
    let mut refused: Option<ZygoError> = None;
    // Passthrough: the client's login goes only to the type's upstream host (§12.2, §12.4).
    let pt = s.passthrough.then(|| CredentialType::for_agent(s.agent)).flatten();
    #[cfg(feature = "test-hooks")]
    let on_host = |t: &CredentialType, u: &Url| crate::passthrough::on_host(t, u, &app.test_hosts);
    #[cfg(not(feature = "test-hooks"))]
    let on_host = |t: &CredentialType, u: &Url| t.host_matches(u);
    for c in &ctx.candidates {
        // count_tokens is not translated (§7.3).
        if route == ModelRoute::CountTokens && c.dialect != ctx.dialect {
            continue;
        }
        let Ok(url) = upstream_url(&c.provider.base_url, upstream_path(route, c.dialect), query.as_deref()) else { continue };
        if pt.is_some_and(|t| !on_host(t, &url)) {
            continue;
        }
        // A translation error is this candidate's failure (§7.6); with no candidate attempted it is the reply (§7.3 note (d)).
        let (body, extra, nulls) = match attempt_body(&mut ctx.body, &ctx.raw, ctx.changed, c, ctx.dialect, &rw) {
            Ok(x) => x,
            Err(e) => {
                refused.get_or_insert(e);
                continue;
            }
        };
        attempted = true;
        // A loopback upstream never goes through a proxy (C3).
        let http = if loopback(&url, &[]) { &app.direct } else { &app.http };
        let mut out = match pt {
            Some(t) => t.outbound(&ctx.headers, &c.provider),
            None => outbound_headers(&ctx.headers, &c.provider, c.dialect, c.key.as_ref()),
        };
        // A Responses client sends no `anthropic-version`; the Anthropic API needs it (K§6.4.3).
        if ctx.dialect == Dialect::Openai && c.dialect == Dialect::Anthropic && !out.contains_key("anthropic-version") {
            out.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
        }
        let sent = http
            .post(url)
            .headers(out)
            .body(body)
            .send()
            .await;
        match sent {
            Ok(r) if r.status().is_server_error() || r.status() == StatusCode::TOO_MANY_REQUESTS => last = Some((r, c, extra, nulls)),
            Ok(r) => {
                served = Some((r, c, extra, nulls));
                break;
            }
            Err(e) if e.is_connect() => last = None,
            // Request bytes went out: no retry (§7.3 note).
            Err(_) => {
                last = None;
                break;
            }
        }
    }
    let model = resolved.model.clone();
    let pins = || s.pins.lock().unwrap_or_else(|e| e.into_inner());
    // A served attempt pins its provider once its response is accepted (below).
    let pin = served.is_some();
    let (resp, c, extra, nulls) = match (served, last) {
        (Some(x), _) => x,
        // All attempts failed (§10.4): the pin is removed; the last 5xx or 429 goes to the client.
        (None, Some(x)) => {
            pins().remove(&model);
            x
        }
        (None, None) => {
            if attempted {
                pins().remove(&model);
            }
            return Err(refused.filter(|_| !attempted).unwrap_or_else(unavailable));
        }
    };

    let mut status = resp.status();
    // The upstream `content-length`: a body that reached it is finished although the server stops polling (FIX-10).
    let body_len = resp.content_length();
    let mut streamed = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim_start().to_ascii_lowercase().starts_with("text/event-stream"));
    // A translated response (§7.6): a 2xx SSE stream goes through the mapper; any other body is read whole (at most
    // COPY_MAX, with the in-flight charge still held) and mapped.
    let sse_upstream = streamed;
    let translated = c.dialect != ctx.dialect;
    // A translated body reaches the tee as Messages events or a Messages body: the mapper output, an Anthropic
    // upstream, or `translate_response` (K§6.4.2 C3).
    let form = if translated { Dialect::Anthropic } else { c.dialect };
    // The configured key without ASCII whitespace at its ends: what an upstream echoes (K§6.4.4).
    let key = c.key.as_ref().map(|k| k.expose().trim_ascii());
    let estimate = string_bytes(&ctx.body).div_ceil(BYTES_PER_TOKEN);
    let mut headers = response_headers(resp.headers(), translated, s.team_label.is_some());
    let mut adjustments = ctx.adjustments;
    adjustments.extend(extra);
    // With a provider key (K§6.4.4): a header that holds it is dropped, on every path; an encoded body is refused,
    // as the key cannot be found in it.
    let keyed = key.is_some_and(|k| !k.is_empty());
    if let Some(k) = key.filter(|_| keyed) {
        if drop_keyed(&mut headers, k) {
            adjustments.push(Adjustment("key_header_dropped".into()));
        }
        if resp.headers().get_all(CONTENT_ENCODING).iter().any(|v| !v.as_bytes().trim_ascii().eq_ignore_ascii_case(b"identity")) {
            // The upstream `retry-after` (a 429 or 5xx) is kept on the reply.
            let e = read_failed(slot, c, &adjustments, upstream_encoded(status));
            let p = pending(slot);
            p.row.status = e.status.as_u16();
            p.row.latency_ms = ms(app, start);
            let mut out = e.response(route.dialect());
            if let Some(v) = headers.get(RETRY_AFTER) {
                out.headers_mut().insert(RETRY_AFTER, v.clone());
            }
            return Ok(out);
        }
    }
    let mut mapper = None;
    // Injected nulls are removed on the way back (§7.6): a Messages 2xx, or a translated Responses 2xx, with a
    // non-empty map.
    let armed = status.is_success()
        && !nulls.is_empty()
        && (route == ModelRoute::Messages || (route == ModelRoute::Responses && translated));
    // The Messages body of a translated 2xx read whole: a Responses client's Response object is built from it.
    let mut msg = None;
    let mut restore = None;
    // An untranslated 2xx JSON body with injected nulls: restored whole, then the key is replaced (below).
    let restore_whole = !translated && armed && !streamed;
    // With a provider key, every other 2xx upstream body, translated or not, loses the key in every byte as it is
    // read, before any stage reads it: the translator, the mapper, the restore filter, the encoder, the tee and the
    // line stages (K§6.4.4 (4)). Its length is no longer known.
    let mut raw: Upstream = Box::pin(resp.bytes_stream());
    if let Some(k) = key.and_then(KeyScrub::new).filter(|_| status.is_success() && !restore_whole) {
        headers.remove(CONTENT_LENGTH);
        raw = key_scrubbed(raw, k);
    }
    let upstream: Upstream = if !translated && armed && streamed {
        restore = Some(RestoreFilter::new(nulls));
        headers.remove(CONTENT_LENGTH);
        raw
    } else if restore_whole {
        let s = restored_body(raw, &nulls, &mut headers).await.map_err(|e| read_failed(slot, c, &adjustments, e))?;
        match key.and_then(KeyScrub::new) {
            Some(k) => {
                headers.remove(CONTENT_LENGTH);
                key_scrubbed(s, k)
            }
            None => s,
        }
    } else if !translated && !status.is_success() && keyed {
        // An untranslated non-2xx body with a provider key is read whole (at most COPY_MAX) and loses the key (K§6.4.4).
        let bytes = read_capped(raw).await.map_err(|e| read_failed(slot, c, &adjustments, e))?;
        let out = match key.and_then(KeyScrub::new) {
            Some(mut k) => {
                let mut out = k.push(&bytes);
                out.extend_from_slice(&k.finish());
                Bytes::from(out)
            }
            None => bytes,
        };
        headers.insert(CONTENT_LENGTH, HeaderValue::from(out.len()));
        futures_util::stream::once(async move { Ok(out) }).boxed()
    } else if !translated && !streamed && keyed {
        // An untranslated 2xx that is not SSE, with a provider key (K§6.4.4 (4)).
        keyed_body(raw, &mut headers).await.map_err(|e| read_failed(slot, c, &adjustments, e))?
    } else if !translated {
        raw
    } else if status.is_success() && streamed {
        // An Anthropic upstream already sends Messages SSE (a Responses client): no mapper (K§6.4.2).
        mapper = (c.dialect != Dialect::Anthropic).then(|| StreamMapper::new(c.dialect, &Target::of(c)).errors(estimate, key));
        restore = armed.then(|| RestoreFilter::new(nulls));
        raw
    } else {
        let bytes = read_capped(raw).await.map_err(|e| read_failed(slot, c, &adjustments, e))?;
        // A 2xx that is no response body of the upstream dialect never reaches the client (§7.3, §7.6).
        let v = if status.is_success() {
            // An Anthropic upstream (a Responses client) answers in the Messages form already.
            let anthropic = c.dialect == Dialect::Anthropic;
            let v = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .filter(|v| if anthropic { is_message(v) } else { is_response(v, c.dialect) })
                .ok_or_else(|| read_failed(slot, c, &adjustments, upstream_bad_response()))?;
            let mut m = if anthropic { v } else { translate_response(&v, c.dialect, &Target::of(c)) };
            if armed {
                nulls.restore_message(&mut m);
            }
            msg = (ctx.dialect == Dialect::Openai).then(|| m.clone());
            m
        } else {
            let (s, v) = if ctx.dialect == Dialect::Openai {
                responses_error(status, &bytes, estimate, key)
            } else {
                translate_error(status, &bytes, estimate, key)
            };
            status = s;
            v
        };
        // A client that asked to stream gets a 2xx as Messages SSE events.
        streamed = status.is_success() && ctx.body.get("stream").and_then(Value::as_bool) == Some(true);
        let out = if streamed {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            message_events(&v).iter().flat_map(SseEvent::to_bytes).collect()
        } else {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            serde_json::to_vec(&v).unwrap_or_default()
        };
        futures_util::stream::once(async move { Ok(Bytes::from(out)) }).boxed()
    };
    // A translated 2xx to a Responses client: the encoder after the tee, or the whole Response object (K§6.4.2 C3).
    let encode = if ctx.dialect == Dialect::Openai && translated && status.is_success() {
        let t = Target::of(c);
        let calls = calls.unwrap_or_default();
        if streamed {
            Some(Encode::Stream(Box::new(Streamed::new(&client_model, &t, calls, key.map(Secret::new)))))
        } else {
            let (r, codes) = responses_client::response(msg.as_ref().unwrap_or(&Value::Null), &client_model, &t, &calls);
            adjustments.extend(codes);
            Some(Encode::Whole(Bytes::from(serde_json::to_vec(&r).unwrap_or_default())))
        }
    } else {
        None
    };
    if pin {
        pins().insert(model, c.provider.name.clone());
    }
    drop(permit);
    let mut tee = Tee::new(streamed).with_lines(form);
    // Passthrough, not 2xx: the copy stays after `finish` for the upstream_errors capture (§14, FIX-10).
    if s.passthrough && !status.is_success() {
        tee.keep_body();
    }
    // Only an SSE upstream body is rewritten (§10.2.6): not a whole body sent to the client as SSE.
    // A Responses stream is read in lines too: its upstream `error` lines are rewritten before the encoder (K§6.4.4).
    let lines = ctx.line_mode || matches!(encode, Some(Encode::Stream(_)));
    if lines && status.is_success() && sse_upstream {
        tee.arm_line_mode();
    }
    if tee.line_mode() {
        headers.remove(CONTENT_LENGTH);
    }
    zygo_headers(&mut headers, &resolved, &adjustments, ctx.redacted);
    let mut p = slot.take().expect("the usage row is pending until the response");
    p.row.status = status.as_u16();
    p.row.latency_ms = ms(app, start);
    p.row.provider = c.provider.name.clone();
    p.row.adjustments = names(&adjustments);
    p.prices = c.provider.price_usd_per_mtok.clone();
    p.notional_prices = resolved.notional_prices.clone();
    let rctx = ResponseCtx {
        session: s.clone(),
        cfg: ctx.cfg.clone(),
        headers: ctx.headers,
        status,
        resolved,
        provider: c.provider.name.clone(),
        dialect: c.dialect,
        lines: form,
        adjustments,
        usage: Usage::default(),
        compact_call: false,
        call_item: false,
    };
    let flow = Flow { upstream, mapper, restore, tee, encode, window: ctx.line_mode, rctx, pipeline: app.pipeline.clone(), pending: p, done: false, failed: false, body_len };
    let mut out = Response::new(Body::from_stream(futures_util::stream::unfold(flow, next_chunk)));
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    Ok(out)
}

/// An upstream error on a translated Responses request, before the first byte (K§6.4.4, K-D50): `translate_error`'s
/// status and error in the OpenAI shape; a context-limit refusal in the shape Codex recovers from (PC14,
/// `fixtures/codex/errors/context-400.json`): a `prompt is too long` message (Anthropic's own text, or `translate_error`'s
/// mapping), or Anthropic's full `max_tokens` limit form with its message cleaned (`pc14_message`). That form is
/// mapped here only: every other client gets it unchanged (Claude Code parses it).
fn responses_error(status: StatusCode, body: &[u8], estimate: u64, key: Option<&str>) -> (StatusCode, Value) {
    let (s, m) = translate_error(status, body, estimate, key);
    let message = m["error"]["message"].as_str().unwrap_or_default();
    if s == StatusCode::BAD_REQUEST && let Some(message) = pc14_form(message) {
        let e = json!({"message": message, "type": "invalid_request_error", "param": "input", "code": CONTEXT_CODE});
        return (s, json!({ "error": e }));
    }
    (s, json!({ "error": m["error"] }))
}

/// The most characters of an upstream message in the PC14 shape: the limit form's numbers survive (K§6.4.4).
const PC14_MESSAGE_MAX: usize = 512;

/// The upstream message, its first `PC14_MESSAGE_MAX` characters, each control (Cc), format (Cf), line separator (Zl)
/// or paragraph separator (Zp) character replaced with `?` (K§6.4.4).
fn pc14_message(m: &str) -> String {
    m.chars()
        .take(PC14_MESSAGE_MAX)
        .map(|c| if c.is_control() || responses_client::is_format_or_separator(c) { '?' } else { c })
        .collect()
}

/// The error code with which Codex runs its own recovery for an over-long prompt (PC14, K§6.4.4).
const CONTEXT_CODE: &str = "context_length_exceeded";

/// An over-long prompt's message by the PC14 rule (K§6.4.4): a `prompt is too long` text or Anthropic's full
/// `max_tokens` limit form, cleaned (`pc14_message`, K§6.4.3 obligation 6); `None` for any other message.
fn pc14_form(message: &str) -> Option<String> {
    (message.starts_with("prompt is too long") || anthropic_limit_form(message)).then(|| pc14_message(message))
}

/// Anthropic's refusal when input plus `max_tokens` is over the window, in full (the form Claude Code parses).
fn anthropic_limit_form(m: &str) -> bool {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"input length and `max_tokens` exceed context limit: \d+ \+ \d+ > \d+").expect("valid pattern"))
        .is_match(m)
}

/// A read error on the served response (Round 2, F6), or a translated 2xx that is not a response (Round 3, D4): the
/// usage row gets the provider and the adjustments.
fn read_failed(slot: &mut Option<Pending>, c: &Candidate, adjustments: &[Adjustment], e: ZygoError) -> ZygoError {
    let p = pending(slot);
    p.row.provider = c.provider.name.clone();
    p.row.adjustments = names(adjustments);
    e
}

/// `X-Zygo-*` response headers (§7.3, §7.7). A value that is no valid header value is left out.
fn zygo_headers(h: &mut HeaderMap, r: &Resolved, adjustments: &[Adjustment], redacted: bool) {
    if let Ok(v) = HeaderValue::from_str(&r.model) {
        h.insert("x-zygo-model", v);
    }
    if let Ok(v) = HeaderValue::from_str(&r.role) {
        h.insert("x-zygo-role", v);
    }
    if !adjustments.is_empty()
        && let Ok(v) = HeaderValue::from_str(&names(adjustments).join(","))
    {
        h.insert("x-zygo-adjustments", v);
    }
    if redacted {
        h.insert("x-zygo-redacted", HeaderValue::from_static("1"));
    }
}

/// UTF-8 bytes of every string value; keys excluded (§7.3 `count_tokens` estimate).
fn string_bytes(v: &Value) -> u64 {
    match v {
        Value::String(s) => s.len() as u64,
        Value::Array(a) => a.iter().map(string_bytes).sum(),
        Value::Object(o) => o.values().map(string_bytes).sum(),
        _ => 0,
    }
}

/// Object keys serde_json reads as its own markers when the `raw_value` and `arbitrary_precision` features are on.
const RESERVED: &[&str] = &["$serde_json::private::RawValue", "$serde_json::private::Number"];

/// What `key_scan` found: a `RESERVED` key, an object that names a key twice.
#[derive(Default)]
struct Keys {
    reserved: Cell<bool>,
    repeated: Cell<bool>,
}

/// A JSON value walked for its object keys; it keeps nothing. A repeat is recorded and the walk goes on; a
/// `RESERVED` key is recorded, then the walk stops.
struct Unique<'a>(&'a Keys);

/// An object key as written in the body, unescaped; `None` for the key serde_json makes for a number.
struct Key(Option<String>);

impl<'de> DeserializeSeed<'de> for Unique<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Unique<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON")
    }
    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        while a.next_element_seed(Unique(self.0))?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let mut seen = std::collections::HashSet::new();
        while let Some(Key(k)) = a.next_key::<Key>()? {
            if let Some(k) = k {
                if RESERVED.contains(&k.as_str()) {
                    self.0.reserved.set(true);
                    return Err(de::Error::custom("reserved key"));
                }
                if !seen.insert(k) {
                    self.0.repeated.set(true);
                }
            }
            a.next_value_seed(Unique(self.0))?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Key {
    // serde_json gives a written key as its raw bytes (`visit_bytes`) and its number key as a `str`.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        d.deserialize_bytes(KeyVisitor)
    }
}

struct KeyVisitor;

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = Key;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an object key")
    }
    fn visit_str<E>(self, _: &str) -> Result<Key, E> {
        Ok(Key(None))
    }
    fn visit_bytes<E: de::Error>(self, b: &[u8]) -> Result<Key, E> {
        let k = if b.contains(&b'\\') {
            serde_json::from_slice::<String>(&[b"\"", b, b"\""].concat()).map_err(E::custom)?
        } else {
            String::from_utf8(b.to_vec()).map_err(E::custom)?
        };
        Ok(Key(Some(k)))
    }
}

/// The object keys of `raw` after unescaping (§7.3 note (a)), before the body is parsed: `400 zygo_bad_request` on a
/// `RESERVED` key anywhere (C4: it would make the parse differ from the bytes); else true when an object names a key
/// twice, or `raw` does not scan (it is then serialized again). The whole body is scanned before either is decided.
/// O(body).
fn key_scan(raw: &[u8]) -> Result<bool, ZygoError> {
    let keys = Keys::default();
    let mut d = serde_json::Deserializer::from_slice(raw);
    let r = Unique(&keys).deserialize(&mut d).and_then(|()| d.end());
    if keys.reserved.get() {
        return Err(ZygoError::bad_request("the body holds a reserved object key"));
    }
    Ok(keys.repeated.get() || r.is_err())
}

/// `name` without a trailing `[1m]`.
fn strip_1m(n: &str) -> String {
    n.strip_suffix("[1m]").unwrap_or(n).to_string()
}

/// `role_requested` (§7.3 note (b), §14): after a stage-6 secret pass that ran and passed, `body.model` as it left it
/// (no second lock, C5); after a stage-6 refusal `None`; else (a refusal at stages 2–5, or the pass off) `row_name`.
fn requested_name(ctx: &RequestCtx, r: &Result<(), ZygoError>, name: &str) -> Option<String> {
    match r {
        Ok(()) if ctx.session.features().redaction.secrets => ctx.body.get("model").and_then(Value::as_str).map(strip_1m),
        Err(e) if matches!(e.code, "zygo_redaction_block" | "zygo_redaction_unavailable") => None,
        _ => row_name(&ctx.session, name),
    }
}

/// The secret pass's output for `name`, `[1m]` stripped; `None` on a `block` match or a panic.
fn row_name(s: &SessionState, name: &str) -> Option<String> {
    crate::redact_stage::secret_pass(s, name, &DeadlinePolicy::default()).map(|n| strip_1m(&n))
}

/// Passthrough (§12.4) sends and serves the name stage 6 left (§7.3 note (b), C4): each candidate's `upstream_model`
/// is `body.model`; an unmatched name's `Resolved.model` is `requested` (`""` when `None`).
fn passthrough_names(ctx: &mut RequestCtx, requested: Option<&str>) {
    if let Some(m) = ctx.body.get("model").and_then(Value::as_str) {
        for c in &mut ctx.candidates {
            c.provider.upstream_model = m.to_string();
        }
    }
    if let Some(rs) = ctx.resolved.as_mut().filter(|rs| rs.model == rs.requested) {
        rs.model = requested.unwrap_or_default().to_string();
    }
}

/// `openrouter.ai` or a subdomain (§7.5).
fn openrouter(base_url: &str) -> bool {
    Url::parse(base_url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).is_some_and(|h| {
        h == "openrouter.ai" || h.ends_with(".openrouter.ai")
    })
}

/// The model's facts, the configured schema rules and `[proxy] strict_schemas` (§7.6).
struct Rewrite<'a> {
    facts: Option<&'a Facts>,
    rules: &'a [SchemaRule],
    strict: bool,
    /// `[cache] insert_markers.<agent>` for the session's agent (§10.4); read only toward an `anthropic` candidate of a
    /// Responses client (K§6.4.2).
    markers: bool,
}

/// One attempt's body (§7.6): translated when the candidate's dialect is not the client's (facts, then the schema
/// rewrite, on a copy); else without the zygo thinking blocks, which belong to no `anthropic` target, and rewritten
/// when `strict`. The rule family is the candidate's vendor. A translation error is `Err`.
fn attempt_body(
    body: &mut Value,
    raw: &Bytes,
    changed: bool,
    c: &Candidate,
    client: Dialect,
    rw: &Rewrite,
) -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError> {
    let family = if c.dialect == Dialect::Anthropic { Family::Anthropic } else { Family::Openai };
    let rules: Vec<SchemaRule> = rw.rules.iter().filter(|r| r.family == family).cloned().collect();
    if client == Dialect::Openai && c.dialect == Dialect::Openai {
        return responses_same(body, raw, changed, c);
    }
    if client == Dialect::Openai && c.dialect != client {
        return responses_attempt(body, raw, c, rw, &rules);
    }
    if c.dialect != client {
        let mut a = body.clone();
        let mut codes = match rw.facts {
            Some(f) => facts::apply(&mut a, f),
            None => vec![Adjustment("facts_missing".into())],
        };
        let (sc, nulls) = schema::rewrite(&mut a, &rules);
        codes.extend(sc);
        let (mut v, tc) = translate_request(&a, c.dialect, &Target::of(c))?;
        if rw.facts.is_some() {
            facts::write_effort(&a, &mut v, c.dialect);
        }
        let (b, mut adj) = model_body(&mut v, raw, true, c)?;
        adj.extend(codes);
        adj.extend(tc);
        return Ok((b, adj, nulls));
    }
    let cleaned = reasoning::without_zygo_thinking(body);
    if rw.strict {
        let (mut v, dropped) = match cleaned {
            Some((v, n)) => (v, Some(n)),
            None => (body.clone(), None),
        };
        let (sc, nulls) = schema::rewrite(&mut v, &rules);
        if !sc.is_empty() || dropped.is_some() {
            let (b, mut adj) = model_body(&mut v, raw, true, c)?;
            adj.extend(dropped.map(|n| Adjustment(format!("thinking_dropped:{n}"))));
            adj.extend(sc);
            return Ok((b, adj, nulls));
        }
        let (b, adj) = model_body(body, raw, changed, c)?;
        return Ok((b, adj, NullMap::default()));
    }
    if let Some((mut v, n)) = cleaned {
        let (b, mut adj) = model_body(&mut v, raw, true, c)?;
        adj.push(Adjustment(format!("thinking_dropped:{n}")));
        return Ok((b, adj, NullMap::default()));
    }
    let (b, adj) = model_body(body, raw, changed, c)?;
    Ok((b, adj, NullMap::default()))
}

/// The reasoning item content Codex needs on the next turn when nothing is stored upstream (K§6.4.3).
const ENCRYPTED_REASONING: &str = "reasoning.encrypted_content";

/// A Responses body toward an openai candidate (K§6.4.3 Responses → Responses): store := false, include gains
/// reasoning.encrypted_content, then model_body. body is left as it was.
fn responses_same(
    body: &mut Value,
    raw: &Bytes,
    changed: bool,
    c: &Candidate,
) -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError> {
    let store_ok = body.get("store") == Some(&Value::Bool(false));
    // `run` refused any other JSON type before the first candidate.
    let include = match body.get("include").and_then(Value::as_array) {
        None => Some(json!([ENCRYPTED_REASONING])),
        Some(a) if a.iter().any(|v| v.as_str() == Some(ENCRYPTED_REASONING)) => None,
        Some(a) => {
            let mut a = a.clone();
            a.push(json!(ENCRYPTED_REASONING));
            Some(Value::Array(a))
        }
    };
    if store_ok && include.is_none() {
        let (b, adj) = model_body(body, raw, changed, c)?;
        return Ok((b, adj, NullMap::default()));
    }
    let mut v = body.clone();
    let obj = v.as_object_mut().ok_or_else(|| ZygoError::bad_request("the body must be a JSON object"))?;
    obj.insert("store".into(), Value::Bool(false));
    if let Some(i) = include {
        obj.insert("include".into(), i);
    }
    let (b, adj) = model_body(&mut v, raw, true, c)?;
    Ok((b, adj, NullMap::default()))
}

/// A Responses body toward an `anthropic` or `openai-chat` candidate (K§6.4.3): the Messages body of
/// `responses_client::request`, then facts and the schema rewrite as on a translated Messages request; to
/// `openai-chat`, the Messages → chat translator after them (Messages is the pivot); to `anthropic`, the §10.4 markers
/// when `rw.markers` (K§6.4.2), last, so they sit on the final `system` text. Codes in the order `model_body`,
/// `request`, facts, schema, translator.
fn responses_attempt(
    body: &Value,
    raw: &Bytes,
    c: &Candidate,
    rw: &Rewrite,
    rules: &[SchemaRule],
) -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError> {
    let t = Target::of(c);
    let (mut m, rc) = responses_client::request(body, &t, rw.facts)?;
    let mut codes = match rw.facts {
        Some(f) => facts::apply(&mut m, f),
        None => vec![Adjustment("facts_missing".into())],
    };
    let (sc, nulls) = schema::rewrite(&mut m, rules);
    codes.extend(sc);
    let (mut v, tc) = if c.dialect == Dialect::OpenaiChat {
        let (mut v, tc) = translate_request(&m, Dialect::OpenaiChat, &t)?;
        if rw.facts.is_some() {
            facts::write_effort(&m, &mut v, Dialect::OpenaiChat);
        }
        (v, tc)
    } else {
        if rw.markers {
            cache::mark_upstream(&mut m);
        }
        (m, Vec::new())
    };
    let (b, mut adj) = model_body(&mut v, raw, true, c)?;
    adj.extend(rc);
    adj.extend(codes);
    adj.extend(tc);
    Ok((b, adj, nulls))
}

/// `model` := `upstream_model`; `provider.zdr = true` on an openrouter.ai host (§7.5), else
/// `zdr` is recorded only. A body that nothing changed is the received bytes (§7.3). `body` is left as it was.
fn model_body(body: &mut Value, raw: &Bytes, changed: bool, c: &Candidate) -> Result<(Bytes, Vec<Adjustment>), ZygoError> {
    let zdr = c.provider.flags.zdr && openrouter(&c.provider.base_url);
    let mut adj = Vec::new();
    if c.provider.flags.zdr && !zdr {
        adj.push(Adjustment("zdr_host_unsupported".into()));
    }
    let same_model = body.get("model").and_then(Value::as_str) == Some(c.provider.upstream_model.as_str());
    let Some(obj) = body.as_object_mut().filter(|_| changed || !same_model || zdr) else { return Ok((raw.clone(), adj)) };
    let model = obj.insert("model".into(), Value::String(c.provider.upstream_model.clone()));
    let provider = zdr.then(|| {
        let old = obj.get("provider").cloned();
        match obj.get_mut("provider").and_then(Value::as_object_mut) {
            Some(p) => {
                p.insert("zdr".into(), Value::Bool(true));
            }
            None => {
                obj.insert("provider".into(), json!({ "zdr": true }));
            }
        }
        old
    });
    let out = serialized(serde_json::to_vec(&*obj));
    if let Some(m) = model {
        obj.insert("model".into(), m);
    }
    match provider {
        Some(Some(p)) => {
            obj.insert("provider".into(), p);
        }
        Some(None) => {
            obj.shift_remove("provider");
        }
        None => {}
    }
    Ok((out?, adj))
}

/// A body serialized again, or `503 zygo_redaction_unavailable`: never the received bytes in its place (C4).
fn serialized(r: serde_json::Result<Vec<u8>>) -> Result<Bytes, ZygoError> {
    r.map(Bytes::from).map_err(|_| {
        ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_redaction_unavailable", "the body did not serialize again")
    })
}

/// The next body chunk; with `idle` (serve mode) a wait longer than `idle` is a read error (§7 note 2026-11-19).
pub(crate) async fn next_within<S: Stream<Item = Result<Bytes, axum::Error>> + Unpin>(s: &mut S, idle: Option<Duration>) -> Option<S::Item> {
    match idle {
        None => s.next().await,
        Some(d) => tokio::time::timeout(d, s.next()).await.unwrap_or_else(|_| Some(Err(axum::Error::new("body read idle")))),
    }
}

/// Reads the body under the 32 MiB limit, charging 4 × its bytes to the in-flight budget first (§7.3): from
/// `content-length` when present, else per received chunk. All charges share one `busy_wait` deadline. A
/// `content-length` over the limit: the body is read and discarded up to the limit, then 413 (a client still sending
/// would otherwise get a broken pipe instead of the reply). `app.body_idle` bounds each wait for a chunk; a stall is
/// an unreadable body.
async fn read_charged(app: &App, req: Request) -> Result<(Bytes, Option<OwnedSemaphorePermit>), ZygoError> {
    let deadline = tokio::time::Instant::now() + app.busy_wait;
    let len = req.headers().get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let unreadable = |_| ZygoError::bad_request("unreadable body");
    if let Some(n) = len {
        if n > BODY_LIMIT {
            discard(&mut req.into_body().into_data_stream(), app.body_idle, 0).await;
            return Err(too_large());
        }
        let permit = match charge(&app.inflight, cost(n)?, deadline).await {
            Ok(p) => p,
            Err(e) => {
                discard(&mut req.into_body().into_data_stream(), app.body_idle, 0).await;
                return Err(e);
            }
        };
        // Hyper's HTTP/1 decoder ends the body at `content-length` (≤ BODY_LIMIT); serve mode speaks only HTTP/1.1.
        let mut buf = BytesMut::new();
        let mut s = req.into_body().into_data_stream();
        while let Some(c) = next_within(&mut s, app.body_idle).await {
            buf.extend_from_slice(&c.map_err(unreadable)?);
        }
        return Ok((buf.freeze(), permit));
    }
    let mut permit: Option<OwnedSemaphorePermit> = None;
    let mut buf = BytesMut::new();
    let mut s = req.into_body().into_data_stream();
    while let Some(chunk) = next_within(&mut s, app.body_idle).await {
        let c = chunk.map_err(unreadable)?;
        if (buf.len() + c.len()) as u64 > BODY_LIMIT {
            return Err(too_large());
        }
        // No hold-and-wait: a request holding permits takes more only when they are free at once; else it releases
        // them and waits for the total as one acquisition (FIFO), so it never waits behind a request waiting for it.
        let more = cost(c.len() as u64)?;
        let charged = match permit.take() {
            None => charge(&app.inflight, more, deadline).await,
            Some(mut held) => match app.inflight.clone().try_acquire_many_owned(more) {
                Ok(m) => {
                    held.merge(m);
                    Ok(Some(held))
                }
                Err(_) => {
                    let total = u32::try_from(held.num_permits()).map_err(|_| too_large())?.saturating_add(more);
                    drop(held);
                    charge(&app.inflight, total, deadline).await
                }
            },
        };
        permit = match charged {
            Ok(p) => p,
            Err(e) => {
                let seen = (buf.len() + c.len()) as u64;
                drop(buf);
                discard(&mut s, app.body_idle, seen).await;
                return Err(e);
            }
        };
        buf.extend_from_slice(&c);
    }
    Ok((buf.freeze(), permit))
}

/// Reads and discards the rest of a request body before an early error reply, until `BODY_LIMIT` bytes in all
/// (`seen` already read) or a read error or stall: a client still sending would otherwise get a reset instead of the
/// reply (on Windows the reset also discards the reply the client already received).
async fn discard<S: Stream<Item = Result<Bytes, axum::Error>> + Unpin>(s: &mut S, idle: Option<Duration>, mut seen: u64) {
    while let Some(Ok(c)) = next_within(s, idle).await {
        seen += c.len() as u64;
        if seen > BODY_LIMIT {
            break;
        }
    }
}

/// An upstream response body as read (K§6.4.4: with a provider key, through `KeyScrub`).
type Upstream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

/// A whole upstream body of at most COPY_MAX bytes (§14), else `502 zygo_upstream_too_large`.
async fn read_capped(mut s: Upstream) -> Result<Bytes, ZygoError> {
    let mut buf = BytesMut::new();
    while let Some(c) = s.next().await {
        let c = c.map_err(|_| unavailable())?;
        if buf.len() + c.len() > COPY_MAX {
            return Err(upstream_too_large());
        }
        buf.extend_from_slice(&c);
    }
    Ok(buf.freeze())
}

/// The text that replaces the provider key (K§6.4.4), as `translate::scrub` writes it.
const REDACTED: &[u8] = b"[REDACTED]";

/// The first position of `needle` (not empty) in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let mut at = 0;
    while hay.len() - at >= needle.len() {
        let p = hay[at..=hay.len() - needle.len()].iter().position(|&b| b == needle[0])?;
        if hay[at + p..].starts_with(needle) {
            return Some(at + p);
        }
        at += p + 1;
    }
    None
}

/// Each occurrence of the provider key in a body read in chunks replaced with `[REDACTED]` (K§6.4.4), whatever the
/// framing; every other byte unchanged. Of the last `key.len() - 1` bytes read, the longest tail that is the start
/// of the key is held back, as a key split across chunks starts there; `finish` returns it. Any other byte goes on at
/// once, so an event end is not held.
struct KeyScrub {
    key: Vec<u8>,
    held: Vec<u8>,
}

impl KeyScrub {
    /// `None` for an empty key.
    fn new(key: &str) -> Option<KeyScrub> {
        (!key.is_empty()).then(|| KeyScrub { key: key.as_bytes().to_vec(), held: Vec::new() })
    }

    /// The held bytes and `chunk`, keys replaced, less the new held tail. With nothing held, `chunk` is not copied
    /// first: a whole body pushed at once costs one more body, the output.
    fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let joined;
        let buf: &[u8] = if self.held.is_empty() {
            chunk
        } else {
            joined = [std::mem::take(&mut self.held).as_slice(), chunk].concat();
            &joined
        };
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        while let Some(p) = find(&buf[i..], &self.key) {
            out.extend_from_slice(&buf[i..i + p]);
            out.extend_from_slice(REDACTED);
            i += p + self.key.len();
        }
        let from = i.max(buf.len().saturating_sub(self.key.len() - 1));
        let keep = (from..buf.len()).find(|&j| self.key.starts_with(&buf[j..])).unwrap_or(buf.len());
        out.extend_from_slice(&buf[i..keep]);
        self.held = buf[keep..].to_vec();
        out
    }

    /// The held tail: a proper start of the key, so no key.
    fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

/// `s` through `k`, its held tail at the end; a read error ends it without the tail.
fn key_scrubbed(s: Upstream, k: KeyScrub) -> Upstream {
    futures_util::stream::unfold(Some((s, k)), |st| async move {
        let (mut s, mut k) = st?;
        match s.next().await {
            Some(Ok(c)) => {
                let out = Bytes::from(k.push(&c));
                Some((Ok(out), Some((s, k))))
            }
            Some(Err(e)) => Some((Err(e), None)),
            None => Some((Ok(Bytes::from(k.finish())), None)),
        }
    })
    .boxed()
}

/// An untranslated 2xx body that is not SSE, with a provider key (K§6.4.4 (4)), its key already replaced (`s`): read
/// up to COPY_MAX; a whole body is sent with its new `content-length`; a longer one goes on as a stream, without it.
async fn keyed_body(mut s: Upstream, headers: &mut HeaderMap) -> Result<Upstream, ZygoError> {
    let mut buf = BytesMut::new();
    while let Some(c) = s.next().await {
        buf.extend_from_slice(&c.map_err(|_| unavailable())?);
        if buf.len() > COPY_MAX {
            let head = buf.freeze();
            return Ok(futures_util::stream::once(async move { Ok(head) }).chain(s).boxed());
        }
    }
    headers.insert(CONTENT_LENGTH, HeaderValue::from(buf.len()));
    let out = buf.freeze();
    Ok(futures_util::stream::once(async move { Ok(out) }).boxed())
}

/// Drops each header whose value holds `key`, or whose name holds it in lower case (names arrive in lower case)
/// (K§6.4.4 (1)); true when one was dropped.
fn drop_keyed(h: &mut HeaderMap, key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let mut out = HeaderMap::with_capacity(h.len());
    for (n, v) in h.iter() {
        if find(v.as_bytes(), key.as_bytes()).is_none() && find(n.as_str().as_bytes(), lower.as_bytes()).is_none() {
            out.append(n.clone(), v.clone());
        }
    }
    let dropped = out.len() != h.len();
    *h = out;
    dropped
}

/// An untranslated non-streamed 2xx with injected nulls (§7.6): read up to COPY_MAX; a whole JSON body from which a
/// null was removed is sent serialized again, without `content-length`. Else the read bytes, then the rest unchanged.
async fn restored_body(mut s: Upstream, nulls: &NullMap, headers: &mut HeaderMap) -> Result<Upstream, ZygoError> {
    let mut buf = BytesMut::new();
    let mut whole = true;
    while let Some(c) = s.next().await {
        buf.extend_from_slice(&c.map_err(|_| unavailable())?);
        if buf.len() > COPY_MAX {
            whole = false;
            break;
        }
    }
    if whole
        && let Ok(mut v) = serde_json::from_slice::<Value>(&buf)
        && nulls.restore_message(&mut v)
    {
        headers.remove(CONTENT_LENGTH);
        let out = Bytes::from(serde_json::to_vec(&v).unwrap_or_default());
        return Ok(futures_util::stream::once(async move { Ok(out) }).boxed());
    }
    let read = buf.freeze();
    Ok(futures_util::stream::once(async move { Ok(read) }).chain(s).boxed())
}

/// Mapper events through the restore filter when armed (`end`: its held deltas too), as bytes.
fn event_bytes(evs: Vec<SseEvent>, restore: &mut Option<RestoreFilter>, end: bool) -> Vec<u8> {
    let evs = match restore {
        Some(r) => {
            let mut out: Vec<SseEvent> = evs.into_iter().flat_map(|e| r.push(e)).collect();
            if end {
                out.extend(r.finish());
            }
            out
        }
        None => evs,
    };
    evs.iter().flat_map(SseEvent::to_bytes).collect()
}

/// The in-flight cost of `bytes` body bytes: 4 × `bytes` permits (§7.3); ≤ 4 × 32 MiB fits.
fn cost(bytes: u64) -> Result<u32, ZygoError> {
    u32::try_from(bytes.saturating_mul(INFLIGHT_FACTOR)).map_err(|_| too_large())
}

/// `n` permits (FIFO) before `deadline`, else `503 zygo_busy`. `None` for 0.
async fn charge(sem: &Arc<Semaphore>, n: u32, deadline: tokio::time::Instant) -> Result<Option<OwnedSemaphorePermit>, ZygoError> {
    if n == 0 {
        return Ok(None);
    }
    match tokio::time::timeout_at(deadline, sem.clone().acquire_many_owned(n)).await {
        Ok(Ok(p)) => Ok(Some(p)),
        _ => Err(busy()),
    }
}

/// A translated 2xx to a Responses client, after the tee (K§6.4.2 C3): the stream encoder, or a whole answer's
/// Response object, sent when the upstream body ends.
enum Encode {
    Stream(Box<Streamed>),
    Whole(Bytes),
}

/// The message of a read error on a started Responses stream (K§6.4.3 stream obligation 6).
const READ_FAILED: &str = "upstream response failed";
/// A Messages `message_start` with no fields: it starts the spare encoder.
const EMPTY_START: &[u8] = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n";

/// A Responses stream after the tee (K§6.4.2): the encoder, and what the proxy adds around it.
struct Streamed {
    enc: responses_client::Encoder,
    /// Starts and fails a stream that a read error ends before its first event: nothing reached the client yet.
    spare: responses_client::Encoder,
    /// Removed from an upstream error message, as the chat mapper does (`translate/sse.rs:420-424`).
    key: Option<Secret>,
    /// An upstream over-long prompt's message by the PC14 rule (K§6.4.4); the encoder cuts messages shorter.
    context: Option<String>,
    /// The current event's `event` field is `error`.
    error_event: bool,
    /// The `data` lines of an error event split over lines, until its blank line.
    held: Held,
    /// The held `data` passed `HOLD_MAX`: the stream fails as the encoder fails an over-long event.
    too_long: bool,
}

/// The `data` of a split error event, joined as SSE joins it (`\n` between lines).
#[derive(Default)]
enum Held {
    #[default]
    None,
    Data(Vec<u8>),
    /// Over `HOLD_MAX`: the rest of the event's `data` lines are dropped too.
    Over,
}

/// The encoder's message for held event `data` over `HOLD_MAX` (`responses_client.rs` `EVENT_TOO_LONG`).
const EVENT_TOO_LONG: &str = "upstream event too long";
/// An SSE comment line: it takes the place of a held `data` line, and the encoder ignores it.
const COMMENT: &[u8] = b":";

impl Streamed {
    fn new(model: &str, t: &Target, calls: responses_client::Calls, key: Option<Secret>) -> Streamed {
        let spare = responses_client::Encoder::new(model, t, calls.clone());
        Streamed {
            enc: responses_client::Encoder::new(model, t, calls),
            spare,
            key,
            context: None,
            error_event: false,
            held: Held::None,
            too_long: false,
        }
    }

    /// A Messages `error` event before the encoder: its message without the provider key. An over-long prompt
    /// (`invalid_request_error` with a PC14 form, K§6.4.4) gets the code `context_length_exceeded`. An error event
    /// split over several `data` lines (its `event` is `error`, or its first `data` line names `"error"`) is held to its
    /// blank line, each held line sent on as an SSE comment, then sent as one `data` line read like a whole one. Lines of
    /// other events are not held. A `data` line that is no JSON and not held has each occurrence of the key replaced.
    fn line(&mut self, l: &mut SseLine) {
        if let Some(new) = self.replaced(&l.0) {
            l.0 = new;
        }
    }

    /// The bytes that take the place of `line` (without its `\n`); `None`: no change.
    fn replaced(&mut self, line: &[u8]) -> Option<Vec<u8>> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            self.error_event = false;
            let Held::Data(d) = std::mem::take(&mut self.held) else { return None };
            // The tee adds the blank line's `\n` after this `data` line's.
            return Some(match self.error_data(&d) {
                Some(v) => format!("data: {v}\n").into_bytes(),
                None => {
                    let t = scrub(&String::from_utf8_lossy(&d), self.key.as_ref().map(Secret::expose));
                    format!("data:{}\n", t.replace('\n', "\ndata:")).into_bytes()
                }
            });
        }
        if let Some(name) = line.strip_prefix(b"event:") {
            self.error_event = name.trim_ascii() == b"error";
            return None;
        }
        let d = line.strip_prefix(b"data:")?;
        let d = d.strip_prefix(b" ").unwrap_or(d);
        match &mut self.held {
            Held::None => {}
            Held::Data(h) if h.len() + 1 + d.len() <= HOLD_MAX => {
                h.push(b'\n');
                h.extend_from_slice(d);
                return Some(COMMENT.to_vec());
            }
            Held::Data(_) | Held::Over => {
                self.held = Held::Over;
                self.too_long = true;
                return Some(COMMENT.to_vec());
            }
        }
        let key = self.key.as_ref().map(Secret::expose).filter(|k| !k.is_empty());
        let keyed = key.is_some_and(|k| d.windows(k.len()).any(|w| w == k.as_bytes()));
        let named = d.windows(7).any(|w| w == b"\"error\"");
        if !keyed && !named && !self.error_event {
            return None;
        }
        if serde_json::from_slice::<serde::de::IgnoredAny>(d).is_ok() {
            return self.error_data(d).map(|v| format!("data: {v}").into_bytes());
        }
        if named || self.error_event {
            self.held = Held::Data(d.to_vec());
            return Some(COMMENT.to_vec());
        }
        Some(format!("data:{}", scrub(&String::from_utf8_lossy(d), key)).into_bytes())
    }

    /// One whole event's `data` when it is a Messages `error`: the message without the key, and an over-long prompt's
    /// PC14 code (its PC14 message kept for `push`); `None` for any other `data`.
    fn error_data(&mut self, d: &[u8]) -> Option<Value> {
        let mut v = serde_json::from_slice::<Value>(d).ok()?;
        if v.get("type").and_then(Value::as_str) != Some("error") {
            return None;
        }
        let e = v.get_mut("error").and_then(Value::as_object_mut)?;
        let m = scrub(e.get("message").and_then(Value::as_str).unwrap_or_default(), self.key.as_ref().map(Secret::expose));
        if e.get("type").and_then(Value::as_str) == Some("invalid_request_error")
            && let Some(pc14) = pc14_form(&m)
        {
            e.insert("type".into(), json!(CONTEXT_CODE));
            self.context = Some(pc14);
        }
        e.insert("message".into(), json!(m));
        Some(v)
    }

    /// The encoder's events for the tee's bytes (and its last ones at `end`), an over-long prompt's `response.failed`
    /// with the PC14 message.
    fn push(&mut self, out: &[u8], end: bool) -> Vec<SseEvent> {
        let mut evs = self.enc.push(out);
        if self.too_long {
            evs.extend(self.enc.fail("api_error", EVENT_TOO_LONG));
        }
        if end {
            evs.extend(self.enc.finish());
        }
        if let Some(m) = &self.context {
            for ev in evs.iter_mut().filter(|ev| ev.event == "response.failed") {
                let e = &mut ev.data["response"]["error"];
                if e["code"] == CONTEXT_CODE {
                    e["message"] = json!(m);
                }
            }
        }
        evs
    }

    /// A read error: one `response.failed` with `READ_FAILED`, the stream started first when it was not (stream
    /// obligation 6); nothing after a terminal event.
    fn read_failed(&mut self) -> Vec<SseEvent> {
        if self.enc.ended() {
            return Vec::new();
        }
        let evs = self.enc.fail("api_error", READ_FAILED);
        if !evs.is_empty() {
            return evs;
        }
        let mut evs = self.spare.push(EMPTY_START);
        evs.extend(self.spare.fail("api_error", READ_FAILED));
        evs
    }
}

fn sse_bytes(evs: &[SseEvent]) -> Bytes {
    Bytes::from(evs.iter().flat_map(SseEvent::to_bytes).collect::<Vec<u8>>())
}

/// The tee's Messages bytes as the client's bytes: unchanged with no encoder; the encoder's Responses events (and its
/// last ones at `end`); a whole Response object at `end` only.
fn encoded(encode: &mut Option<Encode>, out: Bytes, end: bool) -> Bytes {
    match encode {
        None => out,
        Some(Encode::Stream(s)) => sse_bytes(&s.push(&out, end)),
        Some(Encode::Whole(b)) if end => std::mem::take(b),
        Some(Encode::Whole(_)) => Bytes::new(),
    }
}

/// A Messages body (an Anthropic upstream's 2xx to a Responses client).
fn is_message(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("message") && v.get("content").is_some_and(Value::is_array)
}

/// The response body: upstream chunks through the mapper (a translated stream, §7.6) and the restore filter (injected
/// nulls, §7.6), then the tee, then the Responses encoder (a Responses client, K§6.4.2); the usage row is written when it is dropped (stream end or client disconnect, §14).
struct Flow {
    upstream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    mapper: Option<StreamMapper>,
    restore: Option<RestoreFilter>,
    tee: Tee,
    /// After the tee: a translated 2xx to a Responses client.
    encode: Option<Encode>,
    /// The window stage armed line mode at request time (§10.2.6): a Responses stream's lines also reach the line stages (K§6.4.5).
    window: bool,
    rctx: ResponseCtx,
    pipeline: Arc<Pipeline>,
    pending: Pending,
    done: bool,
    /// The upstream body ended with a read error (FIX-10 round 2 M3).
    failed: bool,
    /// The upstream `content-length`, when it sent one.
    body_len: Option<u64>,
}

impl Flow {
    /// The upstream body is whole: no read error, and it ended or reached its `content-length` (FIX-10 round 2 M3).
    fn finished(&self) -> bool {
        !self.failed && (self.done || self.body_len.is_some_and(|n| self.tee.body().is_some_and(|b| b.len() as u64 == n)))
    }
}

impl Drop for Flow {
    /// With a `content-length`, the server stops polling once it has all the bytes: the tee is finished here.
    fn drop(&mut self) {
        if !self.done {
            self.tee.finish(&mut |_| {});
        }
        self.pending.usage = self.tee.usage();
        self.pending.split = self.tee.cache_split();
        // Passthrough, not 2xx: raw bytes and the login for the usage writer; no parse and no pass here (§14, FIX-10 B1).
        // A body that did not finish is not trusted: no head, so both fields are empty (round 2 M3).
        if self.pending.row.passthrough && !self.rctx.status.is_success() {
            let head = match self.tee.body() {
                Some(b) if self.finished() => b[..b.len().min(ERROR_BODY_MAX)].to_vec(),
                _ => Vec::new(),
            };
            let credential = self.rctx.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
                .map(|v| v.split_once(' ').map_or(v, |(_, t)| t).trim())
                .filter(|t| !t.is_empty())
                .map(Secret::new);
            self.pending.upstream_error = Some(ErrorCapture { head, credential, session: self.pending.session.clone() });
        }
        // The encoder's codes (K§6.4.3 obligations 2, 3) reach the row.
        if let Some(Encode::Stream(s)) = &self.encode {
            self.rctx.adjustments.extend(s.enc.adjustments());
        }
        // A 2xx Responses stream with no usage read: the zero counts are marked (K§6.4.2 C3).
        if self.rctx.status.is_success() && self.tee.usage_unread() {
            self.rctx.adjustments.push(Adjustment("usage_unread".into()));
        }
        // Codes a line stage added (§10.2.3) reach the row, not the headers.
        self.pending.row.adjustments = names(&self.rctx.adjustments);
    }
}

async fn next_chunk(mut f: Flow) -> Option<(Result<Bytes, std::io::Error>, Flow)> {
    while !f.done {
        // After the encoder's terminal event the client body ends; the rest of the upstream is read in a detached
        // task, through the tee, for the usage row (written when the flow drops): to its end, a read error or the
        // upstream idle read timeout.
        if let Some(Encode::Stream(s)) = &f.encode
            && s.enc.ended()
        {
            tokio::spawn(drain(f));
            return None;
        }
        match f.step().await {
            Ok(b) if b.is_empty() => {}
            r => return Some((r, f)),
        }
    }
    None
}

/// The rest of the upstream body after the client body ended; the client's bytes are dropped.
async fn drain(mut f: Flow) {
    while !f.done {
        let _ = f.step().await;
    }
}

impl Flow {
    /// One upstream read through the stages: the client's bytes (empty when there are none yet), or the read error.
    async fn step(&mut self) -> Result<Bytes, std::io::Error> {
        let f = self;
        let next = f.upstream.next().await;
        let Flow { tee, rctx, pipeline, mapper, restore, encode, window, done, failed, .. } = f;
        // The stages see the upstream numbers up to and including this line.
        let mut on_line = |l: &mut SseLine| {
            rctx.usage.observe(rctx.lines, &l.0);
            match encode {
                // A Responses stream: its error lines (K§6.4.4), then the line stages when the window stage armed them
                // (K§6.4.5).
                Some(Encode::Stream(s)) => {
                    s.line(l);
                    if *window {
                        pipeline.on_response_line(rctx, l)
                    }
                }
                _ => pipeline.on_response_line(rctx, l),
            }
        };
        let out = match next {
            Some(Ok(chunk)) => match mapper {
                None => match restore {
                    Some(r) => tee.push(Bytes::from(r.push_raw(&chunk)), &mut on_line),
                    None => tee.push(chunk, &mut on_line),
                },
                Some(m) => {
                    let evs = m.push(&chunk);
                    let mapped = event_bytes(evs, restore, m.ended());
                    let mut out = BytesMut::from(&tee.push(Bytes::from(mapped), &mut on_line)[..]);
                    // A terminal or error event: the upstream is no longer read.
                    if m.ended() {
                        out.extend_from_slice(&tee.finish(&mut on_line));
                        *done = true;
                    }
                    out.freeze()
                }
            },
            Some(Err(_)) => {
                *done = true;
                *failed = true;
                // A Responses stream ends with one terminal event (K§6.4.3 stream obligation 6).
                if let Some(Encode::Stream(s)) = encode {
                    return Ok(sse_bytes(&s.read_failed()));
                }
                return Err(std::io::Error::other(READ_FAILED));
            }
            None => {
                let mut out = BytesMut::new();
                if let Some(m) = mapper {
                    let evs = m.finish_events();
                    out.extend_from_slice(&tee.push(Bytes::from(event_bytes(evs, restore, true)), &mut on_line));
                } else if let Some(r) = restore {
                    out.extend_from_slice(&tee.push(Bytes::from(r.finish_raw()), &mut on_line));
                }
                out.extend_from_slice(&tee.finish(&mut on_line));
                *done = true;
                out.freeze()
            }
        };
        let out = encoded(encode, out, *done);
        rctx.usage = tee.usage();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // round 2 N3, C4: a body that does not serialize again is refused 503, never sent as the received bytes
    fn serialized_fails_closed() {
        assert_eq!(serialized(Ok(b"{}".to_vec())).unwrap(), Bytes::from_static(b"{}"));
        let e = serialized(Err(<serde_json::Error as serde::ser::Error>::custom("x"))).unwrap_err();
        assert_eq!((e.status, e.code), (StatusCode::SERVICE_UNAVAILABLE, "zygo_redaction_unavailable"));
    }

    /// `input` through a `KeyScrub` of `key`, cut into chunks at `cuts`.
    fn scrubbed(key: &str, input: &[u8], cuts: &[usize]) -> Vec<u8> {
        let mut k = KeyScrub::new(key).unwrap();
        let mut out = Vec::new();
        let mut at = 0;
        for &c in cuts.iter().chain([input.len()].iter()) {
            out.extend_from_slice(&k.push(&input[at..c]));
            at = c;
        }
        out.extend_from_slice(&k.finish());
        out
    }

    /// K§6.4.4: a key split across two chunks at every byte offset is replaced; the other bytes are unchanged, also
    /// bytes that are no UTF-8, bare CRs and a key prefix at the end.
    #[test]
    fn key_scrub_finds_a_key_split_at_every_offset() {
        let key = "sk-ant-api03-Ab_1-xyz";
        let k = key.as_bytes();
        let (head, mid, end): (&[u8], &[u8], &[u8]) = (b"\xffdata: \r", b"\n\xc3\xa9", b" sk-ant-api03-Ab_");
        let input = [head, k, mid, k, k, end].concat();
        let want = [head, REDACTED, mid, REDACTED, REDACTED, end].concat();
        for cut in 0..=input.len() {
            assert_eq!(scrubbed(key, &input, &[cut]), want, "cut {cut}");
        }
        let every: Vec<usize> = (1..input.len()).collect();
        assert_eq!(scrubbed(key, &input, &every), want);
        assert_eq!(scrubbed(key, b"no key here", &[3]), b"no key here");
        assert!(KeyScrub::new("").is_none());
    }

    /// K§6.4.4: only a tail that starts the key is held, never longer than the key less one byte; an event end is not
    /// held.
    #[test]
    fn key_scrub_holds_only_a_start_of_the_key() {
        let mut k = KeyScrub::new("K3Y9").unwrap();
        assert_eq!(k.push(&[b'x'; 100]).len(), 100);
        assert_eq!(k.push(b"data: {}\n\n"), b"data: {}\n\n");
        assert_eq!(k.push(b"abK3"), b"ab");
        assert_eq!(k.push(b"Y9 tail"), b"[REDACTED] tail");
        assert_eq!(k.push(b"K3YK3"), b"K3Y");
        assert_eq!(k.held, b"K3");
        assert_eq!(k.push(b"Y"), b"");
        assert_eq!(k.push(b"x"), b"K3Yx");
        assert_eq!(k.push(b"xK"), b"x");
        assert_eq!(k.finish(), b"K");
    }

    /// K§6.4.4: a header value that holds the key is dropped, and a header whose name holds it in lower case (round 2
    /// G6); the others are kept.
    #[test]
    fn drop_keyed_drops_only_values_with_the_key() {
        let mut h = HeaderMap::new();
        h.append("x-a", HeaderValue::from_static("Bearer K3Y"));
        h.append("x-a", HeaderValue::from_static("other"));
        h.append("x-b", HeaderValue::from_static("kept"));
        h.append("x-k3y-name", HeaderValue::from_static("v"));
        assert!(drop_keyed(&mut h, "K3Y"));
        assert_eq!(h.get_all("x-a").iter().collect::<Vec<_>>(), [&HeaderValue::from_static("other")]);
        assert_eq!(h["x-b"], "kept");
        assert!(h.get("x-k3y-name").is_none());
        assert!(!drop_keyed(&mut h, "K3Y"));
    }
}
