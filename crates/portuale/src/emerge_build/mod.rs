//! `--buildpkgonly` execution, without `--pretend`: builds a binary
//! package for every entry the pretend gate proved safe to build.
//! Reuses `ebuild_package::run_package`; `locate_candidate` re-derives
//! the winning candidate's repo via `portage_repo::list_candidates`.

use crate::ebuild_merge;

#[cfg(test)]
mod tests;

use crate::ebuild_phases;

use portage_repo::{Candidate, CandidateSource, GraphEntry, PretendOutcome, RepoConfig};

use std::path::{Path, PathBuf};
