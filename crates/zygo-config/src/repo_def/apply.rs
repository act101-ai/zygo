//! The approved definition added to a merged config (R§8 "Not a layer", R§9.1, R§10).

use super::{repository_loadout, Definition, REPOSITORY_LOADOUT};
use crate::{Config, ProfileConfig};

/// What [`apply_to_config`] added and what it left as it was. Names in byte order (the order of `def.profiles`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Repository profiles added to `config.profile`.
    pub profiles: Vec<String>,
    /// Repository profiles not added: a user profile or a team preset has the name (R§8).
    pub shadowed_profiles: Vec<String>,
    /// The `repository` loadout was added.
    pub loadout: bool,
    /// The `repository` loadout was not added: the user config has `[loadout.repository]` (R§9.1).
    pub loadout_shadowed: bool,
}

/// Adds the repository profiles (no `features`) and the `repository` loadout to `cfg`, each only when the name is
/// not already there. Trusts `def`: pass only the `def` of `Use::Approved` or `PinnedUse::Approved`. `parse` validated its
/// values (the R§6.4 allow list); the profile names are validated only by the read (`read.rs`, `^[a-z0-9-]{1,32}$`, not `none`).
pub fn apply_to_config(cfg: &mut Config, def: &Definition) -> Applied {
    let mut a = Applied::default();
    for (name, p) in &def.profiles {
        if cfg.profile.contains_key(name) {
            a.shadowed_profiles.push(name.clone());
        } else {
            let profile = ProfileConfig { overrides: p.overrides.clone(), disabled: p.disabled.clone(), ..ProfileConfig::default() };
            cfg.profile.insert(name.clone(), profile);
            a.profiles.push(name.clone());
        }
    }
    if let Some(l) = repository_loadout(def) {
        if cfg.loadout.contains_key(REPOSITORY_LOADOUT) {
            a.loadout_shadowed = true;
        } else {
            cfg.loadout.insert(REPOSITORY_LOADOUT.into(), l);
            a.loadout = true;
        }
    }
    a
}
