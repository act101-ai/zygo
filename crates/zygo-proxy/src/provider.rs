//! Stage 5, provider (§6.2, §7.3, §7.5): drops disabled, non-ZDR, clear-text remote and keyless providers.

use std::net::IpAddr;
use std::sync::Arc;

use http::StatusCode;
use zygo_config::keys::{self, Secret};
use zygo_config::{Dialect, KeyRef, Provider};
use zygo_core::paths::Paths;

use crate::errors::ZygoError;
use crate::pipeline::{RequestCtx, Stage};
use crate::Url;

pub trait KeyResolver: Send + Sync {
    fn resolve(&self, r: &KeyRef) -> Option<Secret>;
}

/// The OS secret store and the environment, read per call; a store error counts as absent.
pub struct SystemKeys;

impl KeyResolver for SystemKeys {
    fn resolve(&self, r: &KeyRef) -> Option<Secret> {
        keys::resolve_with(r, &keys::default_store(&Paths::from_env()), &|n| std::env::var_os(n)).ok().flatten()
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub provider: Provider,
    /// The upstream dialect.
    pub dialect: Dialect,
    pub key: Option<Secret>,
}

pub struct ProviderStage {
    pub keys: Arc<dyn KeyResolver>,
    /// Host names that count as loopback; set only through `with_test_hosts` (feature `test-hooks`).
    test_hosts: Vec<String>,
}

impl ProviderStage {
    pub fn new(keys: Arc<dyn KeyResolver>) -> ProviderStage {
        ProviderStage { keys, test_hosts: Vec::new() }
    }

    /// `new`, with `test_hosts` counted as loopback.
    #[cfg(feature = "test-hooks")]
    pub fn with_test_hosts(keys: Arc<dyn KeyResolver>, test_hosts: Vec<String>) -> ProviderStage {
        ProviderStage { keys, test_hosts }
    }
}

/// https, or http to a loopback literal host or "localhost" (C3: a key never travels in clear text).
pub fn upstream_allowed(base_url: &str) -> bool {
    allowed(base_url, &[])
}

/// `upstream_allowed`, with `test_hosts` counted as loopback.
pub(crate) fn allowed(base_url: &str, test_hosts: &[String]) -> bool {
    let Ok(u) = Url::parse(base_url) else { return false };
    match u.scheme() {
        "https" => true,
        "http" => loopback(&u, test_hosts),
        _ => false,
    }
}

/// The host is a loopback literal (`127.0.0.0/8`, `::1`), "localhost" or one of `test_hosts`.
pub(crate) fn loopback(u: &Url, test_hosts: &[String]) -> bool {
    u.host_str().is_some_and(|h| match h.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => h.eq_ignore_ascii_case("localhost") || test_hosts.iter().any(|t| t.eq_ignore_ascii_case(h)),
    })
}

impl Stage for ProviderStage {
    /// Drops, in order: disabled providers, providers without `zdr` under `require_zdr`, a clear-text remote base URL,
    /// a key that does not resolve (§6.2, §7.5). A provider without `key` sends no credential.
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        let require_zdr = ctx.cfg.config.proxy.require_zdr;
        // §7.4: the candidates are the enabled providers; §6.2: key_missing when each one lacked its key.
        let enabled = ctx.candidates.iter().filter(|c| c.provider.enabled).count();
        let mut missing = 0;
        let mut kept = Vec::with_capacity(enabled);
        for mut c in std::mem::take(&mut ctx.candidates) {
            if !c.provider.enabled || (require_zdr && !c.provider.flags.zdr) || !allowed(&c.provider.base_url, &self.test_hosts)
            {
                continue;
            }
            if let Some(r) = &c.provider.key {
                match self.keys.resolve(r) {
                    Some(k) => c.key = Some(k),
                    None => {
                        missing += 1;
                        continue;
                    }
                }
            }
            kept.push(c);
        }
        ctx.candidates = kept;
        if !ctx.candidates.is_empty() {
            return Ok(());
        }
        Err(if enabled > 0 && missing == enabled {
            ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_provider_key_missing", "no provider key for this model")
        } else {
            unavailable()
        })
    }
}

pub(crate) fn unavailable() -> ZygoError {
    ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_provider_unavailable", "no provider available for this model")
}
