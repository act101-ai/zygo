pub mod clean;
pub mod command;
pub mod dedup;
pub mod kind_view;
pub mod runner;

/// Raw text above this size gets only the size view (O§7.3).
pub const SHAPE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// A view is used only when it saves at least this many bytes (O§7.8).
pub const MIN_SAVING_BYTES: usize = 256;

use crate::view::VIEW_CAP_BYTES;
use crate::Kind;
use command::Parsed;

/// What the pipeline needs to know about one output (O§7.3).
#[derive(Debug, Clone)]
pub struct ShapeInput {
    /// The parsed command; `None` when the output has no command.
    pub parsed: Option<Parsed>,
    /// The sniffed kind of the raw text.
    pub kind: Kind,
    /// `trim_threshold_bytes`: text over this size needs a view.
    pub threshold: u64,
}

/// The pipeline result (O§7.3, O§7.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shaped {
    /// The text after steps 1-4.
    pub text: String,
    /// The steps that changed the text, in order.
    pub steps: Vec<Step>,
    /// The last step that changed the text, or `Size` when only the size view applies.
    pub shaper: Option<Shaper>,
    /// Step 5, the size view, must run: the text is still over the threshold.
    pub size_view: bool,
}

/// Runs steps 1-4 of O§7.3 in order and tells whether step 5 must run.
pub fn run(raw: &str, input: &ShapeInput) -> Shaped {
    let over = |t: &str| t.len() as u64 > input.threshold;
    if raw.len() > SHAPE_MAX_BYTES {
        let size_view = over(raw);
        return Shaped { text: raw.to_string(), steps: vec![], shaper: size_view.then_some(Shaper::Size), size_view };
    }
    let reuse = input.parsed.as_ref().is_some_and(|p| p.reuse)
        || matches!(input.kind, Kind::Diff | Kind::Json | Kind::Jsonl);
    let mut text = raw.to_string();
    let mut steps = Vec::new();
    let mut shaper = None;
    // Step 1, clean (O§7.4).
    if let Some((t, s)) = clean::clean(&text, reuse) {
        text = t;
        steps.extend(s);
        shaper = Some(Shaper::Clean);
    }
    // Step 2, dedup (O§7.5), only when the model may not reuse the text.
    if !reuse && let Some(t) = dedup::dedup(&text) {
        text = t;
        steps.push(Step::RepeatedLinesCounted);
        shaper = Some(Shaper::Dedup);
    }
    // Step 3, runner view (O§7.6): OS-30.
    // Step 4, kind view (O§7.7), only when over the threshold.
    if over(&text) && let Some((t, s, k)) = kind_view::kind_view(&text, input.kind, VIEW_CAP_BYTES) {
        text = t;
        steps.push(s);
        shaper = Some(k);
    }
    let size_view = over(&text);
    if shaper.is_none() && size_view {
        shaper = Some(Shaper::Size);
    }
    Shaped { text, steps, shaper, size_view }
}

/// One name in the `<steps>` list of the shaped marker (O§7.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    ColorCodesRemoved,
    ProgressLinesRemoved,
    RepeatedLinesCounted,
    PassedTestsHidden,
    BuildProgressHidden,
    DiffFilesListed,
    JsonOutlined,
    LogErrorsKept,
}

impl Step {
    pub fn as_str(&self) -> &'static str {
        match self {
            Step::ColorCodesRemoved => "color codes removed",
            Step::ProgressLinesRemoved => "progress lines removed",
            Step::RepeatedLinesCounted => "repeated lines counted",
            Step::PassedTestsHidden => "passed tests hidden",
            Step::BuildProgressHidden => "build progress hidden",
            Step::DiffFilesListed => "diff files listed",
            Step::JsonOutlined => "json outlined",
            Step::LogErrorsKept => "log errors kept",
        }
    }
}

/// The last step that changed the text, as the `trims.shaper` column records it (O§7.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shaper {
    Libtest,
    Pytest,
    Gotest,
    Diff,
    Json,
    Log,
    Dedup,
    Clean,
    Size,
}

impl Shaper {
    pub fn as_str(&self) -> &'static str {
        match self {
            Shaper::Libtest => "libtest",
            Shaper::Pytest => "pytest",
            Shaper::Gotest => "gotest",
            Shaper::Diff => "diff",
            Shaper::Json => "json",
            Shaper::Log => "log",
            Shaper::Dedup => "dedup",
            Shaper::Clean => "clean",
            Shaper::Size => "size",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_names_are_the_spec_strings() {
        // O§7.8 `<steps>` fixed set
        let all = [
            (Step::ColorCodesRemoved, "color codes removed"),
            (Step::ProgressLinesRemoved, "progress lines removed"),
            (Step::RepeatedLinesCounted, "repeated lines counted"),
            (Step::PassedTestsHidden, "passed tests hidden"),
            (Step::BuildProgressHidden, "build progress hidden"),
            (Step::DiffFilesListed, "diff files listed"),
            (Step::JsonOutlined, "json outlined"),
            (Step::LogErrorsKept, "log errors kept"),
        ];
        for (s, want) in all {
            assert_eq!(s.as_str(), want, "{s:?}");
        }
    }

    #[test]
    fn shaper_names_are_the_spec_strings() {
        // O§7.9 `shaper` values
        let all = [
            (Shaper::Libtest, "libtest"),
            (Shaper::Pytest, "pytest"),
            (Shaper::Gotest, "gotest"),
            (Shaper::Diff, "diff"),
            (Shaper::Json, "json"),
            (Shaper::Log, "log"),
            (Shaper::Dedup, "dedup"),
            (Shaper::Clean, "clean"),
            (Shaper::Size, "size"),
        ];
        for (s, want) in all {
            assert_eq!(s.as_str(), want, "{s:?}");
        }
    }

    #[test]
    fn shape_byte_bounds_are_the_spec_values() {
        // O§7.3 4 MiB; O§7.8 256; O§9 constants, not config
        assert_eq!(SHAPE_MAX_BYTES, 4_194_304);
        assert_eq!(MIN_SAVING_BYTES, 256);
    }
}
