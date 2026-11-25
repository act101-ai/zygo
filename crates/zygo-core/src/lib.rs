use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub mod exit;
pub mod hook;
pub mod hook_opencode;
pub mod ids;
pub mod overrides;
pub mod paths;
pub mod session_dir;
pub mod session_view;
pub mod text;
pub mod tokens;
pub mod verbs;

pub use text::esc;

/// Declares `enum Agent`, `Agent::ALL` and `Agent::COUNT` from one list, so `ALL` holds each variant once.
macro_rules! agents {
    ($($(#[$m:meta])* $v:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum Agent {
            $($(#[$m])* $v,)+
        }

        impl Agent {
            /// The number of variants.
            pub const COUNT: usize = [$(stringify!($v)),+].len();
            /// Every client, in id order (P4-6 moves this to client/mod.rs, K§9).
            pub const ALL: [Agent; Agent::COUNT] = [$(Agent::$v),+];
        }
    };
}

agents! {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl Agent {

    /// The position of this client in `ALL`; the exhaustive match makes a new variant name its place.
    const fn index(self) -> usize {
        match self {
            Agent::ClaudeCode => 0,
            Agent::OpenCode => 1,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude-code",
            Agent::OpenCode => "opencode",
        }
    }
}

// Each variant of `ALL` sits at its own index, checked when the crate compiles.
const _: () = {
    let mut i = 0;
    while i < Agent::ALL.len() {
        assert!(Agent::ALL[i].index() == i);
        i += 1;
    }
};

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Agent {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "claude-code" => Ok(Agent::ClaudeCode),
            "opencode" => Ok(Agent::OpenCode),
            _ => anyhow::bail!("unknown agent `{s}` (claude-code or opencode)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Agent;

    /// `Agent::index` is an exhaustive match, so a new variant does not compile until it has an index,
    /// and this test fails until the variant is in `Agent::ALL` at that index.
    #[test]
    fn agent_all_holds_each_variant_at_its_index() {
        for (i, a) in Agent::ALL.into_iter().enumerate() {
            assert_eq!(a.index(), i, "{a}");
            assert_eq!(a.as_str().parse::<Agent>().unwrap(), a);
        }
    }

    /// `Agent::COUNT` counts the variants of the one list that declares the enum and `Agent::ALL`,
    /// so `ALL` holds each variant once and cannot miss one.
    #[test]
    fn agent_all_len_is_the_variant_count() {
        assert_eq!(Agent::ALL.len(), Agent::COUNT);
        let mut seen = [false; Agent::COUNT];
        for a in Agent::ALL {
            assert!(!std::mem::replace(&mut seen[a.index()], true), "{a} is in ALL twice");
        }
        assert!(seen.iter().all(|&s| s));
    }
}
