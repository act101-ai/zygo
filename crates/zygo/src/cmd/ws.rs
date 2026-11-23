//! `zygo ws forget <name>`, `zygo ws forget-pane <key> [--name <name>]` and the hidden `zygo ws snapshot` (W§12,
//! AU44). `zygo ws` does not sweep (AU71).

use zygo_core::paths::Paths;
use zygo_hooks::workspace::snapshot_child;
use zygo_workspace::admin;
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::snapshot::{self, SkipReason, SnapshotOutcome, HOOK_WAIT, OP_WAIT};
use zygo_workspace::state::Store;

use super::{cwd_or_exit, op5_exit, print_err, tmux_program};

#[derive(clap::Args)]
pub struct WsArgs {
    #[command(subcommand)]
    pub sub: WsCmd,
}

#[derive(clap::Subcommand)]
pub enum WsCmd {
    /// Internal: the snapshot child started by the tmux hooks
    #[command(hide = true)]
    Snapshot {
        #[arg(long = "ws-id")]
        ws_id: String,
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Delete a workspace that is not up: its state and its results
    Forget { name: String },
    /// Delete one pane of a workspace: its record and its results
    ForgetPane {
        key: String,
        /// Workspace name; default: the workspace of this folder
        #[arg(long)]
        name: Option<String>,
    },
}

/// The OP5 check first, for every subcommand, then the subcommand.
pub fn dispatch(args: WsArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let paths = Paths::from_env();
    match args.sub {
        WsCmd::Forget { name } => admin::forget(
            &paths,
            &tmux_program(),
            std::env::var_os("TMUX").as_deref(),
            &name,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr().lock(),
        ),
        WsCmd::ForgetPane { key, name } => {
            // D10: the folder names the workspace only when there is no `--name`; `forget_pane` does not read `cwd`
            // when it has a name, so a folder that cannot be read does not fail that form.
            let cwd = if name.is_some() {
                std::path::PathBuf::new()
            } else {
                match cwd_or_exit() {
                    Ok(d) => d,
                    Err(code) => return code,
                }
            };
            admin::forget_pane(
                &paths,
                &tmux_program(),
                name.as_deref(),
                &cwd,
                &key,
                &mut std::io::stdout().lock(),
                &mut std::io::stderr().lock(),
            )
        }
        WsCmd::Snapshot { ws_id, event, force } => {
            snapshot_cmd(&paths, &ws_id, event.as_deref(), force);
            0
        }
    }
}

/// The whole snapshot body runs through `snapshot_child` (H1 D15), so a body that fails queues `snapshot.pending`
/// again. An invalid id (AU84) and an error are one stderr line each; the exit code is always 0.
fn snapshot_cmd(paths: &Paths, ws_id: &str, event: Option<&str>, force: bool) {
    let mut outcome = None;
    let r = snapshot_child(paths, ws_id, force, &mut || {
        let socket = match Store::open(paths, ws_id).read() {
            Ok(Some(ws)) => ws.tmux_socket,
            _ => None,
        };
        let wait = if force { OP_WAIT } else { HOOK_WAIT };
        outcome = Some(snapshot::run(paths, &Tmux::new(tmux_program(), socket), ws_id, event, force, wait)?);
        Ok(())
    });
    match r {
        Ok(()) => {
            if matches!(outcome, Some(SnapshotOutcome::Skipped(SkipReason::InvalidId))) {
                print_err(&format!("zygo: invalid workspace id {}\n", zygo_core::esc(ws_id)));
            }
        }
        Err(e) => print_err(&format!("zygo: {}\n", zygo_core::esc(&e.to_string()))),
    }
}
