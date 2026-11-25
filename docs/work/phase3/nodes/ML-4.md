# ML-4 — Address resolution and the send operation

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§5 (send, `all`), M§6, M§8 (`to`: name, pane key or `all`; sender `cli` or the member), M§10 row "Target name unknown"
**Files:** `crates/zygo-workspace/src/mail/mod.rs`, `crates/zygo-workspace/src/mail/names.rs`, `crates/zygo-workspace/tests/mail_names.rs`
**Consumes:** `MemberRecord.name` (ML-3), `MemberRecord::valid_name` (ML-3), `Message::new` (ML-2), `deliver` (ML-2), `mail_dir` (ML-2), `MailError` (ML-2), `MailConfig` (ML-1)
**Produces:** `zygo_workspace::mail::names::Target { key: String, name: Option<String> }`, `zygo_workspace::mail::names::members(ws: &Workspace) -> Vec<Target>` (key order), `zygo_workspace::mail::names::resolve(ws: &Workspace, to: &str, sender_key: Option<&str>) -> Result<Vec<Target>, MailError>`, `zygo_workspace::mail::names::sender(ws: &Workspace, key: Option<&str>) -> String`, `zygo_workspace::mail::Sent { id: String, key: String }`, `zygo_workspace::mail::send(paths: &Paths, ws: &Workspace, sender_key: Option<&str>, to: &str, subject: &str, body: &str, cfg: &MailConfig, now: SystemTime) -> Result<SendReport, MailError>`, `zygo_workspace::mail::SendReport { sent: Vec<Sent>, refused: Vec<Refused> }`, `zygo_workspace::mail::Refused { key: String, error: MailError }`
**Acceptance:** `cargo test -p zygo-workspace --test mail_names` → all pass: `to` by name, by pane key and `all` (sender excluded, one file per other member); an unknown name returns the error that names each known name; a member with no name is reachable by pane key only; `from` is the sender's name, its pane key when it has no name, or `cli`; a stored name that fails AU84 is not matched; `send` writes no byte of subject or body to `workspace.json` or `errors.log`

## Plan

