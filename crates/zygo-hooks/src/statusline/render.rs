//! The pure render of the status line HUD (H§6): data in, one or two rows out. It reads no file, no environment and
//! no clock; `RenderEnv::from_env` is the one place that looks at the environment, through a closure.

use std::ffi::OsString;

use serde::{Deserialize, Serialize};
use zygo_core::esc;
use zygo_core::tokens::BYTES_PER_TOKEN;
use zygo_proxy::client::{SessionStatus, WindowArg};

use super::width::{cells, terminal_width, truncate, tty_columns};
use super::{pct, short};

// ---- inputs ----

/// `compact` is one row; `verbose` is two rows (OP7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Compact,
    Verbose,
}

impl Style {
    /// `compact` or `verbose`; any other text is no style (AU1).
    pub fn parse(s: &str) -> Option<Style> {
        match s {
            "compact" => Some(Style::Compact),
            "verbose" => Some(Style::Verbose),
            _ => None,
        }
    }
}

/// The icon set (OP6, H§6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSet {
    Nerd,
    Plain,
}

/// `Off`: no escape byte and no stripe (`NO_COLOR`). `Ansi16`: the 16-color fallback of AU23. `Ansi256`: `38;5;n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Off,
    Ansi16,
    Ansi256,
}

/// The result of the last status fetch (AU20); the cache of the hook stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    #[default]
    Ok,
    Offline,
    Suspended,
    Ended,
}

/// Everything one render shows. Each `Option` that is `None` drops only the segment that needs it (H§6.8).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudData {
    /// The status data, fresh or cached; `None` with no data at all.
    pub status: Option<SessionStatus>,
    /// The status data is older than 90 s (AU20).
    pub stale: bool,
    pub outcome: Outcome,
    /// `ZYGO_WORK` (`source:id`); empty counts as absent, and the status `work` is the fallback (AU11).
    pub work: Option<String>,
    /// The final directory name of a linked worktree; `None` in the main worktree (OP9).
    pub worktree: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// The live branch of the git worker (AU11).
    pub branch: Option<String>,
    /// The profile of `view.json`; the status `profile` is the fallback (AU12).
    pub profile: Option<String>,
    /// The stdin `effort` (verbose).
    pub effort: Option<String>,
    pub trimmed_bytes: Option<u64>,
    /// The workspace phase; only `building`, `draining`, `stopping` and `down` show (AU25).
    pub phase: Option<String>,
    /// The host of `ZYGO_URL`, for the mode `serve <host>` (AU13).
    pub team_host: Option<String>,
}

/// How to draw: the style, the icon set, the color mode and the assumed width W (H§6.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderEnv {
    pub style: Style,
    pub icons: IconSet,
    pub color: ColorMode,
    pub width: u16,
    /// Test seam: `render` panics when it is set.
    #[cfg(feature = "test-hooks")]
    pub force_panic: bool,
}

impl RenderEnv {
    /// The icon set (`icon_set`), the color mode (`color_mode`) and the width (`terminal_width` with the terminal
    /// probe `tty_columns`), each read through `env`. Under `test-hooks`, `ZYGO_TEST_RENDER_PANIC=1` sets the
    /// forced-panic seam.
    pub fn from_env(env: &dyn Fn(&str) -> Option<OsString>, style: Style) -> RenderEnv {
        RenderEnv {
            style,
            icons: icon_set(env),
            color: color_mode(env),
            width: terminal_width(env, &tty_columns),
            #[cfg(feature = "test-hooks")]
            force_panic: var(env, "ZYGO_TEST_RENDER_PANIC").as_deref() == Some("1"),
        }
    }
}

/// A variable that is set and not empty.
fn var(env: &dyn Fn(&str) -> Option<OsString>, key: &str) -> Option<String> {
    env(key).filter(|v| !v.is_empty()).map(|v| v.to_string_lossy().into_owned())
}

/// AU2 for this build: `icon_set_on(env, cfg!(windows))`.
pub fn icon_set(env: &dyn Fn(&str) -> Option<OsString>) -> IconSet {
    icon_set_on(env, cfg!(windows))
}

/// AU2. `ZYGO_NERD_FONT=1` forces the Nerd set and `0` the plain set. Any other value, or none: plain when `TERM`
/// is `linux` or `dumb`, or on Windows (`windows`) when neither `WT_SESSION` nor `TERM_PROGRAM` is set; else Nerd.
/// `NO_COLOR` has no part in it.
pub fn icon_set_on(env: &dyn Fn(&str) -> Option<OsString>, windows: bool) -> IconSet {
    match var(env, "ZYGO_NERD_FONT").as_deref() {
        Some("1") => return IconSet::Nerd,
        Some("0") => return IconSet::Plain,
        _ => {}
    }
    let console = matches!(var(env, "TERM").as_deref(), Some("linux" | "dumb"));
    let legacy = windows && var(env, "WT_SESSION").is_none() && var(env, "TERM_PROGRAM").is_none();
    if console || legacy { IconSet::Plain } else { IconSet::Nerd }
}

