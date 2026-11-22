//! HUD-8: the pure render of the status line HUD (H§6.2-H§6.8) and its width sources (H§6.7).
//!
//! Acceptance items of H§14 that this node owns: #1, #2, #3, #4, #8 (render part), #9, #10, #12 (render part), #20,
//! #21, #29, #31. The golden files in `tests/golden/` hold the bytes of the `printf` commands of H§6.3 and H§6.4.

use std::ffi::OsString;

use serde_json::Value;
use zygo_hooks::statusline::render::{
    color_mode, glyphs, icon_set, icon_set_on, render, spend_text, ColorMode, HudData, IconSet, Outcome, RenderEnv, Style,
};
use zygo_hooks::statusline::width::{cells, terminal_width, truncate};
use zygo_proxy::client::{SessionStatus, WindowArg};

// ---- the reference bytes of the spec (H§6.3 OP13 as amended by OP14; H§6.4 examples 1 and 3) ----

const STRIPE: &str = "\x1b[31m▌\x1b[33m▌\x1b[36m▌\x1b[0m ";
const LOGO: &str = "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}";
const REFLECTION: &str = "z\u{28e}\u{183}o";
const BRANCH: &str = "\u{e0a0}";
const TREE: &str = "\u{f0645}";
const CPU: &str = "\u{f4bc}";
const PROFILE: &str = "\u{f0626}";
const DB: &str = "\u{f01bc}";

/// The verbose `printf` of H§6.3 without its final `\n` (H§6.4 step 7).
fn op13_verbose() -> String {
    format!(
        "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m  \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m \x1b[36m↑1\x1b[0m   \x1b[35m{CPU} opus-4.7 · high\x1b[0m   \x1b[36m{PROFILE} fast\x1b[0m   \x1b[2mlocal\x1b[0m\n{STRIPE}\x1b[2;37m{REFLECTION}\x1b[0m  ctx 196k/\x1b[2m1m\x1b[0m \x1b[32m▰▰\x1b[2m▱▱▱▱▱▱\x1b[0m 20%   \x1b[2m↑ 1.2m in  ↓ 48k out\x1b[0m   \x1b[32m$3.41\x1b[0m   \x1b[2mcache 92%   trimmed 23k tok\x1b[0m"
    )
}

/// The compact `printf` of H§6.3 without its final `\n`.
fn op13_compact() -> String {
    format!(
        "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[35m{CPU} opus-4.7\x1b[0m \x1b[36m{PROFILE} fast\x1b[0m  ctx 196k/\x1b[2m1m\x1b[0m \x1b[32m▰\x1b[2m▱▱▱▱\x1b[0m  \x1b[2m↑1.2m ↓48k\x1b[0m  \x1b[32m$3.41\x1b[0m  \x1b[2m{DB} 92%  ✂ 23k\x1b[0m"
    )
}

/// H§6.4 example 1: compact, threshold 600k, main worktree.
fn example_1() -> String {
    format!(
        "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m  \x1b[35m{CPU} opus-4.7\x1b[0m \x1b[36m{PROFILE} fast\x1b[0m  ctx 196k/\x1b[2m1m\x1b[0m \x1b[32m▰▰\x1b[2m▱▱▱\x1b[0m \x1b[2m⇣600k  ↑1.2m ↓48k\x1b[0m  \x1b[32m$3.41\x1b[0m  \x1b[2m{DB} 92%  ✂ 23k\x1b[0m"
    )
}

/// H§6.4 example 3: compact, OP13 data, status data 100 s old.
fn example_3() -> String {
    format!(
        "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[2m{CPU} opus-4.7\x1b[0m \x1b[36m{PROFILE} fast\x1b[0m  \x1b[2mctx 196k/1m ▰▱▱▱▱  ↑1.2m ↓48k  $3.41  {DB} 92%  ✂ 23k  stale\x1b[0m"
    )
}

// ---- fixtures ----

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

fn golden(name: &str) -> String {
    let path = format!("{DIR}/{name}.ansi");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn fixture_json(name: &str) -> Value {
    let path = format!("{DIR}/{name}.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

/// The fixture inputs as the data of one render: the status reply, its age, the stdin `effort`, the git result, the
/// trimmed count, `ZYGO_WORK` and the profile of `view.json`.
fn data(v: &Value) -> HudData {
    let text = |x: &Value| x.as_str().map(str::to_string);
    let count = |x: &Value| x.as_u64().map(|n| n as u32);
    HudData {
        status: Some(serde_json::from_value(v["status"].clone()).unwrap()),
        stale: v["status_age_s"].as_u64().unwrap() > 90,
        outcome: Outcome::Ok,
        work: text(&v["env"]["ZYGO_WORK"]),
        worktree: text(&v["git"]["worktree"]),
        ahead: count(&v["git"]["ahead"]),
        behind: count(&v["git"]["behind"]),
        branch: text(&v["git"]["branch"]),
        profile: text(&v["view"]["profile"]),
        effort: text(&v["stdin"]["effort"]),
        trimmed_bytes: v["trimmed_bytes"].as_u64(),
        phase: None,
        team_host: None,
    }
}

/// The OP13 data (H§14 #3).
fn op13() -> HudData {
    data(&fixture_json("fixture"))
}

fn width_all() -> HudData {
    data(&fixture_json("width-all"))
}

/// The fixture environment with `extra` on top; an empty value in `extra` removes the key.
fn env_of(v: &Value, extra: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
    let mut vars: Vec<(String, String)> =
        v["env"].as_object().unwrap().iter().map(|(k, x)| (k.clone(), x.as_str().unwrap().to_string())).collect();
    for (k, x) in extra {
        vars.retain(|(have, _)| have != k);
        if !x.is_empty() {
            vars.push((k.to_string(), x.to_string()));
        }
    }
    move |key: &str| vars.iter().find(|(k, _)| k == key).map(|(_, x)| OsString::from(x))
}

fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |key: &str| pairs.iter().find(|(k, _)| *k == key).map(|(_, x)| OsString::from(x))
}

fn no_tty() -> Option<u16> {
    None
}

/// A render environment from an `env` closure, with no terminal probe (a test run can have a real `/dev/tty`).
fn renv_from(env: &dyn Fn(&str) -> Option<OsString>, style: Style) -> RenderEnv {
    RenderEnv { style, icons: icon_set(env), color: color_mode(env), width: terminal_width(env, &no_tty), force_panic: false }
}

fn renv(style: Style, icons: IconSet, color: ColorMode, width: u16) -> RenderEnv {
    RenderEnv { style, icons, color, width, force_panic: false }
}

const C: Style = Style::Compact;
const V: Style = Style::Verbose;

fn nerd(style: Style) -> RenderEnv {
    renv(style, IconSet::Nerd, ColorMode::Ansi256, 200)
}

/// Nerd icons, no color, width 200: the text of a row.
fn text_env(style: Style) -> RenderEnv {
    renv(style, IconSet::Nerd, ColorMode::Off, 200)
}

fn status(d: &mut HudData) -> &mut SessionStatus {
    d.status.as_mut().unwrap()
}

/// OP13 data with a change to its status.
fn with(f: impl FnOnce(&mut SessionStatus)) -> HudData {
    let mut d = op13();
    f(status(&mut d));
    d
}

fn compact_text(d: &HudData) -> String {
    render(d, &text_env(C))
}

fn verbose_text(d: &HudData) -> String {
    render(d, &text_env(V))
}

// ---- a small VT parser (H§14 #3, S1) ----

/// (intensity, foreground): intensity 0 normal, 1 bold, 2 dim; foreground `None` default, a 16-color code, or
/// 1000 + n for `38;5;n`.
type Sty = (u8, Option<u16>);

/// The screen that `s` draws: (row, cell, character, style). A space has no foreground to show, so its style is the
/// empty one.
fn grid(s: &str) -> Vec<(usize, usize, char, Sty)> {
    let (mut out, mut row, mut col, mut sty): (Vec<_>, usize, usize, Sty) = (Vec::new(), 0, 0, (0, None));
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => {
                assert_eq!(it.next(), Some('['), "only CSI sequences: {s:?}");
                let mut params = String::new();
                loop {
                    match it.next().expect("an unfinished escape") {
                        'm' => break,
                        p if p.is_ascii_digit() || p == ';' => params.push(p),
                        other => panic!("not an SGR sequence: {other:?} in {s:?}"),
                    }
                }
                let codes: Vec<u16> = params.split(';').map(|p| if p.is_empty() { 0 } else { p.parse().unwrap() }).collect();
                let mut i = 0;
                while i < codes.len() {
                    match codes[i] {
                        0 => sty = (0, None),
                        1 => sty.0 = 1,
                        2 => sty.0 = 2,
                        22 => sty.0 = 0,
                        39 => sty.1 = None,
                        n @ (30..=37 | 90..=97) => sty.1 = Some(n),
                        38 => {
                            assert_eq!(codes[i + 1], 5, "only 38;5;n: {s:?}");
                            sty.1 = Some(1000 + codes[i + 2]);
                            i += 2;
                        }
                        n => panic!("SGR code {n} in {s:?}"),
                    }
                    i += 1;
                }
            }
            '\n' => {
                row += 1;
                col = 0;
            }
            c => {
                out.push((row, col, c, if c == ' ' { (0, None) } else { sty }));
                col += cells(&c.to_string());
            }
        }
    }
    out
}

