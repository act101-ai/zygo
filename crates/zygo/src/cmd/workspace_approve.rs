//! The CLI form of `zygo workspace approve [--yes] [--digest <hex>]` (workspace definition design R§7.1, R§7.2): the
//! read of the default branch, the listing, the question and the store. Only `workspace::approve` enters it, after the
//! slash form has returned, so the slash verb files name no definition read (R-C8, R§18 item 20; WD-15 plan call 10).

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{anyhow, bail};
use zygo_config::Config;
use zygo_config::repo_def::{approve_workspace_in, check_default, repository_loadout, Definition, Files, Stored, SystemGit, Use};
use zygo_core::esc;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::pool;
use zygo_workspace::up::{self, find_checkout, GIT_BOUND};

use super::verbs::VerbCtx;
use super::workspace::{line, listing};
use super::{cwd_error, user_config, UsageError};

const USAGE: &str = "zygo workspace: usage: workspace approve [--yes] [--digest <hex>]";
const HEX_USAGE: &str = "zygo workspace: --digest takes 64 hex characters";
const NOT_A_REPO: &str = "zygo: workspace approve needs a git repository";
const IN_SESSION: &str =
    "zygo: approval is not available inside an agent session; run it in your own shell: zygo workspace approve";
const NOT_APPROVED: &str = "not approved; run zygo workspace approve";
const QUESTION: &str = "approve these workspace files? [y/N] ";
const NEXT: &str = "to start it: zygo up (a workspace that exists already: zygo down, then zygo up --reset)";

/// The arguments of plan call 3: `--yes` and `--digest <hex>`, each at most once; `--save` or any other token, a
/// repeat or a missing value is a usage error. The digest is checked for 64 ASCII hex characters and kept as given.
fn parse_args(ctx: &VerbCtx, args: &[String]) -> Result<(bool, Option<String>), UsageError> {
    let usage = || UsageError(USAGE.into());
    if ctx.save {
        return Err(usage());
    }
    let (mut yes, mut digest) = (false, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--yes" if !yes => yes = true,
            "--digest" if digest.is_none() => digest = Some(it.next().ok_or_else(usage)?.clone()),
            _ => return Err(usage()),
        }
    }
    if digest.as_deref().is_some_and(|d| d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(UsageError(HEX_USAGE.into()));
    }
    Ok((yes, digest))
}