/// `Off` when `NO_COLOR` is set and not empty (https://no-color.org). Else AU23: 256 colors when `COLORTERM` is
/// `truecolor` or `24bit`, or `TERM` contains `256color`, or `WT_SESSION` is set; else 16 colors.
pub fn color_mode(env: &dyn Fn(&str) -> Option<OsString>) -> ColorMode {
    if var(env, "NO_COLOR").is_some() {
        return ColorMode::Off;
    }
    let rich = matches!(var(env, "COLORTERM").as_deref(), Some("truecolor" | "24bit"))
        || var(env, "TERM").is_some_and(|t| t.contains("256color"))
        || var(env, "WT_SESSION").is_some();
    if rich { ColorMode::Ansi256 } else { ColorMode::Ansi16 }
}

// ---- icons (H§6.5) ----

struct Glyphs {
    logo: &'static str,
    /// Empty in the plain set: four spaces take its place (AU8).
    reflection: &'static str,
    branch: &'static str,
    github: &'static str,
    graph: &'static str,
    bead: &'static str,
    worktree: &'static str,
    model: &'static str,
    profile: &'static str,
    filled: &'static str,
    empty: &'static str,
    threshold: &'static str,
    cache: &'static str,
    trimmed: &'static str,
}

const STRIPE: &str = "▌";
const AHEAD: &str = "↑";
const BEHIND: &str = "↓";
const NOTIONAL: &str = "≈";
const TRUNCATION: &str = "…";

const NERD: Glyphs = Glyphs {
    logo: "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}",
    reflection: "z\u{28e}\u{183}o",
    branch: "\u{e0a0}",
    github: "\u{f408}",
    graph: "◆",
    bead: "\u{f41b}",
    worktree: "\u{f0645}",
    model: "\u{f4bc}",
    profile: "\u{f0626}",
    filled: "▰",
    empty: "▱",
    threshold: "⇣",
    cache: "\u{f01bc}",
    trimmed: "✂",
};

const PLAIN: Glyphs = Glyphs {
    logo: "zygo",
    reflection: "",
    branch: "",
    github: "",
    graph: "♦",
    bead: "○",
    worktree: "⌂",
    model: "",
    profile: "≡",
    filled: "■",
    empty: "□",
    threshold: "▼",
    cache: "cache",
    trimmed: "trim",
};

const fn table(g: &Glyphs) -> [(&'static str, &'static str); 19] {
    [
        ("stripe", STRIPE),
        ("logo", g.logo),
        ("reflection", g.reflection),
        ("branch", g.branch),
        ("github", g.github),
        ("work-graph", g.graph),
        ("bead", g.bead),
        ("worktree", g.worktree),
        ("ahead", AHEAD),
        ("behind", BEHIND),
        ("model", g.model),
        ("profile", g.profile),
        ("bar-filled", g.filled),
        ("bar-empty", g.empty),
        ("threshold", g.threshold),
        ("notional", NOTIONAL),
        ("cache", g.cache),
        ("trimmed", g.trimmed),
        ("truncation", TRUNCATION),
    ]
}

static NERD_TABLE: [(&str, &str); 19] = table(&NERD);
static PLAIN_TABLE: [(&str, &str); 19] = table(&PLAIN);

/// The (name, glyph) table of a set, in the order of H§6.5, for the `zygo doctor` icon test line (AU2). A use that
/// has no glyph in the plain set has an empty glyph there.
pub fn glyphs(set: IconSet) -> &'static [(&'static str, &'static str)] {
    match set {
        IconSet::Nerd => &NERD_TABLE,
        IconSet::Plain => &PLAIN_TABLE,
    }
}

// ---- spend (AU15, H§6.6) ----