/// The text of `s` with every SGR sequence removed.
fn strip(s: &str) -> String {
    grid(s).iter().fold((String::new(), 0), |(mut t, row), cell| {
        if cell.0 != row {
            t.push('\n');
        }
        t.push(cell.2);
        (t, cell.0)
    })
    .0
}

#[test]
fn the_vt_parser_reads_styles_and_cells() {
    let g = grid("\x1b[1;34m\u{e0a0} a\x1b[0m \x1b[38;5;172mb\x1b[2mc\x1b[0m\nd");
    assert_eq!(
        g,
        vec![
            (0, 0, '\u{e0a0}', (1, Some(34))),
            (0, 2, ' ', (0, None)),
            (0, 3, 'a', (1, Some(34))),
            (0, 4, ' ', (0, None)),
            (0, 5, 'b', (0, Some(1172))),
            (0, 6, 'c', (2, Some(1172))),
            (1, 0, 'd', (0, None)),
        ]
    );
    assert_eq!(strip("\x1b[31ma\x1b[0m b\nc"), "a b\nc");
}

// ---- H§14 #1: golden files ----

#[test]
fn h14_01_golden_files_hold_each_render_byte_for_byte() {
    let v = fixture_json("fixture");
    let d = data(&v);
    let mut seen = 0;
    for (style, style_name) in [(C, "compact"), (V, "verbose")] {
        for (font, icons_name) in [("1", "nerd"), ("0", "plain")] {
            for (no_color, color_name) in [("", "color"), ("1", "nocolor")] {
                let env = env_of(&v, &[("ZYGO_NERD_FONT", font), ("NO_COLOR", no_color)]);
                let name = format!("{style_name}-{icons_name}-{color_name}");
                assert_eq!(render(&d, &renv_from(&env, style)), golden(&name), "{name}");
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 8);
    // The two Nerd color files are the `printf` bytes of H§6.3.
    assert_eq!(golden("compact-nerd-color"), op13_compact());
    assert_eq!(golden("verbose-nerd-color"), op13_verbose());
}

// ---- H§14 #2: row starts ----

#[test]
fn h14_02_rows_start_with_the_stripe_then_the_logo_or_its_reflection() {
    let d = op13();
    let logo = format!("\x1b[1;97m{LOGO}\x1b[0m");
    let reflection = format!("\x1b[2;37m{REFLECTION}\x1b[0m");
    let column = |row: &str, mark: &str| cells(&strip(&row[..row.find(mark).unwrap_or_else(|| panic!("{mark:?} in {row:?}"))]));

    let verbose = render(&d, &nerd(V));
    let rows: Vec<&str> = verbose.split('\n').collect();
    assert_eq!(rows.len(), 2);
    let compact = render(&d, &nerd(C));
    assert!(!compact.contains('\n'));
    for row in [rows[0], rows[1], compact.as_str()] {
        assert!(row.starts_with("\x1b[31m▌\x1b[33m▌\x1b[36m▌\x1b[0m "), "{row:?}");
    }
    assert!(rows[0].contains(&logo) && compact.contains(&logo));
    assert!(rows[1].contains(&reflection) && !rows[1].contains(LOGO));
    assert_eq!(column(rows[0], &logo), 4);
    assert_eq!(column(rows[1], &reflection), 4);
    assert_eq!(column(&compact, &logo), 4);

    // Plain set: plain logo; four spaces in place of the reflection, so `ctx` keeps its column.
    let plain = render(&d, &renv(V, IconSet::Plain, ColorMode::Ansi256, 200));
    let rows: Vec<&str> = plain.split('\n').collect();
    assert!(rows[0].contains("\x1b[1;97mzygo\x1b[0m"), "{:?}", rows[0]);
    assert!(rows[1].starts_with(&format!("{STRIPE}      ctx 196k/")), "{:?}", rows[1]);
    assert!(!plain.contains(REFLECTION) && !plain.contains(LOGO));
    let nerd_rows = render(&d, &nerd(V));
    assert_eq!(column(rows[1], "ctx"), column(nerd_rows.split('\n').nth(1).unwrap(), "ctx"));

    // `NO_COLOR=1`: no escape byte and no stripe on any row.
    let v = fixture_json("fixture");
    for style in [C, V] {
        for font in ["1", "0"] {
            let env = env_of(&v, &[("NO_COLOR", "1"), ("ZYGO_NERD_FONT", font)]);
            let out = render(&d, &renv_from(&env, style));
            assert!(!out.contains('\x1b') && !out.contains('▌'), "{out:?}");
            let start = if font == "1" { LOGO } else { "zygo" };
            assert!(out.starts_with(start), "{out:?}");
        }
    }
    let quiet = render(&d, &text_env(V));
    assert!(quiet.split('\n').nth(1).unwrap().starts_with(&format!("{REFLECTION}  ctx")), "{quiet:?}");
}

// ---- H§14 #3: the OP13 mockups ----

#[test]
fn h14_03_op13_data_draws_the_screen_of_the_two_printf_commands() {
    let d = op13();
    assert_eq!(grid(&render(&d, &nerd(V))), grid(&op13_verbose()));
    assert_eq!(grid(&render(&d, &nerd(C))), grid(&op13_compact()));
    // OP14: no dirty mark.
    assert!(!render(&d, &nerd(V)).contains('●') && !render(&d, &nerd(C)).contains('●'));
}

// ---- H§14 #4: emission examples 1 and 3 ----

/// Example 1 data: OP13 with a threshold of 600k, in the main worktree.
fn threshold_case() -> HudData {
    let mut d = with(|s| {
        s.threshold = Some(600_000);
        s.effective_window = Some(600_000);
    });
    d.worktree = None;
    d
}

/// Example 3 data: OP13 with status data 100 s old.
fn stale_case() -> HudData {
    HudData { stale: true, ..op13() }
}

#[test]
fn h14_04_emission_examples_1_and_3_are_golden() {
    let one = render(&threshold_case(), &nerd(C));
    assert_eq!(golden("example-1-threshold"), example_1());
    assert_eq!(grid(&one), grid(&example_1()));
    assert_eq!(one, golden("example-1-threshold"));

    let three = render(&stale_case(), &nerd(C));
    assert_eq!(golden("example-3-stale"), example_3());
    assert_eq!(grid(&three), grid(&example_3()));
    assert_eq!(three, golden("example-3-stale"));
    // The emitter's transitions and gap absorption have their unit test in `statusline::render` (`--lib`).
}

// ---- H§14 #8: ctx and threshold (render part) ----

#[test]
fn h14_08_ctx_numbers_bar_and_threshold_mark() {
    // Model window 1,000,000, used 196,000, no threshold.
    let none = compact_text(&op13());
    assert!(none.contains("ctx 196k/1m ▰▱▱▱▱") && !none.contains('⇣'), "{none}");
    // A threshold below the window.
    let set = with(|s| (s.threshold, s.effective_window) = (Some(600_000), Some(600_000)));
    assert!(compact_text(&set).contains("ctx 196k/1m ▰▰▱▱▱ ⇣600k  "), "{}", compact_text(&set));
    assert!(verbose_text(&set).contains("ctx 196k/1m ▰▰▰▱▱▱▱▱ 33% ⇣600k   "), "{}", verbose_text(&set));
    // Override `off`.
    let off = with(|s| s.compact = Some(WindowArg::Off));
    assert!(compact_text(&off).contains("ctx 196k/1m ▰▱▱▱▱ ⇣off  "), "{}", compact_text(&off));
    // A threshold that is not below the window.
    let at = with(|s| (s.threshold, s.effective_window) = (Some(1_000_000), Some(1_000_000)));
    assert!(!compact_text(&at).contains('⇣') && compact_text(&at).contains("ctx 196k/1m ▰▱▱▱▱"));

    // Safety only: window 200,000, `effective_window` 180,000, no `threshold`, used 90,000 → no mark, fraction 0.5.
    let safety = with(|s| {
        (s.context_window, s.effective_window, s.threshold, s.used_context) = (Some(200_000), Some(180_000), None, Some(90_000));
    });
    assert!(verbose_text(&safety).contains("ctx 90k/200k ▰▰▰▰▱▱▱▱ 50%   "), "{}", verbose_text(&safety));
    assert!(!verbose_text(&safety).contains('⇣') && !compact_text(&safety).contains('⇣'));

    // Model window unknown: `ctx 196k/-`, no bar, no percent, no numeric mark.
    let unknown = with(|s| (s.context_window, s.effective_window, s.threshold) = (None, None, Some(600_000)));
    assert!(compact_text(&unknown).contains("ctx 196k/-  ↑1.2m"), "{}", compact_text(&unknown));
    assert!(verbose_text(&unknown).contains("ctx 196k/-   ↑ 1.2m in"), "{}", verbose_text(&unknown));
    for out in [compact_text(&unknown), verbose_text(&unknown)] {
        assert!(!out.contains('▰') && !out.contains('▱') && !out.contains('⇣') && !out.contains("20%"), "{out}");
    }
    // Window unknown with `compact = off`.
    let unknown_off = with(|s| (s.context_window, s.effective_window, s.compact) = (None, None, Some(WindowArg::Off)));
    assert!(compact_text(&unknown_off).contains("ctx 196k/- ⇣off  "), "{}", compact_text(&unknown_off));
    assert!(verbose_text(&unknown_off).contains("ctx 196k/- ⇣off   "), "{}", verbose_text(&unknown_off));

    // `used_context` unknown: `ctx -/1m`, no bar, no percent.
    let unused = with(|s| s.used_context = None);
    for out in [compact_text(&unused), verbose_text(&unused)] {
        assert!(out.contains("ctx -/1m  "), "{out}");
        assert!(!out.contains('▰') && !out.contains('▱'), "{out}");
    }
    // Window known: the line never holds `/-`.
    for d in [op13(), set, off, at, safety, unused] {
        assert!(!compact_text(&d).contains("/-") && !verbose_text(&d).contains("/-"));
    }

    // Soft 190,000 with safety 180,000 on a 200,000 window: the status has `threshold` 180,000.
    let clamped = with(|s| (s.context_window, s.effective_window, s.threshold) = (Some(200_000), Some(180_000), Some(180_000)));
    assert!(compact_text(&clamped).contains("ctx 196k/200k ▰▰▰▰▰ ⇣180k  "), "{}", compact_text(&clamped));

    // `[1m]` on a 200k model: `context_window` 200,000, `effective_window` 180,000, used 150,000 → 150/180.
    let one_m = with(|s| {
        (s.context_window, s.effective_window, s.threshold, s.used_context) = (Some(200_000), Some(180_000), None, Some(150_000));
    });
    assert!(verbose_text(&one_m).contains("ctx 150k/200k ▰▰▰▰▰▰▰▱ 83%   "), "{}", verbose_text(&one_m));
    assert!(render(&one_m, &nerd(V)).contains("\x1b[33m▰▰▰▰▰▰▰\x1b[2m▱\x1b[0m 83%"));

    // The window number is dim; `used/` has the terminal default (H§6.4).
    assert!(render(&op13(), &nerd(C)).contains("\x1b[0m  ctx 196k/\x1b[2m1m\x1b[0m "));
    // The plain set marks the threshold with U+25BC.
    let plain = render(&threshold_case(), &renv(C, IconSet::Plain, ColorMode::Off, 200));
    assert!(plain.contains("ctx 196k/1m ■■□□□ ▼600k  "), "{plain}");
}

// ---- H§14 #9: bar ----

#[test]
fn h14_09_bar_colors_and_fill() {
    let used = |n: u64| with(|s| s.used_context = Some(n));
    // Fractions 0.59, 0.60, 0.84, 0.85 of a 1m window.
    for (n, sgr) in [(590_000, "32"), (600_000, "33"), (840_000, "33"), (850_000, "31")] {
        for style in [C, V] {
            let out = render(&used(n), &nerd(style));
            assert!(out.contains(&format!("\x1b[{sgr}m▰")), "{n} {sgr}: {out:?}");
            assert_eq!(out.matches("m▰").count(), 1, "{out:?}");
        }
    }
    // 20 %: 1 of 5 (compact), 2 of 8 and `20%` (verbose), as OP13.
    assert!(compact_text(&op13()).contains(" ▰▱▱▱▱  "));
    assert!(verbose_text(&op13()).contains(" ▰▰▱▱▱▱▱▱ 20%   "));
    assert!(render(&op13(), &nerd(C)).contains("\x1b[32m▰\x1b[2m▱▱▱▱\x1b[0m"));
    assert!(render(&op13(), &nerd(V)).contains("\x1b[32m▰▰\x1b[2m▱▱▱▱▱▱\x1b[0m 20%"));
    // One token: one cell.
    assert!(compact_text(&used(1)).contains("ctx 1/1m ▰▱▱▱▱  "), "{}", compact_text(&used(1)));
    assert!(verbose_text(&used(1)).contains("ctx 1/1m ▰▱▱▱▱▱▱▱ 0%   "), "{}", verbose_text(&used(1)));
    // No token: no filled cell; the empty cells are dim with the default foreground.
    // The window number, the cells and the tokens are then one dim run (gap absorption).
    assert!(render(&used(0), &nerd(C)).contains("ctx 0/\x1b[2m1m ▱▱▱▱▱  ↑1.2m ↓48k\x1b[0m  "), "{:?}", render(&used(0), &nerd(C)));
    assert!(verbose_text(&used(0)).contains("ctx 0/1m ▱▱▱▱▱▱▱▱ 0%   "));
    // Used above the threshold: every cell, red.
    let over = with(|s| (s.threshold, s.effective_window, s.used_context) = (Some(600_000), Some(600_000), Some(700_000)));
    assert!(compact_text(&over).contains(" ▰▰▰▰▰ ⇣600k"), "{}", compact_text(&over));
    assert!(verbose_text(&over).contains(" ▰▰▰▰▰▰▰▰ 117% ⇣600k"), "{}", verbose_text(&over));
    assert!(render(&over, &nerd(C)).contains("\x1b[31m▰▰▰▰▰\x1b[0m \x1b[2m⇣600k"), "{:?}", render(&over, &nerd(C)));
    // AU17: threshold 600k and used 300k is a fraction of 0.5, not 0.3.
    let half = with(|s| (s.threshold, s.effective_window, s.used_context) = (Some(600_000), Some(600_000), Some(300_000)));
    assert!(verbose_text(&half).contains("ctx 300k/1m ▰▰▰▰▱▱▱▱ 50% ⇣600k"), "{}", verbose_text(&half));
    assert!(compact_text(&half).contains("ctx 300k/1m ▰▰▰▱▱ ⇣600k"), "{}", compact_text(&half));
    // The plain set draws the bar with U+25A0 and U+25A1.
    assert!(render(&op13(), &renv(V, IconSet::Plain, ColorMode::Off, 200)).contains(" ■■□□□□□□ 20%"));
}

// ---- H§14 #10: spend format ----

#[test]
fn h14_10_spend_format() {
    let plain = |usd: f64| spend_text(Some(usd), Some(false), Some(1), Some(1)).unwrap();
    for (usd, want) in [(3.414, "$3.41"), (99.994, "$99.99"), (99.996, "$100"), (0.004, "<$0.01"), (0.005, "$0.01"), (0.0, "$0.00")] {
        assert_eq!(plain(usd), want, "{usd}");
    }
    assert_eq!(plain(123.456), "$123");
    assert_eq!(plain(100.0), "$100");
    assert_eq!(plain(3.41), "$3.41");
    // The HUD prints the same text in SGR `32`.
    for style in [C, V] {
        assert!(render(&with(|s| s.spend_usd = Some(3.414)), &nerd(style)).contains("\x1b[32m$3.41\x1b[0m"));
    }
    assert!(compact_text(&with(|s| s.spend_usd = Some(99.996))).contains("  $100  "));
}

// ---- H§14 #12: price sanity (render part) ----

#[test]
fn h14_12_spend_above_99999_has_a_fixed_form() {
    assert_eq!(spend_text(Some(123_456.0), None, Some(3), Some(3)).as_deref(), Some(">$99999"));
    assert_eq!(spend_text(Some(99_999.0), None, Some(3), Some(3)).as_deref(), Some("$99999"));
    assert_eq!(spend_text(Some(f64::INFINITY), None, Some(3), Some(3)).as_deref(), Some(">$99999"));
    let big = with(|s| s.spend_usd = Some(123_456.0));
    assert!(compact_text(&big).contains("  >$99999  "), "{}", compact_text(&big));
    assert!(render(&big, &nerd(V)).contains("\x1b[32m>$99999\x1b[0m"));
    // A sum that is not a number is not a price.
    assert_eq!(spend_text(Some(f64::NAN), None, Some(3), Some(3)).as_deref(), Some("$?"));
    assert_eq!(spend_text(Some(-1.0), None, Some(3), Some(3)).as_deref(), Some("$?"));
}

// ---- H§14 #20: compact width fit; width sources ----

#[test]
fn h14_20_compact_drops_segments_in_order_and_width_sources() {
    let v = fixture_json("width-all");
    let d = data(&v);
    let head = format!("▌▌▌ {LOGO}");
    let (work, tree) = (format!(" {BRANCH} main"), format!(" {TREE} hud-work"));
    let (model, profile) = (format!("{CPU} opus-4.7"), format!("{PROFILE} fast"));
    let (ctx, bar, mark, tokens, spend) = ("ctx 196k/1m", "▰▰▱▱▱", "⇣600k", "↑1.2m ↓48k", "$3.41");
    let (cache, trimmed) = (format!("{DB} 92%"), "✂ 23k");
    // One step per segment, in the H§6.7 order.
    let steps = [
        ("all", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  {cache}  {trimmed}  stale")),
        ("trimmed", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  {cache}  stale")),
        ("cache", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {tokens}  {spend}  stale")),
        ("tokens", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar} {mark}  {spend}  stale")),
        ("threshold", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar}  {spend}  stale")),
        ("spend", format!("{head}{work}{tree}  {model} {profile}  {ctx} {bar}  stale")),
        ("profile", format!("{head}{work}{tree}  {model}  {ctx} {bar}  stale")),
        ("model", format!("{head}{work}{tree}  {ctx} {bar}  stale")),
        ("bar cells", format!("{head}{work}{tree}  {ctx}  stale")),
        ("stale", format!("{head}{work}{tree}  {ctx}")),
        ("worktree", format!("{head}{work}  {ctx}")),
        ("ctx numbers", format!("{head}{work}")),
    ];
    let at = |columns: usize| {
        let columns = columns.to_string();
        let env = env_of(&v, &[("COLUMNS", columns.as_str())]);
        strip(&render(&d, &renv_from(&env, C)))
    };
    assert_eq!(cells(&steps[0].1), 115);
    assert_eq!(at(200), steps[0].1);
    for pair in steps.windows(2) {
        let (before, (name, after)) = (&pair[0].1, &pair[1]);
        // The row before this step still fits at its own width; one cell less drops exactly this segment.
        assert_eq!(&at(cells(before)), before, "before {name}");
        assert_eq!(&at(cells(before) - 1), after, "{name}");
    }
    // `COLUMNS=20`: stripe, logo, work item; 16 cells.
    assert_eq!(at(20), format!("▌▌▌ {LOGO} {BRANCH} main"));
    assert_eq!(cells(&at(20)), 16);
    // Below that the work item goes last; the stripe and the logo stay, and the row is printed as it is.
    assert_eq!(strip(&render(&d, &renv(C, IconSet::Nerd, ColorMode::Ansi256, 15))), head);
    assert_eq!(strip(&render(&d, &renv(C, IconSet::Nerd, ColorMode::Ansi256, 1))), head);

    // The phase stays at every `COLUMNS`; so do the stripe and the logo.
    let draining = HudData { phase: Some("draining".into()), ..data(&v) };
    for columns in [200, 115, 100, 80, 60, 40, 30, 20] {
        let env = env_of(&v, &[("COLUMNS", columns.to_string().as_str())]);
        let out = strip(&render(&draining, &renv_from(&env, C)));
        assert!(out.starts_with(&format!("{head} ws draining")), "{columns}: {out}");
        assert!(at(columns).starts_with(&head), "{columns}");
    }
    let env = env_of(&v, &[("COLUMNS", "20")]);
    assert_eq!(strip(&render(&draining, &renv_from(&env, C))), format!("{head} ws draining"));
    // `offline` is never removed.
    let offline = HudData { status: None, outcome: Outcome::Offline, ..data(&v) };
    assert_eq!(strip(&render(&offline, &renv(C, IconSet::Nerd, ColorMode::Ansi256, 20))), format!("{head}  offline"));

    // Width sources: `/dev/tty`, then `COLUMNS`, then 120; a value outside 20 to 1000 is skipped.
    let tty = |n: u16| move || Some(n);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "110")]), &tty(90)), 90);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "110")]), &no_tty), 110);
    assert_eq!(terminal_width(&vars(&[]), &no_tty), 120);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "110")]), &tty(5)), 110);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "110")]), &tty(5000)), 110);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "5")]), &tty(90)), 90);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "5")]), &no_tty), 120);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "5000")]), &no_tty), 120);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "abc")]), &no_tty), 120);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "")]), &no_tty), 120);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "5")]), &tty(5000)), 120);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "20")]), &tty(19)), 20);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "1001")]), &tty(1000)), 1000);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "1000")]), &tty(1001)), 1000);
}