/// R§7.1 steps 2-7 in the order of plan call 1: arguments; `ZYGO_SESSION_DIR`; the checkout; one read of the default
/// branch; `--digest` (in every state, WD-15 review F1); already approved; the user config (call 7); the listing and
/// the question; the store (R§7.2).
/// The digest stored is the digest of the read that was shown: the files are not read again.
pub(crate) fn run(ctx: &VerbCtx, args: &[String], input: &mut dyn BufRead, prompt: &mut dyn Write) -> anyhow::Result<String> {
    let (yes, want) = parse_args(ctx, args)?;
    // Plan call 4: the rule of `zygo plugins approve` (an empty value counts as unset).
    if ctx.session_dir.is_some() {
        bail!(IN_SESSION);
    }
    // Plan call 5 (WD-15 review F3): the checkout lookup of `zygo up`, whose first value is the checkout of the current
    // folder (`zygo_work::repo::checkout`, the call of the launcher); the read runs at its top, the folder the launcher
    // reads (R§8), so the digest and the id are the ones the launcher checks.
    let cwd = std::env::current_dir().map_err(|e| anyhow!("zygo: {}", cwd_error(&e)))?;
    // WD-15 review F5: "not a repository" is exit 2 with this verb's line; any other lookup error (a bare repository with
    // no checkout, a git call that failed) keeps its reason, exit 1.
    let (co, _, _) = find_checkout(&cwd, GIT_BOUND).map_err(|e| -> anyhow::Error {
        if e.message == up::NOT_A_REPO {
            UsageError(NOT_A_REPO.into()).into()
        } else {
            anyhow!("zygo: workspace approve: {}", esc(&e.message))
        }
    })?;
    let id = workspace_id(&co.common_dir);
    let file = &ctx.paths.approvals_file;
    // Plan call 6: the lines of R§7.1 step 3, each value escaped.
    // `read` is `None` when the files of the digest are approved already.
    let (digest, read) = match check_default(&co.top, &id, file, &SystemGit) {
        Use::Absent { full_ref } => bail!("zygo: this repository has no workspace files at {}", esc(&full_ref)),
        Use::Unread { why } => bail!("zygo: this repository's workspace files are unread ({}); nothing approved", esc(&why)),
        Use::Invalid { full_ref, why, .. } => bail!(
            "zygo: this repository's workspace files ({}) are invalid ({}); nothing approved",
            esc(full_ref.as_deref().unwrap_or_default()),
            esc(&why)
        ),
        Use::Approved { pin, .. } => (pin.digest, None),
        Use::NotApproved { full_ref, files, digest, def } => (digest, Some((full_ref, files, def))),
    };
    // R§7.1 step 4 (WD-15 review F1): in every state, before the already-approved check, so a script learns that the
    // files it read are not the files that apply. Upper-case hex is compared in lower case; the line shows the value as
    // given.
    if let Some(hex) = want.filter(|h| h.to_ascii_lowercase() != digest) {
        bail!("zygo: the workspace files changed: their digest is {digest}, not {hex}; nothing approved");
    }
    // R§7.1 step 5.
    let Some((full_ref, files, def)) = read else {
        return Ok(format!("workspace files already approved ({digest})"));
    };
    let cfg = user_config(&ctx.paths)
        .map_err(|e| anyhow!("zygo: user config not read ({}); nothing approved", esc(&e.to_string())))?;
    let home = pool::home();
    let shown_files = show(&co.top, &full_ref, &files, &digest, &def, &cfg, home.as_deref());
    // R§7.1 step 6: with `--yes` the listing is part of the result (stdout); else it and the question go to `prompt`.
    if !yes {
        let _ = write!(prompt, "{shown_files}\n{QUESTION}");
        let _ = prompt.flush();
        if !super::plugins::confirm(input) {
            return Ok(NOT_APPROVED.into());
        }
    }
    // R§7.1 step 7, plan call 8.
    let done = match approve_workspace_in(file, &id, &digest) {
        Ok(Stored::Added) => format!("approved {digest}\n{NEXT}"),
        Ok(Stored::AlreadyPresent) => format!("workspace files already approved ({digest})"),
        Err(e) => bail!("zygo: cannot store the approval: {}; nothing approved", esc(&e.to_string())),
    };
    Ok(if yes { format!("{shown_files}\n{done}") } else { done })
}

/// One text in the character rule of the listing (R§7.1 step 6, R-C7): a newline is `\u{a}`.
fn shown(s: &str) -> String {
    line(s)
}

/// A free-text value of the summary (plan call 9, WD-15 review F2): `esc`, then `\` and `"` escaped with `\`, in double
/// quotes, so a `,` `;` `=` or space in it cannot fake the structure of the line.
fn quoted(s: &str) -> String {
    format!("\"{}\"", esc(s).replace('\\', "\\\\").replace('"', "\\\""))
}

