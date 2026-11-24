/// Raw text above this size gets only the size view (O§7.3).
pub const SHAPE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// A view is used only when it saves at least this many bytes (O§7.8).
pub const MIN_SAVING_BYTES: usize = 256;

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
