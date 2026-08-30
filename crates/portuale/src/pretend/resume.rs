//! `--resume`.

use crate::pretend::*;

/// The `(kind, category, package, version)` of every entry in `entries`
/// (source or binary) that was supposed to merge but has no `CONTENTS`
/// under `root` yet: the resume list for `mtimedb::write_resume_list` on
/// the failure path.
pub(crate) fn entries_not_merged(
    root: &Path,
    entries: &[portage_repo::GraphEntry],
) -> Vec<crate::mtimedb::ResumeCpv> {
    let mut out = Vec::new();
    for e in entries {
        let Some((kind, category, package, version)) = emerge_build::resume_cpv(e) else {
            continue;
        };
        // One `stat` of the entry's `CONTENTS` through the root's backend.
        if !ebuild_merge::entry_file_is_regular(
            root,
            &category,
            &format!("{package}-{version}"),
            "CONTENTS",
        ) {
            out.push((kind, category, package, version));
        }
    }
    out
}
