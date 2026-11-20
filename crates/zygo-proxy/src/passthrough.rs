//! Passthrough (§12): the credential types (§12.2) and stage 2 for a passthrough session (§12.4).

use http::{HeaderMap, StatusCode};
use serde_json::Value;
use zygo_config::{Dialect, Provider};
use zygo_core::Agent;

use crate::errors::ZygoError;
use crate::forward::outbound_headers;
use crate::pipeline::{RequestCtx, Stage};
use crate::provider::Candidate;
use crate::resolve::{ResolveStage, Resolved};
use crate::Url;

/// One §12.2 row, held as data: the stages and the proxy read the row, not literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialType {
    pub name: &'static str,
    pub agent: Agent,
    /// The only host the client's credential is sent to.
    pub upstream_host: &'static str,
    pub dialect: Dialect,
    /// The credential header, its scheme and the token prefix (the allowlist).
    pub header: &'static str,
    pub scheme: &'static str,
    pub prefix: &'static str,
    /// Headers whose presence refuses the request.
    pub refused: &'static [&'static str],
    /// Client headers sent upstream as received.
    pub forwarded: &'static [&'static str],
}

pub const CLAUDE_AI: CredentialType = CredentialType {
    name: "claude-ai",
    agent: Agent::ClaudeCode,
    upstream_host: "api.anthropic.com",
    dialect: Dialect::Anthropic,
    header: "authorization",
    scheme: "Bearer",
    prefix: "sk-ant-oat",
    refused: &["x-api-key"],
    forwarded: &["authorization", "anthropic-beta"],
};

/// The v1 rows (§12.2).
pub const TYPES: &[CredentialType] = &[CLAUDE_AI];

impl CredentialType {
    pub fn for_agent(a: Agent) -> Option<&'static CredentialType> {
        TYPES.iter().find(|t| t.agent == a)
    }

    /// No refused header; exactly one credential header; its value is the scheme (case-insensitive, RFC 9110 §11.1),
    /// one space and one token that starts with the prefix, is longer than it and holds no whitespace and no comma.
    pub fn accepts(&self, h: &HeaderMap) -> bool {
        if self.refused.iter().any(|n| h.contains_key(*n)) {
            return false;
        }
        let mut values = h.get_all(self.header).iter();
        let (Some(v), None) = (values.next(), values.next()) else { return false };
        let Ok(v) = v.to_str() else { return false };
        let Some(token) = v.get(..self.scheme.len()).filter(|s| s.eq_ignore_ascii_case(self.scheme)).and_then(|_| v[self.scheme.len()..].strip_prefix(' '))
        else {
            return false;
        };
        token.len() > self.prefix.len() && token.starts_with(self.prefix) && !token.contains(|c: char| c.is_whitespace() || c == ',')
    }

    /// The outbound headers (§12.4): `outbound_headers` with no key, then the client's values of each forwarded header
    /// the result does not hold yet; the credential header is marked sensitive.
    pub fn outbound(&self, client: &HeaderMap, p: &Provider) -> HeaderMap {
        let mut out = outbound_headers(client, p, self.dialect, None);
        for name in self.forwarded {
            if out.contains_key(*name) {
                continue;
            }
            for v in client.get_all(*name) {
                let mut v = v.clone();
                if *name == self.header {
                    v.set_sensitive(true);
                }
                out.append(*name, v);
            }
        }
        out
    }

    /// `https://<upstream_host>`.
    fn base_url(&self) -> String {
        format!("https://{}", self.upstream_host)
    }

    /// `base_url`, the row dialect, no key.
    pub fn builtin_provider(&self, upstream_model: &str) -> Provider {
        Provider {
            name: self.name.into(),
            base_url: self.base_url(),
            dialects: vec![self.dialect],
            upstream_model: upstream_model.into(),
            ..Default::default()
        }
    }

    /// The URL host is the upstream host (ASCII case-insensitive) and its effective port (explicit, else the scheme
    /// default) is the port of `base_url` (443).
    pub fn host_matches(&self, url: &Url) -> bool {
        same_host_port(url, &self.base_url())
    }
}

/// `url` has the host (ASCII case-insensitive) and the effective port of `base`.
fn same_host_port(url: &Url, base: &str) -> bool {
    Url::parse(base).is_ok_and(|b| {
        url.host_str().zip(b.host_str()).is_some_and(|(h, bh)| h.eq_ignore_ascii_case(bh)) && url.port_or_known_default() == b.port_or_known_default()
    })
}

