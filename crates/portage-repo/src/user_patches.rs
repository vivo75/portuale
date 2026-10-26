//! `--binpkg-respect-user-patches` (backlog #509): a binary whose recorded
//! `USER_PATCHES` differs from the local patch-set digest is not a merge
//! candidate, exactly like `--binpkg-respect-use` rejects a wrong-USE build.
//! Default on (also with `--usepkgonly`); `=n` disables the comparison.
//! An index entry without the key counts as `""` (the `-r1` empty default,
//! bug 984255), so a binary built without patches is trusted when the local
//! configuration has no patches either.
//! Portage: depgraph.py (the `pkg.user_patches != pkgsettings.userPatchDigest`
//! gate, the `"user patches"` mask reason, `_show_ignored_binaries_user_patches`),
//! Package.py (`user_patches`), config.py (`userPatchDigest`/`userPatchFiles`).

use crate::*;
use portage_profile::Config;

/// The local patch-set digest for one binary's package (`""` when no patch
/// set matches, or the configuration has no patches directory at all):
/// the half the index entry's `USER_PATCHES` is compared against.
// Portage: config.py, `userPatchDigest`
pub(crate) fn local_user_patch_digest(
    config: &Config,
    category: &str,
    package: &str,
    version: &str,
    slot: &str,
) -> String {
    config
        .user_patches
        .as_ref()
        .map(|set| set.digest(category, package, version, slot))
        .unwrap_or_default()
}

/// Whether the binary survives the filter: its recorded digest equals the
/// local one (both `""` when neither side has patches).
// Portage: depgraph.py, `binpkg_respect_user_patches and pkg.user_patches != digest`
pub(crate) fn binary_user_patches_ok(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &Config,
) -> bool {
    candidate.user_patches
        == local_user_patch_digest(
            config,
            category,
            package,
            &candidate.version,
            &candidate.slot,
        )
}

/// The patch files Portage would apply for the package, as `cat/dir/file`
/// paths: the `--verbose` tail of the ignored-binaries block.
// Portage: config.py, `userPatchFiles`
pub(crate) fn user_patch_files_for(
    config: &Config,
    category: &str,
    package: &str,
    version: &str,
    slot: &str,
) -> Vec<String> {
    config
        .user_patches
        .as_ref()
        .map(|set| set.files(category, package, version, slot))
        .unwrap_or_default()
}

/// One merge-selected package for the elimination rule below.
pub(crate) struct SelectedPackage<'a> {
    pub version: &'a str,
    pub source: CandidateSource,
    pub slot: &'a str,
}

/// Portage's `_eliminate_ignored_binaries` (upstream `a7c346aa4`): a dropped
/// binary is not reported when a more optimal alternative fills its slot —
/// a higher version selected, or an equal-version binary selected (`>=` is
/// true on equal versions; its build-time arm is unreachable). The
/// identical-installed-instance arm is subsumed by the merge-list filter: a
/// kept installed instance never merges, so its cp is absent there.
pub(crate) fn eliminate_ignored(
    rows: Vec<IgnoredUserPatchBinary>,
    selected: &[SelectedPackage<'_>],
) -> Vec<IgnoredUserPatchBinary> {
    rows.into_iter()
        .filter(|row| {
            !selected.iter().any(|s| {
                s.slot.split('/').next().unwrap_or(s.slot)
                    == row.slot.split('/').next().unwrap_or(&row.slot)
                    && match vercmp_ordering(s.version, &row.version) {
                        std::cmp::Ordering::Greater => true,
                        std::cmp::Ordering::Equal => s.source == CandidateSource::Binary,
                        std::cmp::Ordering::Less => false,
                    }
            })
        })
        .collect()
}

/// One binary the filter dropped, for the ignored-binaries block: the
/// `cpv[:slot][-build_id]` row plus the local patch files behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredUserPatchBinary {
    pub category: String,
    pub package: String,
    pub version: String,
    pub slot: String,
    pub build_id: Option<String>,
    pub files: Vec<String>,
}

/// The binaries of one pool the filter drops: each dropped candidate once
/// (Portage accumulates one entry per dropped package; multi-instance
/// builds share the row only through the caller's cpv dedup).
pub(crate) fn user_patch_ignored_for_pool(
    binaries: &[Candidate],
    category: &str,
    package: &str,
    config: &Config,
) -> Vec<IgnoredUserPatchBinary> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in binaries {
        if c.source != CandidateSource::Binary {
            continue;
        }
        if binary_user_patches_ok(c, category, package, config) {
            continue;
        }
        if !seen.insert((c.version.clone(), c.build_id.clone())) {
            continue;
        }
        out.push(IgnoredUserPatchBinary {
            category: category.to_string(),
            package: package.to_string(),
            version: c.version.clone(),
            slot: c.slot.clone(),
            build_id: c.build_id.clone(),
            files: user_patch_files_for(config, category, package, &c.version, &c.slot),
        });
    }
    out
}

