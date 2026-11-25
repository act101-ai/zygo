# ML-8 — `zygo ws name <key> <name>`

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§6 (last paragraph); `docs/specs/2026-11-20-workspaces-design.md` W§12, AU84
**Files:** `crates/zygo/src/cmd/ws.rs`, `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/tests/admin.rs`
**Consumes:** `MemberRecord.name` (ML-3), `Pane.name` (ML-3), `valid_member_name` (ML-1)
**Produces:** `zygo_workspace::admin::rename_member(paths: &Paths, workspace: Option<&str>, cwd: &Path, key: &str, new_name: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32`, `cmd::ws::WsCmd::Name { key: String, name: String, workspace: Option<String> }`
**Acceptance:** `test "$(cargo test -q -p zygo-workspace --test admin -- rename_member --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-workspace --test admin && cargo test -p zygo --lib` → every count holds (at least 4 named `rename_member…` in zygo-workspace `admin`; a count of 0 fails before any run), then all pass: the record's `name` and the `panes[]` entry change in one write under `lock`; `mail/<key>/` is not moved; an invalid key, an invalid or reserved name, or a name another member holds exits 1 with one line and changes nothing; an unknown key exits 1

## Plan

Premise check (live code at origin/phase-3 c44b6b6a):
- Contract drift: W§7.3 says "Each writer checks `snapshot.pending` after it releases `lock`". That check is `zygo_workspace::snapshot::after_write(paths: &Paths, mux: &dyn Multiplexer, ws_id: &str, wait: Duration)` (`crates/zygo-workspace/src/snapshot.rs:796`) and needs a tmux program. The Produces signature has none. The plan adds `tmux: &Path` after `paths`, as `forget_pane` has it (`crates/zygo-workspace/src/admin.rs:766-768`). No node consumes `rename_member` (`grep -rn rename_member docs/work/phase3` finds only this file).
- `valid_member_name` (ML-1) is `zygo_core::ids::valid_member_name` (`crates/zygo-core/src/ids.rs:29`); it refuses `all`, `cli`, `zygo` and the pane-key form (`ids.rs:21-24`).
- The name-holder rule of ML-7, `name_holder` (`crates/zygo/src/cmd/member.rs:834`), is `pub(crate)` in the `zygo` crate, so `admin.rs` cannot call it. The same rule (another key's record whose valid name equals the name; a `panes[]` entry holds none) is `zygo_workspace::mail::names::members(ws: &Workspace) -> Vec<Target>` (`crates/zygo-workspace/src/mail/names.rs:16`) filtered by key and name. The plan uses that.
- The workspace lookup (`--name`, else the checkout of `cwd`) is inline in `forget_pane_run` (`admin.rs:787-812`). The plan moves it into one function that both verbs call. Its no-folder line `NO_FOLDER` (`admin.rs:34`) says `pass --name`; `ws name` takes `--workspace`, so the function takes that line as an argument.
- `Store::update_existing` (`crates/zygo-workspace/src/state.rs:552`) is the read-change-write under `lock` that makes no folder; `snapshot::OP_WAIT` (`snapshot.rs:52`) is the 5 s bound that `forget_pane` uses (`admin.rs:843`).
- The snapshot copies `Pane.name` from the member record under `lock` (`snapshot.rs:343-347`), so a rename does not change `snapshot_seq`.
- `mail/<key>/` is `mail_dir(paths, id).join(key)` (`crates/zygo-workspace/src/mail/mod.rs:105`); the rename does not touch it.
- Not in scope: the `name` in a live member's session view marker (ML-7) keeps the old value until the next start; no spec text asks `ws name` to change it.

Author's calls (the spec does not decide them):
- Check order: pane key (AU84), then name grammar (M§6), then the workspace lookup, then under `lock` the member record, then the name holder. Each refusal is exit 1, one line on `err`, nothing written.
- Lines: `zygo: invalid pane key <key>` (as `forget-pane`); `zygo: bad name `<n>`: use ^[a-z][a-z0-9-]{0,31}$, not all, cli, zygo or a pane key` (`<n>` through `zygo_core::esc`, as ML-7's `--name` line); `zygo: <ws> has no member record for pane <key>` (as `forget-pane`); `zygo: name <n> belongs to pane <holder>; nothing changed`; `zygo: workspace <ws> is busy; try again` (lock not taken in 5 s); `zygo: no workspace for this folder; run in the workspace's checkout or pass --workspace`. Success: `zygo: pane <key> of <ws> is now named <n>` on `out`, exit 0.
- No `op.lock`: the spec asks only for `lock` (graph review focus 57, W§7.3).

Interfaces

Consumes (exact, as in the code now):
- `zygo_workspace::state::MemberRecord.name: Option<String>` (`state.rs:128-130`, `#[serde(default, deserialize_with = "lenient_name", skip_serializing_if = "Option::is_none")]`) (ML-3)
- `zygo_workspace::state::Pane.name: Option<String>` (`state.rs:91-93`, same attributes) (ML-3)
- `zygo_core::ids::valid_member_name(s: &str) -> bool` (`ids.rs:29`) (ML-1)
- Existing code used: `zygo_workspace::mail::names::members(ws: &Workspace) -> Vec<Target>` (`names.rs:16`); `Store::update_existing(&self, bound: Duration, f: &mut dyn FnMut(&mut Workspace) -> anyhow::Result<()>) -> Result<Option<Workspace>, StateError>` (`state.rs:552`); `snapshot::after_write` (`snapshot.rs:796`); `snapshot::OP_WAIT` (`snapshot.rs:52`); `find` (`admin.rs:528`); `cmd::tmux_program() -> PathBuf` (`crates/zygo/src/cmd/mod.rs:109`); `cmd::cwd_or_exit`.

Produces (exact):
- `zygo_workspace::admin::rename_member(paths: &Paths, tmux: &Path, workspace: Option<&str>, cwd: &Path, key: &str, new_name: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32` (contract drift: `tmux` added, see above)
- `cmd::ws::WsCmd::Name { key: String, name: String, workspace: Option<String> }` (`#[arg(long)] workspace`)
- Support (no node consumes it): `fn pane_workspace(paths: &Paths, name: Option<&str>, cwd: &Path, no_folder: &str) -> Result<Workspace, String>` in `admin.rs`

Review focus: graph line 56 (reserved names refused) is step 1 test `rename_member_refuses_bad_values_and_changes_nothing`; graph line 57 (`ws name` under `lock`, AU78) is `rename_member_waits_for_the_state_lock` and the `extra` asserts of `rename_member_changes_the_record_and_the_pane_in_one_write`.

Steps:

- [ ] 1. In `crates/zygo-workspace/tests/admin.rs`, add `use zygo_workspace::mail::mail_dir;` to the imports and, at the end of the file, these helpers and tests:
  ```rust
  // ---- ML-8: `zygo ws name` ----

  fn name_run(p: &Paths, tmux: &Path, ws: Option<&str>, cwd: &Path, key: &str, name: &str) -> (i32, String, String) {
      let (mut out, mut err) = (Vec::new(), Vec::new());
      let code = admin::rename_member(p, tmux, ws, cwd, key, name, &mut out, &mut err);
      (code, text(out), text(err))
  }

  /// Workspace `app` (id `ID`, phase down): `kaaaaaaaa` named `old` with a pane, `kbbbbbbbb` named `bob` with no
  /// pane, a shell pane `kcccccccc` with no record; record and pane of `kaaaaaaaa` carry an unknown field (AU78).
  fn name_setup(t: &Path, id: &str, root: &Path) -> (Paths, PathBuf, Store) {
      let p = paths(t);
      let tmux = answer_tmux(t);
      let store = ws_state(&p, id, "app", root, |ws| {
          let mut a = member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"));
          a.name = Some("old".into());
          a.extra.insert("future".into(), json!(1));
          let mut b = member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2"));
          b.name = Some("bob".into());
          ws.members.insert("kaaaaaaaa".into(), a);
          ws.members.insert("kbbbbbbbb".into(), b);
          let mut pa = pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, root);
          pa.name = Some("old".into());
          pa.extra.insert("future".into(), json!(2));
          ws.panes = vec![pa, pane("kcccccccc", 0, 1, PaneKind::Shell, root)];
      });
      (p, tmux, store)
  }

  #[test] // M§6: the record and the panes[] entry in one write; mail stays in mail/<key>/; W§7.3 pending check; AU78
  fn rename_member_changes_the_record_and_the_pane_in_one_write() {
      let _s = serial();
      let td = TempDir::new().unwrap();
      let (p, tmux, store) = name_setup(td.path(), ID, &td.path().join("r/app"));
      let inbox = mail_dir(&p, ID).join("kaaaaaaaa/new");
      fs::create_dir_all(&inbox).unwrap();
      fs::write(inbox.join("m1.json"), "{}").unwrap();
      fs::write(store.dir().join("snapshot.pending"), "").unwrap();
      assert_eq!(
          name_run(&p, &tmux, Some("app"), Path::new(""), "kaaaaaaaa", "elena"),
          (0, "zygo: pane kaaaaaaaa of app is now named elena\n".to_string(), String::new())
      );
      let ws = store.read().unwrap().unwrap();
      let a = &ws.members["kaaaaaaaa"];
      assert_eq!((a.name.as_deref(), a.extra.get("future")), (Some("elena"), Some(&json!(1))));
      assert_eq!((ws.panes[0].name.as_deref(), ws.panes[0].extra.get("future")), (Some("elena"), Some(&json!(2))));
      assert_eq!(ws.members["kbbbbbbbb"].name.as_deref(), Some("bob"));
      assert!(inbox.join("m1.json").exists());
      assert!(!mail_dir(&p, ID).join("elena").exists());
      assert!(!store.dir().join("snapshot.pending").exists()); // W§7.3 check ran
      assert_eq!(
          name_run(&p, &tmux, Some("app"), Path::new(""), "kbbbbbbbb", "ops"),
          (0, "zygo: pane kbbbbbbbb of app is now named ops\n".to_string(), String::new())
      ); // a member with no panes[] entry: the record only
      let ws = store.read().unwrap().unwrap();
      assert_eq!(ws.members["kbbbbbbbb"].name.as_deref(), Some("ops"));
      assert_eq!((ws.panes.len(), ws.panes[1].name.as_deref()), (2, None));
  }

  #[test] // W§7.3, graph review focus 57: the write waits for `lock`
  fn rename_member_waits_for_the_state_lock() {
      let _s = serial();
      let td = TempDir::new().unwrap();
      let (p, tmux, store) = name_setup(td.path(), ID, &td.path().join("r/app"));
      let held = hold(store.dir().join("lock"));
      let t0 = Instant::now();
      let h = std::thread::spawn(move || {
          std::thread::sleep(Duration::from_millis(300));
          drop(held);
      });
      assert_eq!(name_run(&p, &tmux, Some("app"), Path::new(""), "kaaaaaaaa", "elena").0, 0);
      assert!(t0.elapsed() >= Duration::from_millis(300));
      h.join().unwrap();
      assert_eq!(store.read().unwrap().unwrap().members["kaaaaaaaa"].name.as_deref(), Some("elena"));
  }

  #[test] // M§6, AU84, graph review focus 56: bad key, bad or reserved name, a held name: exit 1, one line, no change
  fn rename_member_refuses_bad_values_and_changes_nothing() {
      let _s = serial();
      let td = TempDir::new().unwrap();
      let (p, tmux, _store) = name_setup(td.path(), ID, &td.path().join("r/app"));
      let before = state_bytes(&p, ID);
      let bad = |n: &str| format!("zygo: bad name `{n}`: use ^[a-z][a-z0-9-]{{0,31}}$, not all, cli, zygo or a pane key\n");
      let long = "a".repeat(33);
      for (key, name, line) in [
          ("k$", "elena", "zygo: invalid pane key k$\n".to_string()),
          ("kaaaaaaaa", "Elena", bad("Elena")),
          ("kaaaaaaaa", "", bad("")),
          ("kaaaaaaaa", "9lives", bad("9lives")),
          ("kaaaaaaaa", long.as_str(), bad(&long)),
          ("kaaaaaaaa", "a\u{1b}", bad("a\\u{1b}")),
          ("kaaaaaaaa", "all", bad("all")),
          ("kaaaaaaaa", "cli", bad("cli")),
          ("kaaaaaaaa", "zygo", bad("zygo")),
          ("kaaaaaaaa", "kbbbbbbbb", bad("kbbbbbbbb")),
          ("kaaaaaaaa", "bob", "zygo: name bob belongs to pane kbbbbbbbb; nothing changed\n".to_string()),
      ] {
          assert_eq!(name_run(&p, &tmux, Some("app"), Path::new(""), key, name), (1, String::new(), line), "{key} {name:?}");
          assert_eq!(state_bytes(&p, ID), before, "{key} {name:?}");
      }
  }

  #[test] // M§6: a key with no member record, a panes[] entry with no record, an unknown workspace: exit 1, no change
  fn rename_member_unknown_key_or_workspace_exits_1() {
      let _s = serial();
      let td = TempDir::new().unwrap();
      let (p, tmux, _store) = name_setup(td.path(), ID, &td.path().join("r/app"));
      let before = state_bytes(&p, ID);
      for (ws, key, line) in [
          ("app", "kzzzzzzzz", "zygo: app has no member record for pane kzzzzzzzz\n"),
          ("app", "kcccccccc", "zygo: app has no member record for pane kcccccccc\n"),
          ("nosuch", "kaaaaaaaa", "zygo: no workspace named nosuch\n"),
      ] {
          assert_eq!(name_run(&p, &tmux, Some(ws), Path::new(""), key, "elena"), (1, String::new(), line.to_string()), "{ws} {key}");
          assert_eq!(state_bytes(&p, ID), before);
      }
  }

  #[test] // M§6, W§12: no --workspace: the workspace of the checkout of cwd
  #[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
  fn rename_member_by_the_checkout_folder() {
      let _s = serial();
      let td = TempDir::new().unwrap();
      let t = td.path();
      let r = canon(&repo(t, "app"));
      let id = workspace_id(&r.join(".git"));
      let (p, tmux, store) = name_setup(t, &id, &r);
      assert_eq!(
          name_run(&p, &tmux, None, &r, "kaaaaaaaa", "elena"),
          (0, "zygo: pane kaaaaaaaa of app is now named elena\n".to_string(), String::new())
      );
      assert_eq!(store.read().unwrap().unwrap().members["kaaaaaaaa"].name.as_deref(), Some("elena"));
      let plain = t.join("plain");
      fs::create_dir_all(&plain).unwrap();
      assert_eq!(
          name_run(&p, &tmux, None, &plain, "kaaaaaaaa", "ops"),
          (1, String::new(), "zygo: no workspace for this folder; run in the workspace's checkout or pass --workspace\n".to_string())
      );
  }
  ```
- [ ] 2. Run `cargo test -p zygo-workspace --test admin -- rename_member`. Check: build fails with E0425, no function `rename_member` in `admin`.
- [ ] 3. In `crates/zygo-workspace/src/admin.rs`, move the block `let ws = match name { … };` (`admin.rs:787-812`) into `fn pane_workspace(paths: &Paths, name: Option<&str>, cwd: &Path, no_folder: &str) -> Result<Workspace, String>`, with its own `not_found` and `link` closures as at `admin.rs:785-786` and `no_folder` in place of `NO_FOLDER`. In `forget_pane_run` replace the block with `let ws = pane_workspace(paths, name, cwd, NO_FOLDER)?;` and keep `not_found` and `link` (used at `admin.rs:820-821`). Check: `cargo build -p zygo-workspace` passes; `forget_pane_run` gives the same lines.
- [ ] 4. In `admin.rs`, add `use zygo_core::ids::valid_member_name;`, `use crate::mail::names;`, the const `NO_FOLDER_WS: &str = "zygo: no workspace for this folder; run in the workspace's checkout or pass --workspace";`, and `pub fn rename_member(paths: &Paths, tmux: &Path, workspace: Option<&str>, cwd: &Path, key: &str, new_name: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32`, doc `/// zygo ws name <key> <name> (M§6, W§7.3)`. It prints the `Ok` line on `out` (exit 0) or the `Err` line on `err` (exit 1), as `forget_pane` (`admin.rs:766-779`). The body of its `fn rename_run(..) -> Result<String, String>`:
  1. `!valid_pane_key(key)`: `Err("zygo: invalid pane key <esc(key)>")`.
  2. `!valid_member_name(new_name)`: `Err` of the bad-name line of the Author's calls.
  3. `let ws = pane_workspace(paths, workspace, cwd, NO_FOLDER_WS)?;` `label = ws.name`.
  4. `Store::open(paths, &ws.id).update_existing(snapshot::OP_WAIT, ..)`; in the closure: no `c.members[key]` → `bail!("zygo: {label} has no member record for pane {key}")`; `names::members(c)` has a target with `t.key != key` and `t.name == Some(new_name)` → `bail!("zygo: name {new_name} belongs to pane {}; nothing changed", t.key)`; else set `c.members[key].name` and the `name` of the `c.panes` entry with `p.key == key` (when there is one) to `Some(new_name)`.
  5. `Ok(Some(w))`: `snapshot::after_write(paths, &Tmux::new(tmux.to_path_buf(), w.tmux_socket.clone()), &ws.id, snapshot::OP_WAIT)`, result ignored as at `admin.rs:866`; `Ok("zygo: pane {key} of {label} is now named {new_name}")`. `Ok(None)`: `Err("zygo: no workspace named <esc(label)>")`. `Err(StateError::Refused(e))`: `Err(e.to_string())`. `Err(StateError::Busy)`: `Err("zygo: workspace {label} is busy; try again")`. Other `Err(e)`: `Err("zygo: {e}")`.
  Check: `cargo build -p zygo-workspace` passes.
- [ ] 5. Run `cargo test -p zygo-workspace --test admin -- rename_member --include-ignored`. Check: 5 passed, 0 failed.
- [ ] 6. Run `cargo test -p zygo-workspace --test admin -- forget_pane --include-ignored`. Check: all pass (the move of step 3 changed no `forget-pane` line).
- [ ] 7. Commit `crates/zygo-workspace/src/admin.rs` and `crates/zygo-workspace/tests/admin.rs`, message `ML-8: zygo_workspace::admin::rename_member` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:ML-8:implement`, each on its own line. Check: `git show --stat HEAD` lists those two files.
- [ ] 8. In the `tests` module of `crates/zygo/src/cmd/ws.rs` (`ws.rs:485`), add:
  ```rust
  #[test] // M§6: `zygo ws name <key> <name> [--workspace <w>]`
  fn ws_name_parses_key_name_and_workspace() {
      use clap::Parser;
      use super::super::{Cli, Cmd};
      let Cmd::Ws(WsArgs { sub: WsCmd::Name { key, name, workspace } }) =
          Cli::try_parse_from(["zygo", "ws", "name", "kaaaaaaaa", "elena", "--workspace", "app"]).unwrap().command
      else { panic!() };
      assert_eq!((key.as_str(), name.as_str(), workspace.as_deref()), ("kaaaaaaaa", "elena", Some("app")));
      let Cmd::Ws(WsArgs { sub: WsCmd::Name { workspace, .. } }) =
          Cli::try_parse_from(["zygo", "ws", "name", "kaaaaaaaa", "elena"]).unwrap().command
      else { panic!() };
      assert_eq!(workspace, None);
      assert!(Cli::try_parse_from(["zygo", "ws", "name", "kaaaaaaaa"]).is_err());
  }
  ```
- [ ] 9. Run `cargo test -p zygo --lib ws_name_parses`. Check: build fails with E0599, no variant `Name` on `WsCmd`.
- [ ] 10. In `ws.rs`, add to `WsCmd` after `ForgetPane` (`ws.rs:56-61`): `/// Name one member of a workspace; mail stays in its pane-key folder` `Name { key: String, name: String, /// Workspace name; default: the workspace of this folder  #[arg(long)] workspace: Option<String> }`. In `dispatch` add the arm `WsCmd::Name { key, name, workspace }`: `cwd` as the `ForgetPane` arm does it (`ws.rs:82-89`, keyed on `workspace`), then `admin::rename_member(&paths, &tmux_program(), workspace.as_deref(), &cwd, &key, &name, &mut std::io::stdout().lock(), &mut std::io::stderr().lock())`. Add `zygo ws name <key> <name> [--workspace <w>]` to the module doc at `ws.rs:1`. Check: `cargo build -p zygo` passes.
- [ ] 11. Run the Acceptance: `test "$(cargo test -q -p zygo-workspace --test admin -- rename_member --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-workspace --test admin && cargo test -p zygo --lib`. Check: the count is 5; all pass, `ws_name_parses_key_name_and_workspace` among them.
- [ ] 12. Commit `crates/zygo/src/cmd/ws.rs`, message `ML-8: zygo ws name` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:ML-8:implement`, each on its own line. Check: `git show --stat HEAD` lists that one file.