// ---- H§14 #21: verbose width fit ----

#[test]
fn h14_21_verbose_rows_fit_separately() {
    let d = width_all();
    let at = |d: &HudData, width: usize| -> Vec<String> {
        strip(&render(d, &renv(V, IconSet::Nerd, ColorMode::Ansi256, width as u16))).split('\n').map(str::to_string).collect()
    };
    let (work, tree) = (format!("{BRANCH} main"), format!("{TREE} hud-work"));
    let (model, profile) = (format!("{CPU} opus-4.7"), format!("{PROFILE} fast"));
    let h1 = format!("▌▌▌ {LOGO}");
    let row1 = [
        ("all", format!("{h1}  {work} {tree} ↑1   {model} · high   {profile}   local")),
        ("mode", format!("{h1}  {work} {tree} ↑1   {model} · high   {profile}")),
        ("profile", format!("{h1}  {work} {tree} ↑1   {model} · high")),
        ("effort", format!("{h1}  {work} {tree} ↑1   {model}")),
        ("ahead/behind", format!("{h1}  {work} {tree}   {model}")),
        ("model", format!("{h1}  {work} {tree}")),
        ("worktree", format!("{h1}  {work}")),
        ("work item", h1.clone()),
    ];
    let h2 = format!("▌▌▌ {REFLECTION}");
    let (ctx, bar, tokens) = ("ctx 196k/1m", "▰▰▰▱▱▱▱▱", "↑ 1.2m in  ↓ 48k out");
    let row2 = [
        ("all", format!("{h2}  {ctx} {bar} 33% ⇣600k   {tokens}   $3.41   cache 92%   trimmed 23k tok   stale")),
        ("trimmed", format!("{h2}  {ctx} {bar} 33% ⇣600k   {tokens}   $3.41   cache 92%   stale")),
        ("cache", format!("{h2}  {ctx} {bar} 33% ⇣600k   {tokens}   $3.41   stale")),
        ("tokens", format!("{h2}  {ctx} {bar} 33% ⇣600k   $3.41   stale")),
        ("spend", format!("{h2}  {ctx} {bar} 33% ⇣600k   stale")),
        ("percent", format!("{h2}  {ctx} {bar} ⇣600k   stale")),
        ("threshold", format!("{h2}  {ctx} {bar}   stale")),
        ("bar cells", format!("{h2}  {ctx}   stale")),
        ("stale", format!("{h2}  {ctx}")),
        ("ctx numbers", h2.clone()),
    ];
    assert_eq!(at(&d, 200), vec![row1[0].1.clone(), row2[0].1.clone()]);
    for (line, steps) in [(0, &row1[..]), (1, &row2[..])] {
        for pair in steps.windows(2) {
            let (before, (name, after)) = (&pair[0].1, &pair[1]);
            assert_eq!(&at(&d, cells(before))[line], before, "row {} before {name}", line + 1);
            assert_eq!(&at(&d, cells(before) - 1)[line], after, "row {} {name}", line + 1);
        }
    }
    // Row 2 is the wider row here: it drops segments at widths where row 1 is whole.
    assert!(cells(&row2[0].1) > cells(&row1[0].1));
    let rows = at(&d, cells(&row1[0].1));
    assert_eq!(rows[0], row1[0].1);
    assert_ne!(rows[1], row2[0].1);
    // When row 1 drops a segment, row 2 does not change.
    let short = HudData { trimmed_bytes: None, ..width_all() };
    let short = HudData {
        status: short.status.map(|s| SessionStatus { tokens_in: None, tokens_out: None, spend_usd: None, cache_read_share: None, ..s }),
        ..short
    };
    let whole = at(&short, 200);
    assert_eq!(whole[1], format!("{h2}  {ctx} {bar} 33% ⇣600k   stale"));
    let narrow = at(&short, cells(&whole[1]));
    assert_eq!(narrow[1], whole[1]);
    assert_eq!(narrow[0], row1[3].1, "row 1 dropped mode, profile and effort");
    // The reflection keeps its column at every width; the logo and the stripe stay.
    for width in [200, 120, 100, 80, 60, 40, 30, 20, 10] {
        let rows = at(&d, width);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with(&h1) && rows[1].starts_with(&h2), "{width}: {rows:?}");
    }
    // The plain set keeps the column with four spaces.
    let plain = strip(&render(&d, &renv(V, IconSet::Plain, ColorMode::Ansi256, 30)));
    assert!(plain.split('\n').nth(1).unwrap().starts_with("▌▌▌       ctx 196k/1m"), "{plain:?}");
}

