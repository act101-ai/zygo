//! The request stages (§7.3): context types, the `Stage` trait and the stage list.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde_json::Value;
use zygo_config::{Dialect, Merged};

use crate::cache::CacheStage;
use crate::passthrough::PassthroughStage;
use crate::errors::ZygoError;
use crate::provider::{Candidate, KeyResolver, ProviderStage};
use crate::redact_stage::RedactStage;
#[cfg(feature = "test-hooks")]
use crate::redact_stage::RedactLimits;
use crate::resolve::Resolved;
use crate::session::SessionState;
use crate::tee::{SseLine, Usage};
use crate::window::WindowStage;

/// An adjustment code (§7.7): names and counts only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjustment(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRoute {
    Messages,
    CountTokens,
    /// `POST /openai/v1/responses` (K§6.4).
    Responses,
}

impl ModelRoute {
    /// The client's dialect on this route.
    pub fn dialect(self) -> Dialect {
        match self {
            ModelRoute::Responses => Dialect::Openai,
            ModelRoute::Messages | ModelRoute::CountTokens => Dialect::Anthropic,
        }
    }
}

pub struct RequestCtx {
    pub session: Arc<SessionState>,
    /// The serving config now.
    pub cfg: Arc<Merged>,
    pub route: ModelRoute,
    /// The client's dialect.
    pub dialect: Dialect,
    /// The client's headers.
    pub headers: HeaderMap,
    /// The body as received.
    pub raw: Bytes,
    /// The body, parsed once.
    pub body: Value,
    /// A stage changed `body`, or `raw` repeats an object key (§7.3 note (a)): it is serialized again, else `raw` is sent.
    pub changed: bool,
    pub resolved: Option<Resolved>,
    pub candidates: Vec<Candidate>,
    pub adjustments: Vec<Adjustment>,
    /// Pattern id → count (N12); also set on a refusal, for the usage row.
    pub redactions: BTreeMap<String, u32>,
    pub redacted: bool,
    /// Arms the tee's line mode on a 2xx SSE response (N15).
    pub line_mode: bool,
}

/// C3: no header, body or key.
impl fmt::Debug for RequestCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestCtx")
            .field("route", &self.route)
            .field("dialect", &self.dialect)
            .field("changed", &self.changed)
            .field("resolved", &self.resolved)
            .field("adjustments", &self.adjustments)
            .finish()
    }
}

pub struct ResponseCtx {
    pub session: Arc<SessionState>,
    pub cfg: Arc<Merged>,
    /// The client's request headers.
    pub headers: HeaderMap,
    pub status: StatusCode,
    pub resolved: Resolved,
    pub provider: String,
    /// The upstream dialect.
    pub dialect: Dialect,
    /// The form of the body the tee and the response stages read: Openai only for a Responses upstream answering a
    /// Responses client (K§6.4.2 C3), else Anthropic.
    pub lines: Dialect,
    pub adjustments: Vec<Adjustment>,
    /// Upstream numbers, before any rewrite.
    pub usage: Usage,
    /// A zygo compact tool_use was seen (§10.2.3 trigger 2).
    pub compact_call: bool,
    /// A Responses call item was seen in a streamed `response.output_item.added` or `.done` (K§6.4.5 end of turn).
    pub call_item: bool,
}

/// C3: no header.
impl fmt::Debug for ResponseCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseCtx")
            .field("status", &self.status)
            .field("resolved", &self.resolved)
            .field("provider", &self.provider)
            .field("dialect", &self.dialect)
            .field("lines", &self.lines)
            .field("adjustments", &self.adjustments)
            .field("usage", &self.usage)
            .field("compact_call", &self.compact_call)
            .field("call_item", &self.call_item)
            .finish()
    }
}

pub trait Stage: Send + Sync {
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError>;
    fn on_response_line(&self, _ctx: &mut ResponseCtx, _line: &mut SseLine) {}
}

pub struct Pipeline {
    pub stages: Vec<Arc<dyn Stage>>,
}

impl Pipeline {
    /// Stages 2 to 6 (§7.3): passthrough, window, cache, provider, redact. Stage 2 is `PassthroughStage` (§12.4), which
    /// runs `ResolveStage` outside passthrough.
    pub fn new(keys: Arc<dyn KeyResolver>) -> Pipeline {
        Pipeline::with(PassthroughStage::new(), ProviderStage::new(keys), RedactStage::new())
    }

    /// `new`, with `test_hosts` counted as loopback by stage 5.
    #[cfg(feature = "test-hooks")]
    pub fn with_test_hosts(keys: Arc<dyn KeyResolver>, test_hosts: Vec<String>) -> Pipeline {
        Pipeline::with(PassthroughStage::with_test_hosts(test_hosts.clone()), ProviderStage::with_test_hosts(keys, test_hosts), RedactStage::new())
    }

    /// `with_test_hosts`, with the redaction deadlines and panic hook of `l`.
    #[cfg(feature = "test-hooks")]
    pub fn with_redact_limits(keys: Arc<dyn KeyResolver>, test_hosts: Vec<String>, l: RedactLimits) -> Pipeline {
        Pipeline::with(
            PassthroughStage::with_test_hosts(test_hosts.clone()),
            ProviderStage::with_test_hosts(keys, test_hosts),
            RedactStage::with_limits(l),
        )
    }

    fn with(passthrough: PassthroughStage, provider: ProviderStage, redact: RedactStage) -> Pipeline {
        Pipeline {
            stages: vec![Arc::new(passthrough), Arc::new(WindowStage), Arc::new(CacheStage), Arc::new(provider), Arc::new(redact)],
        }
    }

    /// In order; the first error stops.
    pub fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        self.stages.iter().try_for_each(|s| s.on_request(ctx))
    }

    pub fn on_response_line(&self, ctx: &mut ResponseCtx, line: &mut SseLine) {
        for s in &self.stages {
            s.on_response_line(ctx, line);
        }
    }
}
