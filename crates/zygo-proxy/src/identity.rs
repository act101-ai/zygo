//! Stage 1, identity (§7.2, §7.3).

use std::sync::Arc;

use http::HeaderMap;

use crate::errors::ZygoError;
use crate::passthrough::{self, CredentialType};
use crate::session::{SessionState, Sessions, TeamAuth};

fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok()).filter(|v| !v.is_empty())
}

/// `authorization: Bearer <t>`; the scheme is case-insensitive (RFC 9110 §11.1).
fn bearer(h: &HeaderMap) -> Option<&str> {
    let (scheme, t) = header(h, "authorization")?.split_once(' ')?;
    let t = t.trim_start_matches(' ');
    (scheme.eq_ignore_ascii_case("bearer") && !t.is_empty()).then_some(t)
}

/// `x-api-key`, else `authorization: Bearer <t>`.
pub fn session_token(h: &HeaderMap) -> Option<&str> {
    header(h, "x-api-key").or_else(|| bearer(h))
}

/// Stage 1: `x-zygo-session` id + session token → live session. N11 calls it on model routes. Call
/// `Sessions::accept` once the request is accepted (§7.2).
pub(crate) fn authenticate(h: &HeaderMap, sessions: &Sessions) -> Result<Arc<SessionState>, ZygoError> {
    let token = session_token(h).ok_or_else(ZygoError::token_invalid)?;
    let id = header(h, "x-zygo-session").ok_or_else(ZygoError::token_invalid)?;
    sessions.by_token(id, token)
}

/// Stage 1 on the `/anthropic/*` routes (§12.4): with `x-zygo-token` present (even empty or not UTF-8), that token and
/// `x-zygo-session` name a live passthrough session, else `zygo_passthrough_token_invalid`; without it, `authenticate`.
/// A passthrough session then needs the client's login on its type's allowlist, else `zygo_passthrough_no_login`.
pub(crate) fn model_identity(h: &HeaderMap, sessions: &Sessions) -> Result<Arc<SessionState>, ZygoError> {
    let s = match h.get("x-zygo-token") {
        Some(t) => {
            let t = t.to_str().ok().filter(|t| !t.is_empty()).ok_or_else(passthrough::token_invalid)?;
            let id = header(h, "x-zygo-session").ok_or_else(passthrough::token_invalid)?;
            sessions.by_token(id, t).ok().filter(|s| s.passthrough).ok_or_else(passthrough::token_invalid)?
        }
        None => authenticate(h, sessions)?,
    };
    if s.passthrough && !CredentialType::for_agent(s.agent).is_some_and(|t| t.accepts(h)) {
        return Err(passthrough::no_login());
    }
    Ok(s)
}

/// Team routes: `authorization: Bearer <t>` → `team.label_for(t)`; no TeamAuth, no bearer or no label → token_invalid.
pub(crate) fn team_label(h: &HeaderMap, team: Option<&dyn TeamAuth>) -> Result<String, ZygoError> {
    let (Some(team), Some(t)) = (team, bearer(h)) else { return Err(ZygoError::token_invalid()) };
    team.label_for(t).ok_or_else(ZygoError::token_invalid)
}
