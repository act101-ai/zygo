# ML-5 — Multiplexer operations for the wake: literal keys and Enter

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C5, M§7.3 step 2; `docs/specs/2026-11-20-workspaces-design.md` W-C3, W-C8, AU62
**Files:** `crates/zygo-workspace/src/mux/mod.rs`, `crates/zygo-workspace/src/mux/tmux.rs`, `crates/zygo-workspace/tests/mux.rs`, `crates/zygo-workspace/tests/snapshot.rs`, `crates/zygo-workspace/tests/admin.rs`
**Consumes:** —
**Produces:** `zygo_workspace::mux::Multiplexer::send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError>` (`send-keys -t <pane> -l -- <text>`; a pane id `%<digits>` only; a text with a control character refused before any spawn), `zygo_workspace::mux::Multiplexer::send_enter(&self, pane: &str) -> Result<(), MuxError>` (`send-keys -t <pane> Enter`)
**Acceptance:** `test "$(cargo test -q -p zygo-workspace --test mux -- send_keys --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-workspace --test mux --test snapshot --test admin -- --include-ignored` → every count holds (at least 3 named `send_keys…` in zygo-workspace `mux`; a count of 0 fails before any run), then all pass (tmux cases run on tmux 3.2 or later, else print the skip reason; `cfg(unix)`): on a private server the literal text appears in the pane once and `send_enter` submits it; both calls pass `-S <socket>`; a target that is not `%<digits>` and a text with `\n` or `\x1b` are refused with no tmux spawn; the test fakes (`tests/snapshot.rs`, `tests/admin.rs`) implement the two operations

## Plan

Premise check (live code at origin/phase-3, 3ae083c7; this branch has it):
- The trait `zygo_workspace::mux::Multiplexer` is at `crates/zygo-workspace/src/mux/mod.rs:308-384`; its last method is `switch_client` (`mod.rs:383`). It has no send-keys operation. `MuxError` (`mod.rs:403-416`, enum `Gone`, `NoTarget`, `Failed`, `TimedOut`, `Spawn`) already carries the refusal kind: every refusal before a spawn is `MuxError::Failed { message }` (the trait doc, `mod.rs:297-300`; `check_window_name` with `CONTROL_NAME_MESSAGE`, `mod.rs:256-265`). No new `MuxError` variant is needed.
- The tmux impl `Tmux` (`src/mux/tmux.rs:197-258`) builds each call with `Tmux::command` (`tmux.rs:210-223`): the program, `-S <socket>` when a socket is set (AU62), `-u`, and `LC_ALL=C`. `Tmux::run(&self, args: &[&str]) -> Result<(), MuxError>` (`tmux.rs:252`) is the bounded call whose output is not used; a pane id names no session, so a pane operation keeps `NoTarget` (`mod.rs:301-304`). The new calls use `Tmux::run`.
- No pane-id check exists in `mux`. The rule `^%[0-9]+$` exists as the private `snapshot::valid_pane_id` (`src/snapshot.rs:254-257`), outside this node's Files. Two other copies, `pane_id` in `src/down.rs:471` and `crates/zygo/src/cmd/member.rs:674`, take 1 to 10 digits (W§8.4 `$TMUX_PANE`). The spec value for this node is `%<digits>`, so the new check is `^%[0-9]+$`, as `snapshot.rs:255`.
- The existing "no spawn" check: a `Tmux` on the log stub `log_stub(dir)` (`tests/mux.rs:221-230`), which appends one record per call to `<dir>/log`; a refusal test then asserts `!dir.path().join("log").exists()` (`tests/mux.rs:499`, `:518`, `:536`). These refusal tests are not `#[ignore]`. The new refusal tests use the same seam.
- The existing `-S` check: `every_call_passes_the_socket_the_c_locale_and_u` (`tests/mux.rs:242-318`, `#[ignore = "timing: stub tmux under the fixed 2 s tmux bound"]`) on the log stub with socket `/tmp/zygo-mux-x/s`; each record is `LC_ALL=C -S /tmp/zygo-mux-x/s -u <arguments>`.
- The live harness: `require_tmux()` (`src/mux/mod.rs:59-69`) gives `None` and writes `tmux tests skipped: <reason>` below tmux 3.2 (`MIN_VERSION`, `tmux_gate`, `mod.rs:21-34`); `PrivateServer::start()` (`mod.rs:95`) makes `/tmp/zygo-mux-<pid>-<n>-<hex>/s` and a wrapper; `srv.tmux()` is `Tmux::new(wrapper, Some(socket))`; `srv.run(&[..])` runs the real tmux on the socket. Each live test holds `forks()` (`tests/mux.rs:30-33`), returns at `let Some(_) = require_tmux() else { return };`, and has `#[ignore = "timing: real tmux under the fixed 2 s tmux bound"]`. The helpers `env()`, `os()` (`tests/mux.rs:617`, `:240`) and `until(f)` (5 s poll, `tests/mux.rs:1312-1321`) exist.
- Imports in `tests/mux.rs`: `MuxError` is imported in every build (`:22-26`); `Tmux`, `Multiplexer`, `window_target`, `NewSession` are under `cfg(unix)` (`:19-21`).
- The trait has exactly three implementations in the repository: `Tmux` (`src/mux/tmux.rs:282`), `Fake` (`tests/snapshot.rs:151-216`, unscripted calls go through `self.other("<name>")`) and `ListFake` (`tests/admin.rs:284-321`, unscripted calls go through `self.other()`). No implementation in another crate.
- Downstream: ML-6 Consumes `send_keys_literal` and `send_enter` and calls them through `mux: &dyn Multiplexer` (ML-6 Produces `wake(..)`); the two signatures below match. ML-8's Consumes names no ML-5 item; its edge on ML-5 (`docs/work/phase3/graph.md:225`) is the shared file `tests/admin.rs`, whose `ListFake` this node changes. No change to either node.

