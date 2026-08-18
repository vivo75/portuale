//! Progress and status lines of the build scheduler.

use crate::emerge_build::*;

/// Portage's `Scheduler._background_mode` as a value: `background` is a
/// parallel `--jobs` run, `--quiet`, or `--quiet-build=y` (except that a
/// one-package mergelist without `--quiet`/`--quiet-build=y` resets to
/// serial and non-background); `show_jobs` is `background &&
/// (!quiet || verbose)`.
///
/// Two deliberate cuts: interactive packages (portuale models no
/// `interactive` PROPERTIES) and the `--pretend`/`--fetchonly`/
/// `--fetch-all-uri` exemption (those modes never reach this code).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct StatusMode {
    /// Real `Scheduler._background`: status lines print with no leading
    /// blank, and build output is captured, not streamed.
    pub background: bool,
    /// Real `JobStatusDisplay` is live: `>>> Jobs:` lines print.
    pub show_jobs: bool,
}

impl StatusMode {
    /// A mode for unit tests: serial, non-background, no Jobs lines.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        StatusMode {
            background: false,
            show_jobs: false,
        }
    }
}
