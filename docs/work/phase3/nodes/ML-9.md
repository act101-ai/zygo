# ML-9 — Session text item 8 (workspace mail) at main-loop SessionStart

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§8a items 1 and 2; `docs/specs/2026-11-18-zygo-design.md` §8.6
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-hooks/src/lib.rs`, `crates/zygo-hooks/src/mail.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/session_text.rs`
**Consumes:** `Features.mail` (ML-1), `members` (ML-4), `Member.name` (ML-7)
**Produces:** `zygo_core::session_view::SessionView.mail: bool` (`#[serde(default)]`; the launcher sets it from `features.mail`), `zygo_hooks::mail::session_item(ws: &zygo_workspace::state::Workspace, own_key: &str) -> String`
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test session_text -- mail_item --list 2>/dev/null | grep -c ': test$')" -ge 5 && cargo test -p zygo-hooks --test session_text` → every count holds (at least 5 named `mail_item…` in zygo-hooks `session_text`; a count of 0 fails before any run), then all pass: a named member with `mail` on gets item 8 after the mechanics text and before the resume note, with its name, the workspace name and every member name read from `workspace.json` at this start (a member added while it was down is listed: M§13 #12, second half); a member with no name gets the no-name clause with its pane key; with `mail` off, or outside a workspace, there is no item 8; `mechanics` (the subagent text) has no item 8; each name passes `esc`; the item holds no subject or body text

## Plan
