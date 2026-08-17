//! Selecting one atom's candidate (`resolve_pretend`).

use crate::*;

/// Arguments of [`dependency_avoid_update_candidate`].
pub(crate) struct DependencyAvoidUpdateCandidateArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) atom: &'a portage_dep::Atom,
    pub(crate) atom_str: &'a str,
    pub(crate) candidates: &'a [Candidate],
    pub(crate) installed: &'a [(String, String, String)],
    pub(crate) config: &'a portage_profile::Config,
}

/// Portage's early `avoid_update` return for a DEPENDENCY atom: the highest installed
/// version of `category/package` matching `atom_str` (version/slot/repo, via the FULL
/// `candidates` list, deliberately NOT `is_visible`-filtered) that also satisfies the
/// atom's USE-deps against that version's own installed vdb `USE`/`IUSE` (not the
/// current tree's). `None` when no installed version qualifies.
///
/// `installed` is the vdb's `(version, slot, sub_slot)` triples: a repo candidate only
/// counts when that *exact `(version, slot)` pair* is installed (Portage's
/// `vardb.match(atom)` matches the slot too).
///
/// Called twice in `resolve_pretend`: once *before* tree visibility filtering (so a
/// dependency satisfied only by a masked-but-installed version never spuriously hits
/// `NoVisibleCandidate`), and once more from the ordinary `!update` shortcut (the only
/// place still reachable with `--exclude`).
// Portage: lib/_emerge/depgraph.py, _select_pkg_highest_available_imp;
// _want_update_pkg; _iter_match_pkgs; dbapi._iuse_implicit_cnstr
pub(crate) fn dependency_avoid_update_candidate<'a>(
    args: DependencyAvoidUpdateCandidateArgs<'a>,
) -> Option<&'a Candidate> {
    let DependencyAvoidUpdateCandidateArgs {
        root,
        atom,
        atom_str,
        candidates,
        installed,
        config,
    } = args;
    let all_candidates_by_str: HashMap<String, &Candidate> = candidates
        .iter()
        .map(|c| {
            (
                format!(
                    "{}/{}-{}:{}/{}::{}",
                    atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
                ),
                c,
            )
        })
        .collect();
    let all_refs: Vec<&str> = all_candidates_by_str.keys().map(String::as_str).collect();
    portage_dep::match_from_list(atom_str, &all_refs)?
        .into_iter()
        .filter_map(|m| all_candidates_by_str.get(m).copied())
        .filter(|c| {
            installed
                .iter()
                .any(|(v, s, _sub)| v == &c.version && s == &c.slot)
        })
        .filter(|c| match &atom.use_deps {
            Some(use_deps) if !use_deps.is_empty() => {
                let vdb_iuse =
                    read_vdb_flag_set(root, &atom.category, &atom.package, &c.version, "IUSE");
                let vdb_use =
                    read_vdb_flag_set(root, &atom.category, &atom.package, &c.version, "USE");
                // Portage's `dbapi._iuse_implicit_cnstr` domain for a *built* package: recorded
                // `IUSE` ∪ profile `IUSE_EFFECTIVE` (via `valid_iuse`) ∪ the package's own
                // recorded `USE` (a built package's own `USE` is authoritative,
                // e.g. an ebuild that has since dropped a flag from `IUSE`). Portuale
                // recomputes rather than reading a persisted `IUSE_EFFECTIVE` file.
                let mut valid = valid_iuse(&vdb_iuse, config);
                valid.extend(vdb_use.iter().cloned());
                portage_dep::use_deps_satisfied(use_deps, &valid, &vdb_use)
            }
            _ => true,
        })
        .max_by(|a, b| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
}