// ---- H§14 #29: mode ----

#[test]
fn h14_29_mode_segment() {
    let mode = |passthrough: Option<bool>, team: Option<bool>, host: Option<&str>| {
        let mut d = with(|s| (s.passthrough, s.team_session) = (passthrough, team));
        d.team_host = host.map(str::to_string);
        render(&d, &nerd(V)).split('\n').next().unwrap().to_string()
    };
    assert!(mode(Some(true), Some(false), None).ends_with("   \x1b[2mpassthrough\x1b[0m"));
    assert!(mode(Some(false), Some(true), Some("zygo.example.org")).ends_with("   \x1b[32mserve zygo.example.org\x1b[0m"));
    assert!(mode(Some(false), Some(true), Some("127.0.0.1")).ends_with("   \x1b[32mserve 127.0.0.1\x1b[0m"));
    assert!(mode(Some(false), Some(false), Some("127.0.0.1")).ends_with("   \x1b[2mlocal\x1b[0m"));
    // First match wins.
    assert!(mode(Some(true), Some(true), Some("h")).ends_with("\x1b[2mpassthrough\x1b[0m"));
    // Either field absent: no mode segment.
    for (p, t) in [(None, Some(true)), (Some(true), None), (None, None), (None, Some(false)), (Some(false), None)] {
        let row = strip(&mode(p, t, Some("zygo.example.org")));
        assert!(row.ends_with(&format!("{PROFILE} fast")), "{p:?} {t:?}: {row}");
    }
    // No status data: no mode segment.
    let offline = HudData { status: None, outcome: Outcome::Offline, team_host: Some("h".into()), ..op13() };
    let row = verbose_text(&offline);
    assert!(!row.contains("local") && !row.contains("serve") && !row.contains("passthrough"), "{row}");
    // Compact has no mode segment.
    let compact = compact_text(&op13());
    assert!(!compact.contains("local"), "{compact}");
    // The host is escaped and cut to 32 cells.
    let long = mode(Some(false), Some(true), Some(&format!("a\x1b{}", "b".repeat(40))));
    assert!(strip(&long).ends_with(&format!("serve a\\u{{1b}}{}…", "b".repeat(24))), "{long:?}");
}

