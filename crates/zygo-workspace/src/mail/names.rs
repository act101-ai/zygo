//! Mail address resolution: `to` by name, pane key or `all`, and the sender's `from` (M§5, M§6, M§8). Owner: node ML-4.
use std::collections::BTreeSet;

use crate::identity::valid_pane_key;
use crate::mail::MailError;
use crate::state::Workspace;

/// One member a message can go to: its pane key and its valid name, if any (AU84).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub key: String,
    pub name: Option<String>,
}

/// Every member in key order, each with its valid name (a stored name that fails AU84 reads as none).
pub fn members(ws: &Workspace) -> Vec<Target> {
    ws.members
        .iter()
        .map(|(key, rec)| Target {
            key: key.clone(),
            name: rec.valid_name().map(String::from),
        })
        .collect()
}

/// The targets of `to` (M§8): `all` is every member but the sender; a pane key that is a member is that member;
/// any other `to` is the members with that valid name, in key order. No target gives `UnknownTarget` with the
/// valid names, sorted and without duplicates (M§10).
pub fn resolve(
    ws: &Workspace,
    to: &str,
    sender_key: Option<&str>,
) -> Result<Vec<Target>, MailError> {
    let all = members(ws);
    if to == "all" {
        return Ok(all
            .into_iter()
            .filter(|t| Some(t.key.as_str()) != sender_key)
            .collect());
    }
    let found: Vec<Target> = if valid_pane_key(to) && ws.members.contains_key(to) {
        all.iter().filter(|t| t.key == to).cloned().collect()
    } else {
        all.iter()
            .filter(|t| t.name.as_deref() == Some(to))
            .cloned()
            .collect()
    };
    if found.is_empty() {
        let known: BTreeSet<String> = all.into_iter().filter_map(|t| t.name).collect();
        return Err(MailError::UnknownTarget {
            known: known.into_iter().collect(),
        });
    }
    Ok(found)
}

/// The `from` of a message (M§8): the sender's valid name, its pane key when it has none, or `cli` when there is
/// no sender or the key is not a member.
pub fn sender(ws: &Workspace, key: Option<&str>) -> String {
    match key.and_then(|k| ws.members.get_key_value(k)) {
        Some((k, rec)) => rec.valid_name().unwrap_or(k).to_string(),
        None => "cli".to_string(),
    }
}