/// Shared by both of `resolve_pretend`'s `!update` shortcut call sites: once an
/// installed version is chosen to keep, decides between `AlreadyInstalled` and
/// `Reinstall`. Each of `newuse`/`changed_use`/`changed_deps`/`changed_slot`/
/// `rebuilt_binaries`/`newrepo` can independently trigger a reinstall with no
/// version change at all.
pub(crate) fn already_installed_or_reinstall(
    source: &SelectSource,
    opts: &SelectOptions,
    atom: &portage_dep::Atom,
    installed_best: &Candidate,
    // `BUILD_TIME` of the binary candidate at `installed_best.version`, for the
    // `--rebuilt-binaries` check; `None` when the pool has no binary for it.
    binary_build_time: Option<i64>,
) -> Result<PretendOutcome, Error> {
    let SelectSource {
        repos,
        root,
        config,
        ..
    } = *source;
    let SelectOptions {
        newuse,
        changed_use,
        changed_deps,
        with_bdeps,
        changed_slot,
        usepkg,
        usepkgonly,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        newrepo,
        // `--emptytree`/`-e`: installed packages are not selected, so every atom in
        // the deep tree resolves to a merge. The pool is already tree-only, so `empty`
        // forces the would-be `AlreadyInstalled` fallback to a bare `Reinstall`
        // (Portage's reasonless `[ebuild R]`).
        // Portage: create_depgraph_params.py; depgraph.py; output.py attr_display
        empty,
        ..
    } = *opts;
    let changed_flags = if newuse || changed_use {
        reinstall_flags_for_use_change(ReinstallFlagsForUseChangeArgs {
            root,
            category: &atom.category,
            package: &atom.package,
            candidate: installed_best,
            config,
            newuse,
        })
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    let deps_changed_flag = changed_deps
        && deps_changed(DepsChangedArgs {
            root,
            repos,
            category: &atom.category,
            package: &atom.package,
            version: &installed_best.version,
            with_bdeps,
        });
    let slot_changed_flag = changed_slot
        && slot_changed(
            root,
            repos,
            &atom.category,
            &atom.package,
            &installed_best.version,
        );
    let rebuilt_binary_flag = (usepkg || usepkgonly)
        && rebuilt_binaries
        && rebuilt_binary_changed(RebuiltBinaryChangedArgs {
            root,
            binary_build_time,
            category: &atom.category,
            package: &atom.package,
            version: &installed_best.version,
            rebuilt_binaries_timestamp,
        });
    let new_repo_flag = newrepo
        && new_repo_changed(
            root,
            &atom.category,
            &atom.package,
            &installed_best.version,
            &installed_best.repo_name,
        );
    if empty
        || !changed_flags.is_empty()
        || deps_changed_flag
        || slot_changed_flag
        || rebuilt_binary_flag
        || new_repo_flag
    {
        return Ok(PretendOutcome::Reinstall {
            version: installed_best.version.clone(),
            changed_flags,
            deps_changed: deps_changed_flag,
            slot_changed: slot_changed_flag,
            rebuilt_binary: rebuilt_binary_flag,
            new_repo: new_repo_flag,
            slot_operator_rebuild: false,
        });
    }
    Ok(PretendOutcome::AlreadyInstalled {
        version: installed_best.version.clone(),
    })
}

// Reads one `slot_operator_mask_built` negative in binary-marked form
// (`!=cat/pkg-ver[binary]`). Portage keys the mask by package object so only the
// binary instance hides; portuale's string readers match both, so the marker keeps
// them apart: tree readers skip marked entries, only the binary-pool filter honours
// them (via this function).
// Portage: depgraph.py, _slot_change_backtrack; MaskReason::SlotOperatorBuilt
pub(crate) fn slot_operator_binary_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[binary]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}

// Whether a string constraint is a binary-only `slot_operator_mask_built` negative:
// every `extra_constraints` filter and the missing-dep probe skip these (the
// same-version ebuild stays visible); only the binary-pool filter honours them.
pub(crate) fn is_binary_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[binary]"))
}

// The installed-only half of the same per-instance distinction
// (`!=cat/pkg-ver[installed]`, filed by the missing-dep trigger for an installed
// parent with no merge-bound version): tree readers skip marked entries, only the
// installed-instance selection honours them. A marked negative never parses as a
// version, so the missed-update collection skips it too.
// Portage: depgraph.py, _add_dep missing-dependency mask
pub(crate) fn is_installed_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[installed]"))
}

// Reads one installed-only missing-dependency negative. Mirror of
// [`slot_operator_binary_masked_version`] for the other instance kind.
pub(crate) fn slot_operator_installed_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[installed]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}

// Every per-instance string negative (`[binary]` and `[installed]` marks):
// tree-candidate readers skip them all; instance pools keep honouring their own mark.
// Portage keys every such mask by package object.
pub(crate) fn is_instance_only_mask(constraint: &str) -> bool {
    is_binary_only_mask(constraint) || is_installed_only_mask(constraint)
}

// A bundled options struct would touch every call site for each single-flag
// addition; not worth it (same reasoning as `resolve_pretend_graph`'s allow).
/// Where [`resolve_pretend`] selects from: the repos, the target root,
/// its config and the local binary-package index.
#[derive(Clone, Copy)]
pub struct SelectSource<'a> {
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub config: &'a portage_profile::Config,
    /// The run's already-built local `$PKGDIR` binary index.
    pub local_binpkg: &'a std::sync::Arc<BinaryIndex>,
}

/// The emerge options that shape one [`resolve_pretend`] selection.
/// `Default` is emerge with no option given (`--with-bdeps` and
/// `--selective` on, everything else off) for a dependency atom.
#[derive(Debug, Clone, Copy)]
pub struct SelectOptions<'a> {
    pub newuse: bool,
    pub changed_use: bool,
    pub update: bool,
    pub excluded: &'a [String],
    pub changed_deps: bool,
    pub with_bdeps: bool,
    pub changed_slot: bool,
    pub selective: bool,
    pub is_top_level: bool,
    pub usepkg: bool,
    pub usepkgonly: bool,
    pub binpkg_respect_use: bool,
    pub usepkg_exclude: &'a [String],
    pub usepkg_include: &'a [String],
    pub rebuilt_binaries: bool,
    pub rebuilt_binaries_timestamp: Option<u64>,
    pub newrepo: bool,
    pub empty: bool,
    pub getbinpkg: bool,
    pub autounmask_keywords: bool,
    pub autounmask_use: bool,
    pub autounmask_license: bool,
    pub autounmask_masks: bool,
    pub extra_constraints: &'a [String],
}

impl Default for SelectOptions<'_> {
    fn default() -> Self {
        Self {
            newuse: false,
            changed_use: false,
            update: false,
            excluded: &[],
            changed_deps: false,
            with_bdeps: true,
            changed_slot: false,
            selective: true,
            is_top_level: false,
            usepkg: false,
            usepkgonly: false,
            binpkg_respect_use: false,
            usepkg_exclude: &[],
            usepkg_include: &[],
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            newrepo: false,
            empty: false,
            getbinpkg: false,
            autounmask_keywords: false,
            autounmask_use: false,
            autounmask_license: false,
            autounmask_masks: false,
            extra_constraints: &[],
        }
    }
}