/// The spend text of the HUD and of `zygo status`. `None` without `spend_usd` (an old server). Forms:
/// `requests_total = 0` → `$0.00`; `requests_priced = 0` → `$?`; `requests_priced < requests_total` → `$X+`; else
/// `$X`. A notional spend has `≈` in front. A sum that is negative or not a number counts as not priced (`$?`).
pub fn spend_text(spend_usd: Option<f64>, notional: Option<bool>, total: Option<u64>, priced: Option<u64>) -> Option<String> {
    let usd = spend_usd?;
    let form = if total == Some(0) {
        "$0.00".to_string()
    } else if priced == Some(0) || usd.is_nan() || usd < 0.0 {
        "$?".to_string()
    } else {
        let part = matches!((total, priced), (Some(t), Some(p)) if p < t);
        format!("{}{}", amount(usd), if part { "+" } else { "" })
    };
    Some(if notional == Some(true) { format!("{NOTIONAL}{form}") } else { form })
}

/// Whole cents first (half up), then the form: `<$0.01`, two decimals below $100.00, whole dollars (half up) from
/// there, `>$99999` above $99999 (AU32).
fn amount(usd: f64) -> String {
    if usd > 99_999.0 {
        return ">$99999".to_string();
    }
    let cents = (usd * 100.0).round() as u64;
    match cents {
        0 if usd > 0.0 => "<$0.01".to_string(),
        0..10_000 => format!("${}.{:02}", cents / 100, cents % 100),
        _ => format!("${}", (cents + 50) / 100),
    }
}

// ---- styles and emission (H§6.4) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intensity {
    Normal,
    Bold,
    Dim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fg {
    Default,
    /// A 16-color code: `31`-`37`, `97`.
    Basic(u8),
    /// `38;5;n`.
    Indexed(u8),
}

/// H§6.4 step 1: a style is a pair (intensity, foreground).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sgr {
    intensity: Intensity,
    fg: Fg,
}

const EMPTY: Sgr = Sgr { intensity: Intensity::Normal, fg: Fg::Default };
const DIM: Sgr = Sgr { intensity: Intensity::Dim, fg: Fg::Default };

const fn fg(code: u8) -> Sgr {
    Sgr { intensity: Intensity::Normal, fg: Fg::Basic(code) }
}

const fn bold(code: u8) -> Sgr {
    Sgr { intensity: Intensity::Bold, fg: Fg::Basic(code) }
}

impl Sgr {
    /// The codes of the parts that differ from `from`, intensity first.
    fn codes(self, from: Sgr) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.intensity != from.intensity {
            match self.intensity {
                Intensity::Bold => parts.push("1".into()),
                Intensity::Dim => parts.push("2".into()),
                Intensity::Normal => {}
            }
        }
        if self.fg != from.fg {
            match self.fg {
                Fg::Basic(n) => parts.push(n.to_string()),
                Fg::Indexed(n) => parts.push(format!("38;5;{n}")),
                Fg::Default => {}
            }
        }
        parts.join(";")
    }
}

/// H§6.4 step 2: a run of text in one style. A gap is spaces with no style of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    sgr: Sgr,
    text: String,
    gap: bool,
}

fn run(sgr: Sgr, text: impl Into<String>) -> Run {
    Run { sgr, text: text.into(), gap: false }
}

fn gap(n: usize) -> Run {
    Run { sgr: EMPTY, text: " ".repeat(n), gap: true }
}

/// H§6.4 step 6: the escapes from the current style `c` to the next style `n`.
fn transition(c: Sgr, n: Sgr, out: &mut String) {
    if n == c {
        return;
    }
    let intensity_stays_on = c.intensity == Intensity::Normal || c.intensity == n.intensity;
    let fg_stays_on = c.fg == Fg::Default || n.fg != Fg::Default;
    if intensity_stays_on && fg_stays_on {
        out.push_str(&format!("\x1b[{}m", n.codes(c)));
    } else {
        out.push_str("\x1b[0m");
        if n != EMPTY {
            out.push_str(&format!("\x1b[{}m", n.codes(EMPTY)));
        }
    }
}

/// H§6.4 steps 3, 4, 6, 7 and 8: one row of runs as bytes. With `color` false: the text alone.
fn emit(mut runs: Vec<Run>, color: bool) -> String {
    if !color {
        return runs.iter().map(|r| r.text.as_str()).collect();
    }
    // Step 3: a gap between two runs of one style takes that style.
    let own: Vec<Option<Sgr>> = runs.iter().map(|r| (!r.gap).then_some(r.sgr)).collect();
    for (i, r) in runs.iter_mut().enumerate() {
        if r.gap {
            let before = own[..i].iter().rev().flatten().next();
            let after = own[i + 1..].iter().flatten().next();
            r.sgr = match (before, after) {
                (Some(a), Some(b)) if a == b => *a,
                _ => EMPTY,
            };
        }
    }
    // Steps 4 and 6: runs of one style need no escape between them.
    let (mut out, mut current) = (String::new(), EMPTY);
    for r in runs.iter().filter(|r| !r.text.is_empty()) {
        transition(current, r.sgr, &mut out);
        current = r.sgr;
        out.push_str(&r.text);
    }
    // Step 7.
    if current != EMPTY {
        out.push_str("\x1b[0m");
    }
    out
}