// ---- H§14 #31: icon set ----

#[test]
fn h14_31_icon_set_selection() {
    assert_eq!(icon_set(&vars(&[("ZYGO_NERD_FONT", "0")])), IconSet::Plain);
    assert_eq!(icon_set(&vars(&[("ZYGO_NERD_FONT", "0"), ("TERM", "xterm-256color"), ("WT_SESSION", "x")])), IconSet::Plain);
    assert_eq!(icon_set(&vars(&[("ZYGO_NERD_FONT", "1"), ("TERM", "linux")])), IconSet::Nerd);
    assert_eq!(icon_set(&vars(&[("ZYGO_NERD_FONT", "1"), ("TERM", "dumb")])), IconSet::Nerd);
    // A Windows build with no `WT_SESSION`: `1` forces the Nerd set.
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "1")]), true), IconSet::Nerd);
    // Unset: automatic.
    for windows in [false, true] {
        assert_eq!(icon_set_on(&vars(&[("TERM", "linux"), ("WT_SESSION", "x")]), windows), IconSet::Plain);
        assert_eq!(icon_set_on(&vars(&[("TERM", "dumb"), ("TERM_PROGRAM", "vscode")]), windows), IconSet::Plain);
    }
    assert_eq!(icon_set_on(&vars(&[]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("TERM", "xterm-256color")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("WT_SESSION", "x")]), true), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("TERM_PROGRAM", "vscode")]), true), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[]), false), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("TERM", "xterm-256color")]), false), IconSet::Nerd);
    // Any other value: the automatic rules.
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "yes"), ("TERM", "linux")]), false), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "yes"), ("TERM", "xterm")]), false), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "yes")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "")]), false), IconSet::Nerd);
    // `icon_set` is the rule for this build.
    for env in [vars(&[]), vars(&[("TERM", "linux")]), vars(&[("WT_SESSION", "x")])] {
        assert_eq!(icon_set(&env), icon_set_on(&env, cfg!(windows)));
    }
    // `NO_COLOR=1` alone removes color only: Nerd icons, no escapes.
    let quiet = vars(&[("NO_COLOR", "1")]);
    assert_eq!((icon_set_on(&quiet, false), color_mode(&quiet)), (IconSet::Nerd, ColorMode::Off));
    let out = render(&op13(), &renv(C, icon_set_on(&quiet, false), color_mode(&quiet), 200));
    assert!(out.contains(BRANCH) && out.contains(DB) && out.contains(LOGO) && !out.contains('\x1b'), "{out:?}");
    // The render follows the set.
    let plain = render(&op13(), &renv(C, icon_set(&vars(&[("ZYGO_NERD_FONT", "0")])), ColorMode::Off, 200));
    assert_eq!(plain, golden("compact-plain-nocolor"));
    assert!(plain.chars().all(|c| !matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0x10FFFF)), "{plain:?}");
}

