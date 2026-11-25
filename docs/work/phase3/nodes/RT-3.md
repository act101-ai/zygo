# RT-3 — Run-time HUD segment in compact and verbose, with its place in the width-removal order

**Spec:** docs/specs/2026-11-24-session-runtime-design.md §2.4, §2.6, §3.1, §5 (row "Small terminal width"), §6 (rows `data.rs`, `render.rs` segment), §7 items 3, 6 and 7; base-spec amendments H§6.2, H AU9, H AU20, H§6.4, H§6.5, H§6.6, H§6.7, H§6.8, H§7.6 and the H§6.3/H§6.4 example note (docs/specs/2026-11-21-statusline-hud-design.md)
**Files:** `crates/zygo-hooks/src/statusline/data.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo-hooks/tests/statusline_phase.rs`, `docs/specs/2026-11-21-statusline-hud-design.md`, `crates/zygo-hooks/tests/statusline_team.rs`, `crates/zygo/tests/compact_cmd.rs`
**Consumes:** `member_read` (RT-2), `MemberRead` (RT-2), `run_time_text` (RT-2)
**Produces:** `zygo_hooks::statusline::render::HudData.run_time: Option<u64>` (seconds, `now.saturating_sub(start)`; `None` with no start); the private `render::Seg::RunTime` in `COMPACT` (own group after `[Trimmed]`, before `[Marker]`), `ROW_1` (own group after `[Mode]`), `COMPACT_DROPS` (third, after `Trimmed`, `Cache`) and `ROW_1_DROPS` (second, after `Mode`); the private `Glyphs.clock` (Nerd `󰅐` U+F0150, plain `run`)
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test statusline_render -- run_time_segment --list 2>/dev/null | grep -c ': test$')" -ge 4 && test "$(cargo test -q -p zygo-hooks --test statusline_render -- run_time_width --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test --locked -p zygo-hooks --test statusline_render -- h14_01_golden_files_hold_each_render_byte_for_byte h14_04_emission_examples_1_and_3_are_golden --exact && cargo test --locked -p zygo-hooks` → every count holds (at least 4 named `run_time_segment…` in zygo-hooks `statusline_render`; at least 2 named `run_time_width…` in zygo-hooks `statusline_render`; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows CI: the golden tests `h14_01_golden_files_hold_each_render_byte_for_byte` and `h14_04_emission_examples_1_and_3_are_golden` pass with the reference files in `crates/zygo-hooks/tests/golden/` unchanged by this node (the HUD without a run time keeps its bytes; T§3.1 last row), and the tests whose names contain `run_time_segment` (T§7 items 3 and 6: OP13 compact, OP13 verbose and example 3 stale bytes with `󰅐 5h12m` at `now` = start + 18720 through `ZYGO_TEST_NOW`; plain set `run 5h12m`; `stale` leaves the run-time run unchanged; start > now gives `0m`) and `run_time_width` (T§7 item 7: compact keeps tokens at the width that fits only without trimmed, cache and run time, and keeps the run time at one cell more; verbose row 1 keeps profile at the width that fits only without mode and run time) run and pass

## Plan

Premise check (live, at `0bd0da71`, branch `phase3/RT-3` from `origin/phase-3`): RT-2 and ML-15 are DONE.
- RT-2 built the one `workspace.json` read for phase and start, `phase::member_read`, but did not wire it into the HUD: `collect` still calls `super::phase::read(paths, env)` (`crates/zygo-hooks/src/statusline/data.rs:586`), which loads `view.json` a second time from `ZYGO_SESSION_DIR` through `env` (`phase.rs:90-93`). `HudData` has no run-time field (`render.rs:63-90`). Step 7 replaces that call (graph row 65).
- `phase::read` stays. 13 tests in `crates/zygo-hooks/tests/statusline_phase.rs` (`Fx::read`, `:75-77`, and the direct calls at `:132-133`, `:207`, `:231`) use it, and RT-2's Produces keeps it. The `collect` call is the only call outside tests (`data.rs:586`).
- ML-15 groups in the code (`render.rs:454-472`): `COMPACT` first group `[Phase, Name, Mail, Work, Worktree]`, `ROW_1` `[Phase], [Name, Mail], [Work, Worktree, Git], [Model], [Profile], [Mode]`. These are the T§3.1 groups without the run time, so the new group goes in as T§3.1 says.
- `Glyphs.mail` is kept out of `table` (comment at `render.rs:174`), so the `zygo doctor` icon line still has 19 entries (`statusline_render.rs:830`). FX-11 changes that count to 20. `Glyphs.clock` stays out of `table` for the same reason (author's call), so this node does not change the FX-11 count or `crates/zygo/tests/usage_doctor.rs`.
- The HUD spec has no M§9a text yet (FX-10 adds it). T§3.1 gives the AU9 groups with the member name and mail already in them. Step 9 writes the T§3.1 AU9 text as it is, so FX-10 finds AU9 done and still adds the H§6.2 and H§6.5 rows for the name, the mail and `✉`.
- `cells` counts U+F0000-U+FFFFD as 2 (`crates/zygo-hooks/src/statusline/width.rs:63-67`), so `󰅐` (U+F0150) counts 2 cells and needs no width code.
- Line drift only for the T§6 citations: `COMPACT` is at `render.rs:454` (was `:428`), `ROW_1` at `:465`, `COMPACT_DROPS` at `:483`, `ROW_1_DROPS` at `:497`, `Glyphs` at `:158-176`. `now` is still `data.rs:584`.
- Out of Files, and not run by CI (`cargo test --workspace --locked --no-fail-fast`, `.github/workflows/ci.yml:45`, runs no `#[ignore]` test): two ignored tests render a session whose `view.session` is a real proxy ULID and check the exact end of verbose row 1. After this node, row 1 ends with the run time, so both fail under `--ignored`: `h14_15_team_session_hud_shows_notional_spend_at_list_prices` (`crates/zygo-hooks/tests/statusline_team.rs:312` `"  serve 127.0.0.1\n"`, `:341` `"  local\n"`) and `statusline_style_from_the_overrides_then_the_view` (`crates/zygo/tests/compact_cmd.rs:444-447`, exact verbose text ending `local\n`). This plan does not change them. See the report.

Consumes (live):
- `#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct MemberRead { pub phase: Option<String>, pub start: Option<u64> }` (`crates/zygo-hooks/src/statusline/phase.rs:57-62`).
- `pub fn member_read(paths: &Paths, view: &SessionView) -> MemberRead` (`phase.rs:107`).
- `pub fn run_time_text(secs: u64) -> String` (`crates/zygo-hooks/src/statusline/render.rs:279`).
- Also used: `pub fn collect(stdin: Box<dyn Read + Send + 'static>, session_dir: &Path, paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, deadline: Instant) -> HudData` (`data.rs:570-576`); `Seams.now` from `ZYGO_TEST_NOW` under `test-hooks` (`data.rs:423`, `:445`); `fn iconed(icon: &str, text: &str) -> String` (`render.rs:525`); `const DIM: Sgr` (`render.rs:327`); test helpers in `statusline_render.rs`: `op13`, `width_all`, `stale_case`, `data`, `golden`, `grid`, `strip`, `renv`, `nerd`, `compact_text`; in `statusline_phase.rs`: `Fx`, `member`, `with_record`, `SID`, `OWN`, `ID`.

Produces (exact, after this node):
- `zygo_hooks::statusline::render::HudData.run_time: Option<u64>`: seconds, `now.saturating_sub(start)`; `None` with no start.
- Private `render::Seg::RunTime` in `COMPACT` (its own group after `[Trimmed]`, before `[Marker]`), `ROW_1` (its own group after `[Mode]`), `COMPACT_DROPS` (third, after `Trimmed`, `Cache`) and `ROW_1_DROPS` (second, after `Mode`).
- Private `Glyphs.clock`: Nerd `"\u{f0150}"` (`󰅐`), plain `"run"`. Not in `table`.
- `collect` reads `workspace.json` once, through `member_read` on the `view.json` of `session_dir`. It no longer calls `phase::read`.

Steps:

- [x] 1. In `crates/zygo-hooks/tests/statusline_render.rs`, in the `data` helper (`:74-90`), add `run_time: None,` after `unread: None,` (`:88`). Add these imports after the `use zygo_hooks::statusline::render::run_time_text;` line (`:14`):
  ```rust
  use zygo_core::paths::Paths;
  use zygo_core::session_dir::SessionDir;
  use zygo_core::session_view::SessionView;
  use zygo_hooks::statusline::data::collect;
  ```
  Then add at the end of the file:
  ```rust
  // ---- run time segment (session run time design T§2.4, T§3.1, T§7 items 3, 6 and 7) ----

  /// A ULID whose time is 1_800_000_000_000 ms: the own start of a session outside a workspace is `OWN` (T§2.1).
  const SID: &str = "01MCC5RM000000000000000000";
  const OWN: u64 = 1_800_000_000;
  /// nf-md-clock_outline (T§2.4).
  const CLOCK: &str = "\u{f0150}";

  /// The run time that `collect` gives at `ZYGO_TEST_NOW` = `now` for the session `SID` outside a workspace
  /// (T§2.2 rule 1). No proxy: the run time is local.
  fn collected_run_time(now: u64) -> Option<u64> {
      let t = tempfile::tempdir().unwrap();
      let dir = t.path().join("sessions").join(SID);
      let sd = SessionDir::new(dir.clone());
      sd.create().unwrap();
      SessionView { session: SID.into(), ..Default::default() }.save(&sd).unwrap();
      let (cache_dir, data_dir) = (t.path().join("cache"), t.path().join("data"));
      let paths = Paths {
          config_file: t.path().join("config.toml"),
          sessions_dir: t.path().join("sessions"),
          marketplaces_dir: cache_dir.join("marketplaces"),
          plugin_store: data_dir.join("plugins"),
          approvals_file: data_dir.join("approvals.toml"),
          usage_db: data_dir.join("usage.db"),
          cache_dir,
          data_dir,
      };
      let stdin = serde_json::json!({ "cwd": t.path() }).to_string().into_bytes();
      let env = move |key: &str| (key == "ZYGO_TEST_NOW").then(|| OsString::from(now.to_string()));
      let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
      collect(Box::new(std::io::Cursor::new(stdin)), &dir, &paths, &env, deadline).run_time
  }

  /// The cells of the run-time run on row `row` of `s`: the clock, the space and `5h12m`.
  fn clock_cells(s: &str, row: usize) -> Vec<(usize, usize, char, Sty)> {
      grid(s).into_iter().filter(|c| c.0 == row).skip_while(|c| c.2 != '\u{f0150}').take(7).collect()
  }

  #[test] // T§3.1 example note, T§7 item 6: OP13 compact gets `  󰅐 5h12m` after `✂ 23k`, in the same dim run
  fn run_time_segment_op13_compact_bytes() {
      let run_time = collected_run_time(OWN + 18_720);
      assert_eq!(run_time, Some(18_720));
      let d = HudData { run_time, ..op13() };
      assert_eq!(
          render(&d, &nerd(C)),
          format!(
              "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[35m{CPU} opus-4.7\x1b[0m \x1b[36m{PROFILE} fast\x1b[0m  ctx 196k/\x1b[2m1m\x1b[0m \x1b[32m▰\x1b[2m▱▱▱▱\x1b[0m  \x1b[2m↑1.2m ↓48k\x1b[0m  \x1b[32m$3.41\x1b[0m  \x1b[2m{DB} 92%  ✂ 23k  {CLOCK} 5h12m\x1b[0m"
          )
      );
      // Without a run time the bytes are the reference bytes (T§3.1 last row).
      assert_eq!(render(&op13(), &nerd(C)), op13_compact());
  }

  #[test] // T§3.1 example note, T§7 item 6: OP13 verbose row 1 gets `   󰅐 5h12m` after `local`; row 2 does not change
  fn run_time_segment_op13_verbose_bytes() {
      let d = HudData { run_time: collected_run_time(OWN + 18_720), ..op13() };
      assert_eq!(
          render(&d, &nerd(V)),
          format!(
              "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m  \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m \x1b[36m↑1\x1b[0m   \x1b[35m{CPU} opus-4.7 · high\x1b[0m   \x1b[36m{PROFILE} fast\x1b[0m   \x1b[2mlocal   {CLOCK} 5h12m\x1b[0m\n{STRIPE}\x1b[2;37m{REFLECTION}\x1b[0m  ctx 196k/\x1b[2m1m\x1b[0m \x1b[32m▰▰\x1b[2m▱▱▱▱▱▱\x1b[0m 20%   \x1b[2m↑ 1.2m in  ↓ 48k out\x1b[0m   \x1b[32m$3.41\x1b[0m   \x1b[2mcache 92%   trimmed 23k tok\x1b[0m"
          )
      );
      assert_eq!(render(&op13(), &nerd(V)), op13_verbose());
  }

  #[test] // T§3.1 example note, T§2.4 "Kind", H AU20: example 3 gets `  󰅐 5h12m` before `  stale`; a local segment
  fn run_time_segment_example_3_stale_bytes_and_a_local_run() {
      let run_time = collected_run_time(OWN + 18_720);
      let stale = HudData { run_time, ..stale_case() };
      assert_eq!(
          render(&stale, &nerd(C)),
          format!(
              "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[2m{CPU} opus-4.7\x1b[0m \x1b[36m{PROFILE} fast\x1b[0m  \x1b[2mctx 196k/1m ▰▱▱▱▱  ↑1.2m ↓48k  $3.41  {DB} 92%  ✂ 23k  {CLOCK} 5h12m  stale\x1b[0m"
          )
      );
      // `stale` leaves the run-time run unchanged: same cells, style (dim, default) in both styles.
      let fresh = HudData { run_time, ..op13() };
      for (style, row) in [(C, 0), (V, 0)] {
          let (f, s) = (clock_cells(&render(&fresh, &nerd(style)), row), clock_cells(&render(&stale, &nerd(style)), row));
          assert_eq!(f.len(), 7, "{style:?}");
          assert_eq!(f, s, "{style:?}");
          assert_eq!((f[0].2, f[0].3), ('\u{f0150}', (2, None)), "{style:?}");
      }
      // With no status data the run time stays; `ended` does not drop it.
      let offline = HudData { status: None, outcome: Outcome::Offline, run_time, ..op13() };
      assert_eq!(
          render(&offline, &nerd(C)),
          format!("{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[36m{PROFILE} fast\x1b[0m  \x1b[2m✂ 23k  {CLOCK} 5h12m\x1b[0m  \x1b[31moffline\x1b[0m")
      );
      let ended = HudData { outcome: Outcome::Ended, stale: true, profile: None, run_time, ..op13() };
      assert_eq!(
          render(&ended, &nerd(C)),
          format!("{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[2m✂ 23k  {CLOCK} 5h12m\x1b[0m  \x1b[31mended\x1b[0m")
      );
  }

  #[test] // T§2.4 "Text", T§7 item 6: the plain set shows `run 5h12m`, the same text in compact and verbose
  fn run_time_segment_plain_set_shows_run() {
      let d = HudData { run_time: collected_run_time(OWN + 18_720), ..op13() };
      let plain = |style: Style, color: ColorMode| render(&d, &renv(style, IconSet::Plain, color, 200));
      assert_eq!(plain(C, ColorMode::Off), format!("{}  run 5h12m", golden("compact-plain-nocolor")));
      let reference = golden("verbose-plain-nocolor");
      let (row_1, row_2) = reference.split_once('\n').unwrap();
      assert_eq!(plain(V, ColorMode::Off), format!("{row_1}   run 5h12m\n{row_2}"));
      assert!(plain(C, ColorMode::Ansi256).ends_with("\x1b[2mcache 92%  trim 23k  run 5h12m\x1b[0m"), "{:?}", plain(C, ColorMode::Ansi256));
  }

  #[test] // T§2.3, T§7 item 3: start > now gives `0m`
  fn run_time_segment_clock_behind_start_shows_0m() {
      assert_eq!(collected_run_time(OWN - 1), Some(0));
      assert_eq!(collected_run_time(OWN), Some(0));
      let d = HudData { run_time: collected_run_time(OWN - 1), ..op13() };
      assert!(compact_text(&d).ends_with(&format!("✂ 23k  {CLOCK} 0m")), "{}", compact_text(&d));
      assert!(verbose_text(&d).contains(&format!("local   {CLOCK} 0m\n")), "{}", verbose_text(&d));
  }

  #[test] // T§2.4 "Width", H§6.7 as amended, T§7 item 7: compact removes the run time third, after trimmed and cache
  fn run_time_width_compact_goes_third_after_trimmed_and_cache() {
      let d = HudData { run_time: Some(18_720), ..width_all() };
      let at = |w: usize| strip(&render(&d, &renv(C, IconSet::Nerd, ColorMode::Ansi256, w as u16)));
      let head = format!("▌▌▌ {LOGO}");
      let (work, tree) = (format!(" {BRANCH} main"), format!(" {TREE} hud-work"));
      let (model, profile) = (format!("{CPU} opus-4.7"), format!("{PROFILE} fast"));
      let (ctx, bar, mark, tokens, spend) = ("ctx 196k/1m", "▰▰▱▱▱", "⇣600k", "↑1.2m ↓48k", "$3.41");
      let (cache, trimmed, rt) = (format!("{DB} 92%"), "✂ 23k", format!("{CLOCK} 5h12m"));
      // The Nerd clock counts 2 cells (H§6.7).
      assert_eq!(cells(&rt), 8);
      let steps = [
          ("all", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  {cache}  {trimmed}  {rt}  stale")),
          ("trimmed", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  {cache}  {rt}  stale")),
          ("cache", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  {rt}  stale")),
          ("run time", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  stale")),
          ("tokens", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {spend}  stale")),
      ];
      assert_eq!(at(200), steps[0].1);
      for pair in steps.windows(2) {
          let (before, (name, after)) = (&pair[0].1, &pair[1]);
          // The row before this step fits at its own width; one cell less removes exactly this segment.
          assert_eq!(&at(cells(before)), before, "before {name}");
          assert_eq!(&at(cells(before) - 1), after, "{name}");
      }
      // T§7 item 7 in its own words: at the width that fits only without trimmed, cache and run time, tokens stay;
      // one cell more keeps the run time.
      let w = cells(&steps[2].1) - 1;
      assert!(at(w).contains(tokens) && !at(w).contains(&rt), "{}", at(w));
      assert!(at(w + 1).contains(tokens) && at(w + 1).contains(&rt), "{}", at(w + 1));
  }

  #[test] // T§2.4 "Width", H§6.7 as amended, T§7 item 7: verbose row 1 removes the run time second, after mode
  fn run_time_width_verbose_row_1_goes_second_after_mode() {
      let d = HudData { run_time: Some(18_720), ..width_all() };
      let at = |d: &HudData, w: usize| -> Vec<String> {
          strip(&render(d, &renv(V, IconSet::Nerd, ColorMode::Ansi256, w as u16))).split('\n').map(str::to_string).collect()
      };
      let (work, tree) = (format!("{BRANCH} main"), format!("{TREE} hud-work"));
      let (model, profile, rt) = (format!("{CPU} opus-4.7"), format!("{PROFILE} fast"), format!("{CLOCK} 5h12m"));
      let h1 = format!("▌▌▌ {LOGO}");
      let row1 = [
          ("all", format!("{h1}  {work} {tree} ↑1   {model} · high   {profile}   local   {rt}")),
          ("mode", format!("{h1}  {work} {tree} ↑1   {model} · high   {profile}   {rt}")),
          ("run time", format!("{h1}  {work} {tree} ↑1   {model} · high   {profile}")),
          ("profile", format!("{h1}  {work} {tree} ↑1   {model} · high")),
      ];
      assert_eq!(at(&d, 200)[0], row1[0].1);
      // The run time is on row 1 only: row 2 is the row of the data without it.
      assert_eq!(at(&d, 200)[1], at(&width_all(), 200)[1]);
      for pair in row1.windows(2) {
          let (before, (name, after)) = (&pair[0].1, &pair[1]);
          assert_eq!(&at(&d, cells(before))[0], before, "before {name}");
          assert_eq!(&at(&d, cells(before) - 1)[0], after, "{name}");
      }
      // T§7 item 7: the row 1 that fits only without mode and run time keeps profile.
      let w = cells(&row1[1].1) - 1;
      assert_eq!(at(&d, w)[0], row1[2].1);
      assert!(at(&d, w)[0].ends_with(&profile));
  }
  ```
  Check: `cargo test --locked -p zygo-hooks --test statusline_render run_time_` fails to compile: `HudData` has no field `run_time`.

- [x] 2. In `crates/zygo-hooks/tests/statusline_phase.rs`, add after `start_rule_pending_of_another_session_gives_the_own_start` (`:501-508`), before `read_keeps_the_phase_when_members_does_not_parse` (`:509`):
  ```rust
  #[test] // T§2.6, H§7.6 as amended (graph row 65): `collect` takes the phase and the start from one `member_read` of
  // the `view.json` of its session dir. `ZYGO_SESSION_DIR` is not set here: the old `phase::read` call would give no phase.
  fn collect_run_time_and_phase_come_from_one_member_read() {
      let fx = Fx::new();
      let sd = SessionDir::new(fx.dir.clone());
      let at = |now: u64| {
          let env = move |key: &str| (key == "ZYGO_TEST_NOW").then(|| OsString::from(now.to_string()));
          let d = collect(Box::new(Cursor::new(Vec::new())), &fx.dir, &fx.paths(), &env, Instant::now() + Duration::from_secs(10));
          (d.phase, d.run_time)
      };
      // A member whose record names this session: `first_started_at`.
      member(SID, ID).save(&sd).unwrap();
      fx.state(ID, &with_record(serde_json::json!({ "zygo_session": SID, "first_started_at": 1_700_000_000u64 })));
      assert_eq!(at(1_700_000_000 + 18_720), (Some("up".into()), Some(18_720)));
      // Start > now: 0 (T§2.3).
      assert_eq!(at(1_699_999_999), (Some("up".into()), Some(0)));
      // `members` does not parse: the phase stays, no run time (RT-2 review fix).
      fx.state(ID, br#"{"version":1,"phase":"up","members":7}"#);
      assert_eq!(at(1_700_000_000), (Some("up".into()), None));
      // A failed read check (`version` 2): neither, and no ULID fallback (T§2.2 rule 2).
      fx.state(ID, br#"{"version":2,"phase":"up"}"#);
      assert_eq!(at(OWN + 18_720), (None, None));
      // Outside a workspace: the ULID time of `view.session` (rule 1); a session that is not a ULID: none.
      SessionView { session: SID.into(), ..Default::default() }.save(&sd).unwrap();
      assert_eq!(at(OWN + 18_720), (None, Some(18_720)));
      SessionView { session: "S1".into(), ..Default::default() }.save(&sd).unwrap();
      assert_eq!(at(OWN + 18_720), (None, None));
  }
  ```
  Check: `cargo test --locked -p zygo-hooks --test statusline_phase collect_run_time` fails to compile: `HudData` has no field `run_time`.

- [x] 3. In `crates/zygo-hooks/src/statusline/render.rs`, in `HudData`, add after `pub unread: Option<usize>,` (`:87`):
  ```rust
  /// Seconds since the start of session run time design T§2.2 (`now.saturating_sub(start)`); `None` with no start.
  pub run_time: Option<u64>,
  ```
  Check: `cargo build --locked -p zygo-hooks` fails: missing field `run_time` in the `HudData` literal of `collect` (`data.rs:692`).

- [x] 4. In `crates/zygo-hooks/src/statusline/render.rs`, glyphs: add to `struct Glyphs` after `mail` (`:175`):
  ```rust
  /// The run time (T§2.4). Not in `table`, as `mail`.
  clock: &'static str,
  ```
  Add `clock: "\u{f0150}",` to `NERD` after `mail: "\u{2709}",` (`:199`), and `clock: "run",` to `PLAIN` after `mail: "mail",` (`:217`).

- [x] 5. In `crates/zygo-hooks/src/statusline/render.rs`, the segment:
  - In `enum Seg` (`:427`), add before `Marker`:
    ```rust
    /// The run time (session run time design T§2.4); a local segment.
    RunTime,
    ```
  - `COMPACT` (`:454-463`): add `&[Seg::RunTime],` between `&[Seg::Trimmed],` and `&[Seg::Marker],`.
  - `ROW_1` (`:465-472`): add `&[Seg::RunTime],` after `&[Seg::Mode],`.
  - `COMPACT_DROPS` (`:483-496`): `Seg::Trimmed, Seg::Cache, Seg::RunTime, Seg::Tokens, …` (insert `Seg::RunTime` after `Seg::Cache`; the rest unchanged).
  - `ROW_1_DROPS` (`:497`): `&[Seg::Mode, Seg::RunTime, Seg::Profile, Seg::Effort, Seg::Git, Seg::Model, Seg::Worktree, Seg::Work]`.
  - In `impl Hud`, after `fn trimmed` (`:744-749`):
    ```rust
    /// T§2.4: `󰅐 <form>` / `run <form>`, dim, the same text in both styles. A local segment (H AU20): `stale` does
    /// not dim it and `ended` does not drop it.
    fn run_time(&self) -> Option<Vec<Run>> {
        Some(vec![run(DIM, iconed(self.g.clock, &run_time_text(self.d.run_time?)))])
    }
    ```
  - In `fn seg` (`:762-789`), add `Seg::RunTime => self.run_time(),` after `Seg::Trimmed => self.trimmed(),` (it is after `_ if gone(seg) => None`, so width can remove it).
  Check: `cargo build --locked -p zygo-hooks` still fails, only with the missing field `run_time` at `data.rs:692` (no other error). Steps 1 and 2 run after step 6.

- [x] 6. In `crates/zygo-hooks/src/statusline/data.rs`, `collect` (`:570`):
  - Replace `let phase = super::phase::read(paths, env);` (`:586`) with:
    ```rust
    // One `workspace.json` read gives the phase and the run-time start (T§2.6, H§7.6).
    let member = view.as_ref().map(|v| super::phase::member_read(paths, v)).unwrap_or_default();
    ```
  - In the `HudData` literal (`:692-708`), replace `phase,` (`:704`) with `phase: member.phase,`, and add after `unread,` (`:706`):
    ```rust
    run_time: member.start.map(|start| now.saturating_sub(start)),
    ```
  - In the doc comment of `collect` (`:567`), replace "the workspace phase from `paths.workspaces_dir()` (AU25, never cached)" with "the workspace phase and the run-time start from one read of `paths.workspaces_dir()` (AU25, session run time design T§2.6; never cached)".
  Check: `cargo test --locked -p zygo-hooks --test statusline_render run_time_` → 8 passed (5 `run_time_segment`, 2 `run_time_width`, `run_time_format_t7_item_1`); `cargo test --locked -p zygo-hooks --test statusline_phase collect_run_time` → 1 passed.

- [x] 7. Check the golden bytes did not move: `cargo test --locked -p zygo-hooks --test statusline_render -- h14_01_golden_files_hold_each_render_byte_for_byte h14_04_emission_examples_1_and_3_are_golden --exact` → 2 passed, and `git diff --stat -- crates/zygo-hooks/tests/golden` prints nothing.

- [x] 8. Check the counts of the contract: `cargo test -q -p zygo-hooks --test statusline_render -- run_time_segment --list` lists 5 tests, and `-- run_time_width --list` lists 2.

- [x] 9. Amend `docs/specs/2026-11-21-statusline-hud-design.md` as T§3.1 says. "T§" in the new text means a section of `docs/specs/2026-11-24-session-runtime-design.md`, named "session run time design" on first use in each place.
  - AU9 (`:104`): compact groups become `[logo, workspace phase, member name, mail, work item, worktree], [model, profile], [ctx, bar, threshold], [tokens], [spend], [cache], [trimmed], [run time], [stale/offline/suspended/ended]`. Verbose row 1 groups become `[workspace phase], [member name, mail], [work item, worktree, ahead/behind], [model · effort], [profile], [mode], [run time]`. Row 2 does not change.
  - AU20 (`:115`): the local segment list becomes `(stripe, logo, phase, work item, worktree, git, trimmed, run time, profile from `view.json`)`.
  - H§6.2 (`:150`): after row 14 add `| 14a | Run time | `workspace.json` `members.<pane>.first_started_at`, else `pending.started_at`, else the ULID time of `view.session` (session run time design T§2.2) | `2` | yes, `󰅐 5h12m` | row 1, `󰅐 5h12m` |`. The number `14a` keeps row 15 and the "row 14" citation of `docs/specs/2026-11-24-output-shaping-design.md:178` valid.
  - H§6.4, after the example 3 code block (`:255-257`): add the paragraph "Run time (session run time design T§3.1): the reference bytes of H§6.3 and of these examples are those of a HUD without the run time. With a run time of `5h12m`, OP13 compact gets `  󰅐 5h12m` after `✂ 23k` and before the last `\e[0m` (the dim gap is absorbed, step 3); OP13 verbose row 1 gets `   󰅐 5h12m` after `local` and before its `\e[0m`; example 3 (stale) gets `  󰅐 5h12m` before `  stale` in the same dim run."
  - H§6.4 color table (`:284`): after the `Trimmed` row add `| Run time | `2` | same |`.
  - H§6.5 icon table (`:317`): after the `Trimmed` row add `| Run time | `󰅐` (nf-md-clock_outline) | U+F0150 | word `run` | — |`.
  - H§6.6 (`:328`): add the bullet "- Run time uses the form of session run time design T§2.3 (`47m`, `5h12m`, `2d04h`). It has no maximum width and no truncation."
  - H§6.7 (`:346-347`): compact becomes "trimmed → cache → run time → tokens → threshold `⇣` → spend → profile → model → bar cells (the numbers stay) → `stale` → worktree → ctx numbers → work item." Verbose row 1 becomes "mode → run time → profile → effort → ahead/behind → model → worktree → work item." and keeps its next sentence.
  - H§6.8 table, after the `Usage store missing …` row (`:369`): add `| Member, and `workspace.json` cannot be read (bad id, missing, not a regular file, above 64 KiB, does not parse, `version` not 1) | No run time segment. |` and `| Not a member, and `view.session` is not a ULID | No run time segment. |`.
  - H§7.6 (`:463`): "It deserializes only `version` and `phase`" becomes "It deserializes `version` and `phase`". After the AU25 sentence add: "For the run time (session run time design T§2.2), the reader also takes, for the member's pane key, `first_started_at`, `zygo_session` and `pending.{zygo_session, started_at}` with its own small serde struct (unknown fields ignored), from the same read. Still no dependency on `zygo-workspace` from the status line module."
  - `## Change log`: add the last row `| 2026-11-25 | AU9, AU20, H§6.2, H§6.4 (example note, color table), H§6.5, H§6.6, H§6.7, H§6.8, H§7.6 | operator-approved session run time design T§3.1 (2026-11-24); built by RT-3 | The run time segment: its row, groups, color, glyph, form, place in the width order, missing-data rows and the one `workspace.json` read. |`.
  Check: `grep -c 'run time' docs/specs/2026-11-21-statusline-hud-design.md` is at least 9, and `grep -c 'U+F0150' docs/specs/2026-11-21-statusline-hud-design.md` is 1.

- [x] 10. Acceptance: `cargo test --locked -p zygo-hooks` → all pass, with the 5 `run_time_segment…` and 2 `run_time_width…` tests of `statusline_render` and `collect_run_time_and_phase_come_from_one_member_read` of `statusline_phase` in the run, and `crates/zygo-hooks/tests/golden/` unchanged.

### Round 2

- [x] F2. `crates/zygo-hooks/tests/statusline_team.rs`: the helper `run_time` took the expected text from `d.run_time`, the value under test. It now decodes the ULID time of the test's own session id with a local Crockford decoder (no new dependency), computes `T - start`, asserts `d.run_time == Some(that)` and gives `󰅐 <run_time_text>` for the row-1 expectations.
- [x] F4. `docs/specs/2026-11-21-statusline-hud-design.md` H§6.8: add the row for a readable `workspace.json` whose `members` is not an object or null, or whose pane record does not decode: the phase shows, no run time (`phase.rs` `member_read`; test `statusline_phase.rs` start rule for an undecodable record). Change log row added.