// ---- segments (H§6.2) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg {
    Phase,
    Work,
    Worktree,
    Git,
    Model,
    /// The ` · effort` part of the verbose model segment.
    Effort,
    Profile,
    Mode,
    Ctx,
    Bar,
    Percent,
    Threshold,
    Tokens,
    Spend,
    Cache,
    Trimmed,
    /// `stale`, `offline`, `suspended` or `ended`; only `stale` can be removed for width.
    Marker,
}

/// AU9, compact: one space inside a group, two between groups; the first group follows the logo after one space.
const COMPACT: &[&[Seg]] = &[
    &[Seg::Phase, Seg::Work, Seg::Worktree],
    &[Seg::Model, Seg::Profile],
    &[Seg::Ctx, Seg::Bar, Seg::Threshold],
    &[Seg::Tokens],
    &[Seg::Spend],
    &[Seg::Cache],
    &[Seg::Trimmed],
    &[Seg::Marker],
];
/// AU9, verbose: two spaces after the logo or the reflection, three between groups, one inside a group.
const ROW_1: &[&[Seg]] =
    &[&[Seg::Phase], &[Seg::Work, Seg::Worktree, Seg::Git], &[Seg::Model], &[Seg::Profile], &[Seg::Mode]];
const ROW_2: &[&[Seg]] = &[
    &[Seg::Ctx, Seg::Bar, Seg::Percent, Seg::Threshold],
    &[Seg::Tokens],
    &[Seg::Spend],
    &[Seg::Cache],
    &[Seg::Trimmed],
    &[Seg::Marker],
];

/// H§6.7: the segments to remove, first to last, while a row is wider than W.
const COMPACT_DROPS: &[Seg] = &[
    Seg::Trimmed,
    Seg::Cache,
    Seg::Tokens,
    Seg::Threshold,
    Seg::Spend,
    Seg::Profile,
    Seg::Model,
    Seg::Bar,
    Seg::Marker,
    Seg::Worktree,
    Seg::Ctx,
    Seg::Work,
];
const ROW_1_DROPS: &[Seg] = &[Seg::Mode, Seg::Profile, Seg::Effort, Seg::Git, Seg::Model, Seg::Worktree, Seg::Work];
const ROW_2_DROPS: &[Seg] = &[
    Seg::Trimmed,
    Seg::Cache,
    Seg::Tokens,
    Seg::Spend,
    Seg::Percent,
    Seg::Threshold,
    Seg::Bar,
    Seg::Marker,
    Seg::Ctx,
];

/// Maximum widths in cells (H§6.6, AU19).
const WORK_CELLS: usize = 32;
const WORKTREE_CELLS: usize = 24;
const MODEL_CELLS: usize = 24;
const PROFILE_CELLS: usize = 16;
const EFFORT_CELLS: usize = 8;
const HOST_CELLS: usize = 32;

/// Text from outside zygo: escaped (H-C6), then cut (AU19).
fn name(text: &str, max: usize) -> String {
    truncate(&esc(text), max)
}

/// An icon and the space after it are in the run of their text (H§6.4 step 2); a set with no icon has the text alone.
fn iconed(icon: &str, text: &str) -> String {
    if icon.is_empty() { text.to_string() } else { format!("{icon} {text}") }
}

fn present(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

struct Hud<'a> {
    d: &'a HudData,
    env: &'a RenderEnv,
    g: &'static Glyphs,
    /// The status data that the proxy-derived segments use: none with the outcome `ended` (AU20).
    status: Option<&'a SessionStatus>,
    /// Stale: every run of a proxy-derived segment is dim, default foreground (AU20).
    dim: bool,
}

