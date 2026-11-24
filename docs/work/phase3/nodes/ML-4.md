# ML-4 — Address resolution and the send operation

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§5 (send, `all`), M§6, M§8 (`to`: name, pane key or `all`; sender `cli` or the member), M§10 row "Target name unknown"
**Files:** `crates/zygo-workspace/src/mail/mod.rs`, `crates/zygo-workspace/src/mail/names.rs`, `crates/zygo-workspace/tests/mail_names.rs`
**Consumes:** `MemberRecord.name` (ML-3), `MemberRecord::valid_name` (ML-3), `Message::new` (ML-2), `deliver` (ML-2), `mail_dir` (ML-2), `MailError` (ML-2), `MailConfig` (ML-1)
**Produces:** `zygo_workspace::mail::names::Target { key: String, name: Option<String> }`, `zygo_workspace::mail::names::members(ws: &Workspace) -> Vec<Target>` (key order), `zygo_workspace::mail::names::resolve(ws: &Workspace, to: &str, sender_key: Option<&str>) -> Result<Vec<Target>, MailError>`, `zygo_workspace::mail::names::sender(ws: &Workspace, key: Option<&str>) -> String`, `zygo_workspace::mail::Sent { id: String, key: String }`, `zygo_workspace::mail::send(paths: &Paths, ws: &Workspace, sender_key: Option<&str>, to: &str, subject: &str, body: &str, cfg: &MailConfig, now: SystemTime) -> Result<Vec<Sent>, MailError>`
**Acceptance:** `cargo test -p zygo-workspace --test mail_names` → all pass: `to` by name, by pane key and `all` (sender excluded, one file per other member); an unknown name returns the error that names each known name; a member with no name is reachable by pane key only; `from` is the sender's name, its pane key when it has no name, or `cli`; a stored name that fails AU84 is not matched; `send` writes no byte of subject or body to `workspace.json` or `errors.log`

## Plan
