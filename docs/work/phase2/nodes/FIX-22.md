# FIX-22 — Session-folder readers do not wait on a FIFO: activity.json and view.json

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W-C1 (C6), AU26 (an `activity.json` that cannot be read counts as busy), AU56, W§9.1; H1 plan item C9 (2026-11-22, H1 plan review round 1, controller decision 16); controller decisions of 2026-11-23 (FIX-22 plan review: premise 9, F3); operator rule of 2026-11-22: no time limit, cap or clean-up may end or delete user work. Defect: `activity::load` (`crates/zygo-workspace/src/activity.rs:85`, used by `activity::read` and `activity::update`) opens `activity.json` with `std::fs::File::open`, and `SessionView::load` (`crates/zygo-core/src/session_view.rs:166-168`) reads `view.json` with `std::fs::read`. Both follow a link and wait for a writer on a FIFO, so a FIFO at `activity.json` stops `down` (W10 reads it at each drain poll) and a FIFO at `view.json` stops every reader of the view: `session-start`, `trim`, `statusline` and the member hooks. H1 does not fix `session-start`: it keeps today's `view.json` read for launches that are not members, and membership is known only after that read. Fix: `activity::load` opens with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` on unix and needs the opened handle to be a regular file; a link or any other entry is an error (`<activity.json path>: is a link` or `<activity.json path>: is not a regular file`), which the drain counts as busy (AU26), and `activity::update` returns that error and does not replace the entry. `SessionView::load` refuses only a FIFO, the one entry that makes the read wait: it opens with `O_NONBLOCK | O_CLOEXEC` on unix, and an opened handle that is a FIFO is the error `<view.json path>: is a FIFO`, for every launch, member or not (controller decision 2026-11-23); every other entry (a regular file, a folder, a device, a socket, a link to any of them) gives today's result exactly, with today's `std::fs::read` and serde error texts. The open of `view.json` no longer waits on a device that waits at open (for example a serial line with no carrier), because the open is `O_NONBLOCK`; the read result of every entry that is not a FIFO is unchanged (review round 1, F5). A link is still followed, so no launch gets a new refusal for a link. No size cap is added anywhere. `activity::update` as H1 left it is kept: it refuses a result larger than the 1 MiB read limit, and it refuses to replace an existing `activity.json` that is over that limit or does not parse, writing nothing in each case (H1 attempt-2 review round 1, F1). On other systems the opens have no flag and the handle checks stay. Then the D6 pre-check of H1 in `crates/zygo-hooks/src/workspace.rs` (the `symlink_metadata` test before each `activity::update`) is removed, and H1's hostile-input cases keep their session-log lines through the reader's own errors. After the pre-check is removed, the link and not-a-regular-file cases come back as reader errors of `activity::update` of the persistent class (the error kind `activity::FileCondition`, H1 attempt-2 review rounds 3 and 4) and keep the once-per-turn rule: their line is written only for `prompt`, `session` and `session-end`. W§18 #89 (a non-member FIFO `view.json`) is amended by FIX-24, which depends on FIX-22.
**Files:** `crates/zygo-workspace/src/activity.rs`, `crates/zygo-workspace/tests/activity.rs`, `crates/zygo-workspace/tests/drain.rs`, `crates/zygo-core/src/session_view.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/workspace.rs`, `crates/zygo-hooks/src/session_start.rs`
**Consumes:** `activity::read` (W9), `activity::update` (W9), `workspace::run` (H1)
**Produces:** `activity::read(dir: &SessionDir) -> anyhow::Result<Activity>` and `activity::update(dir: &SessionDir, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()>` that never wait on a FIFO and never write through a link; `SessionView::load(dir: &SessionDir) -> anyhow::Result<SessionView>` that refuses a FIFO (`<view.json path>: is a FIFO`) and never waits on one, and gives today's read result for every other entry (its open no longer waits on a device that waits at open)
**Acceptance:** `cargo test -p zygo-workspace --test activity --test drain && cargo test -p zygo-core --test core_basics && cargo test -p zygo-hooks --test workspace` → all pass, including `activity_fifo_and_link_are_errors_without_a_wait` (a FIFO at `activity.json`: `read` and `update` return `Err` within a 5 s thread bound and the FIFO stays; a link: `Err`, the link target unchanged), `fifo_and_linked_activity_files_are_busy_without_a_wait` (a FIFO, a link and a dangling link at `activity.json`: the drain counts the member busy within a 5 s thread bound), `session_view_load_does_not_wait_on_a_fifo` (a FIFO at `view.json`: `Err` `<view.json path>: is a FIFO` within a 5 s thread bound; a link to a regular view still loads; a folder gives today's `Is a directory (os error 21)`; a view larger than 1 MiB still loads) and H1's `hostile_inputs_change_nothing` with the D6 pre-check removed

## Plan

Planned 2026-11-23 against the work branch at `7557a60` (H1 merged). Revised the same day for the plan review (3 major, 4 minor) and the controller decisions of 2026-11-23.

### Premise check (2026-11-23, against `7557a60`)

1. `activity::load` opens `activity.json` with `std::fs::File::open`: holds. `crates/zygo-workspace/src/activity.rs:83-89`; the open is at `:85` (the contract said `:86`; line fixed in Spec). `read` (`:153`) and `update` (`:197`) both call `load`.
2. `SessionView::load` reads with `std::fs::read`: holds, at `crates/zygo-core/src/session_view.rs:166-168` (the contract said `:129-131`; line fixed in Spec). Its errors today are the bare `std::io::Error` and `serde_json::Error`, with no path; a folder gives `Is a directory (os error 21)` from the read on Linux.
3. `session-start` keeps today's read for launches that are not members: holds. `crates/zygo-hooks/src/session_start.rs:28` (`run_in`), `:73` and `:78` (`run_member`) call `SessionView::load`. The doc comment of `run_member` says at `:60-61` that a FIFO "still waits … until FIX-22", and at `:56-57` that a `view.json` that is "a link or not a regular file" gives the session text with no note. Both become wrong or partly wrong here, so `session_start.rs` is in Files (doc comment only). No open node lists that file; H1, its last writer, is an ancestor.
4. The member hooks already read `view.json` without a wait: `workspace::read_view` (`crates/zygo-hooks/src/workspace.rs:85`) opens with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` and checks the handle. Not changed here.
5. The D6 pre-check: holds. `change_within` (`crates/zygo-hooks/src/workspace.rs:186`) runs `fs::symlink_metadata(&path)` at `:193` and writes the persistent lines `<path>: is a link; not changed` (`:195`) or `<path>: is not a regular file; not changed` (`:199`), then returns `Ok(())`; the `activity::update` call is at `:204`, and its `FileCondition` errors are persistent. The other `; not changed` texts of the file (`:401`, `:423`, member records) are not part of the pre-check.
6. `activity::FileCondition` exists (`activity.rs:171-180`) and `update` already refuses an over-limit result and an over-limit or unparsable file, writing nothing (`activity.rs:207-226`): holds.
7. `libc` is available: `crates/zygo-workspace/Cargo.toml:19` (all targets), `crates/zygo-core/Cargo.toml:16` and `crates/zygo-hooks/Cargo.toml:26` (`cfg(unix)`). `zygo-core` has no `forbid(unsafe_code)`.
8. Tests that pin today's behaviour and must change: `non_member_session_start_is_unchanged` (`crates/zygo-hooks/tests/workspace.rs:1584`) has a FIFO case that is the writer of a FIFO `view.json` (`:1612-1632`); after the change the read returns at once and that writer's open would wait forever with no reader, so the case is rewritten (step 9). `hostile_inputs_change_nothing` asserts the two pre-check texts at `:1390` and `:1400` (step 16). No test of `zygo-workspace` or `zygo-core` pins a FIFO or link at `activity.json` or `view.json`; the `activity.json` folder cases (`tests/activity.rs:71`, `:150`, `:439`; `tests/drain.rs:236`) assert only the path prefix or the error kind and still pass.
9. Spec conflict, decided (controller, 2026-11-23): W§18 #89 (`docs/specs/2026-11-20-workspaces-design.md:831`) says a non-member with a FIFO `view.json` gets the result of the code before H1, which is a wait. A FIFO `view.json` is an error for every launch, member or not; FIX-24 amends the #89 text and gains FIX-22 as a dependency (the controller adds that edge in `graph.md`). This node does not edit the spec.

### Commit procedure

Each commit step writes its message to a temporary file outside the repository (`mktemp`) and runs `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com git commit -F <file>`. The message is the subject the step gives, an empty line, then exactly these two lines and nothing after them:

```
Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
Zygo-Work: FIX-22
```

No other trailer, no attribution line, and no text that names an assistant or a co-author.

### Interfaces

Consumed, as they are in the code now:

- `zygo_workspace::activity::read` (W9): `pub fn read(dir: &SessionDir) -> anyhow::Result<Activity>` (`crates/zygo-workspace/src/activity.rs:153`).
- `zygo_workspace::activity::update` (W9): `pub fn update(dir: &SessionDir, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()>` (`activity.rs:197`).
- `zygo_hooks::workspace::run` (H1): `pub fn run(event: &str, stdin: &[u8], paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, zygo: &Path) -> anyhow::Result<Option<String>>` (`crates/zygo-hooks/src/workspace.rs`).

Produced (signatures unchanged; behaviour changed):

- `pub fn read(dir: &SessionDir) -> anyhow::Result<Activity>`
- `pub fn update(dir: &SessionDir, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()>`
- `impl SessionView { pub fn load(dir: &SessionDir) -> anyhow::Result<Self> }` (`Self` is `SessionView`)

Also used, unchanged: `activity::FileCondition` (`pub struct FileCondition(String)`, `activity.rs:172`), `activity::LOCK_BOUND`, `drain::idle(activity: Result<&Activity, &str>, status: Result<&SessionStatus, &str>, now: SystemTime) -> Idle` (`crates/zygo-workspace/src/drain.rs:83`), `Idle::Busy { reason, rule_skipped }`, `session_start::run_member(input: &HookInput, dir: &SessionDir, paths: &Paths, zygo: &Path, out: &mut dyn Write) -> anyhow::Result<HookOutput>`, `session_start::run_in(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput>`.

### Decisions

- D1 (author's call). `activity.rs` gets its own open, not `state::open_nofollow`: that helper is private to `state.rs`, which is not in Files and is in FIX-24's Files. The link test is the same as there: on unix the failed open gives `ELOOP`, or `symlink_metadata` of the path then shows a link.
- D2 (author's call). `Load` gains two variants, `Link` and `NotRegular`. A link is tested before `NotFound`, so a dangling link at `activity.json` is `Link` (an error, busy for the drain), not `Missing`. Before this node it read as the default (idle) and `update` replaced the link with a file. `activity.json` has no reader outside the workspace code, so no non-member launch sees this (controller, 2026-11-23, F3).
- D3 (contract). On other systems `activity.json` opens with no flag (a link is followed, a dangling link is `Missing`) and the regular-file check of the handle stays.
- D4 (controller, 2026-11-23, F3). `SessionView::load` refuses only a FIFO, with the text `<view.json path>: is a FIFO`. Every other entry keeps today's result exactly: the open error, the read error and the parse error go up as the bare `std::io::Error` or `serde_json::Error`, as `std::fs::read` and `serde_json::from_slice` give them today. A folder still gives `Is a directory (os error 21)` on Linux. A link is followed.
- D5 (author's call). The hooks' persistent lines take the reader's text: `<activity.json path>: is a link` and `<activity.json path>: is not a regular file`, with no `; not changed` suffix.
- D6 (author's call). A member `SessionStart` (`--event member`) with a FIFO `view.json` gets a test: the load error writes its one session-log line (`workspace hook session-start: <view.json path>: is a FIFO`) and prints nothing, which is the load-error path of `run_member` and the W§9.1 `SessionStart` row ("A member's session view that cannot be read gives one session-log line and no output").
- D7 (author's call). Three commits, so that each leaves the tree green: activity and the drain (part A); the view reader with the hooks tests that a FIFO view would otherwise hang (part B); the pre-check removal (part C).
- D8 (author's call, from D4). On unix, after the FIFO check, `SessionView::load` clears `O_NONBLOCK` on the handle (`fcntl` `F_GETFL`, then `F_SETFL` without `O_NONBLOCK`) before it reads, so a device or any other non-FIFO entry reads as it does today (a blocking read, not `EAGAIN`). A failed `fcntl` returns `std::io::Error::last_os_error()`. Opening the path a second time without the flag is not used: an entry swapped to a FIFO between the two opens would make the read wait.

### Steps

Part A: `activity.json`.

- [x] 1. Test. In `crates/zygo-workspace/tests/activity.rs` add the helper and the test below, both `#[cfg(unix)]`:

  ```rust
  #[cfg(unix)]
  fn mkfifo(path: &Path) {
      use std::os::unix::ffi::OsStrExt;
      let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
      // SAFETY: `c` is a valid NUL-terminated path.
      assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
  }

  // FIX-22: W-C1 (C6), AU26, AU56. No reader waits on a FIFO or writes through a link.
  #[cfg(unix)]
  #[test]
  fn activity_fifo_and_link_are_errors_without_a_wait() {
      use std::os::unix::fs::FileTypeExt;
      // Both calls in one thread; the answer must come within 5 s.
      let both = |d: &SessionDir| {
          let d = d.clone();
          let (tx, rx) = std::sync::mpsc::channel();
          std::thread::spawn(move || {
              let r = activity::read(&d).map_err(|e| format!("{e:#}"));
              let u = activity::update(&d, LOCK_BOUND, &mut |a| a.busy = true)
                  .map_err(|e| (e.is::<activity::FileCondition>(), format!("{e:#}")));
              let _ = tx.send((r, u));
          });
          rx.recv_timeout(Duration::from_secs(5)).expect("read or update waited")
      };
      let (t, d) = dir();
      let act = d.activity_json().display().to_string();
      let not_file = format!("{act}: is not a regular file");
      let link = format!("{act}: is a link");
      // A FIFO: an error at once; the FIFO stays.
      mkfifo(&d.activity_json());
      let (r, u) = both(&d);
      assert_eq!(r.unwrap_err(), not_file);
      assert_eq!(u.unwrap_err(), (true, not_file.clone()));
      assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_fifo());
      fs::remove_file(d.activity_json()).unwrap();
      // A link to a regular file: an error; the target and the link are not changed.
      let outside = t.path().join("outside.json");
      let body = br#"{"version":1,"busy":false,"running":{},"updated_at":1}"#;
      fs::write(&outside, body).unwrap();
      std::os::unix::fs::symlink(&outside, d.activity_json()).unwrap();
      let (r, u) = both(&d);
      assert_eq!(r.unwrap_err(), link);
      assert_eq!(u.unwrap_err(), (true, link.clone()));
      assert_eq!(fs::read(&outside).unwrap(), body.to_vec());
      assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_symlink());
      fs::remove_file(d.activity_json()).unwrap();
      // A dangling link: an error, not the default; no target is made and the link stays.
      let missing = t.path().join("missing.json");
      std::os::unix::fs::symlink(&missing, d.activity_json()).unwrap();
      let (r, u) = both(&d);
      assert_eq!(r.unwrap_err(), link);
      assert_eq!(u.unwrap_err(), (true, link.clone()));
      assert!(!missing.exists());
      assert!(fs::symlink_metadata(d.activity_json()).unwrap().file_type().is_symlink());
      fs::remove_file(d.activity_json()).unwrap();
      // A folder: not a regular file.
      fs::create_dir(d.activity_json()).unwrap();
      let (r, u) = both(&d);
      assert_eq!(r.unwrap_err(), not_file);
      assert_eq!(u.unwrap_err(), (true, not_file));
  }
  ```

- [x] 2. Test. In `crates/zygo-workspace/tests/drain.rs` add (after `unreadable_activity_file_reads_as_busy`):

  ```rust
  // FIX-22: W-C1 (C6), AU26. A FIFO, a link and a dangling link at activity.json make the member busy, and the read
  // that the drain poll makes returns at once.
  #[cfg(unix)]
  #[test]
  fn fifo_and_linked_activity_files_are_busy_without_a_wait() {
      use std::os::unix::ffi::OsStrExt;
      let t = TempDir::new().unwrap();
      std::fs::write(t.path().join("outside.json"), br#"{"version":1,"busy":false,"running":{},"updated_at":1}"#).unwrap();
      for case in ["fifo", "link", "dangling link"] {
          let d = SessionDir::new(t.path().join(case.replace(' ', "-")));
          std::fs::create_dir(&d.root).unwrap();
          let act = d.activity_json();
          match case {
              "fifo" => {
                  let c = std::ffi::CString::new(act.as_os_str().as_bytes()).unwrap();
                  // SAFETY: `c` is a valid NUL-terminated path.
                  assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
              }
              "link" => std::os::unix::fs::symlink(t.path().join("outside.json"), &act).unwrap(),
              _ => std::os::unix::fs::symlink(t.path().join("missing.json"), &act).unwrap(),
          }
          let (tx, rx) = std::sync::mpsc::channel();
          let d2 = d.clone();
          std::thread::spawn(move || {
              let read = activity::read(&d2).map_err(|e| format!("{e:#}"));
              let _ = tx.send(drain::idle(read.as_ref().map_err(String::as_str), Ok(&quiet()), now()));
          });
          let got = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("{case}: the drain read waited"));
          let why = if case == "fifo" { "is not a regular file" } else { "is a link" };
          assert_eq!(
              got,
              Idle::Busy { reason: format!("activity file unreadable: {}: {why}", act.display()), rule_skipped: false },
              "{case}"
          );
      }
  }
  ```

  `drain.rs` names `std::fs` in full, as its other tests do; `Idle` derives `Debug` and `PartialEq` (`crates/zygo-workspace/src/drain.rs:14`), so `assert_eq!` compares it.
- [x] 3. Run `cargo test -p zygo-workspace --test activity activity_fifo_and_link_are_errors_without_a_wait` → FAILS with the panic `read or update waited` (today's `File::open` waits on the FIFO). Run `cargo test -p zygo-workspace --test drain fifo_and_linked_activity_files_are_busy_without_a_wait` → FAILS with the panic `fifo: the drain read waited`.
- [x] 4. Code, `crates/zygo-workspace/src/activity.rs`. `enum Load` gains `Link` and `NotRegular` (doc lines: "a symbolic link; it was not followed (unix)", "an entry that is not a regular file: a FIFO, a folder, a socket, a device"). `fn load(path: &Path) -> Load` opens with `std::fs::OpenOptions::new().read(true)`, on unix with `custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)` (`std::os::unix::fs::OpenOptionsExt`). Open errors, in this order: on unix, `e.raw_os_error() == Some(libc::ELOOP)` or `std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())` gives `Load::Link` (D1, D2); `NotFound` gives `Load::Missing`; any other gives `Load::ReadFailed(e)`. Then `file.metadata()`: an error gives `Load::ReadFailed(e)`, `!m.is_file()` gives `Load::NotRegular` (on every system, D3). The 1 MiB + 1 byte read and all later steps stay as they are. `read` maps `Link` to `anyhow!("{}: is a link", path.display())` and `NotRegular` to `anyhow!("{}: is not a regular file", path.display())`. `update` maps them to `condition("is a link".to_string())` and `condition("is not a regular file".to_string())` (kind `FileCondition`), before `f` runs, so nothing is written. Doc comments: `read` adds the two errors and "a FIFO does not make the read wait; on unix a link is not followed"; `update` adds the two errors to its list and to the `FileCondition` list; the `FileCondition` doc adds "is a link or is not a regular file" to its `activity.json` conditions.
- [x] 5. Run `cargo test -p zygo-workspace --test activity --test drain` → all pass, including both new tests. Run `cargo test -p zygo-workspace` → all pass (`down` reads `activity.json` too). Run `cargo test -p zygo-hooks --test workspace` → all pass (the D6 pre-check still answers first).
- [x] 6. Commit `crates/zygo-workspace/src/activity.rs`, `crates/zygo-workspace/tests/activity.rs` and `crates/zygo-workspace/tests/drain.rs`, subject `fix(workspace): activity.json reads do not wait on a FIFO or follow a link`.

Part B: `view.json`.

- [x] 7. Test. In `crates/zygo-core/tests/core_basics.rs` add the test below. The FIFO case comes first, so the red run of step 10 shows the wait:

  ```rust
  #[test] // FIX-22: W-C1 (C6); only a FIFO is refused; no size cap; a link is still followed
  fn session_view_load_does_not_wait_on_a_fifo() {
      use zygo_core::session_view::SessionView;
      let t = tempfile::tempdir().unwrap();
      let d = SessionDir::new(t.path().join("s"));
      d.create().unwrap();
      #[cfg(unix)]
      {
          use std::os::unix::ffi::OsStrExt;
          use std::os::unix::fs::FileTypeExt;
          // A FIFO: the error `<path>: is a FIFO` within 5 s; the FIFO stays.
          let c = std::ffi::CString::new(d.view_json().as_os_str().as_bytes()).unwrap();
          // SAFETY: `c` is a valid NUL-terminated path.
          assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
          let (tx, rx) = std::sync::mpsc::channel();
          let d2 = d.clone();
          std::thread::spawn(move || {
              let _ = tx.send(SessionView::load(&d2).map_err(|e| format!("{e:#}")));
          });
          let r = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("SessionView::load waited on a FIFO");
          assert_eq!(r.unwrap_err(), format!("{}: is a FIFO", d.view_json().display()));
          assert!(std::fs::symlink_metadata(d.view_json()).unwrap().file_type().is_fifo());
          std::fs::remove_file(d.view_json()).unwrap();
          // A folder, and a link to a folder: today's error, the one of `std::fs::read`.
          std::fs::create_dir(t.path().join("dir")).unwrap();
          let today = std::fs::read(t.path().join("dir")).unwrap_err().to_string();
          #[cfg(target_os = "linux")]
          assert_eq!(today, "Is a directory (os error 21)");
          std::fs::create_dir(d.view_json()).unwrap();
          assert_eq!(format!("{:#}", SessionView::load(&d).unwrap_err()), today);
          std::fs::remove_dir(d.view_json()).unwrap();
          std::os::unix::fs::symlink(t.path().join("dir"), d.view_json()).unwrap();
          assert_eq!(format!("{:#}", SessionView::load(&d).unwrap_err()), today);
          std::fs::remove_file(d.view_json()).unwrap();
      }
      // A missing view keeps today's error: the bare io error, no path added.
      let e = SessionView::load(&d).unwrap_err();
      assert_eq!(e.downcast_ref::<std::io::Error>().map(|e| e.kind()), Some(std::io::ErrorKind::NotFound), "{e:#}");
      assert!(!format!("{e:#}").contains("view.json"), "{e:#}");
      let v = SessionView { session: "s".into(), main_role: "main".into(), ..Default::default() };
      v.save(&d).unwrap();
      // More than 1 MiB still loads: no cap.
      let mut b = std::fs::read(d.view_json()).unwrap();
      b.extend(std::iter::repeat_n(b' ', 2 << 20));
      std::fs::write(d.view_json(), &b).unwrap();
      assert_eq!(SessionView::load(&d).unwrap(), v);
      // A link to a regular view still loads.
      #[cfg(unix)]
      {
          std::fs::rename(d.view_json(), t.path().join("v.json")).unwrap();
          std::os::unix::fs::symlink(t.path().join("v.json"), d.view_json()).unwrap();
          assert_eq!(SessionView::load(&d).unwrap(), v);
      }
  }
  ```

- [x] 8. Test. In `crates/zygo-hooks/tests/workspace.rs`:
  - In `non_member_session_start_is_unchanged`, replace the block from the comment `// A FIFO view.json: the read waits for a writer as today; the test is that writer.` to the end of the test with this case: in a fresh `fx_with(Agent::ClaudeCode, None)` (`g`) whose `view.json` is removed and made a FIFO (`mkfifo`), one spawned thread runs `session_start::run_member(&ss(AS1), &g.dir, &g.paths, &g.zygo, &mut out)` and then `session_start::run_in(&ss(AS1), &g.dir)`, and sends `(a.map(|o| o.to_stdout()).map_err(|e| format!("{e:#}")), b.map(|o| o.to_stdout()).map_err(|e| format!("{e:#}")), out.is_empty())`; `rx.recv_timeout(S5).expect("the hook waited")`. Assertions: `a == Err(format!("{}: is a FIFO", g.dir.view_json().display()))`; `a == b`; `out` empty; `is_fifo(&g.dir.view_json())`; `!g.dir.log().exists()`; `!g.dir.activity_json().exists()`; `!g.paths.state_dir().exists()`. The comment above it: `// A FIFO view.json (FIX-22, controller 2026-11-23): the read does not wait; run_member gives run_in's error and writes nothing.`
  - In `session_start_read_errors_member_and_not`, after the member link case, add a member FIFO case: `let f = fx_note(); let missing = f.t.path().join("missing"); ws_state(&f, Phase::Up, rec_pending());`, then `view.json` removed and made a FIFO; a spawned thread (with clones of `f.dir`, `f.paths` and `missing`) runs `session_start::run_member(&ssm(AS1), &dir, &paths, &missing, &mut out)` and sends `(matches!(r, Ok(HookOutput::None)), out.is_empty())`; `rx.recv_timeout(S5).expect("the hook waited")` → `(true, true)`. Then `slog(&f) == [format!("workspace hook session-start: {}: is a FIFO", f.dir.view_json().display())]`, `member(&f) == rec_pending()`, and `is_fifo(&f.dir.view_json())`. Comment: `// A member SessionStart whose view.json is a FIFO (FIX-22): no wait, one line, no output, no change.`
- [x] 9. Run `cargo test -p zygo-core --test core_basics session_view_load_does_not_wait_on_a_fifo` → FAILS with the panic `SessionView::load waited on a FIFO`.
- [x] 10. Run, as two commands, `cargo test -p zygo-hooks --test workspace -- --exact non_member_session_start_is_unchanged` and `cargo test -p zygo-hooks --test workspace -- --exact session_start_read_errors_member_and_not` → each FAILS with the panic `the hook waited`.
- [x] 11. Code, `crates/zygo-core/src/session_view.rs`: `pub fn load(dir: &SessionDir) -> anyhow::Result<Self>` opens `dir.view_json()` with `std::fs::OpenOptions::new().read(true)`, on unix with `custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)` (no `O_NOFOLLOW`: a link is followed). On unix, a handle whose `metadata()?.file_type().is_fifo()` (`std::os::unix::fs::FileTypeExt`) gives `anyhow::anyhow!("{}: is a FIFO", path.display())`; otherwise `O_NONBLOCK` is cleared on the handle (D8: `libc::fcntl(fd, libc::F_GETFL)`, then `libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK)` with `std::os::unix::io::AsRawFd`; a return of -1 gives `std::io::Error::last_os_error()`; each `unsafe` block gets a `// SAFETY:` line: `fd` is the open descriptor of `f`, which lives for the call). The open error, the `metadata()` error and the `read_to_end` error go up with `?` unchanged (D4); the whole file is read (no cap) and parsed with `serde_json::from_slice(&bytes)?`. On other systems: the open with no flag, then the read and the parse as above. Doc comment on `load`: "Reads `view.json`. A FIFO does not make the read wait: on unix the open is `O_NONBLOCK`, and a FIFO is the error `<path>: is a FIFO`; every other entry gives the result of a plain read (`O_NONBLOCK` is cleared before it). A link is followed. The whole file is read; there is no size cap (FIX-22)."
- [x] 12. Doc, `crates/zygo-hooks/src/session_start.rs`, the doc comment of `run_member` (no code change in this file):
  - At `:55-57`, replace "after the check a load error writes one session-log line and prints nothing, and a `view.json` that is a link or not a regular file writes one session-log line and gives the session text with no note and no other change." with "after the check a load error writes one session-log line and prints nothing (a `view.json` that is a FIFO, a folder or not JSON is a load error); a `view.json` that is a link to a regular view loads, then writes one session-log line and gives the session text with no note and no other change, as does an entry that is no longer a regular file after the load."
  - At `:60-61`, replace "A `view.json` that is a FIFO still waits in `SessionView::load`, as in `run_in`, until FIX-22." with "A `view.json` that is a FIFO is a load error of `SessionView::load` (`<path>: is a FIFO`), with no wait, for every launch (FIX-22)."
- [x] 13. Run `cargo test -p zygo-core --test core_basics` → all pass. Run `cargo test -p zygo-hooks --test workspace` → all pass. Run `rg -n "until FIX-22" crates` → no match.
- [x] 14. Commit `crates/zygo-core/src/session_view.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-hooks/src/session_start.rs` and `crates/zygo-hooks/tests/workspace.rs`, subject `fix(core): the session view read does not wait on a FIFO`.

Part C: the D6 pre-check.

- [x] 15. Test. In `hostile_inputs_change_nothing` (`crates/zygo-hooks/tests/workspace.rs`), change the two expected lines: the link case to `[format!("workspace hook prompt: {act_path}: is a link")]` and the FIFO case to `[format!("workspace hook prompt: {act_path}: is not a regular file")]` (D5). Every other assertion of those cases stays: `pre-tool`, `tool-end` and `stop` write no line; `outside.json` still holds `{}`; the FIFO stays a FIFO; the case with both `activity.json` and `session.log` FIFOs returns `Ok(None)` through `ws_bg`.
- [x] 16. Run `cargo test -p zygo-hooks --test workspace -- --exact hostile_inputs_change_nothing` → FAILS: the line is `...: is a link; not changed` (the pre-check still answers).
- [x] 17. Code, `crates/zygo-hooks/src/workspace.rs`: in `fn change_within(c: &Ctx, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()>`, remove the `let path = c.dir.activity_json();` binding and the `match fs::symlink_metadata(&path) { ... }` block (`:187` and `:193-202`); the function is then the `persistent` closure and the `match activity::update(c.dir, bound, f)` with its `FileCondition` arm, unchanged. Doc of `change` (`:171-177`): replace "An `activity.json` that is a link or not a regular file is not changed." with "An `activity.json` that is a link or not a regular file is not changed: `activity::update` refuses it with a `FileCondition` (FIX-22)." and in its list of persistent conditions replace "`activity.json` is a link or not a regular file, or the update gives `activity::FileCondition`" with "the update gives `activity::FileCondition`". No import becomes unused (`fs` is still used by `check_folder` and `open_met_link`).
- [x] 18. Run `cargo test -p zygo-hooks --test workspace` → all pass, including `hostile_inputs_change_nothing` and `view_and_marker_lines_once_per_turn`. Run `rg -n -e 'is a link; not changed' -e 'is not a regular file; not changed' crates/zygo-hooks/src/workspace.rs` → no match (these are the two pre-check texts, today at `:195` and `:199`; the member-record texts `; not changed` at `:401` and `:423` stay).
- [x] 19. Commit `crates/zygo-hooks/src/workspace.rs` and `crates/zygo-hooks/tests/workspace.rs`, subject `fix(hooks): activity.json links and FIFOs come back as reader errors`.

Verification.

- [x] 20. Acceptance: `cargo test -p zygo-workspace --test activity --test drain && cargo test -p zygo-core --test core_basics && cargo test -p zygo-hooks --test workspace` → exit 0; the output lists `activity_fifo_and_link_are_errors_without_a_wait ... ok`, `fifo_and_linked_activity_files_are_busy_without_a_wait ... ok`, `session_view_load_does_not_wait_on_a_fifo ... ok` and `hostile_inputs_change_nothing ... ok`.
- [x] 21. Regression and lint, scoped to the changed packages: `cargo test -p zygo-workspace` → all pass; `cargo test -p zygo-core` → all pass (lib, doc tests, `core_basics`); `cargo test -p zygo-hooks` → all pass; `cargo clippy -p zygo-workspace -p zygo-core -p zygo-hooks --locked --tests -- -D warnings` → no warning; `cargo check --workspace --tests --locked` → no error (the callers in `zygo`, `zygo-mcp` and `zygo-work` build; no signature changed); `cargo check -p zygo-core -p zygo-workspace -p zygo-hooks --locked --tests --target x86_64-pc-windows-gnu` when that target is installed (`rustup target list --installed`), else the windows-latest CI job is the check (`libc`, the open flags and `fcntl` are `cfg(unix)`).
- [x] 22. Contract scan: `git diff --stat HEAD~3..HEAD` (the three commits of steps 6, 14 and 19) lists only the eight files of Files; `rg -n "MAX_FILE|1 << 20|take\(" crates/zygo-core/src/session_view.rs` → no match (no cap added to the view read); `rg -n "is not a regular file" crates/zygo-core/src/session_view.rs` → no match (the view read refuses only a FIFO); `git log -3 --format=%B` → each message ends with exactly the two trailer lines of the commit procedure and has no other trailer.

### Self-review (2026-11-23, again after the plan review)

1. Spec coverage. `activity::load` flags and regular-file check → step 4; link and not-a-regular-file errors with their texts → steps 1, 4; the drain counts them busy (AU26) without a wait → step 2; `update` returns the error and does not replace the entry → steps 1 (target, link and FIFO unchanged), 4; `SessionView::load` refuses only a FIFO with `<view.json path>: is a FIFO`, follows a link, keeps today's result for every other entry → steps 7 (FIFO, folder, link to a folder, missing, link to a regular view), 11 (D4, D8); the FIFO is an error for every launch (controller, premise 9) → step 8 (non-member and member); no size cap → steps 7 (2 MiB view loads), 11, 22; `update` H1 refusals kept → step 4 changes only the open, step 5 runs the H1 tests (`update_refuses_*`); other systems: no flag, handle checks stay → steps 4, 11, 21 (Windows check); D6 pre-check removed → step 17; hostile cases keep their lines → step 15; persistent class and once-per-turn rule → step 15 (`pre-tool`, `tool-end`, `stop` write none; `prompt` writes one) through the `FileCondition` arm kept in step 17; the `run_member` doc matches the new result → step 12.
2. Step scan. Each step is one action. Step 10 is two commands of one red check. The test steps carry the test code, which holds the exact texts. The code steps give signatures, flags, error texts, the order of the open checks (link before `NotFound`) and the `fcntl` sequence of D8, which the tests do not decide; no other body.
3. Interface match. `activity::read`, `activity::update` and `SessionView::load` keep their signatures (Interfaces copied from the code at `7557a60`); W9's Produces lists `read` and `update`, and H1's Produces lists `workspace::run`. FIX-24 depends on FIX-22 for the #89 text and the shared `crates/zygo-workspace/tests/drain.rs`; it consumes no name from FIX-22.
4. Review focus. No review-focus line names FIX-22. Line 6 (H1, a hook past its lock bound) is untouched: the lock path of `update` does not change.
5. Proportion. The plan carries four test bodies and no implementation body; the code change is about 50 lines across three source files.

### Deviations (implementation)

1. Commit procedure: each message file is in the session scratchpad folder (outside the repository), not made by `mktemp`, as the controller's brief says. The text of each message is as the procedure says.
2. Step 21, Windows check: `cargo check -p zygo-core -p zygo-workspace -p zygo-hooks --locked --tests --target x86_64-pc-windows-gnu` cannot build on this machine for a reason outside the code: the build script of `libsqlite3-sys` (a dependency of `zygo-workspace` and `zygo-hooks`) needs `x86_64-w64-mingw32-gcc`, which is not installed. `cargo check -p zygo-core --locked --target x86_64-pc-windows-gnu` and the same with `--tests` pass. The windows-latest CI job is the check for the other two crates, as step 21 permits.
3. The ticked boxes and this section are in a fourth commit (`docs(work): FIX-22 steps done`), after the three commits of steps 6, 14 and 19, so that the scan of step 22 (`git diff --stat HEAD~3..HEAD`, run before that commit) lists only the eight files of Files.

### Review round 1 fixes (2026-11-23)

The diff review found no blocker, 5 minor items and a nit; the controller confirmed all of them. Each code item was test first.

- F1. A socket at `activity.json` failed the open with ENXIO before the handle check, so it read as `ReadFailed` with the ENXIO text. Fix: when the open fails, `load` examines the entry by its metadata (`refused_entry`): a link stays `Link`, any other entry that is not a regular file is `NotRegular`; only then `NotFound` is `Missing` and any other error is `ReadFailed`. Tests: `activity_socket_is_not_a_regular_file` (`read` and `update` give `<path>: is not a regular file`, of kind `FileCondition` for `update`; the socket stays) and a `socket` case in `fifo_and_linked_activity_files_are_busy_without_a_wait` (busy). Red: `...activity.json: No such device or address (os error 6)` in both.
- F2. `Load::Link` was never made on other systems (dead code on Windows). Choice, consistent with D3 (on other systems a link is followed): the variant and every match arm on it are `#[cfg(unix)]`. `refused_entry` uses `symlink_metadata` on unix and `metadata` (which follows a link) on other systems, so a link to a regular file is never refused there. No test runs on this machine for it: the Windows check of `zygo-workspace` cannot build here (see the coverage limits below); the windows-latest CI job is the check.
- F3. New test `session_view_load_keeps_todays_result_for_other_entries` compares `SessionView::load` with today's read (`std::fs::read`, then serde) on the same path: a link to `/dev/null` (the serde error `EOF while parsing ...`), a bound socket (`No such device or address (os error 6)` on Linux), a mode-0 regular file (today's permission error; both reads succeed when the test runs as root), and a link to a FIFO (`<view.json path>: is a FIFO` within a 5 s thread bound). It is a characterization test: it passed on the code of `d6735f4` with no change, which is the expected result.
- F4. A link, a FIFO or another entry that is not a regular file at `activity.json` is now refused by `update` before it takes or waits for `activity.lock`, with the same `FileCondition` text as the load gives; the handle check of `load` under the lock stays the one that decides. Tests: `update_refuses_a_link_or_fifo_before_the_lock` (lock held; a FIFO, a link, a dangling link and a folder each give the persistent error in less than 500 ms with a 3 s bound) and, in `zygo-hooks`, `a_link_or_fifo_activity_file_is_refused_before_the_lock` (lock held; `pre-tool`, `tool-end` and `stop` write no line and return in less than 500 ms, `prompt` writes one line, the link target is not changed). Red: `(false, ".../activity.lock: lock not taken within 3000 ms")` against the expected `(true, ".../activity.json: is not a regular file")`, and `link pre-tool: 1.00170914s`.
- F5. The contract text (above) and the doc comment of `SessionView::load` now say that the open of `view.json` no longer waits on a device that waits at open (for example a serial line with no carrier), because the open is `O_NONBLOCK`; the read result of every entry that is not a FIFO is unchanged. The spec (`docs/specs/2026-11-20-workspaces-design.md`, W§9.1) is not in FIX-22's Files, so the sentence is not there; the controller adds it to FIX-24's spec scope.
- Nit. The doc of `change` in `crates/zygo-hooks/src/workspace.rs` lists "is a link or is not a regular file" among the `FileCondition` conditions and says the refusal comes before the lock.

Coverage limits:

- D8 (clearing `O_NONBLOCK` before the read of `view.json`) has no direct test. A regular file ignores the flag; the only entries that honour it at read time are devices, which the test machine cannot provide safely.
- The Windows check of `zygo-workspace` and `zygo-hooks` (`cargo check ... --target x86_64-pc-windows-gnu`) cannot build on this machine: the build scripts of `ring` and `libsqlite3-sys` need `x86_64-w64-mingw32-gcc`, which is not installed. `zygo-core` passes the check with and without `--tests`.

### Review round 2 fixes (2026-11-23)

The controller confirmed one minor item and three nits. Production code changed only in two doc comments; each test item has red and green evidence.

- R1 (minor). No test reached the `Load::Link` and `Load::NotRegular` arms under the lock in `update`; the check before the lock caught every tested case first. New test `update_refuses_a_fifo_or_link_that_appears_while_it_waits_for_the_lock` (`crates/zygo-workspace/tests/activity.rs`): activity.json is a regular file, the test holds `activity.lock`, starts `update` (30 s bound) in a thread, waits until this process has a second handle open on `activity.lock` (from `/proc/self/fd`; `update` opens the lock file only after the check before the lock), replaces activity.json with a FIFO (first case) or a link to a regular file (second case), checks that `update` has not returned, then releases the lock. It asserts the persistent error (`<path>: is not a regular file` or `<path>: is a link`, of kind `FileCondition`), that `f` was not called, that the FIFO or the link stays, and that the link target is unchanged. It is `cfg(target_os = "linux")`, because the wait for the handle needs `/proc/self/fd`; no hook was added to production code. Red, with the reviewer's mutation (both arms give `Activity::default()`): `assertion left == right failed: fifo; left: Ok(()); right: Err((true, "/tmp/.tmpe715kp/activity.json: is not a regular file"))`. Red, with only the `Link` arm mutated: `assertion left == right failed: link; left: Ok(()); right: Err((true, "/tmp/.tmpcIXNBm/activity.json: is a link"))`. Mutation reverted (no diff in `src/`); green: `1 passed`.
- R2 (nit). The three socket tests made their temp folder under TMPDIR, so a long TMPDIR made the socket path longer than `sun_path`. `activity_socket_is_not_a_regular_file` (`tests/activity.rs`), `fifo_and_linked_activity_files_are_busy_without_a_wait` (`tests/drain.rs`) and `session_view_load_keeps_todays_result_for_other_entries` (`crates/zygo-core/tests/core_basics.rs`) now use `tempfile::Builder::new().tempdir_in("/tmp")`. Red, with `TMPDIR=/tmp/claude-1000/-home-kagemartin-Work-zygo--claude-worktrees-zygo-phase-2/58560339-0d35-4cf4-b289-b88946e38748/scratchpad`: each of the three panicked at its `UnixListener::bind` with `Error { kind: InvalidInput, message: "path must be shorter than SUN_LEN" }` (`activity.rs:640`, `drain.rs:270`, `core_basics.rs:517`). Green, with the same TMPDIR: each `1 passed`.
- R3 (nit). `a_link_or_fifo_activity_file_is_refused_before_the_lock` (`crates/zygo-hooks/tests/workspace.rs`) had two 500 ms guards over a whole debug hook run. They are removed. The lock stays held for the whole case, and the session log tells the two outcomes apart with no time measure: a lock not taken within `LOCK_BOUND` is a one-off failure that `change` logs on every event (`<activity.lock path>: lock not taken within 1000 ms`), while the refusal is a `FileCondition` that is logged only on `prompt`, with the refusal text. The test now checks that the log is empty after each of `pre-tool`, `tool-end` and `stop`, and has the one `prompt` line. Red, with the check before the lock in `update` disabled (`match None::<Load>`): `link pre-tool: ["workspace hook pre-tool: /tmp/.tmpn5ZPwd/cache/sessions/01JTESTSESSIONAAAAAAAAAAAA/activity.lock: lock not taken within 1000 ms"]`. Mutation reverted; green: `1 passed`.
- R4 (nit). The doc of `FileCondition` (`crates/zygo-workspace/src/activity.rs`) says "is a link (on unix)", and its broken wrap after "; an" is joined. The doc of `SessionView::load` (`crates/zygo-core/src/session_view.rs`) says that for a device that waits at open, such as a tty with no carrier, the wait moves from the open to the read, and the read result is unchanged.

Checks after the fixes: `cargo test --locked -p zygo-workspace -p zygo-hooks -p zygo-core` all `ok` (activity 41 passed, 1 ignored; drain 34 passed; core_basics 22 passed; hooks workspace 23 passed, 1 ignored); `cargo clippy --locked -p zygo-workspace -p zygo-hooks -p zygo-core --tests -- -D warnings` finished with no warning; the Acceptance command passes; `work_graph.py check docs/work/phase2/graph.md` gives `ok`.

Not changed (outside the brief): the Spec text of this node (line 3) and the F5 item above still say that the open "no longer waits" on such a device, without the sentence that the wait moves to the read.