impl<'a> Hud<'a> {
    fn new(d: &'a HudData, env: &'a RenderEnv) -> Hud<'a> {
        let status = if d.outcome == Outcome::Ended { None } else { d.status.as_ref() };
        let g = match env.icons {
            IconSet::Nerd => &NERD,
            IconSet::Plain => &PLAIN,
        };
        Hud { d, env, g, status, dim: d.stale && status.is_some() }
    }

    fn verbose(&self) -> bool {
        self.env.style == Style::Verbose
    }

    /// The style of a run of a proxy-derived segment.
    fn proxy(&self, sgr: Sgr) -> Sgr {
        if self.dim { DIM } else { sgr }
    }

    fn phase(&self) -> Option<Vec<Run>> {
        let phase = self.d.phase.as_deref()?;
        let sgr = match phase {
            "building" => fg(33),
            "draining" => fg(31),
            "stopping" => bold(31),
            "down" => DIM,
            _ => return None,
        };
        Some(vec![run(sgr, format!("ws {phase}"))])
    }

    /// OP3, AU11 and the marker table of H§6.2.
    fn work(&self) -> Option<Vec<Run>> {
        let live = present(&self.d.branch);
        let value = present(&self.d.work).or_else(|| self.status.and_then(|s| present(&s.work)));
        let Some(value) = value else {
            return live.map(|b| vec![run(fg(34), iconed(self.g.branch, &name(b, WORK_CELLS)))]);
        };
        let (icon, text) = match value.split_once(':') {
            Some(("branch", id)) if live.is_some() || !id.is_empty() => (self.g.branch, live.unwrap_or(id)),
            Some(("github", id)) if id.contains('#') => (self.g.github, &id[id.rfind('#').unwrap_or(0)..]),
            Some(("work-graph", id)) if !id.is_empty() => (self.g.graph, id),
            Some(("beads", id)) if !id.is_empty() => (self.g.bead, id),
            _ => ("", value),
        };
        Some(vec![run(bold(34), iconed(icon, &name(text, WORK_CELLS)))])
    }

    fn worktree(&self) -> Option<Vec<Run>> {
        let tree = present(&self.d.worktree)?;
        // H§6.4 step 9.
        let amber = if self.env.color == ColorMode::Ansi256 { Fg::Indexed(172) } else { Fg::Basic(33) };
        let sgr = Sgr { intensity: Intensity::Normal, fg: amber };
        Some(vec![run(sgr, format!("{} {}", self.g.worktree, name(tree, WORKTREE_CELLS)))])
    }

    fn git(&self) -> Option<Vec<Run>> {
        let mark = |n: Option<u32>, arrow: &str| n.filter(|n| *n > 0).map(|n| format!("{arrow}{n}"));
        let marks: Vec<String> = [mark(self.d.ahead, AHEAD), mark(self.d.behind, BEHIND)].into_iter().flatten().collect();
        (!marks.is_empty()).then(|| vec![run(fg(36), marks.join(" "))])
    }

    /// AU10: the served model without a leading `claude-`; `effort` adds ` · <effort>` in the same span.
    fn model(&self, effort: bool) -> Option<Vec<Run>> {
        let id = present(&self.status?.model)?;
        let short_id = Some(id.strip_prefix("claude-").unwrap_or(id)).filter(|s| !s.is_empty())?;
        let mut text = iconed(self.g.model, &name(short_id, MODEL_CELLS));
        if let Some(e) = present(&self.d.effort).filter(|_| effort) {
            text.push_str(&format!(" · {}", name(e, EFFORT_CELLS)));
        }
        Some(vec![run(self.proxy(fg(35)), text)])
    }

    /// AU12: `view.json` (local), else the status (proxy-derived).
    fn profile(&self) -> Option<Vec<Run>> {
        let (profile, sgr) = match present(&self.d.profile) {
            Some(p) => (p, fg(36)),
            None => (present(&self.status?.profile)?, self.proxy(fg(36))),
        };
        Some(vec![run(sgr, iconed(self.g.profile, &name(profile, PROFILE_CELLS)))])
    }

    /// AU13: first match wins; no segment when the status lacks either flag.
    fn mode(&self) -> Option<Vec<Run>> {
        let s = self.status?;
        let (passthrough, team) = (s.passthrough?, s.team_session?);
        Some(vec![if passthrough {
            run(self.proxy(DIM), "passthrough")
        } else if team {
            let host = present(&self.d.team_host).map(|h| format!(" {}", name(h, HOST_CELLS))).unwrap_or_default();
            run(self.proxy(fg(32)), format!("serve{host}"))
        } else {
            run(self.proxy(DIM), "local")
        }])
    }

    /// OP2, AU7: `ctx used/window`; an unknown number is `-`.
    fn ctx(&self) -> Option<Vec<Run>> {
        let s = self.status?;
        let number = |n: Option<u64>| n.map_or_else(|| "-".to_string(), short);
        Some(vec![
            run(self.proxy(EMPTY), format!("ctx {}/", number(s.used_context))),
            run(self.proxy(DIM), number(s.context_window)),
        ])
    }

    /// AU7, AU17: (used, D), where D is `effective_window` when it is below the model window, else the model
    /// window. `None` when `used` or the model window is unknown.
    fn pressure(&self) -> Option<(u128, u128)> {
        let s = self.status?;
        let (used, window) = (s.used_context?, s.context_window.filter(|w| *w > 0)?);
        let d = s.effective_window.filter(|e| *e > 0 && *e < window).unwrap_or(window);
        Some((u128::from(used), u128::from(d)))
    }

    /// AU18 and the bar colors of H§6.4.
    fn bar(&self, cells: u128) -> Option<Vec<Run>> {
        let (used, d) = self.pressure()?;
        // round_half_up(used ÷ D × cells), at least 1 when used > 0, at most `cells`.
        let filled = ((2 * used * cells + d) / (2 * d)).max(u128::from(used > 0)).min(cells);
        let color = if used * 100 < d * 60 {
            32
        } else if used * 100 < d * 85 {
            33
        } else {
            31
        };
        let mut runs = Vec::new();
        if filled > 0 {
            runs.push(run(self.proxy(fg(color)), self.g.filled.repeat(filled as usize)));
        }
        if filled < cells {
            // Step 5: dim with the foreground of the filled cells.
            let rest = if filled > 0 { Sgr { intensity: Intensity::Dim, fg: Fg::Basic(color) } } else { DIM };
            runs.push(run(self.proxy(rest), self.g.empty.repeat((cells - filled) as usize)));
        }
        Some(runs)
    }

    fn percent(&self) -> Option<Vec<Run>> {
        let (used, d) = self.pressure()?;
        Some(vec![run(self.proxy(EMPTY), format!("{}%", (200 * used + d) / (2 * d)))])
    }

    /// AU28: `⇣off` for the override `off`; `⇣<n>` for a threshold below a known model window.
    fn threshold(&self) -> Option<Vec<Run>> {
        let s = self.status?;
        let text = if s.compact == Some(WindowArg::Off) {
            "off".to_string()
        } else {
            let (threshold, window) = (s.threshold?, s.context_window?);
            (threshold < window).then(|| short(threshold))?
        };
        Some(vec![run(self.proxy(DIM), format!("{}{text}", self.g.threshold))])
    }

    fn tokens(&self) -> Option<Vec<Run>> {
        let s = self.status?;
        let (tin, tout) = (short(s.tokens_in?), short(s.tokens_out?));
        let text = if self.verbose() {
            format!("{AHEAD} {tin} in  {BEHIND} {tout} out")
        } else {
            format!("{AHEAD}{tin} {BEHIND}{tout}")
        };
        Some(vec![run(self.proxy(DIM), text)])
    }

    /// AU15: the `≈` is one dim run; the rest of the form is green.
    fn spend(&self) -> Option<Vec<Run>> {
        let s = self.status?;
        let text = spend_text(s.spend_usd, s.spend_notional, s.requests_total, s.requests_priced)?;
        Some(match text.strip_prefix(NOTIONAL) {
            Some(rest) => vec![run(self.proxy(DIM), NOTIONAL), run(self.proxy(fg(32)), rest)],
            None => vec![run(self.proxy(fg(32)), text)],
        })
    }

    fn cache(&self) -> Option<Vec<Run>> {
        let share = self.status?.cache_read_share.filter(|s| s.is_finite())?;
        let label = if self.verbose() { "cache" } else { self.g.cache };
        let sgr = if share < 0.5 { fg(31) } else { DIM };
        Some(vec![run(self.proxy(sgr), format!("{label} {}", pct(Some(share))))])
    }

    /// OP1: estimated tokens = ceil(bytes ÷ `BYTES_PER_TOKEN`). A local segment.
    fn trimmed(&self) -> Option<Vec<Run>> {
        let tokens = short(self.d.trimmed_bytes?.div_ceil(BYTES_PER_TOKEN));
        let text = if self.verbose() { format!("trimmed {tokens} tok") } else { format!("{} {tokens}", self.g.trimmed) };
        Some(vec![run(DIM, text)])
    }

    /// AU20: the marker and whether it is `stale`, the one marker that width can remove.
    fn marker(&self) -> Option<(Run, bool)> {
        match self.d.outcome {
            Outcome::Ended => Some((run(fg(31), "ended"), false)),
            Outcome::Suspended => Some((run(fg(33), "suspended"), false)),
            _ if self.status.is_none() => Some((run(fg(31), "offline"), false)),
            _ if self.d.stale => Some((run(DIM, "stale"), true)),
            _ => None,
        }
    }

    /// The runs of one segment; `None` when it has no data or is in `drops`.
    fn seg(&self, seg: Seg, drops: &[Seg]) -> Option<Vec<Run>> {
        let gone = |s: Seg| drops.contains(&s);
        match seg {
            // Never removed (H§6.7).
            Seg::Phase => self.phase(),
            Seg::Marker => self.marker().filter(|(_, stale)| !(*stale && gone(Seg::Marker))).map(|(r, _)| vec![r]),
            _ if gone(seg) => None,
            Seg::Work => self.work(),
            Seg::Worktree => self.worktree(),
            Seg::Git => self.git(),
            Seg::Model => self.model(self.verbose() && !gone(Seg::Effort)),
            Seg::Effort => None,
            Seg::Profile => self.profile(),
            Seg::Mode => self.mode(),
            Seg::Ctx => self.ctx(),
            Seg::Bar if gone(Seg::Ctx) => None,
            Seg::Bar => self.bar(if self.verbose() { 8 } else { 5 }),
            Seg::Percent if gone(Seg::Ctx) => None,
            Seg::Percent => self.percent(),
            Seg::Threshold => self.threshold(),
            Seg::Tokens => self.tokens(),
            Seg::Spend => self.spend(),
            Seg::Cache => self.cache(),
            Seg::Trimmed => self.trimmed(),
        }
    }

    /// AU8: the stripe and one space (not with `NO_COLOR`), then the logo, or on verbose row 2 its reflection or
    /// four spaces.
    fn head(&self, second_row: bool) -> Vec<Run> {
        let mut runs = Vec::new();
        if self.env.color != ColorMode::Off {
            runs.extend([run(fg(31), STRIPE), run(fg(33), STRIPE), run(fg(36), STRIPE), gap(1)]);
        }
        runs.push(match (second_row, self.g.reflection.is_empty()) {
            (false, _) => run(bold(97), self.g.logo),
            (true, false) => run(Sgr { intensity: Intensity::Dim, fg: Fg::Basic(37) }, self.g.reflection),
            (true, true) => gap(4),
        });
        runs
    }

    /// One row: the head, then each group that has a segment. `lead` is the gap after the head when the first
    /// group shown is `groups[0]`, and when it is a later group; `between` is the gap between groups.
    fn row(&self, second_row: bool, groups: &[&[Seg]], lead: [usize; 2], between: usize, drops: &[Seg]) -> Vec<Run> {
        let mut runs = self.head(second_row);
        let mut first = true;
        for (i, group) in groups.iter().enumerate() {
            let segs: Vec<Vec<Run>> = group.iter().filter_map(|s| self.seg(*s, drops)).collect();
            for (j, seg) in segs.into_iter().enumerate() {
                runs.push(gap(match (j, first) {
                    (0, true) => lead[usize::from(i > 0)],
                    (0, false) => between,
                    _ => 1,
                }));
                first = false;
                runs.extend(seg);
            }
        }
        runs
    }

    /// H§6.7: removes whole segments in the order of `drops` until the row fits W; a row that still does not fit
    /// is printed as it is.
    fn fit(&self, drops: &[Seg], build: impl Fn(&[Seg]) -> Vec<Run>) -> String {
        let width = |runs: &[Run]| runs.iter().map(|r| cells(&r.text)).sum::<usize>();
        let mut row = build(&[]);
        for n in 1..=drops.len() {
            if width(&row) <= usize::from(self.env.width) {
                break;
            }
            row = build(&drops[..n]);
        }
        emit(row, self.env.color != ColorMode::Off)
    }
}