/// The test seam: `host_matches`, or, when `test_hosts` names the upstream host, `http://<upstream_host>` (port 80,
/// which `upstream_test` resolves to the fake upstream's address).
#[cfg(feature = "test-hooks")]
pub(crate) fn on_host(t: &CredentialType, url: &Url, test_hosts: &[String]) -> bool {
    t.host_matches(url) || (is_test_host(t, test_hosts) && same_host_port(url, &test_base_url(t)))
}

#[cfg(feature = "test-hooks")]
fn is_test_host(t: &CredentialType, test_hosts: &[String]) -> bool {
    test_hosts.iter().any(|h| h.eq_ignore_ascii_case(t.upstream_host))
}

#[cfg(feature = "test-hooks")]
fn test_base_url(t: &CredentialType) -> String {
    format!("http://{}", t.upstream_host)
}

pub fn token_invalid() -> ZygoError {
    ZygoError::new(StatusCode::UNAUTHORIZED, "zygo_passthrough_token_invalid", "relaunch zygo claude --passthrough")
}

pub fn no_login() -> ZygoError {
    ZygoError::new(StatusCode::UNAUTHORIZED, "zygo_passthrough_no_login", "passthrough needs a claude.ai login (/login)")
}

/// Stage 2 (§12.4): for a passthrough session, no role resolution and one candidate on the type's host; else
/// `ResolveStage`.
pub struct PassthroughStage {
    /// Hosts whose built-in provider uses `http://`; set only through `with_test_hosts`.
    #[cfg(feature = "test-hooks")]
    test_hosts: Vec<String>,
}

impl Default for PassthroughStage {
    fn default() -> Self {
        PassthroughStage::new()
    }
}

impl PassthroughStage {
    pub fn new() -> PassthroughStage {
        PassthroughStage {
            #[cfg(feature = "test-hooks")]
            test_hosts: Vec::new(),
        }
    }

    /// `t.host_matches`; with the test seam, `on_host`.
    fn on_host(&self, t: &CredentialType, url: &Url) -> bool {
        #[cfg(feature = "test-hooks")]
        return on_host(t, url, &self.test_hosts);
        #[cfg(not(feature = "test-hooks"))]
        t.host_matches(url)
    }

    /// `new`, with a built-in provider on one of `test_hosts` reached over `http://`.
    #[cfg(feature = "test-hooks")]
    pub fn with_test_hosts(test_hosts: Vec<String>) -> PassthroughStage {
        PassthroughStage { test_hosts }
    }
}

impl Stage for PassthroughStage {
    /// The catalog match is the first model, in config order, with a provider on the row dialect whose
    /// `upstream_model` is the name without `[1m]` (a disabled provider counts). The candidate is the match's
    /// provider on the type's host, else the built-in provider; its clone sends `model` as received, with no key and
    /// no price (cost 0, §14).
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        if !ctx.session.passthrough {
            return ResolveStage.on_request(ctx);
        }
        let name = ctx
            .body
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| ZygoError::bad_request("model must be a string"))?
            .to_string();
        let t = CredentialType::for_agent(ctx.session.agent).ok_or_else(no_login)?;
        let (base, one_m) = match name.strip_suffix("[1m]") {
            Some(b) => (b, true),
            None => (name.as_str(), false),
        };
        let matched = ctx
            .cfg
            .config
            .catalog
            .0
            .iter()
            .find(|row| row.provider.iter().any(|p| p.dialects.contains(&t.dialect) && p.upstream_model == base));
        let on_host = matched.and_then(|row| {
            row.provider.iter().find(|p| {
                p.dialects.contains(&t.dialect) && Url::parse(&p.base_url).is_ok_and(|u| self.on_host(t, &u))
            })
        });
        let mut provider = match on_host {
            Some(p) => p.clone(),
            None => {
                #[allow(unused_mut)]
                let mut p = t.builtin_provider(&name);
                #[cfg(feature = "test-hooks")]
                if is_test_host(t, &self.test_hosts) {
                    p.base_url = test_base_url(t);
                }
                p
            }
        };
        provider.upstream_model = name.clone();
        provider.key = None;
        provider.price_usd_per_mtok = None;
        let model = matched.map_or_else(|| base.to_string(), |row| row.id.clone());
        ctx.candidates = vec![Candidate { provider: provider.clone(), dialect: t.dialect, key: None }];
        ctx.resolved = Some(Resolved {
            requested: base.to_string(),
            one_m,
            role: "subscription".into(),
            model,
            candidates: vec![provider],
            adjustments: Vec::new(),
        });
        Ok(())
    }
}
