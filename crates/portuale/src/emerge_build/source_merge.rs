//! The source-merge loop: build, then merge, one entry at a time.

use crate::emerge_build::*;

/// Arguments of [`run_source_merge`].
pub struct RunSourceMergeArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a ebuild_merge::MergeOptions,
    pub keep_going: bool,
    pub buildpkg: Option<&'a ebuild_package::PackageOptions>,
    pub buildpkg_exclude: &'a [String],
    pub jobs: usize,
    pub load_average: Option<f64>,
    /// Whether to redirect each package's build output to `${T}/build.log`.
    /// Always true for `jobs` > 1; otherwise driven by `--quiet-build=y` / `-q`.
    pub capture_log: bool,
    /// Scheduler-display mode, computed once by the caller from the CLI flags
    /// and the mergelist length. Carries the display half only, never the
    /// capture decision itself.
    pub mode: StatusMode,
}