/// The HUD for `d`: one row (compact), or two rows with `\n` between them (verbose). No newline after the last row
/// (H§6.4 step 7). Each row is fitted to `env.width` on its own (H§6.7).
pub fn render(d: &HudData, env: &RenderEnv) -> String {
    #[cfg(feature = "test-hooks")]
    if env.force_panic {
        panic!("forced by ZYGO_TEST_RENDER_PANIC");
    }
    let hud = Hud::new(d, env);
    match env.style {
        Style::Compact => hud.fit(COMPACT_DROPS, |drops| hud.row(false, COMPACT, [1, 2], 2, drops)),
        Style::Verbose => {
            let first = hud.fit(ROW_1_DROPS, |drops| hud.row(false, ROW_1, [2, 2], 3, drops));
            let second = hud.fit(ROW_2_DROPS, |drops| hud.row(true, ROW_2, [2, 2], 3, drops));
            format!("{first}\n{second}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Sgr = fg(31);
    const GREEN: Sgr = fg(32);
    const DIM_GREEN: Sgr = Sgr { intensity: Intensity::Dim, fg: Fg::Basic(32) };
    const AMBER: Sgr = Sgr { intensity: Intensity::Normal, fg: Fg::Indexed(172) };

    fn between(c: Sgr, n: Sgr) -> String {
        let mut out = String::new();
        transition(c, n, &mut out);
        out
    }

    /// H§14 #4: each transition kind of H§6.4 step 6.
    #[test]
    fn emitter_transitions() {
        // No change: nothing.
        for s in [EMPTY, DIM, RED, bold(97), DIM_GREEN, AMBER] {
            assert_eq!(between(s, s), "");
        }
        // Add a part: one escape with the parts that change, intensity first.
        assert_eq!(between(EMPTY, bold(97)), "\x1b[1;97m");
        assert_eq!(between(EMPTY, RED), "\x1b[31m");
        assert_eq!(between(EMPTY, DIM), "\x1b[2m");
        assert_eq!(between(EMPTY, AMBER), "\x1b[38;5;172m");
        assert_eq!(between(EMPTY, Sgr { intensity: Intensity::Dim, fg: Fg::Basic(37) }), "\x1b[2;37m");
        assert_eq!(between(RED, fg(33)), "\x1b[33m"); // the stripe: one color to the next
        assert_eq!(between(GREEN, DIM_GREEN), "\x1b[2m"); // the bar: dim added to the green
        assert_eq!(between(DIM, DIM_GREEN), "\x1b[32m");
        assert_eq!(between(GREEN, bold(34)), "\x1b[1;34m");
        // Reset, then set: an intensity or a foreground must be turned off.
        assert_eq!(between(DIM_GREEN, EMPTY), "\x1b[0m");
        assert_eq!(between(RED, EMPTY), "\x1b[0m");
        assert_eq!(between(DIM, GREEN), "\x1b[0m\x1b[32m"); // intensity back to normal
        assert_eq!(between(bold(34), DIM), "\x1b[0m\x1b[2m"); // to another intensity
        assert_eq!(between(DIM_GREEN, DIM), "\x1b[0m\x1b[2m"); // foreground back to default
        assert_eq!(between(bold(97), Sgr { intensity: Intensity::Dim, fg: Fg::Basic(37) }), "\x1b[0m\x1b[2;37m");
        assert_eq!(between(DIM_GREEN, AMBER), "\x1b[0m\x1b[38;5;172m");
    }

    /// H§14 #4: gap absorption on both sides (H§6.4 step 3), merge (step 4) and the row end (step 7).
    #[test]
    fn emitter_gap_absorption_and_merge() {
        // The same style before and after: the gap takes it, and the three runs are one.
        assert_eq!(emit(vec![run(DIM, "a"), gap(2), run(DIM, "b")], true), "\x1b[2ma  b\x1b[0m");
        // A different style on the left, then on the right: the gap keeps the empty style.
        assert_eq!(emit(vec![run(GREEN, "a"), gap(2), run(DIM, "b")], true), "\x1b[32ma\x1b[0m  \x1b[2mb\x1b[0m");
        assert_eq!(emit(vec![run(DIM, "a"), gap(2), run(GREEN, "b")], true), "\x1b[2ma\x1b[0m  \x1b[32mb\x1b[0m");
        // The neighbors are the next runs that are not gaps.
        assert_eq!(emit(vec![run(DIM, "a"), gap(1), gap(4), gap(2), run(DIM, "b")], true), "\x1b[2ma       b\x1b[0m");
        // No neighbor on one side: the empty style.
        assert_eq!(emit(vec![gap(1), run(DIM, "a"), gap(1)], true), " \x1b[2ma\x1b[0m ");
        // Dim green and dim default are not one style (the bar, then the threshold).
        assert_eq!(
            emit(vec![run(GREEN, "▰"), run(DIM_GREEN, "▱"), gap(1), run(DIM, "⇣1k"), gap(2), run(DIM, "↑1")], true),
            "\x1b[32m▰\x1b[2m▱\x1b[0m \x1b[2m⇣1k  ↑1\x1b[0m"
        );
        // Runs of the empty style, and a row that ends in it, need no escape.
        assert_eq!(emit(vec![run(EMPTY, "a"), gap(1), run(EMPTY, "b")], true), "a b");
        assert_eq!(emit(vec![run(RED, "a"), gap(1), run(EMPTY, "b")], true), "\x1b[31ma\x1b[0m b");
        // No color: no escape at all.
        assert_eq!(emit(vec![run(RED, "a"), gap(1), run(DIM, "b")], false), "a b");
        assert_eq!(emit(Vec::new(), true), "");
    }
}
