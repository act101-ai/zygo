//! The proxy step (§7.3): body limits and in-flight memory, the stages, model rewrite, forward, fallback, pinning,
//! response headers and the usage row (§7.7, §10.4, §14).

use std::collections::BTreeMap;
use std::pin::Pin;
use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use http::header::{HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderMap, StatusCode};
use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zygo_config::keys::Secret;
use zygo_config::{Dialect, Facts, Family, Prices, Provider, SchemaRule};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RedactionCount, RequestRow};

use crate::errors::ZygoError;
use crate::passthrough::CredentialType;
use crate::pipeline::{Adjustment, ModelRoute, Pipeline, RequestCtx, ResponseCtx};
use crate::provider::{loopback, unavailable, Candidate};
use crate::resolve::Resolved;
use crate::server::App;
use crate::session::{PromptUsage, SessionState};
use crate::tee::{SseLine, Tee, Ticket, Usage, UsageSink, COPY_MAX};
use crate::translate::schema::{NullMap, RestoreFilter};
use crate::translate::{
    facts, is_response, message_events, reasoning, schema, translate_error, translate_request, translate_response, SseEvent, StreamMapper,
    Target,
};
use crate::window;
use crate::Url;

pub const BUSY_WAIT: Duration = Duration::from_secs(5); // §7.3
pub const INFLIGHT_FACTOR: u64 = 4; // §7.3

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
        }
    }
    fn upstream_path(self) -> &'static str {
        match self {
            ModelRoute::Messages => "/v1/messages",
            ModelRoute::CountTokens => "/v1/messages/count_tokens",
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

fn upstream_bad_response() -> ZygoError {
    ZygoError::new(StatusCode::BAD_GATEWAY, "zygo_upstream_bad_response", "upstream response could not be translated")
}

/// The usage row of one request (§14); written when dropped: the tokens and cost, `add_usage`, then the row.
struct Pending {
    row: RequestRow,
    session: Arc<SessionState>,
    sink: Arc<UsageSink>,
    prices: Option<Prices>,
    usage: Usage,
    /// The rows reply waits for it (§14, Q2).
    ticket: Option<Ticket>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        let u = self.usage;
        let r = &mut self.row;
        (r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens) = (u.input, u.output, u.cache_read, u.cache_write);
        r.estimated_usd = self.prices.as_ref().map_or(0.0, |p| {
            (u.input as f64 * p.input + u.output as f64 * p.output + u.cache_read as f64 * p.cache_read
                + u.cache_write as f64 * p.cache_write)
                / 1e6
        });
        self.session.add_usage(PromptUsage { input: u.input, cache_read: u.cache_read, cache_write: u.cache_write });
        // Used context (§10.2, A-9): the upstream prompt of a main-loop (class `main` or absent) Messages 2xx.
        let prompt = window::prompt(u);
        if r.route == "/anthropic/v1/messages"
            && (200..300).contains(&r.status)
            && r.request_class.as_deref().is_none_or(|c| c == "main")
            && prompt > 0
        {
            self.session.set_used_context(prompt);
        }
        self.sink.send(std::mem::take(&mut self.row), self.ticket.take());
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
    let mut slot = Some(Pending { row, session: s.clone(), sink: app.usage.clone(), prices: None, usage: Usage::default(), ticket });
    match run(&app, &s, route, req, &mut slot, start).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(p) = slot.as_mut() {
                p.row.status = e.status.as_u16();
                p.row.latency_ms = ms(&app, start);
            }
            e.into_response()
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
    let name = body.get("model").and_then(Value::as_str).ok_or_else(|| ZygoError::bad_request("model must be a string"))?.to_string();
    let mut ctx = RequestCtx {
        session: s.clone(),
        cfg: app.served().cfg.clone(),
        route,
        dialect: Dialect::Anthropic,
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
    app.sessions.accept(s)?;
    let resolved = ctx.resolved.clone().expect("stage 2 resolved the request");

    // count_tokens on a provider that does not speak `anthropic`: a local estimate (§7.3).
    if route == ModelRoute::CountTokens && ctx.candidates.first().is_some_and(|c| c.dialect != Dialect::Anthropic) {
        drop(permit);
        ctx.adjustments.push(Adjustment("count_estimated".into()));
        let p = pending(slot);
        p.row.status = 200;
        p.row.latency_ms = ms(app, start);
        p.row.adjustments = names(&ctx.adjustments);
        let mut resp = Json(json!({ "input_tokens": string_bytes(&ctx.body).div_ceil(4) })).into_response();
        zygo_headers(resp.headers_mut(), &resolved, &ctx.adjustments, ctx.redacted);
        return Ok(resp);
    }

    // Model facts and strict schemas (§7.6).
    let cfg = ctx.cfg.clone();
    let rw = Rewrite {
        facts: cfg.config.catalog.get(&resolved.model).and_then(|m| m.facts.as_ref()),
        rules: &cfg.config.schema_rule,
        strict: cfg.config.proxy.strict_schemas,
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
        let sent = http
            .post(url)
            .headers(match pt {
                Some(t) => t.outbound(&ctx.headers, &c.provider),
                None => outbound_headers(&ctx.headers, &c.provider, c.dialect, c.key.as_ref()),
            })
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
    let mut streamed = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim_start().to_ascii_lowercase().starts_with("text/event-stream"));
    // A translated response (§7.6): a 2xx SSE stream goes through the mapper; any other body is read whole (at most
    // COPY_MAX, with the in-flight charge still held) and mapped.
    let sse_upstream = streamed;
    let translated = c.dialect != ctx.dialect;
    let key = c.key.as_ref().map(|k| k.expose());
    let estimate = string_bytes(&ctx.body).div_ceil(4);
    let mut headers = response_headers(resp.headers(), translated, s.team_label.is_some());
    let mut adjustments = ctx.adjustments;
    adjustments.extend(extra);
    let mut mapper = None;
    // Injected nulls are removed on the way back (§7.6): a Messages 2xx with a non-empty map.
    let armed = route == ModelRoute::Messages && status.is_success() && !nulls.is_empty();
    let mut restore = None;
    let upstream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>> = if !translated && armed && streamed {
        restore = Some(RestoreFilter::new(nulls));
        headers.remove(CONTENT_LENGTH);
        Box::pin(resp.bytes_stream())
    } else if !translated && armed {
        restored_body(resp, &nulls, &mut headers).await.map_err(|e| read_failed(slot, c, &adjustments, e))?
    } else if !translated {
        Box::pin(resp.bytes_stream())
    } else if status.is_success() && streamed {
        mapper = Some(StreamMapper::new(c.dialect, &Target::of(c)).errors(estimate, key));
        restore = armed.then(|| RestoreFilter::new(nulls));
        Box::pin(resp.bytes_stream())
    } else {
        let bytes = read_capped(resp).await.map_err(|e| read_failed(slot, c, &adjustments, e))?;
        // A 2xx that is no response body of the upstream dialect never reaches the client (§7.3, §7.6).
        let v = if status.is_success() {
            let v = serde_json::from_slice::<Value>(&bytes).ok().filter(|v| is_response(v, c.dialect)).ok_or_else(|| read_failed(slot, c, &adjustments, upstream_bad_response()))?;
            let mut m = translate_response(&v, c.dialect, &Target::of(c));
            if armed {
                nulls.restore_message(&mut m);
            }
            m
        } else {
            let (s, v) = translate_error(status, &bytes, estimate, key);
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
    if pin {
        pins().insert(model, c.provider.name.clone());
    }
    drop(permit);
    let mut tee = Tee::new(streamed);
    // Only an SSE upstream body is rewritten (§10.2.6): not a whole body sent to the client as SSE.
    if ctx.line_mode && status.is_success() && sse_upstream {
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
    let rctx = ResponseCtx {
        session: s.clone(),
        cfg: ctx.cfg.clone(),
        headers: ctx.headers,
        status,
        resolved,
        provider: c.provider.name.clone(),
        dialect: c.dialect,
        adjustments,
        usage: Usage::default(),
        compact_call: false,
    };
    let flow = Flow { upstream, mapper, restore, tee, rctx, pipeline: app.pipeline.clone(), pending: p, done: false };
    let mut out = Response::new(Body::from_stream(futures_util::stream::unfold(flow, next_chunk)));
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    Ok(out)
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
    let mut v = Value::String(name.to_string());
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut cache = s.redaction.secret_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        zygo_redact::secret::redact(&mut v, &s.redaction.compiled, &mut cache, &DeadlinePolicy::default())
    }));
    match (r, v) {
        (Ok(Ok(_)), Value::String(n)) => Some(strip_1m(&n)),
        _ => None,
    }
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

/// A whole upstream body of at most COPY_MAX bytes (§14), else `502 zygo_upstream_too_large`.
async fn read_capped(resp: reqwest::Response) -> Result<Bytes, ZygoError> {
    let mut buf = BytesMut::new();
    let mut s = resp.bytes_stream();
    while let Some(c) = s.next().await {
        let c = c.map_err(|_| unavailable())?;
        if buf.len() + c.len() > COPY_MAX {
            return Err(upstream_too_large());
        }
        buf.extend_from_slice(&c);
    }
    Ok(buf.freeze())
}

/// An untranslated non-streamed 2xx with injected nulls (§7.6): read up to COPY_MAX; a whole JSON body from which a
/// null was removed is sent serialized again, without `content-length`. Else the read bytes, then the rest unchanged.
async fn restored_body(
    resp: reqwest::Response,
    nulls: &NullMap,
    headers: &mut HeaderMap,
) -> Result<Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>, ZygoError> {
    let mut s = resp.bytes_stream();
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

/// The response body: upstream chunks through the mapper (a translated stream, §7.6) and the restore filter (injected
/// nulls, §7.6), then the tee; the usage row is written when it is dropped (stream end or client disconnect, §14).
struct Flow {
    upstream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    mapper: Option<StreamMapper>,
    restore: Option<RestoreFilter>,
    tee: Tee,
    rctx: ResponseCtx,
    pipeline: Arc<Pipeline>,
    pending: Pending,
    done: bool,
}

impl Drop for Flow {
    /// With a `content-length`, the server stops polling once it has all the bytes: the tee is finished here.
    fn drop(&mut self) {
        if !self.done {
            self.tee.finish(&mut |_| {});
        }
        self.pending.usage = self.tee.usage();
        // Codes a line stage added (§10.2.3) reach the row, not the headers.
        self.pending.row.adjustments = names(&self.rctx.adjustments);
    }
}

async fn next_chunk(mut f: Flow) -> Option<(Result<Bytes, std::io::Error>, Flow)> {
    while !f.done {
        let next = f.upstream.next().await;
        let Flow { tee, rctx, pipeline, mapper, restore, .. } = &mut f;
        // The stages see the upstream numbers up to and including this line.
        let mut on_line = |l: &mut SseLine| {
            rctx.usage.observe(&l.0);
            pipeline.on_response_line(rctx, l)
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
                        f.done = true;
                    }
                    out.freeze()
                }
            },
            Some(Err(_)) => {
                f.done = true;
                return Some((Err(std::io::Error::other("upstream response failed")), f));
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
                f.done = true;
                out.freeze()
            }
        };
        f.rctx.usage = f.tee.usage();
        if !out.is_empty() {
            return Some((Ok(out), f));
        }
    }
    None
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
}