/// The values [`ignored_user_patches_for_merging`] reads from its caller.
pub(crate) struct IgnoredUserPatchesForMergingArgs<'a> {
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) respect: bool,
    pub(crate) local_binpkg: &'a std::sync::Arc<BinaryIndex>,
    pub(crate) binrepos: &'a [portage_profile::BinRepo],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) merging: &'a [((String, String), Vec<SelectedPackage<'a>>)],
}

/// Success-path rows: for every merging `(cp, slot)`, the dropped binaries
/// of the local and (under `--getbinpkg`) remote pools, minus
/// `_eliminate_ignored_binaries`. Empty unless a binary pool was consulted
/// and the filter is on. Portage's displayed-list filter is the merge list
/// itself (`_serialized_tasks_cache`).
// Portage: depgraph.py, _show_ignored_binaries
pub(crate) fn ignored_user_patches_for_merging(
    args: IgnoredUserPatchesForMergingArgs<'_>,
) -> Vec<IgnoredUserPatchBinary> {
    let IgnoredUserPatchesForMergingArgs {
        usepkg,
        usepkgonly,
        getbinpkg,
        respect,
        local_binpkg,
        binrepos,
        root,
        config,
        merging,
    } = args;
    if !respect || !(usepkg || usepkgonly || getbinpkg) {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for ((category, package), selected) in merging {
        let mut binaries = list_binary_candidates(local_binpkg, category, package);
        if getbinpkg {
            binaries.extend(list_remote_binary_candidates(
                binrepos,
                root,
                local_binpkg,
                category,
                package,
            ));
        }
        rows.extend(ignored_user_patches_for_atom(
            &binaries, category, package, config, selected,
        ));
    }
    rows.sort_by(|a, b| {
        (
            a.category.as_str(),
            a.package.as_str(),
            a.version.as_str(),
            a.build_id.as_deref(),
        )
            .cmp(&(
                b.category.as_str(),
                b.package.as_str(),
                b.version.as_str(),
                b.build_id.as_deref(),
            ))
    });
    rows
}

/// Recompute Portage's `ignored_binaries["user_patches"]` rows for one atom
/// over the pools selection consulted. `selected` is empty on the abort
/// path (nothing was selected) and the merging entries on the success path
/// (Portage's displayed-list filter plus `_eliminate_ignored_binaries`).
pub(crate) fn ignored_user_patches_for_atom(
    binaries: &[Candidate],
    category: &str,
    package: &str,
    config: &Config,
    selected: &[SelectedPackage<'_>],
) -> Vec<IgnoredUserPatchBinary> {
    eliminate_ignored(
        user_patch_ignored_for_pool(binaries, category, package, config),
        selected,
    )
}

/// Render the ignored-binaries block (`None` when no row): the rows sorted
/// like Portage (`sorted(messages)`), the `--verbose` triggering-files tail,
/// and the `=n`/`=y` NOTE. The trailing newline belongs to the caller stream
/// position (Portage's `writemsg` lines each carry their own `\n`).
// Portage: depgraph.py, _show_ignored_binaries_user_patches
pub fn render_ignored_user_patch_binaries(
    rows: &[IgnoredUserPatchBinary],
    root: &Path,
    verbose: bool,
) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = rows
        .iter()
        .map(|r| {
            let mut line = format!("     {}/{}-{}", r.category, r.package, r.version);
            if r.slot != "0" {
                line.push(':');
                line.push_str(&r.slot);
            }
            if let Some(id) = r.build_id.as_deref().filter(|s| !s.is_empty()) {
                line.push('-');
                line.push_str(id);
            }
            // Portage appends ` for <root>` (trailing slash: its `ROOT`
            // always ends in one) whenever the root is not `/`.
            let norm = root.display().to_string();
            let norm = norm.trim_end_matches('/');
            if !norm.is_empty() && norm != "/" {
                line.push_str(&format!(" for {norm}/"));
            }
            line
        })
        .collect();
    lines.sort();
    let mut out = String::from(
        "\n!!! The following binary packages have been ignored due to user patches:\n\n",
    );
    for line in &lines {
        out.push_str(line);
        out.push('\n');
    }
    if verbose {
        let mut patchset: Vec<&str> = rows
            .iter()
            .flat_map(|r| r.files.iter().map(String::as_str))
            .collect();
        patchset.sort();
        patchset.dedup();
        if !patchset.is_empty() {
            out.push_str("\n!!! These user patches are triggering this warning:\n\n");
            for file in patchset {
                out.push_str(&format!("    {file}\n"));
            }
        }
    }
    out.push_str(
        "\nNOTE: The --binpkg-respect-user-patches=n option will prevent\n      emerge from ignoring these binary packages.\n      Using --binpkg-respect-user-patches=y will silence this\n      warning.\n",
    );
    Some(out)
}