Author's calls (the spec text does not decide them):
- The control-character rule is `char::is_control` (C0 including `\n`, `\r`, `\t` and `\x1b`, DEL, C1), the same rule as `check_window_name` (`mod.rs:260`). Refusal text: the new `pub const CONTROL_TEXT_MESSAGE: &str = "send-keys text holds a control character"`. A refused pane gives `Failed` with `invalid tmux pane id: {pane:?}`, the form of `check_session` (`mod.rs:283`).
- `send_keys_literal` checks the pane first, then the text. An empty text is not refused: the spec states no such rule.
- `every_call_passes_the_socket_the_c_locale_and_u` stays unchanged; the `-S` check of the two new calls is its own `send_keys…` test on the same stub and socket, so the Acceptance count covers it.
- The live test uses a pane program `sh -c 'read -r l; printf '%s' "$l" > <dir>/got; sleep 600'`: the tty echo puts the typed text in the pane once, `got` stays absent until Enter, and `got` then holds the text. The text is the M§7.3 step 2 wake text.

Consumes: — (ML-3 has landed; no ML-3 item is used.) Existing code used:
- `zygo_workspace::mux::{Multiplexer, MuxError}` (`src/mux/mod.rs:308`, `:404`)
- `Tmux::run(&self, args: &[&str]) -> Result<(), MuxError>` (`src/mux/tmux.rs:252`, private)
- `zygo_workspace::mux::testing::{require_tmux, PrivateServer}` (`src/mux/mod.rs:59`, `:95`; feature `test-hooks`, on for this crate's tests, `Cargo.toml:30`)

Produces (exact):
- `zygo_workspace::mux::Multiplexer::send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError>` (`send-keys -t <pane> -l -- <text>`; a pane id `%<digits>` only; a text with a control character refused before any spawn)
- `zygo_workspace::mux::Multiplexer::send_enter(&self, pane: &str) -> Result<(), MuxError>` (`send-keys -t <pane> Enter`)
- Support in `src/mux/mod.rs` (not in Consumes of any node): `pub const CONTROL_TEXT_MESSAGE: &str = "send-keys text holds a control character";`, `pub(crate) fn check_pane(pane: &str) -> Result<(), MuxError>`, `pub(crate) fn check_keys_text(text: &str) -> Result<(), MuxError>`

Steps:

- [x] 1. In `crates/zygo-workspace/tests/mux.rs`, add `CONTROL_TEXT_MESSAGE` to the `cfg(unix)` import at `:21` (`use zygo_workspace::mux::{window_target, HookScope, Multiplexer, NewSession, NewWindow, SessionAnswer, Split, CONTROL_TEXT_MESSAGE};`), add the wake text after `stub` (`:42`):
  ```rust
  /// The wake text of M§7.3 step 2.
  #[cfg(unix)]
  const WAKE: &str = "zygo: you have mail; read it with mail_read";
  ```
  and add two refusal tests after `control_characters_in_window_names_are_refused_before_any_spawn` (`:537`):
  ```rust
  #[cfg(unix)]
  #[test]
  fn send_keys_refuse_a_target_that_is_not_a_pane_id_before_any_spawn() {
      // M-C5, W-C8: the target is `%<digits>` only
      let _g = forks();
      let dir = tempfile::tempdir().unwrap();
      let t = Tmux::new(log_stub(dir.path()), Some(PathBuf::from("/tmp/zygo-mux-x/s")));
      for p in ["", "%", "1", "%1a", "%-1", "% 1", " %1", "%1 ", "%1\n", "=zygo-t:0", "zygo-t", "-t", "%\u{ff11}"] {
          let want = Err(MuxError::Failed { message: format!("invalid tmux pane id: {p:?}") });
          assert_eq!(t.send_keys_literal(p, WAKE), want, "{p:?}");
          assert_eq!(t.send_enter(p), want, "{p:?}");
      }
      assert_eq!(t.send_keys_literal("=zygo-t:0", "a\nb"), Err(MuxError::Failed { message: "invalid tmux pane id: \"=zygo-t:0\"".into() }));
      assert!(!dir.path().join("log").exists());
  }

  #[cfg(unix)]
  #[test]
  fn send_keys_refuse_control_characters_before_any_spawn() {
      // M-C5: no newline, escape or other control character reaches send-keys
      let _g = forks();
      let dir = tempfile::tempdir().unwrap();
      let t = Tmux::new(log_stub(dir.path()), Some(PathBuf::from("/tmp/zygo-mux-x/s")));
      for text in ["a\nb", "\n", "a\rb", "\u{1b}[31m", "\u{1b}", "a\tb", "nul\u{0}", "\u{3}", "del\u{7f}", "c1\u{85}", "zygo: you have mail\n"] {
          assert_eq!(
              t.send_keys_literal("%1", text),
              Err(MuxError::Failed { message: CONTROL_TEXT_MESSAGE.into() }),
              "{text:?}"
          );
      }
      assert_eq!(CONTROL_TEXT_MESSAGE, "send-keys text holds a control character");
      assert!(!dir.path().join("log").exists());
  }
  ```
  Check: the file holds both tests; `grep -c '^fn send_keys' crates/zygo-workspace/tests/mux.rs` gives 2.

- [x] 2. In the same file, add the stub test after `no_socket_means_no_s_flag` (`:334`) and the live test after `missing_targets` (`:987`):
  ```rust
  #[cfg(unix)]
  #[test]
  #[ignore = "timing: stub tmux under the fixed 2 s tmux bound"]
  fn send_keys_pass_the_socket_and_the_literal_form() {
      // AU62, M§7.3 step 2: two calls, each with -S <socket>
      let _g = forks();
      let dir = tempfile::tempdir().unwrap();
      let t = Tmux::new(log_stub(dir.path()), Some(PathBuf::from("/tmp/zygo-mux-x/s")));
      assert_eq!(t.send_keys_literal("%1", WAKE), Ok(()));
      assert_eq!(t.send_enter("%1"), Ok(()));
      assert_eq!(t.send_keys_literal("%12", "Enter"), Ok(()));
      assert_eq!(
          log_lines(dir.path()),
          [
              format!("LC_ALL=C -S /tmp/zygo-mux-x/s -u send-keys -t %1 -l -- {WAKE}"),
              "LC_ALL=C -S /tmp/zygo-mux-x/s -u send-keys -t %1 Enter".to_string(),
              "LC_ALL=C -S /tmp/zygo-mux-x/s -u send-keys -t %12 -l -- Enter".to_string(),
          ]
      );
  }

  #[cfg(unix)]
  #[test]
  #[ignore = "timing: real tmux under the fixed 2 s tmux bound"]
  fn send_keys_type_the_text_once_and_enter_submits_it() {
      // M§7.3 step 2 on a private server: the literal text reaches the pane once and is not submitted; Enter submits it
      let _g = forks();
      let Some(_) = require_tmux() else { return };
      let dir = tempfile::tempdir().unwrap();
      let got = dir.path().join("got");
      let srv = PrivateServer::start();
      let t = srv.tmux();
      let script = format!("read -r l; printf '%s' \"$l\" > '{}'; sleep 600", got.display());
      let argv = vec![OsString::from("sh"), OsString::from("-c"), OsString::from(script)];
      let w0 = t
          .new_session(&NewSession { name: "zygo-t", start_dir: dir.path(), width: 100, height: 30, env: &env(), argv: &argv, owner: None })
          .unwrap();
      let pane = t.display(&window_target("zygo-t", w0), "#{pane_id}").unwrap();
      assert!(pane.starts_with('%'), "{pane}");
      let screen = || srv.run(&["capture-pane", "-p", "-t", &pane]).unwrap();
      t.send_keys_literal(&pane, WAKE).unwrap();
      assert!(until(|| screen().contains(WAKE)), "{}", screen());
      assert_eq!(screen().matches(WAKE).count(), 1, "{}", screen());
      assert!(!got.exists(), "the literal call submitted the line");
      t.send_enter(&pane).unwrap();
      assert!(until(|| fs::read_to_string(&got).is_ok_and(|s| s == WAKE)), "{:?}", fs::read_to_string(&got));
      assert_eq!(screen().matches(WAKE).count(), 1, "{}", screen());
      assert!(matches!(t.send_enter("%99999"), Err(MuxError::NoTarget { .. })));
  }
  ```
  Check: `grep -c '^fn send_keys' crates/zygo-workspace/tests/mux.rs` gives 4.

- [x] 3. Run `cargo test -p zygo-workspace --test mux -- send_keys --include-ignored`. Check: it fails to compile with E0599 `no method named send_keys_literal` / `send_enter` for `Tmux`, and E0432 for `CONTROL_TEXT_MESSAGE`.

- [x] 4. In `crates/zygo-workspace/src/mux/mod.rs`, after `check_window_name` (`:265`), add:
  ```rust
  /// The refusal of a send-keys text that holds a control character (M-C5, W-C8): a newline would submit the line and
  /// an escape would reach the terminal.
  pub const CONTROL_TEXT_MESSAGE: &str = "send-keys text holds a control character";

  /// `Err(Failed)` with [`CONTROL_TEXT_MESSAGE`] when `text` holds a character for which `char::is_control` is true.
  pub(crate) fn check_keys_text(text: &str) -> Result<(), MuxError>

  /// `Err(Failed)` with `invalid tmux pane id: {pane:?}` unless `pane` is `%` and one or more ASCII digits (M-C5), so
  /// a target holds no session, window, flag or format.
  pub(crate) fn check_pane(pane: &str) -> Result<(), MuxError>
  ```
  with the bodies `if text.chars().any(char::is_control) { return Err(MuxError::Failed { message: CONTROL_TEXT_MESSAGE.into() }); } Ok(())` and `if !pane.strip_prefix('%').is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())) { return Err(MuxError::Failed { message: format!("invalid tmux pane id: {pane:?}") }); } Ok(())`. In the trait, after `switch_client` (`:383`), add:
  ```rust
  /// `send-keys -t <pane> -l -- <text>`: types `text` literally into the pane (M§7.3 step 2). A pane that is not
  /// `%<digits>` or a text with a control character gives `Failed` before any spawn (M-C5). A missing pane: `NoTarget`.
  fn send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError>;
  /// `send-keys -t <pane> Enter` (M§7.3 step 2). A pane that is not `%<digits>` gives `Failed` before any spawn. A
  /// missing pane: `NoTarget`.
  fn send_enter(&self, pane: &str) -> Result<(), MuxError>;
  ```
  Check: `cargo build -p zygo-workspace` fails only with E0046 (missing `send_keys_literal`, `send_enter`) in `impl Multiplexer for Tmux`.

- [x] 5. In `crates/zygo-workspace/src/mux/tmux.rs`, add `check_keys_text, check_pane` to the `use super::{..}` list (`:16-20`), and after `switch_client` (`:507-510`) in `impl Multiplexer for Tmux` add:
  ```rust
  fn send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError> {
      check_pane(pane)?;
      check_keys_text(text)?;
      self.run(&["send-keys", "-t", pane, "-l", "--", text])
  }

  fn send_enter(&self, pane: &str) -> Result<(), MuxError> {
      check_pane(pane)?;
      self.run(&["send-keys", "-t", pane, "Enter"])
  }
  ```
  Check: `cargo build -p zygo-workspace` succeeds with no warning.

- [x] 6. Implement the two operations in the test fakes. In `crates/zygo-workspace/tests/snapshot.rs`, after `switch_client` (`:215`) in `impl Multiplexer for Fake`: `fn send_keys_literal(&self, _: &str, _: &str) -> Result<(), MuxError> { self.other("send_keys_literal") }` and `fn send_enter(&self, _: &str) -> Result<(), MuxError> { self.other("send_enter") }`. In `crates/zygo-workspace/tests/admin.rs`, after `switch_client` (`:320`) in `impl Multiplexer for ListFake`: `fn send_keys_literal(&self, _: &str, _: &str) -> Result<(), MuxError> { self.other() }` and `fn send_enter(&self, _: &str) -> Result<(), MuxError> { self.other() }`. Check: `cargo test -p zygo-workspace --no-run` compiles every test target.

- [x] 7. Run `cargo test -p zygo-workspace --test mux -- send_keys --include-ignored`. Check: 4 passed, 0 failed; on a host without tmux 3.2 or later the live test prints `tmux tests skipped: <reason>` and returns.

- [x] 8. Run the Acceptance: `test "$(cargo test -q -p zygo-workspace --test mux -- send_keys --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-workspace --test mux --test snapshot --test admin -- --include-ignored`, then `cargo fmt --check -p zygo-workspace` and `cargo clippy -p zygo-workspace --all-targets -- -D warnings`. Check: the count is 4; all three targets pass; fmt and clippy report nothing.

- [x] 9. Commit `crates/zygo-workspace/src/mux/mod.rs`, `src/mux/tmux.rs`, `tests/mux.rs`, `tests/snapshot.rs`, `tests/admin.rs` with the message `ML-5: send_keys_literal and send_enter` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:ML-5:implement`, each on its own line. Check: `git show --stat HEAD` lists those five files only.

Fix round 1 (review findings F1, F4, F5):

- [x] F1. `send_keys_literal` sends a final `;` as `\;`: tmux 3.8 reads an argument that ends in `;` as a command separator and removes one `\` before a final `;`, so one added `\` gives the exact text for `a;`, `a\;`, `a\;` and `;`; a `;` inside the text stays. Tests: `send_keys_escape_a_final_semicolon_only` (argv stub, not ignored) and `send_keys_type_a_final_semicolon_literally` (live).
- [x] F4a. `send_keys_type_the_text_once_and_enter_submits_it` reads a second, sentinel line after the Enter; an Enter from the literal call gives an empty second line, so the result does not depend on timing.
- [x] F4b. The send_keys argv tests use `argv_stub`, which keeps the argument boundaries; `log_stub` is unchanged.
- [x] F5. The trait docs state the typed-but-not-submitted state after a failed or timed-out Enter.