Premise check (live code at 57cc1691): ML-2 (history.md row, PR #24) and ML-3 (`863b6b05`, PR #62) are in `phase-3`. Used as they are:
`Message::new(from: &str, to: &str, subject: &str, body: &str, now: SystemTime) -> Result<Message, MailError>` (`crates/zygo-workspace/src/mail/mod.rs:35`, checks the subject and body limits);
`store::deliver(mail_dir: &Path, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError>` (`crates/zygo-workspace/src/mail/store.rs:188`, makes `mail/` and the maildir `0700`, gives `Full { label, max }`);
`mail_dir(paths: &Paths, ws_id: &str) -> PathBuf` (`mail/mod.rs:91`); `MailError` (`mail/mod.rs:59`, no variant for an unknown target yet);
`MemberRecord.name: Option<String>` and `MemberRecord::valid_name(&self) -> Option<&str>` (`crates/zygo-workspace/src/state.rs:130`, `:344`; `zygo_core::ids::valid_member_name` refuses `all`, `cli`, `zygo` and pane-key forms, `crates/zygo-core/src/ids.rs:17-37`);
`Workspace.members: BTreeMap<String, MemberRecord>` (`state.rs:57`, keys checked as pane keys at read, `state.rs:410-412`);
`zygo_config::MailConfig { max_unread: u32, .. }` (`crates/zygo-config/src/schema.rs:690`, default 500); `identity::valid_pane_key` (`crates/zygo-workspace/src/identity.rs`).
No existing function maps a name to a pane key (`grep -rn "valid_name()" crates/zygo-workspace/src` finds only the two accessors). `names.rs` does not exist.

Author's calls (M§5, M§6, M§10 leave them open; no new gate):
- `resolve` order: `all` first, then a pane key (`valid_pane_key`) that is a key of `members`, then a name. M§6 makes these sets disjoint, so the order changes no result.
- A name held by two members (M§6 makes a name unique in the loadout, but nothing joins hand-started names) resolves to each holder in key order, so no mail is lost.
- `all` with no other member gives `Ok(vec![])`. `send` stops at the first `deliver` error. Files already delivered stay in `new/`, and M§7.1-7.2 deliver them.
- The `to` field of each file and the `deliver` label are the target's name, or its pane key when it has no name (M§5 `"to": <name>`).
- `sender(ws, Some(k))` for a `k` that is not a key of `members` gives `cli`, so `from` never names a pane that is not a member.
- Unknown-target line: `zygo: unknown target; known names: <names>`, with the valid names sorted, without duplicates, joined by `, `, or `none`. The line does not repeat the `to` text.

- [x] **1. Tests first.** Create `crates/zygo-workspace/tests/mail_names.rs`, `#![cfg(unix)]` (global constraint: tests that use workspace state). Copy the `paths(p: &Path) -> Paths` helper from `tests/mail_store.rs:30-34`. Fixture:
  ```rust
  const ID: &str = "0123456789abcdef";
  const LEAD: &str = "klead2222"; const ELENA: &str = "kelena222"; const ANON: &str = "kanon2222"; const BAD: &str = "kbad22222";
  const T0: u64 = 1_700_000_000;
  fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
  fn t(key: &str, name: Option<&str>) -> Target { Target { key: key.into(), name: name.map(String::from) } }
  fn ws() -> Workspace { // identity fields as tests/state.rs:20-24, so Store::update accepts it
      let mut ws = Workspace::new(ID);
      ws.name = "app".into(); ws.common_dir = "/srv/app/.git".into(); ws.main_root = "/srv/app".into();
      ws.tmux_session = "zygo-app".into(); ws.loadout = "solo".into(); ws.phase = Phase::Up;
      for (k, n) in [(LEAD, Some("lead")), (ELENA, Some("elena")), (ANON, None), (BAD, Some("Bad_Name"))] {
          let mut m = MemberRecord::new(Agent::ClaudeCode); m.name = n.map(String::from); ws.members.insert(k.into(), m);
      }
      ws
  }
  fn new_files(paths: &Paths, key: &str) -> Vec<Message> // parses each file in mail/<key>/new/; empty when missing
  ```
  Tests and assertions:
  - `members_lists_every_member_in_key_order`: `assert_eq!(members(&ws()), vec![t(ANON, None), t(BAD, None), t(ELENA, Some("elena")), t(LEAD, Some("lead"))])`.
  - `resolve_by_name`: `assert_eq!(resolve(&ws(), "elena", Some(LEAD)).unwrap(), vec![t(ELENA, Some("elena"))])`.
  - `resolve_by_pane_key`: `resolve(&ws(), ANON, Some(LEAD))` is `vec![t(ANON, None)]`; `resolve(&ws(), ELENA, None)` is `vec![t(ELENA, Some("elena"))]`.
  - `resolve_all_excludes_the_sender`: `resolve(&ws(), "all", Some(LEAD))` is `vec![t(ANON, None), t(BAD, None), t(ELENA, Some("elena"))]`; `resolve(&ws(), "all", None)` equals `members(&ws())`.
  - `unknown_target_names_each_known_name`: for each of `"bob"`, `"Bad_Name"`, `"kzzzzzzzz"`, `""`, `"cli"`, `"zygo"`, `"elena@host"`: `let e = resolve(&ws(), to, Some(LEAD)).unwrap_err(); assert!(matches!(&e, MailError::UnknownTarget { known } if known == &["elena", "lead"])); assert_eq!(e.to_string(), "zygo: unknown target; known names: elena, lead");`. With every name removed: `"zygo: unknown target; known names: none"`.
  - `stored_name_that_fails_au84_is_not_matched`: with `members[BAD].name = Some("kelena222")`, `resolve(.., "kelena222", None)` is `vec![t(ELENA, Some("elena"))]`; with `Some("all")`, `resolve(.., "all", Some(LEAD))` holds `t(BAD, None)`; `resolve(&ws(), BAD, None)` is `vec![t(BAD, None)]` (reachable by key only).
  - `sender_is_name_then_key_then_cli`: `sender(&w, Some(LEAD)) == "lead"`, `Some(ANON) == ANON`, `Some(BAD) == BAD`, `None == "cli"`, `Some("kzzzzzzzz") == "cli"`.
  - `send_by_name_writes_one_file`: `send(&p, &ws(), Some(LEAD), "elena", "Plan", "Body\n", &MailConfig::default(), at(T0))` → one `Sent { key: ELENA, id }`; `new_files(&p, ELENA)` is one message with `id`, `from == "lead"`, `to == "elena"`, `subject == "Plan"`, `body == "Body\n"`.
  - `send_to_a_member_with_no_name`: `send(.., None, ANON, ..)` → the file has `from == "cli"`, `to == ANON`.
  - `send_to_all_writes_one_file_per_other_member`: `send(.., Some(LEAD), "all", ..)` → keys `[ANON, BAD, ELENA]`, three distinct ids; `new_files` length 1 for each, with `to` `ANON`, `BAD`, `"elena"`; `mail/klead2222` does not exist.
  - `send_unknown_target_writes_nothing`: `Err(MailError::UnknownTarget { .. })` and `!mail_dir(&p, ID).exists()`.
  - `send_full_names_the_target`: with `cfg.max_unread = 1`, the second send to `"elena"` gives `e.to_string() == "zygo: elena has 1 unread messages"`.
  - `send_writes_no_content_outside_mail` (review focus 55, M-C2): write `ws()` with `Store::open(&p, ID).update(Duration::from_secs(5), &mut |w| { *w = ws(); Ok(()) })` and call `store.log_error("x")`. Run a good send to `all`, an unknown-target send and a full send, each with subject `"subj-7f3a"` and body `"body-9c1e"`. Then `workspace.json` and `errors.log` hold neither marker, and every file under `store.dir()` that holds a marker is under `mail_dir(&p, ID)`.

  Check: `cargo test -p zygo-workspace --test mail_names` fails to compile (no `names`, `send`, `Sent`, `UnknownTarget`).

- [x] **2. `MailError::UnknownTarget`** in `crates/zygo-workspace/src/mail/mod.rs`: variant `UnknownTarget { known: Vec<String> }`. `Display`: `zygo: unknown target; known names: {}` with `known.join(", ")`, or `none` when `known` is empty. Add `pub mod names;`. Change the module doc owner to "nodes ML-2, ML-4".

- [x] **3. `names.rs`** (new, `crates/zygo-workspace/src/mail/names.rs`):
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Target { pub key: String, pub name: Option<String> }`
  - `pub fn members(ws: &Workspace) -> Vec<Target>`: one per `ws.members` entry in map (key) order, `name: rec.valid_name().map(String::from)`.
  - `pub fn resolve(ws: &Workspace, to: &str, sender_key: Option<&str>) -> Result<Vec<Target>, MailError>`: `"all"` → `members` without `key == sender_key`; `valid_pane_key(to)` and a key of `ws.members` → that one target; otherwise the members whose `name == Some(to)`. An empty result for a non-`all` `to` → `Err(UnknownTarget { known })`, with `known` = the valid names as a `BTreeSet` collected into a `Vec`.
  - `pub fn sender(ws: &Workspace, key: Option<&str>) -> String`: `Some(k)` that is a key of `ws.members` → its `valid_name()`, else `k`; otherwise `"cli"`.

- [x] **4. `send`** in `mail/mod.rs`:
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Sent { pub id: String, pub key: String }`
  - `pub fn send(paths: &Paths, ws: &Workspace, sender_key: Option<&str>, to: &str, subject: &str, body: &str, cfg: &zygo_config::MailConfig, now: SystemTime) -> Result<Vec<Sent>, MailError>`: `resolve` first (an unknown target writes nothing), `from = sender(..)`, `dir = mail_dir(paths, &ws.id)`. For each target: `label = name or key`; `Message::new(&from, label, subject, body, now)?`; `store::deliver(&dir, &t.key, label, &msg, cfg.max_unread)?`; push `Sent { id: msg.id, key: t.key }`. Write no log line, so subject and body reach only `mail/` (M-C2).

  Check: `cargo test -p zygo-workspace --test mail_names` passes. `cargo test -p zygo-workspace --test mail_store` stays green. `cargo clippy -p zygo-workspace --all-targets -- -D warnings` is clean.

Consumers (signatures above match their Consumes lines): ML-6 (`Target`), ML-9 (`members`), ML-11 (`send`, `members`, the unknown-target error), ML-12 (`send`, `resolve`), MS-12 (`members`, `Target`), MS-9 (`send`; see the note below).

Note for MS-9 (out of this node): S§8.6 makes the refusal notice come from `zygo`. `send` takes its `from` from `sender(ws, sender_key)`, which gives only a member name, a pane key or `cli`. MS-9 therefore cannot make the notice with `send`. It can build it with `Message::new("zygo", ..)` and `store::deliver`, and get the target from `resolve`.

## Review round 1

- F1 fixed (`47a70e67` tests, fix in commit "ML-4: review round 1 fixes"): `send` returns `SendReport { sent, refused }`. A target whose message cannot be made or delivered (for example `Full`) goes into `refused`; the send continues with the next target. `Err` only for `UnknownTarget`, `SubjectTooLong`, `BodyTooLarge`, before any file is written.
- F3 fixed (same commits): `send` checks the subject and body limits (private `check_limits`, shared with `Message::new`) before `resolve`, so `all` with no other member still refuses an over-limit subject or body.
- F5 fixed (`47a70e67`): `stored_name_that_fails_au84_is_not_matched` stores the reserved name `cli`; `resolve(.., "cli", ..)` is `UnknownTarget` and the member stays reachable by its pane key.
- F2 handled by the controller: the MS-9 contract change is a controller chore, not this node.
- F4 refuted: the state load checks `ws.id` (`crates/zygo-workspace/src/state.rs:404`).