/// The summary of R§7.1 step 6 and R§9.5 "Shown to the operator" (plan call 9): each member of the repository loadout
/// with the defaults filled in, the agent count, each profile, and with a pool the members that run in passthrough, or,
/// when `zygo up` refuses the pool (`up::pool_refused`, WD-15 review F6), its reason. Names the shape restricts to `[a-z0-9-]` (member, profile and role names) and the fixed values of `kind` and `job`
/// are bare; the free text of the files (`cwd`, each override's model) is `quoted`.
fn summary(def: &Definition, cfg: &Config, home: Option<&Path>) -> Vec<String> {
    let panes: Vec<_> = repository_loadout(def).map(|l| l.window.into_iter().flat_map(|w| w.pane).collect()).unwrap_or_default();
    let or_dash = |v: &Option<String>| v.as_deref().map_or_else(|| "-".to_string(), esc);
    let mut out = vec!["summary:".to_string(), format!("  agents: {}", panes.len())];
    for p in &panes {
        out.push(format!(
            "  member {}: kind {}, job {}, profile {}, pool {}, cwd {}",
            or_dash(&p.name),
            esc(&p.kind),
            or_dash(&p.job),
            or_dash(&p.profile),
            p.pool,
            p.cwd.as_deref().map_or_else(|| "-".to_string(), quoted)
        ));
    }
    for (name, prof) in &def.profiles {
        let overrides: Vec<String> = prof.overrides.iter().map(|(r, m)| format!("{} = {}", esc(r), quoted(m))).collect();
        let disabled: Vec<String> = prof.disabled.iter().map(|r| esc(r)).collect();
        let list = |v: Vec<String>| if v.is_empty() { "-".to_string() } else { v.join(", ") };
        out.push(format!("  profile {}: overrides {}; disabled {}", esc(name), list(overrides), list(disabled)));
    }
    // R§9.5: the pool members by the rule of `zygo up` (`up::pool_member`); named in `pool::order`, the text of `zygo up`.
    let members: Vec<(usize, _)> = panes
        .iter()
        .enumerate()
        .filter(|(_, p)| up::pool_member(p.pool, p.kind == "claude-code", p.server.as_deref()))
        .collect();
    if let Some(text) = cfg.workspace.pool.as_deref().filter(|_| !members.is_empty()) {
        let dir = match pool::expand(text, home) {
            Ok(d) => esc(&d.to_string_lossy()),
            Err(r) => format!("{} ({})", esc(text), esc(&r)),
        };
        // R§9.5 "Local mode only": `zygo up` refuses the pool, so no member runs in passthrough.
        if let Some(why) = up::pool_refused(cfg) {
            out.push(format!("  pool {dir}: {why}; zygo up starts nothing"));
            return out;
        }
        let names: Vec<Option<String>> = members.iter().map(|(_, p)| p.name.clone()).collect();
        let label = |i: usize| match &members[i].1.name {
            Some(n) => esc(n),
            None => format!("pane {}", members[i].0 + 1),
        };
        let order = pool::order(&names);
        let labels: Vec<String> = order.iter().map(|&i| label(i)).collect();
        let mut l = format!("  {} run in passthrough on pool {dir}", labels.join(", "));
        for &i in &order {
            if let Some(p) = &members[i].1.profile {
                l.push_str(&format!("; profile {} of {} does not apply in passthrough", esc(p), label(i)));
            }
        }
        out.push(l);
    }
    out
}

/// The listing of R§7.1 step 6 (plan call 9): the folder, the full ref, the read commit, the digest, each file with its
/// size and its lines through `listing` (4 spaces in), the ignored entries and the summary.
fn show(folder: &Path, full_ref: &str, files: &Files, digest: &str, def: &Definition, cfg: &Config,
    home: Option<&Path>) -> String {
    let mut out = vec![
        format!("folder: {}", esc(&folder.to_string_lossy())),
        format!("ref: {}", esc(full_ref)),
        format!("commit: {}", esc(&files.commit)),
        format!("digest: {digest}"),
    ];
    let mut file = |path: String, bytes: &[u8]| {
        out.push(format!("{path} ({} bytes):", bytes.len()));
        if !bytes.is_empty() {
            out.extend(listing(bytes).split('\n').map(|l| format!("    {l}")));
        }
    };
    if let Some(ws) = &files.workspace {
        file(".zygo/workspace.toml".into(), ws);
    }
    for (name, bytes) in &files.profiles {
        file(format!(".zygo/profiles/{}.toml", shown(name)), bytes);
    }
    out.push(if files.ignored.is_empty() {
        "ignored: none".to_string()
    } else {
        format!("ignored: {}", files.ignored.iter().map(|e| shown(e)).collect::<Vec<_>>().join(", "))
    });
    out.extend(summary(def, cfg, home));
    out.join("\n")
}