// ---- contract: environment, colors, seams ----

#[test]
fn color_mode_follows_no_color_and_au23() {
    assert_eq!(color_mode(&vars(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")])), ColorMode::Off);
    assert_eq!(color_mode(&vars(&[("NO_COLOR", ""), ("COLORTERM", "truecolor")])), ColorMode::Ansi256);
    assert_eq!(color_mode(&vars(&[("COLORTERM", "24bit")])), ColorMode::Ansi256);
    assert_eq!(color_mode(&vars(&[("TERM", "screen-256color")])), ColorMode::Ansi256);
    assert_eq!(color_mode(&vars(&[("WT_SESSION", "x")])), ColorMode::Ansi256);
    assert_eq!(color_mode(&vars(&[("TERM", "xterm")])), ColorMode::Ansi16);
    assert_eq!(color_mode(&vars(&[("TERM", "xterm"), ("COLORTERM", "yes")])), ColorMode::Ansi16);
    assert_eq!(color_mode(&vars(&[])), ColorMode::Ansi16);
    // The worktree: 172 with 256 colors, yellow with 16.
    let v = fixture_json("fixture");
    let rich = render(&op13(), &renv_from(&env_of(&v, &[("TERM", "xterm")]), C));
    assert!(rich.contains(&format!("\x1b[38;5;172m{TREE} hud-work\x1b[0m")), "{rich:?}");
    let basic = render(&op13(), &renv_from(&env_of(&v, &[("TERM", "xterm"), ("COLORTERM", "")]), C));
    assert!(basic.contains(&format!("\x1b[33m{TREE} hud-work\x1b[0m")) && !basic.contains("38;5"), "{basic:?}");
}

#[test]
fn from_env_reads_everything_through_its_closure() {
    let env = vars(&[("ZYGO_NERD_FONT", "0"), ("NO_COLOR", "1"), ("COLUMNS", "77"), ("ZYGO_TEST_RENDER_PANIC", "0")]);
    let e = RenderEnv::from_env(&env, V);
    assert_eq!((e.style, e.icons, e.color, e.force_panic), (V, IconSet::Plain, ColorMode::Off, false));
    // The width comes from the terminal when the test run has one, else from `COLUMNS`.
    assert!((20..=1000).contains(&e.width));
    assert!(RenderEnv::from_env(&vars(&[("ZYGO_TEST_RENDER_PANIC", "1")]), C).force_panic);
    assert!(!RenderEnv::from_env(&vars(&[]), C).force_panic);
    assert!(!RenderEnv::from_env(&vars(&[("ZYGO_TEST_RENDER_PANIC", "true")]), C).force_panic);
    assert_eq!((Style::parse("compact"), Style::parse("verbose"), Style::parse("wide")), (Some(C), Some(V), None));
}

#[test]
fn the_forced_panic_seam_panics_in_render() {
    let env = RenderEnv::from_env(&vars(&[("ZYGO_TEST_RENDER_PANIC", "1")]), C);
    let d = op13();
    assert!(std::panic::catch_unwind(|| render(&d, &env)).is_err());
    // Without the seam the same data renders.
    assert_eq!(render(&d, &RenderEnv { force_panic: false, ..nerd(C) }), op13_compact());
}

#[test]
fn the_glyph_table_names_each_icon_of_a_set() {
    let table = |set| glyphs(set).iter().copied().collect::<std::collections::BTreeMap<_, _>>();
    let (n, p) = (table(IconSet::Nerd), table(IconSet::Plain));
    assert_eq!(n.keys().collect::<Vec<_>>(), p.keys().collect::<Vec<_>>());
    assert_eq!(n.len(), glyphs(IconSet::Nerd).len(), "no name twice");
    for (name, nerd, plain) in [
        ("stripe", "▌", "▌"),
        ("logo", LOGO, "zygo"),
        ("reflection", REFLECTION, ""),
        ("branch", BRANCH, ""),
        ("github", "\u{f408}", ""),
        ("work-graph", "◆", "♦"),
        ("bead", "\u{f41b}", "○"),
        ("worktree", TREE, "⌂"),
        ("ahead", "↑", "↑"),
        ("behind", "↓", "↓"),
        ("model", CPU, ""),
        ("profile", PROFILE, "≡"),
        ("bar-filled", "▰", "■"),
        ("bar-empty", "▱", "□"),
        ("threshold", "⇣", "▼"),
        ("notional", "≈", "≈"),
        ("cache", DB, "cache"),
        ("trimmed", "✂", "trim"),
        ("truncation", "…", "…"),
    ] {
        assert_eq!((n[name], p[name]), (nerd, plain), "{name}");
    }
    assert_eq!(n.len(), 19);
}

// ---- segments ----

fn worked(work: &str, branch: Option<&str>) -> HudData {
    HudData { work: Some(work.into()), branch: branch.map(str::to_string), worktree: None, ..op13() }
}

/// The compact row up to the model group.
fn first_group(d: &HudData, env: &RenderEnv) -> String {
    let out = render(d, env);
    let cut = out.find(if env.icons == IconSet::Nerd { CPU } else { "opus" }).unwrap();
    strip(&out[..cut]).trim_end().to_string()
}

#[test]
fn work_item_markers() {
    let plain = renv(C, IconSet::Plain, ColorMode::Off, 200);
    let cases = [
        ("branch:main", format!("{BRANCH} main"), "main"),
        ("github:o/n#123", "\u{f408} #123".to_string(), "#123"),
        ("github:a#b#7", "\u{f408} #7".to_string(), "#7"),
        ("github:o/n", "github:o/n".to_string(), "github:o/n"),
        ("work-graph:hud/N3", "◆ hud/N3".to_string(), "♦ hud/N3"),
        ("beads:zy-a1b2", "\u{f41b} zy-a1b2".to_string(), "○ zy-a1b2"),
        ("x:y", "x:y".to_string(), "x:y"),
        ("nocolon", "nocolon".to_string(), "nocolon"),
    ];
    for (work, nerd_text, plain_text) in cases {
        let d = worked(work, None);
        assert_eq!(first_group(&d, &text_env(C)), format!("{LOGO} {nerd_text}"), "{work}");
        assert_eq!(first_group(&d, &plain), format!("zygo {plain_text}"), "{work}");
        assert!(render(&d, &nerd(C)).contains(&format!("\x1b[1;34m{nerd_text}\x1b[0m")), "{work}");
    }
    assert!(!compact_text(&worked("branch:main", None)).contains("branch:"));
    // The live branch replaces the launch-time id of the `branch` source only.
    assert_eq!(first_group(&worked("branch:main", Some("feat")), &text_env(C)), format!("{LOGO} {BRANCH} feat"));
    assert_eq!(first_group(&worked("github:o/n#123", Some("feat")), &text_env(C)), format!("{LOGO} \u{f408} #123"));
    // No `ZYGO_WORK`: the status `work`, then the live branch in plain blue, then nothing.
    let d = HudData { work: Some(String::new()), worktree: None, ..with(|s| s.work = Some("beads:zy-9".into())) };
    assert_eq!(first_group(&d, &text_env(C)), format!("{LOGO} \u{f41b} zy-9"));
    let live = HudData { work: Some(String::new()), branch: Some("feat".into()), worktree: None, ..with(|s| s.work = None) };
    assert!(render(&live, &nerd(C)).contains(&format!("\x1b[0m \x1b[34m{BRANCH} feat\x1b[0m  ")), "{:?}", render(&live, &nerd(C)));
    assert_eq!(first_group(&live, &plain), "zygo feat");
    let none = HudData { work: None, branch: None, worktree: None, ..with(|s| s.work = None) };
    assert_eq!(first_group(&none, &text_env(C)), LOGO);
    assert!(compact_text(&none).starts_with(&format!("{LOGO}  {CPU} opus-4.7 ")), "{}", compact_text(&none));
    // Escaped, then cut to 32 cells.
    let forged = worked("x\x1b[31m\ny", None);
    assert_eq!(first_group(&forged, &text_env(C)), format!("{LOGO} x\\u{{1b}}[31m\\ny"));
    let long = worked(&format!("work-graph:{}", "n".repeat(40)), None);
    assert_eq!(first_group(&long, &text_env(C)), format!("{LOGO} ◆ {}…", "n".repeat(31)));
}

#[test]
fn worktree_git_marks_model_profile() {
    // Worktree: escaped, 24 cells at most; none in the main worktree.
    let tree = |name: &str| HudData { worktree: Some(name.into()), ..op13() };
    assert!(compact_text(&tree("x\x1b[31my")).contains(&format!("main {TREE} x\\u{{1b}}[31my  ")));
    let long = compact_text(&tree(&"w".repeat(40)));
    assert!(long.contains(&format!("{TREE} {}…  ", "w".repeat(23))), "{long}");
    assert!(!compact_text(&HudData { worktree: None, ..op13() }).contains(TREE));
    let plain = render(&op13(), &renv(C, IconSet::Plain, ColorMode::Off, 200));
    assert!(plain.starts_with("zygo main ⌂ hud-work  opus-4.7 ≡ fast  "), "{plain}");

    // Git marks: verbose only, cyan, after the worktree.
    let marks = |ahead: Option<u32>, behind: Option<u32>| HudData { ahead, behind, ..op13() };
    assert!(render(&marks(Some(1), Some(0)), &nerd(V)).contains(&format!("{TREE} hud-work\x1b[0m \x1b[36m↑1\x1b[0m   ")));
    assert!(verbose_text(&marks(Some(0), Some(2))).contains("hud-work ↓2   "));
    assert!(verbose_text(&marks(Some(3), Some(2))).contains("hud-work ↑3 ↓2   "));
    let quiet = verbose_text(&marks(None, None));
    assert!(quiet.contains(&format!("hud-work   {CPU}")) && !quiet.contains("↑1"), "{quiet}");
    assert!(verbose_text(&marks(Some(0), Some(0))).contains(&format!("hud-work   {CPU}")));
    assert!(!compact_text(&marks(Some(7), Some(9))).contains("↑7") && !compact_text(&marks(Some(7), Some(9))).contains("↓9"));
    assert!(verbose_text(&marks(Some(7), Some(9))).contains("hud-work ↑7 ↓9   "));

    // Model: the served model without a leading `claude-`; effort in verbose only; none without a status `model`.
    assert!(compact_text(&op13()).contains(&format!("{CPU} opus-4.7 {PROFILE}")));
    assert!(verbose_text(&op13()).contains(&format!("{CPU} opus-4.7 · high   ")));
    assert!(verbose_text(&HudData { effort: None, ..op13() }).contains(&format!("{CPU} opus-4.7   ")));
    let other = with(|s| s.model = Some("gpt-5.5\x07".into()));
    assert!(compact_text(&other).contains(&format!("{CPU} gpt-5.5\\u{{07}} ")));
    let no_model = with(|s| s.model = None);
    assert!(!verbose_text(&no_model).contains(CPU) && !verbose_text(&no_model).contains("high"));
    let wide = HudData { effort: Some("e".repeat(20)), ..with(|s| s.model = Some(format!("claude-{}", "m".repeat(40)))) };
    assert!(verbose_text(&wide).contains(&format!("{CPU} {}… · {}…   ", "m".repeat(23), "e".repeat(7))));

    // Profile: `view.json`, else the status; 16 cells at most.
    let fallback = HudData { profile: None, ..with(|s| s.profile = Some("deep".into())) };
    assert!(compact_text(&fallback).contains(&format!("{PROFILE} deep  ")));
    let none = HudData { profile: None, ..with(|s| s.profile = None) };
    assert!(!compact_text(&none).contains(PROFILE));
    let long = HudData { profile: Some("p".repeat(30)), ..op13() };
    assert!(compact_text(&long).contains(&format!("{PROFILE} {}…  ", "p".repeat(15))));
}

#[test]
fn tokens_spend_cache_trimmed() {
    // Spend forms (AU15).
    let forms = [
        (Some(1.84), Some(true), Some(2), Some(2), Some("≈$1.84")),
        (Some(1.84), Some(true), Some(2), Some(1), Some("≈$1.84+")),
        (Some(0.004), Some(true), Some(2), Some(2), Some("≈<$0.01")),
        (Some(0.0), Some(true), Some(2), Some(0), Some("≈$?")),
        (Some(0.0), Some(true), Some(0), Some(0), Some("≈$0.00")),
        (Some(123.0), Some(true), Some(1), Some(1), Some("≈$123")),
        (Some(1.84), None, Some(2), Some(2), Some("$1.84")),
        (Some(1.84), Some(false), Some(2), Some(1), Some("$1.84+")),
        (Some(0.0), Some(false), Some(1), Some(0), Some("$?")),
        (Some(0.0), Some(false), Some(0), Some(0), Some("$0.00")),
        (None, Some(true), Some(2), Some(2), None),
        (None, None, None, None, None),
    ];
    for (usd, notional, total, priced, want) in forms {
        assert_eq!(spend_text(usd, notional, total, priced).as_deref(), want, "{usd:?} {notional:?} {total:?} {priced:?}");
    }
    // The `≈` is one dim run; the rest is green.
    let notional = with(|s| (s.spend_usd, s.spend_notional) = (Some(1.84), Some(true)));
    assert!(render(&notional, &nerd(C)).contains("\x1b[2m↑1.2m ↓48k  ≈\x1b[0m\x1b[32m$1.84\x1b[0m  "), "{:?}", render(&notional, &nerd(C)));
    let plain = render(&notional, &renv(C, IconSet::Plain, ColorMode::Off, 200));
    assert_eq!(plain, "zygo main ⌂ hud-work  opus-4.7 ≡ fast  ctx 196k/1m ■□□□□  ↑1.2m ↓48k  ≈$1.84  cache 92%  trim 23k");
    // A team session has no `spend_usd` yet: no spend segment.
    let team = with(|s| (s.spend_usd, s.requests_priced, s.spend_notional) = (None, None, Some(true)));
    assert!(compact_text(&team).contains(&format!("↑1.2m ↓48k  {DB} 92%")), "{}", compact_text(&team));

    // Cache: red below 50 %.
    let low = with(|s| s.cache_read_share = Some(0.49));
    assert!(render(&low, &nerd(C)).contains(&format!("\x1b[31m{DB} 49%\x1b[0m  ")));
    assert!(render(&low, &nerd(V)).contains("\x1b[31mcache 49%\x1b[0m   "));
    assert!(render(&with(|s| s.cache_read_share = Some(0.5)), &nerd(C)).contains(&format!("\x1b[2m{DB} 50%  ✂ 23k\x1b[0m")));
    assert!(!compact_text(&with(|s| s.cache_read_share = None)).contains(DB));

    // Trimmed: estimated tokens = ceil(bytes ÷ 4).
    let trimmed = |bytes: Option<u64>| HudData { trimmed_bytes: bytes, ..op13() };
    assert!(compact_text(&trimmed(Some(92_160))).ends_with("  ✂ 23k"));
    assert!(verbose_text(&trimmed(Some(92_160))).ends_with("   trimmed 23k tok"));
    assert!(render(&trimmed(Some(92_160)), &renv(C, IconSet::Plain, ColorMode::Off, 200)).ends_with("  trim 23k"));
    assert!(compact_text(&trimmed(Some(5))).ends_with("  ✂ 2"));
    assert!(compact_text(&trimmed(Some(0))).ends_with("  ✂ 0"));
    assert!(compact_text(&trimmed(None)).ends_with(&format!("  {DB} 92%")));

    // Tokens need both sums.
    let half = with(|s| s.tokens_out = None);
    assert!(!compact_text(&half).contains("↑1.2m") && !verbose_text(&half).contains(" in "));
}

#[test]
fn failure_markers_and_stale_dimming() {
    // Stale: every proxy-derived run is dim; local segments keep their styles.
    let stale = HudData { stale: true, ..op13() };
    let rows = render(&stale, &nerd(V));
    assert_eq!(
        rows,
        format!(
            "{STRIPE}\x1b[1;97m{LOGO}\x1b[0m  \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m \x1b[36m↑1\x1b[0m   \x1b[2m{CPU} opus-4.7 · high\x1b[0m   \x1b[36m{PROFILE} fast\x1b[0m   \x1b[2mlocal\x1b[0m\n{STRIPE}\x1b[2;37m{REFLECTION}\x1b[0m  \x1b[2mctx 196k/1m ▰▰▱▱▱▱▱▱ 20%   ↑ 1.2m in  ↓ 48k out   $3.41   cache 92%   trimmed 23k tok   stale\x1b[0m"
        )
    );
    // A profile that comes from the status is proxy-derived.
    let fallback = HudData { profile: None, ..stale.clone() };
    assert!(render(&fallback, &nerd(C)).contains(&format!("\x1b[2m{CPU} opus-4.7 {PROFILE} fast  ctx 196k/1m")));
    // A red cache share and a green `serve` are dim too.
    let mut team = HudData { team_host: Some("h".into()), ..stale.clone() };
    status(&mut team).team_session = Some(true);
    status(&mut team).cache_read_share = Some(0.1);
    let out = render(&team, &nerd(V));
    assert!(out.contains("\x1b[2mserve h\x1b[0m\n") && out.contains("   cache 10%   ") && !out.contains("\x1b[31mcache"), "{out:?}");

    // No status data: local segments and `offline` in red.
    let offline = HudData { status: None, outcome: Outcome::Offline, ..op13() };
    assert_eq!(
        render(&offline, &nerd(C)),
        format!("{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[36m{PROFILE} fast\x1b[0m  \x1b[2m✂ 23k\x1b[0m  \x1b[31moffline\x1b[0m")
    );
    assert_eq!(
        verbose_text(&offline),
        format!("{LOGO}  {BRANCH} main {TREE} hud-work ↑1   {PROFILE} fast\n{REFLECTION}  trimmed 23k tok   offline")
    );
    // A failed fetch with status data 90 s old or less: no marker.
    let fresh = HudData { outcome: Outcome::Offline, ..op13() };
    assert_eq!(render(&fresh, &nerd(C)), op13_compact());
    // Suspended: the marker is `suspended`, never `offline` or `stale`.
    for d in [
        HudData { outcome: Outcome::Suspended, ..op13() },
        HudData { outcome: Outcome::Suspended, stale: true, ..op13() },
        HudData { outcome: Outcome::Suspended, status: None, ..op13() },
    ] {
        let out = render(&d, &nerd(C));
        assert!(out.ends_with("  \x1b[33msuspended\x1b[0m"), "{out:?}");
        assert!(!out.contains("offline") && !out.contains("stale"), "{out:?}");
        assert_eq!(out.contains("ctx 196k/"), d.status.is_some());
    }
    // Ended: the cached data is ignored.
    let ended = HudData { outcome: Outcome::Ended, stale: true, profile: None, ..op13() };
    assert_eq!(
        render(&ended, &nerd(C)),
        format!("{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} main\x1b[0m \x1b[38;5;172m{TREE} hud-work\x1b[0m  \x1b[2m✂ 23k\x1b[0m  \x1b[31mended\x1b[0m")
    );
    // An old status reply: no model, mode, tokens, spend or threshold segment; ctx stays.
    let old: SessionStatus = serde_json::from_value(serde_json::json!({"session":"S1","profile":"p1","work":"ENG-42",
        "used_context":412345,"effective_window":600000,"compact":600000,"cache_read_share":0.6,"expires_at":""}))
    .unwrap();
    let d = HudData { status: Some(old), trimmed_bytes: Some(92_160), ..Default::default() };
    assert_eq!(compact_text(&d), format!("{LOGO} ENG-42  {PROFILE} p1  ctx 412k/-  {DB} 60%  ✂ 23k"));
    assert_eq!(verbose_text(&d), format!("{LOGO}  ENG-42   {PROFILE} p1\n{REFLECTION}  ctx 412k/-   cache 60%   trimmed 23k tok"));
    // Nothing at all.
    assert_eq!(compact_text(&HudData::default()), format!("{LOGO}  offline"));
}

#[test]
fn workspace_phase() {
    let phase = |p: &str| HudData { phase: Some(p.into()), worktree: None, ..op13() };
    for (p, sgr) in [("building", "33"), ("draining", "31"), ("stopping", "1;31"), ("down", "2")] {
        let compact = render(&phase(p), &nerd(C));
        assert!(compact.contains(&format!("{LOGO}\x1b[0m \x1b[{sgr}mws {p}\x1b[0m \x1b[1;34m{BRANCH} main")), "{compact:?}");
        let verbose = render(&phase(p), &nerd(V));
        assert!(verbose.contains(&format!("{LOGO}\x1b[0m  \x1b[{sgr}mws {p}\x1b[0m   \x1b[1;34m{BRANCH} main")), "{verbose:?}");
    }
    for p in ["up", "other", "", "DRAINING", "ws\x1b"] {
        assert!(!compact_text(&phase(p)).contains("ws "), "{p}");
        assert_eq!(compact_text(&phase(p)), compact_text(&HudData { worktree: None, ..op13() }));
    }
    // The serve mockup of H§6.3.
    let mut d = phase("draining");
    d.team_host = Some("zygo.example.org".into());
    d.status = Some(SessionStatus {
        used_context: Some(512_000),
        threshold: Some(600_000),
        effective_window: Some(600_000),
        tokens_in: Some(9_800_000),
        tokens_out: Some(210_000),
        spend_usd: Some(21.07),
        spend_notional: Some(true),
        cache_read_share: Some(0.88),
        team_session: Some(true),
        ..d.status.clone().unwrap()
    });
    d.trimmed_bytes = Some(4_400_000);
    d.ahead = None;
    assert_eq!(
        verbose_text(&d),
        format!(
            "{LOGO}  ws draining   {BRANCH} main   {CPU} opus-4.7 · high   {PROFILE} fast   serve zygo.example.org\n{REFLECTION}  ctx 512k/1m ▰▰▰▰▰▰▰▱ 85% ⇣600k   ↑ 9.8m in  ↓ 210k out   ≈$21.07   cache 88%   trimmed 1.1m tok"
        )
    );
    assert!(render(&d, &nerd(V)).contains("\x1b[31m▰▰▰▰▰▰▰\x1b[2m▱\x1b[0m 85% \x1b[2m⇣600k   ↑ 9.8m in  ↓ 210k out   ≈\x1b[0m\x1b[32m$21.07\x1b[0m"));
}

#[test]
fn cells_and_truncation() {
    assert_eq!(cells(LOGO), 4);
    assert_eq!(cells(REFLECTION), 4);
    assert_eq!(cells("▌▌▌"), 3);
    for glyph in [BRANCH, TREE, CPU, PROFILE, DB, "\u{f408}", "\u{f41b}"] {
        assert_eq!(cells(glyph), 2, "{:x}", glyph.chars().next().unwrap() as u32);
    }
    assert_eq!(cells("▰▱■□⇣▼≈✂…↑↓◆♦○⌂≡·"), 17);
    assert_eq!(cells("日本"), 4);
    assert_eq!(cells("\x1b[31m"), 4, "the caller counts text, not escapes");
    assert_eq!(truncate("abcdef", 6), "abcdef");
    assert_eq!(truncate("abcdefg", 6), "abcde…");
    assert_eq!(truncate("日本語日本語", 6), "日本…");
    assert_eq!(truncate("日本語", 6), "日本語");
    assert_eq!(truncate("a日本語", 6), "a日本…");
    assert_eq!(truncate("", 6), "");
}
