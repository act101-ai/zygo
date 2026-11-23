//! `zygo loadout list` (W§12; D7).

use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_workspace::loadout::{self, Source};

use super::{op5_exit, print_out, user_config_or_exit};

#[derive(clap::Args)]
pub struct LoadoutArgs {
    #[command(subcommand)]
    pub sub: LoadoutCmd,
}

#[derive(clap::Subcommand)]
pub enum LoadoutCmd {
    /// Every loadout and its source
    List,
}

/// The OP5 check, the user config (D3), then D7: `<name escaped>  built-in` or `<name escaped>  user` per line, in the
/// order of `loadout::list`.
pub fn run(a: &LoadoutArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let cfg = match user_config_or_exit(&Paths::from_env(), "loadout") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match &a.sub {
        LoadoutCmd::List => {
            let text: String = loadout::list(&cfg)
                .into_iter()
                .map(|(name, source)| {
                    let source = match source {
                        Source::Builtin => "built-in",
                        Source::User => "user",
                    };
                    format!("{}  {source}\n", esc(&name))
                })
                .collect();
            print_out(&text);
            0
        }
    }
}
