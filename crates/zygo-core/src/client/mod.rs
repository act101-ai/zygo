//! The data registry of the clients (K§6.1.1, lower layer): the client kind `Agent`, `Agent::ALL`,
//! `Agent::DEFAULT` and `Agent::spec()`, which gives each client's descriptor (`ClientSpec`, K§6.1.2).
//! `FromStr` and `as_str` are built from the descriptors. The only matches on the kind are in this file.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod claude_code;
pub mod opencode;

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
            /// Every client, in id order.
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

/// The facts of one client that do not need a behavior (K§6.1.2): concern 1 (identity and updates) and
/// concern 2 (hook input). Each client's value is in its own file of this module.
#[derive(Debug, Clone)]
pub struct ClientSpec {
    /// The client id: the serde name, the `ZYGO_AGENT` value and the grade file name.
    pub id: &'static str,
    /// The display name, as in the `docs/premises.md` and `docs/live-checks.md` headers.
    pub name: &'static str,
    /// The program that zygo runs.
    pub program: &'static str,
    /// The zygo CLI verb that launches the client.
    pub verb: &'static str,
    /// The client's config folder name (K§6.1.4).
    pub config_dir: &'static str,
    /// The text printed when the program is not on `PATH`.
    pub install_hint: &'static str,
    /// The minimum supported version (= the pinned version of the grade file and of `docs/premises.md`).
    pub min_version: &'static str,
    /// A supported client must meet the conformance bar (K-D9).
    pub supported: bool,
    /// The status surface: the client runs a `statusLine` command.
    pub status_line: bool,
    /// Turns the client's hook JSON into the common (Claude Code) form.
    pub hook_input: fn(Value) -> Value,
}

impl Agent {
    /// The client that a value with no client names (a session view, a hook, a loadout).
    pub const DEFAULT: Agent = Agent::ClaudeCode;

    /// This client's descriptor.
    pub fn spec(self) -> &'static ClientSpec {
        match self {
            Agent::ClaudeCode => &claude_code::SPEC,
            Agent::OpenCode => &opencode::SPEC,
        }
    }

    /// The position of this client in `ALL`; the exhaustive match makes a new variant name its place.
    pub(crate) const fn index(self) -> usize {
        match self {
            Agent::ClaudeCode => 0,
            Agent::OpenCode => 1,
        }
    }

    /// The client id (`ClientSpec::id`).
    pub fn as_str(&self) -> &'static str {
        self.spec().id
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
    /// The client of `Agent::ALL` whose id is `s`.
    fn from_str(s: &str) -> anyhow::Result<Self> {
        if let Some(a) = Agent::ALL.into_iter().find(|a| a.spec().id == s) {
            return Ok(a);
        }
        let ids: Vec<&str> = Agent::ALL.iter().map(|a| a.spec().id).collect();
        anyhow::bail!("unknown agent `{s}` ({})", ids.join(" or "))
    }
}

/// The wire format of a model API: the proxy routes and translates by dialect, not by client (K-D17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dialect {
    Anthropic,
    Openai,
    OpenaiChat,
}
