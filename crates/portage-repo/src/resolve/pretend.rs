//! Selecting one atom's candidate (`resolve_pretend`).

use crate::*;
use portage_dep::Atom;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::sync::Arc;

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

/// Portage `_too_deep` for `_want_update_pkg` (`depgraph.py:7359-7375`),
/// defined directly: with no `--deep` any dep depth counts as too deep,
/// unlimited `--deep` never does, `--deep=N` counts past `N`. Deliberately
/// not negated `Deep::recurses_at`: that answers whether an installed
/// package's own deps are walked, a different question (never, for the
/// default) with a different signature.
pub(crate) fn update_too_deep(deep: Deep, depth: u32) -> bool {
    match deep {
        Deep::NotRequested => depth > 0,
        Deep::Unlimited => false,
        Deep::Bounded(n) => depth > n,
    }
}

/// Portage `_wrapped_add_pkg_dep_string`'s `recurse_satisfied`
/// (`depgraph.py:4474`): with unlimited `--deep`, or the dep's own depth
/// within a bounded `--deep`, the installed satisfier's subtree is walked,
/// so `_ignore_dependency` never drops the edge.
pub(crate) fn update_recurse_satisfied(deep: Deep, depth: u32) -> bool {
    match deep {
        Deep::Unlimited => true,
        Deep::Bounded(n) => depth <= n,
        Deep::NotRequested => depth == 0,
    }
}

/// The values [`installed_matches_argument`] reads from its caller.
pub(crate) struct InstalledArgumentMatchArgs<'a> {
    pub(crate) arg_atoms: &'a [String],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) repo_name: &'a str,
}

/// Whether the installed `category/package-version` matches a top-level
/// argument atom: Portage `_iter_atoms_for_pkg` minus the higher-slot guard
/// (greedy-slot territory, owned by #359 S3) and the internal-set
/// `reset_depth=False` cases (slot-operator seeds never sit in `arg_atoms`).
/// A match resets the update depth to 0.
pub(crate) fn installed_matches_argument(args: InstalledArgumentMatchArgs<'_>) -> bool {
    let InstalledArgumentMatchArgs {
        arg_atoms,
        category,
        package,
        version,
        slot,
        sub_slot,
        repo_name,
    } = args;
    let probe = format!("{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}");
    with_arg_cp_index(arg_atoms, |index| {
        index
            .get(category)
            .and_then(|by_pkg| by_pkg.get(package))
            .is_some_and(|args| {
                args.iter().any(|&i| {
                    portage_dep::match_from_list(&arg_atoms[i], &[probe.as_str()])
                        .is_some_and(|r| !r.is_empty())
                })
            })
    })
}

type ArgCpIndex = HashMap<String, HashMap<String, Vec<usize>>>;

/// Runs `f` on the `category -> package -> arg indices` index of `arg_atoms`.
/// The last index built is kept and reused while the atoms compare equal, so
/// a walk's thousands of calls over its fixed argument list do not re-hash
/// every argument each time.
fn with_arg_cp_index<R>(arg_atoms: &[String], f: impl FnOnce(&ArgCpIndex) -> R) -> R {
    thread_local! {
        static LAST: std::cell::RefCell<Option<(Vec<String>, ArgCpIndex)>> =
            const { std::cell::RefCell::new(None) };
    }
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if last.as_ref().is_none_or(|(atoms, _)| atoms != arg_atoms) {
            let mut index = ArgCpIndex::new();
            for (i, arg) in arg_atoms.iter().enumerate() {
                if let Some(a) = portage_dep::parse_atom(arg) {
                    index
                        .entry(a.category)
                        .or_default()
                        .entry(a.package)
                        .or_default()
                        .push(i);
                }
            }
            *last = Some((arg_atoms.to_vec(), index));
        }
        f(&last.as_ref().expect("just set").1)
    })
}

/// Whether every positively-required USE-dep flag of `atom` appears in some
/// *visible* tree candidate's IUSE: without that, Portage reports the dep
/// unsatisfiable (`Missing IUSE`) instead of keeping an installed instance
/// whose own recorded USE happens to carry the flag. Checked against the
/// tree, not the vdb, so an installed-only flag carrier still errors.
fn tree_knows_use_dep_flags(
    candidates: &[Candidate],
    atom: &portage_dep::Atom,
    config: &portage_profile::Config,
) -> bool {
    let Some(deps) = atom.use_deps.as_deref() else {
        return true;
    };
    deps.iter()
        .filter(|d| d.op == portage_dep::UseDepOp::Enabled)
        .all(|need| {
            candidates.iter().any(|c| {
                is_visible(c, &atom.category, &atom.package, config)
                    && candidate_iuse_and_use(c, &atom.category, &atom.package, config)
                        .is_some_and(|(iuse, _)| iuse.contains(&need.flag))
            })
        })
}

/// Whether an installed `cat/pkg-version` string satisfies every extra
/// constraint of the request: the string twin of
/// [`satisfies_extra_constraints`] for installed instances with no tree
/// candidate to build the string from.
fn installed_probe_satisfies_extra_constraints(probe: &str, extra_constraints: &[String]) -> bool {
    extra_constraints
        .iter()
        .all(|ec| match ec.strip_prefix('!') {
            Some(_) if is_instance_only_mask(ec) => true,
            Some(neg) => {
                !portage_dep::match_from_list(neg, &[probe]).is_some_and(|r| !r.is_empty())
            }
            None => portage_dep::match_from_list(ec, &[probe]).is_some_and(|r| !r.is_empty()),
        })
}

/// The values [`avoid_update_outcome`] reads from its caller.
pub(crate) struct AvoidUpdateOutcomeArgs<'a> {
    pub(crate) source: &'a SelectSource<'a>,
    pub(crate) opts: &'a SelectOptions<'a>,
    pub(crate) atom: &'a portage_dep::Atom,
    pub(crate) candidates: &'a [Candidate],
    pub(crate) installed_best: &'a Candidate,
    pub(crate) config: &'a portage_profile::Config,
}

/// What a dependency atom resolves to when Portage would not want it
/// updated (`!_want_update_pkg`): the installed instance is kept without
/// the `--newuse`/`--changed-use` reinstall flip and without the visibility
/// filter (`:8445-8458` return, `:7972-7979` masked-installed keep), unless a
/// reinstall genuinely hides the vardb and Portage keeps the ebuild child
/// (invisible satisfier, recursion, or an argument naming it).
// Portage: lib/_emerge/depgraph.py, _wrapped_select_pkg_highest_available_imp;
// _want_update_pkg; _ignore_dependency
pub(crate) fn avoid_update_outcome(
    args: AvoidUpdateOutcomeArgs<'_>,
) -> Result<PretendOutcome, Error> {
    let AvoidUpdateOutcomeArgs {
        source,
        opts,
        atom,
        candidates,
        installed_best,
        config,
    } = args;
    let SelectOptions {
        newuse,
        changed_use,
        changed_deps,
        with_bdeps,
        changed_slot,
        newrepo,
        deep,
        depth,
        arg_atoms,
        ..
    } = *opts;
    // A pending reinstall hides the vardb in Portage only for a same-cpv
    // ebuild USE-drift comparison (`:8311-8329`, gated on
    // `cpv in vardb.match(atom)`), `--newrepo`, or the installed-side
    // `break` for `--changed-deps`/`--changed-slot` (`:8291-8307`: the
    // installed instance is skipped, the ebuild child selected). A masked
    // installed package has no visible same-cpv ebuild, so its USE drift
    // never hides it. (`--reinstall-atoms` is deliberately not a hiding
    // trigger here: the pass fallback flips the kept instance back to
    // `Reinstall` afterwards. `--rebuilt-binaries` never hides the vardb
    // either: a drifting binary is skipped, the installed instance kept.)
    let use_drift = (newuse || changed_use)
        && reinstall_flags_for_use_change(ReinstallFlagsForUseChangeArgs {
            root: source.root,
            category: &atom.category,
            package: &atom.package,
            candidate: installed_best,
            config,
            newuse,
        })
        .is_some_and(|flags| !flags.is_empty());
    let same_cpv_visible = candidates.iter().any(|c| {
        c.version == installed_best.version && is_visible(c, &atom.category, &atom.package, config)
    });
    let newrepo_fires = newrepo
        && new_repo_changed(
            source.root,
            &atom.category,
            &atom.package,
            &installed_best.version,
            &installed_best.repo_name,
        );
    let deps_changed_fires = changed_deps
        && deps_changed(DepsChangedArgs {
            root: source.root,
            repos: source.repos,
            category: &atom.category,
            package: &atom.package,
            version: &installed_best.version,
            with_bdeps,
        });
    let slot_changed_fires = changed_slot
        && slot_changed(
            source.root,
            source.repos,
            &atom.category,
            &atom.package,
            &installed_best.version,
        );
    let flip = already_installed_or_reinstall(
        source,
        opts,
        atom,
        installed_best,
        best_binary_build_time(candidates, &installed_best.version),
    )?;
    if !matches!(flip, PretendOutcome::Reinstall { .. }) {
        return Ok(flip);
    }
    if !(use_drift && same_cpv_visible)
        && !newrepo_fires
        && !deps_changed_fires
        && !slot_changed_fires
    {
        return Ok(PretendOutcome::AlreadyInstalled {
            version: installed_best.version.clone(),
        });
    }
    // The vardb is hidden and the ebuild child selected; Portage drops that
    // edge (`_ignore_dependency`) unless an argument names the child, the
    // installed satisfier is invisible, or the walk recurses into it. The
    // reinstall and the child share one version by construction, so the
    // argument test reuses the installed instance.
    let arg_match = installed_matches_argument(InstalledArgumentMatchArgs {
        arg_atoms,
        category: &atom.category,
        package: &atom.package,
        version: &installed_best.version,
        slot: &installed_best.slot,
        sub_slot: &installed_best.sub_slot,
        repo_name: &installed_best.repo_name,
    });
    if !arg_match && same_cpv_visible && !update_recurse_satisfied(deep, depth) {
        return Ok(PretendOutcome::AlreadyInstalled {
            version: installed_best.version.clone(),
        });
    }
    Ok(flip)
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
/// `depth`/`deep`/`complete_mode`/`arg_atoms` are walk state for Portage's
/// `_want_update_pkg` (`_depth_increment` reset included): direct callers
/// resolving a dependency set `depth` to the dep's own depth (top-level
/// atoms resolve at depth 0 through the walk, never through `Default`).
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
    /// The queued depth of the atom being resolved (0 for a top-level atom).
    pub depth: u32,
    /// The run's `--deep` setting, for `_too_deep`.
    pub deep: Deep,
    /// Portage's `_complete_mode`: nothing is ever wanted for update in it.
    pub complete_mode: bool,
    /// The run's top-level atom texts (set members included), for the
    /// `_iter_atoms_for_pkg` argument match (a match resets depth to 0).
    pub arg_atoms: &'a [String],
    /// The edge from the owner to this atom is an unsatisfied build-time
    /// dep with no run-time alternative (`QueueItem::buildtime_hard`): an
    /// installed-only instance still satisfies it here (the S0
    /// `s0-e-gbapp-pvuD` capture: Portage keeps installed-only `pg34-gone`
    /// for a `BDEPEND` edge of a package being merged).
    pub buildtime_hard: bool,
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
            depth: 0,
            deep: Deep::NotRequested,
            complete_mode: false,
            arg_atoms: &[],
            buildtime_hard: false,
        }
    }
}

impl ResolveCtx<'_> {
    /// The selection options this walk passes for every atom, with
    /// `bp`'s autounmask switches; callers set `is_top_level` and
    /// `extra_constraints` per atom.
    /// The options [`enqueue_dependencies`] reads from this walk.
    pub(crate) fn enqueue_options(&self) -> EnqueueOptions<'_> {
        EnqueueOptions {
            dynamic_deps: self.dynamic_deps,
            ignore_built_slot_operator_deps: self.ignore_built_slot_operator_deps,
            with_bdeps: self.with_bdeps,
            root_deps_running_root: self.root_deps_running_root,
            update: self.update,
        }
    }

    pub(crate) fn select_options(&self, bp: &BacktrackParams) -> SelectOptions<'_> {
        SelectOptions {
            newuse: self.newuse,
            changed_use: self.changed_use,
            update: self.update,
            excluded: self.excluded,
            changed_deps: self.changed_deps,
            with_bdeps: self.with_bdeps,
            changed_slot: self.changed_slot,
            selective: self.selective,
            usepkg: self.usepkg,
            usepkgonly: self.usepkgonly,
            binpkg_respect_use: self.binpkg_respect_use,
            usepkg_exclude: self.usepkg_exclude,
            usepkg_include: self.usepkg_include,
            rebuilt_binaries: self.rebuilt_binaries,
            rebuilt_binaries_timestamp: self.rebuilt_binaries_timestamp,
            newrepo: self.newrepo,
            empty: self.empty,
            getbinpkg: self.getbinpkg,
            autounmask_keywords: bp.autounmask_suggest_keywords,
            autounmask_use: bp.autounmask_suggest_use,
            autounmask_license: bp.autounmask_suggest_license,
            autounmask_masks: bp.autounmask_suggest_masks,
            is_top_level: false,
            extra_constraints: &[],
            depth: 0,
            deep: Deep::NotRequested,
            complete_mode: false,
            arg_atoms: &[],
            buildtime_hard: false,
        }
    }
}

pub fn resolve_pretend(
    source: &SelectSource,
    atom_str: &str,
    opts: &SelectOptions,
) -> Result<PretendOutcome, Error> {
    let SelectSource {
        repos,
        root,
        config,
        local_binpkg,
    } = *source;
    let SelectOptions {
        newuse,
        changed_use,
        update,
        excluded,
        changed_deps,
        with_bdeps,
        changed_slot,
        selective,
        is_top_level,
        usepkg,
        usepkgonly,
        binpkg_respect_use,
        usepkg_exclude,
        usepkg_include,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        newrepo,
        empty,
        getbinpkg,
        autounmask_keywords,
        autounmask_use,
        autounmask_license,
        autounmask_masks,
        extra_constraints,
        depth,
        deep,
        complete_mode,
        arg_atoms,
        ..
    } = *opts;
    // Portage: create_depgraph_params.py (`--emptytree` pops `selective`).
    let selective = selective && !empty;
    let atom = portage_dep::parse_atom(atom_str).ok_or_else(|| Error::InvalidAtom {
        atom: atom_str.to_string(),
    })?;

    // `--usepkgonly` excludes ebuilds from the pool; either flag makes `<PKGDIR>`
    // binaries eligible alongside. Binaries reuse `is_visible` unchanged (it only
    // consults source-independent fields). `--usepkg-exclude`/`--usepkg-include`
    // drop a binary from the pool before it is ever considered, like Portage's
    // break-out-of-the-loop rejection.
    // Portage: depgraph.py dbs construction; list_binary_candidates
    let mut candidates = if usepkgonly || crate::globals::ebuild_db_dropped() {
        Vec::new()
    } else {
        // Owned copy: the resolver's mutable pool (binaries are `extend`ed in and
        // `retain`ed below). One clone per resolved atom, cheap next to the walk.
        list_candidates(repos, &atom.category, &atom.package)?.to_vec()
    };
    crate::resolve::pool_memo::collect_memoized(CollectBinaryCandidatesArgs {
        usepkg,
        usepkgonly,
        local_binpkg,
        atom: &atom,
        getbinpkg,
        config,
        root,
        binpkg_respect_use,
        newuse,
        changed_use,
        candidates: &mut candidates,
        repos,
        with_bdeps,
        atom_str,
        extra_constraints,
        usepkg_exclude,
        usepkg_include,
    });

    // Portage's `avoid_update` early return for a dependency atom happens before it
    // ever looks for a "best available" candidate, so it is checked here too, before
    // the tree visibility filtering below can wrongly bail out with
    // `NoVisibleCandidate` for an atom whose installed version already satisfies it.
    // `--exclude` keeps the narrower pre-existing behavior (see the `!update`
    // shortcut below), so this site skips when it is active.
    //
    // `extra_constraints` gates both `avoid_update` shortcuts and the `matched` list
    // they bypass: they are every *other* parent atom targeting this `cat/pkg:slot`
    // (Portage weighs the whole atom set), so an installed version one of them
    // rejects falls through to the ordinary search, letting a solvable
    // installed-vs-merge slot collision reconcile on retry instead of re-deriving
    // the same conflict.
    // Portage: depgraph.py _select_pkg_highest_available
    // `--emptytree` skips installed packages as candidates entirely, so the
    // dependency `avoid_update` early return below can never fire (its pool holds no
    // installed instance).
    // Portage: depgraph.py
    if !empty && !is_top_level && excluded.is_empty() {
        let installed = installed_candidates(root, &atom.category, &atom.package);
        if let Some(installed_best) =
            dependency_avoid_update_candidate(DependencyAvoidUpdateCandidateArgs {
                root,
                atom: &atom,
                atom_str,
                candidates: &candidates,
                installed: &installed,
                config,
            })
            .filter(|c| satisfies_extra_constraints(c, &atom, extra_constraints))
        {
            // Portage `_want_update_pkg` (`depgraph.py:7304-7333`): no
            // complete mode, an argument (depth reset to 0) or `--update`,
            // and the effective depth not `_too_deep`. `--exclude`d packages
            // count as not-wanted there too, but the pre-existing
            // `excluded.is_empty()` gate above keeps those on the older
            // path (no S0 cell distinguishes them).
            let arg_match = installed_matches_argument(InstalledArgumentMatchArgs {
                arg_atoms,
                category: &atom.category,
                package: &atom.package,
                version: &installed_best.version,
                slot: &installed_best.slot,
                sub_slot: &installed_best.sub_slot,
                repo_name: &installed_best.repo_name,
            });
            let want_update = !complete_mode
                && (arg_match || update)
                && !update_too_deep(deep, if arg_match { 0 } else { depth });
            if !want_update && tree_knows_use_dep_flags(&candidates, &atom, config) {
                return avoid_update_outcome(AvoidUpdateOutcomeArgs {
                    source,
                    opts,
                    atom: &atom,
                    candidates: &candidates,
                    installed_best,
                    config,
                });
            }
            if !update {
                return already_installed_or_reinstall(
                    source,
                    opts,
                    &atom,
                    installed_best,
                    best_binary_build_time(&candidates, &installed_best.version),
                );
            }
        } else if let Some(inst_version) = best_installed_matching(root, atom_str, config) {
            // No tree candidate sits at an installed version (an installed
            // version with no ebuild, e.g. a removed tree version): Portage
            // still keeps the installed instance when it does not want it
            // updated (`:8445-8458`, the vardb half of `matched_packages`),
            // with no reinstall comparison at all (no same-cpv ebuild is
            // ever compared at `:8311-8329`). A wanted instance falls
            // through to the ordinary search below. Build-time-only edges
            // satisfy here too (the S0 `s0-e-gbapp-pvuD` capture keeps
            // installed-only `pg34-gone` for a `BDEPEND` edge).
            let (slot, sub_slot) = installed_candidates(root, &atom.category, &atom.package)
                .into_iter()
                .find(|(v, _, _)| v == &inst_version)
                .map(|(_, s, ss)| (s, ss))
                .unwrap_or_default();
            let repo = installed_pkg_repo(root, &atom.category, &atom.package, &inst_version);
            let probe = format!(
                "{}/{}-{inst_version}:{slot}/{sub_slot}::{repo}",
                atom.category, atom.package
            );
            if installed_probe_satisfies_extra_constraints(&probe, extra_constraints) {
                let arg_match = installed_matches_argument(InstalledArgumentMatchArgs {
                    arg_atoms,
                    category: &atom.category,
                    package: &atom.package,
                    version: &inst_version,
                    slot: &slot,
                    sub_slot: &sub_slot,
                    repo_name: &repo,
                });
                let want_update = !complete_mode
                    && (arg_match || update)
                    && !update_too_deep(deep, if arg_match { 0 } else { depth });
                if !want_update {
                    return Ok(PretendOutcome::AlreadyInstalled {
                        version: inst_version,
                    });
                }
            }
            // Otherwise falls through to the ordinary search below.
        }
    }

    let mut visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if let Some(outcome) = relax_until_usable(RelaxUntilUsableArgs {
        autounmask_license,
        autounmask_keywords,
        autounmask_masks,
        visible: &mut visible,
        atom: &atom,
        atom_str,
        extra_constraints,
        candidates: &candidates,
        config,
        empty,
        is_top_level,
        usepkg,
        usepkgonly,
        root,
    }) {
        return outcome;
    }

    // Match through portage-dep's `match_from_list`; candidate strings round-trip
    // back to their `Candidate` via `by_str` below. Each pair carries the
    // candidate's binary build-id, so an `=`+build-id atom matches only the
    // equal instance (dep:2994-3008, S9).
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let candidate_pairs: Vec<(&str, Option<&str>)> = candidate_str_refs
        .iter()
        .copied()
        .zip(visible.iter().map(|c| c.build_id.as_deref()))
        .collect();
    let matched = portage_dep::match_from_list_with_build_id(atom_str, &candidate_pairs)
        .ok_or_else(|| Error::InvalidAtom {
            atom: atom_str.to_string(),
        })?;

    let NarrowMatchedCandidatesOut { matched, by_str } =
        match narrow_matched_candidates(NarrowMatchedCandidatesArgs {
            extra_constraints,
            matched,
            candidate_str_refs: &candidate_str_refs,
            visible: &visible,
            atom: &atom,
            config,
            autounmask_use,
        }) {
            ControlFlow::Continue(out) => out,
            ControlFlow::Break(outcome) => return outcome,
        };

    // `installed_pairs` carries each installed version's own main slot, so "already
    // installed" is answered against the resolved candidate's *own* main slot (not
    // merely "this version exists in some slot"); otherwise `emerge -p cat/foo:1`
    // with only `foo:0` installed mis-classifies a new-slot install as an upgrade.
    // Portage: output.py _get_installed_best; vardb.match(pkg.slot_atom)
    let installed_pairs = installed_candidates(root, &atom.category, &atom.package);

    let best = match select_best_candidate(SelectBestCandidateArgs {
        excluded,
        matched: &matched,
        by_str: &by_str,
        installed_pairs: &installed_pairs,
        extra_constraints,
        atom: &atom,
        is_top_level,
        repos,
        atom_str,
        config,
        root,
        update,
        empty,
        selective,
        candidates: &candidates,
        source,
        opts,
    }) {
        ControlFlow::Continue(out) => out,
        ControlFlow::Break(outcome) => return outcome,
    };

    classify_best_against_installed(ClassifyBestAgainstInstalledArgs {
        best,
        installed_pairs: &installed_pairs,
        extra_constraints,
        atom: &atom,
        newuse,
        changed_use,
        root,
        config,
        changed_deps,
        repos,
        with_bdeps,
        changed_slot,
        usepkg,
        usepkgonly,
        candidates: &candidates,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        update,
        newrepo,
        empty,
        is_top_level,
        selective,
    })
}

/// What [`narrow_matched_candidates`] hands back to its caller.
pub(crate) struct NarrowMatchedCandidatesOut<'a, 'b> {
    pub(crate) matched: Vec<&'a str>,
    pub(crate) by_str: HashMap<&'a str, &'b Candidate>,
}

/// The values [`narrow_matched_candidates`] reads from its caller.
pub(crate) struct NarrowMatchedCandidatesArgs<'a, 'b> {
    pub(crate) extra_constraints: &'a [String],
    pub(crate) matched: Vec<&'a str>,
    pub(crate) candidate_str_refs: &'a [&'a str],
    pub(crate) visible: &'a [&'b Candidate],
    pub(crate) atom: &'a Atom,
    pub(crate) config: &'a Config,
    pub(crate) autounmask_use: bool,
}

/// Narrow the matched candidates by extra constraints, USE dependencies and --useoldpkg-atoms.
pub(crate) fn narrow_matched_candidates<'a, 'b>(
    args: NarrowMatchedCandidatesArgs<'a, 'b>,
) -> ControlFlow<Result<PretendOutcome, Error>, NarrowMatchedCandidatesOut<'a, 'b>> {
    let NarrowMatchedCandidatesArgs {
        extra_constraints,
        matched,
        candidate_str_refs,
        visible,
        atom,
        config,
        autounmask_use,
    } = args;
    // Slot-conflict reconciliation: on a re-resolve the caller passes every *other*
    // parent atom targeting the same `cat/pkg:slot`, and the winner must satisfy all
    // of them at once. Empty (the default) is a strict no-op. A `!`-prefixed entry
    // is a *negative* constraint (Portage's `runtime_pkg_mask`): the winner must NOT
    // match the atom after the `!`.
    // Portage: depgraph.py _process_slot_conflicts; _select_pkg
    let matched: Vec<&str> = if extra_constraints.is_empty() {
        matched
    } else {
        matched
            .into_iter()
            .filter(|m| {
                extra_constraints.iter().all(|c| {
                    // A per-instance negative hides no tree candidate (see
                    // [`is_instance_only_mask`]).
                    if is_instance_only_mask(c) {
                        return true;
                    }
                    if let Some(neg) = c.strip_prefix('!') {
                        !portage_dep::match_from_list(neg, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    } else {
                        portage_dep::match_from_list(c, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    }
                })
            })
            .collect()
    };

    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }

    // USE deps (PMS 8.3.4): a post-filter on version/slot/repo matching, where
    // Portage's own `match_from_list` applies its USE-dep post-pass (see
    // `use_deps_satisfied`). Each survivor's own current-tree IUSE/effective-USE
    // decides; an unreadable candidate is dropped ("can't tell, so exclude").
    let matched: Vec<&str> = match &atom.use_deps {
        Some(use_deps) if !use_deps.is_empty() => matched
            .into_iter()
            .filter(|m| {
                let Some(candidate) = by_str.get(m) else {
                    return false;
                };
                let Some((iuse, use_flags)) =
                    candidate_iuse_and_use(candidate, &atom.category, &atom.package, config)
                else {
                    return false;
                };
                if portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
                {
                    return true;
                }
                // Portage `--autounmask-use`: a mismatch a `package.use` flip would fix
                // keeps the candidate; the graph layer applies the flip and records it.
                autounmask_use
                    && suggested_use_flip(
                        candidate,
                        &atom.category,
                        &atom.package,
                        use_deps,
                        config,
                    )
                    .is_some()
            })
            .collect(),
        _ => matched,
    };

    // `--binpkg-respect-use` ran as a candidate-level filter above.
    if matched.is_empty() {
        return ControlFlow::Break(Ok(PretendOutcome::NoVisibleCandidate));
    }

    // `--useoldpkg-atoms`: for a matching package prefer a *built* (binary) candidate
    // over any newer *unbuilt* ebuild, so every downstream check sees only the old
    // binaries. Empty (the default) or a pool with no binaries is a strict no-op.
    // Portage: depgraph.py matched_oldpkg/visible_matches
    let matched: Vec<&str> = {
        let oldpkg: Vec<&str> = matched
            .iter()
            .copied()
            .filter(|m| {
                by_str.get(m).is_some_and(|c| {
                    c.source == CandidateSource::Binary
                        && useoldpkg_atom_matches(&atom.category, &atom.package, &c.version)
                })
            })
            .collect();
        if oldpkg.is_empty() { matched } else { oldpkg }
    };
    ControlFlow::Continue(NarrowMatchedCandidatesOut { matched, by_str })
}

/// The values [`relax_until_usable`] reads from its caller.
pub(crate) struct RelaxUntilUsableArgs<'a, 'b> {
    pub(crate) autounmask_license: bool,
    pub(crate) autounmask_keywords: bool,
    pub(crate) autounmask_masks: bool,
    pub(crate) visible: &'a mut Vec<&'b Candidate>,
    pub(crate) atom: &'a Atom,
    pub(crate) atom_str: &'a str,
    pub(crate) extra_constraints: &'a [String],
    pub(crate) candidates: &'b [Candidate],
    pub(crate) config: &'a Config,
    pub(crate) empty: bool,
    pub(crate) is_top_level: bool,
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) root: &'a Path,
}

/// Relax licence, keyword and mask checks level by level until a candidate is usable; answer early when none is.
pub(crate) fn relax_until_usable(
    args: RelaxUntilUsableArgs<'_, '_>,
) -> Option<Result<PretendOutcome, Error>> {
    let RelaxUntilUsableArgs {
        autounmask_license,
        autounmask_keywords,
        autounmask_masks,
        visible,
        atom,
        atom_str,
        extra_constraints,
        candidates,
        config,
        empty,
        is_top_level,
        usepkg,
        usepkgonly,
        root,
    } = args;
    // Portage's `_autounmask_levels` + per-level loop: try each allowed relaxation
    // set least-to-most invasive, re-scanning every version at each level, stopping
    // at the first level with a satisfying candidate. A later level relaxes `LICENSE`
    // only if level 1 was allowed to. The `missing keywords` steps are skipped (no
    // `**` suggestion). `(relax_license, relax_keywords, relax_masks)`:
    // Portage: depgraph.py
    let mut levels: Vec<(bool, bool, bool)> = Vec::new();
    let mut relax_license = false;
    if autounmask_license {
        relax_license = true;
        levels.push((true, false, false)); // 1. +license
    }
    if autounmask_keywords {
        levels.push((relax_license, true, false)); // 2. +~arch (+license)
    }
    if autounmask_masks {
        // 4. +masks, keywords still respected.
        levels.push((relax_license, false, true));
        if autounmask_keywords {
            // 5. +~arch +masks (+license).
            levels.push((relax_license, true, true));
        }
    }
    for (rl, rk, rm) in levels {
        if !need_fallback(visible, atom, atom_str, extra_constraints) {
            break;
        }
        *visible = candidates
            .iter()
            .filter(|c| {
                visible_with_relax(VisibleWithRelaxArgs {
                    candidate: c,
                    category: &atom.category,
                    package: &atom.package,
                    config,
                    relax_license: rl,
                    relax_keywords: rk,
                    relax_masks: rm,
                })
            })
            .collect();
    }
    if need_fallback(visible, atom, atom_str, extra_constraints) {
        // Portage's installed fallback after the merge-candidate search comes up
        // empty: in `selective` mode an installed version satisfying the atom is
        // accepted as-is (`operation="nomerge"`). This vdb-only check also covers a
        // dependency whose ebuild left the tree and `--usepkgonly` with no binary for
        // an installed dependency (Portage restricts the *merge* pool, not
        // installed-satisfaction). A top-level atom still reports the miss: Portage
        // requires something mergeable for a command-line package. Scoped to
        // `--usepkg`/`--usepkgonly` and non-top-level deps (not `--emptytree`, which
        // must rebuild everything); runtime-keyed deps get their hearing one layer up
        // instead (the queue loop sees the edge kind this function never receives).
        // Portage: depgraph.py _iter_match_pkgs installed
        if !empty
            && !is_top_level
            && (usepkg || usepkgonly || crate::globals::ebuild_db_dropped())
            && let Some(version) =
                best_installed_for_atom(root, atom_str, &atom.category, &atom.package)
        {
            return Some(Ok(PretendOutcome::AlreadyInstalled { version }));
        }
        return Some(Ok(PretendOutcome::NoVisibleCandidate));
    }
    None
}

/// The values [`collect_binary_candidates`] reads from its caller.
pub(crate) struct CollectBinaryCandidatesArgs<'a> {
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) local_binpkg: &'a Arc<BinaryIndex>,
    pub(crate) atom: &'a Atom,
    pub(crate) getbinpkg: bool,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) binpkg_respect_use: bool,
    pub(crate) newuse: bool,
    pub(crate) changed_use: bool,
    pub(crate) candidates: &'a mut Vec<Candidate>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) with_bdeps: bool,
    pub(crate) atom_str: &'a str,
    pub(crate) extra_constraints: &'a [String],
    pub(crate) usepkg_exclude: &'a [String],
    pub(crate) usepkg_include: &'a [String],
}

/// Add the usable binary packages to the candidate pool (--usepkg, --usepkgonly, --getbinpkg).
pub(crate) fn collect_binary_candidates(args: CollectBinaryCandidatesArgs<'_>) {
    let CollectBinaryCandidatesArgs {
        usepkg,
        usepkgonly,
        local_binpkg,
        atom,
        getbinpkg,
        config,
        root,
        binpkg_respect_use,
        newuse,
        changed_use,
        candidates,
        repos,
        with_bdeps,
        atom_str,
        extra_constraints,
        usepkg_exclude,
        usepkg_include,
    } = args;
    if usepkg || usepkgonly {
        let mut binary_candidates =
            list_binary_candidates(local_binpkg, &atom.category, &atom.package);
        // `--getbinpkg`/`-g`: remote binaries from each binrepo's on-disk `Packages`
        // index. A remote build of a cpv the local `$PKGDIR` also has counts as
        // already-downloaded, so `list_remote_binary_candidates` drops it.
        // Portage: bintree.isremote; _additional_pkgs
        if getbinpkg {
            binary_candidates.extend(list_remote_binary_candidates(
                &config.binrepos,
                root,
                local_binpkg,
                &atom.category,
                &atom.package,
            ));
        }
        // `--binpkg-respect-use`: reject a binary built with the wrong USE and keep
        // searching. A *candidate*-level filter, not a matched-string one: an ebuild
        // and a binary at the same `cpv:slot::repo` collapse to one key downstream
        // (`by_str`), so a mismatched binary left in the pool would take the
        // same-version ebuild with it. `--newuse`/`--reinstall` open the gate on
        // their own; a same-version ebuild is the comparison's `cur_` side.
        // Portage: depgraph.py; gate reinstall_use or (not installed and respect_use)
        if binpkg_respect_use || newuse || changed_use {
            let ebuild_at_version = |version: &str| {
                candidates
                    .iter()
                    .find(|e| e.source == CandidateSource::Ebuild && e.version == version)
            };
            binary_candidates.retain(|c| {
                binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
                    candidate: c,
                    ebuild_at_version: ebuild_at_version(&c.version),
                    category: &atom.category,
                    package: &atom.package,
                    config,
                    newuse,
                    changed_use,
                    respect_use: binpkg_respect_use,
                })
            });
        }
        // `--binpkg-changed-deps` (auto-enabled without `--usepkgonly`): reject a
        // binhost binary whose recorded `*DEPEND` differs from the current ebuild's,
        // so a stale build is rebuilt from source. Explicit `=y|n` forces the check
        // on or off (`binpkg_changed_deps_active`).
        // Portage: create_depgraph_params.py; depgraph.py ignored_binaries
        if binpkg_changed_deps_active(usepkgonly) {
            binary_candidates.retain(|c| {
                !binary_deps_changed(BinaryDepsChangedArgs {
                    root,
                    repos,
                    candidate: c,
                    category: &atom.category,
                    package: &atom.package,
                    with_bdeps,
                })
            });
        }
        // `_equiv_ebuild_visible`: once a visible ebuild has matched the atom, a binary
        // is only eligible if a visible ebuild exists at its *exact* version (a binhost
        // package whose ebuild left the tree or gained a mask is not merged stale).
        // Skipped for a binhost-only atom and under `--usepkgonly`;
        // `--use-ebuild-visibility` overrides both skips.
        // Portage: depgraph.py
        let uev = use_ebuild_visibility();
        if (!usepkgonly || uev) && !binary_candidates.is_empty() {
            let ebuild_visible_at = |ver: &str| {
                candidates.iter().any(|c| {
                    c.source == CandidateSource::Ebuild
                        && c.version == ver
                        && is_visible(c, &atom.category, &atom.package, config)
                })
            };
            let some_ebuild_matches_atom = candidates.iter().any(|c| {
                c.source == CandidateSource::Ebuild
                    && is_visible(c, &atom.category, &atom.package, config)
                    && {
                        let s = format!(
                            "{}/{}-{}:{}/{}::{}",
                            atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
                        );
                        portage_dep::match_from_list(atom_str, &[s.as_str()])
                            .is_some_and(|r| !r.is_empty())
                    }
            });
            if some_ebuild_matches_atom || uev {
                // Portage's `(usepkgonly or useoldpkg)` exemption: a binary matching
                // `--useoldpkg-atoms` skips the ebuild-visibility check unless
                // `--use-ebuild-visibility` was given.
                binary_candidates.retain(|c| {
                    ebuild_visible_at(&c.version)
                        || (!uev
                            && useoldpkg_atom_matches(&atom.category, &atom.package, &c.version))
                });
            }
        }
        // binpkg-multi-instance: keep only the highest-`BUILD_TIME` build of each
        // `cpv:slot::repo` (Portage yields instances newest-first and stops at the
        // first acceptable one).
        // Portage: depgraph.py, _iter_match_pkgs
        let mut binary_candidates = dedup_binary_instances(binary_candidates, atom, config);
        // `slot_operator_mask_built` steers the backtrack-config restart away from the
        // stale binary. A candidate-level filter like the rejections above: the
        // same-version ebuild must survive, so the marked negative must not reach the
        // matched-string pipeline below.
        // Portage: depgraph.py, _slot_change_backtrack; resolver/backtracking.py _feedback_config
        binary_candidates.retain(|c| {
            !extra_constraints.iter().any(|ec| {
                slot_operator_binary_masked_version(ec, &atom.category, &atom.package)
                    .is_some_and(|ver| ver == c.version)
            })
        });
        candidates.extend(filter_usepkg_exclude_include(
            binary_candidates,
            &atom.category,
            &atom.package,
            usepkg_exclude,
            usepkg_include,
        ));
    }
}

/// The values [`select_best_candidate`] reads from its caller.
pub(crate) struct SelectBestCandidateArgs<'a> {
    pub(crate) excluded: &'a [String],
    pub(crate) matched: &'a [&'a str],
    pub(crate) by_str: &'a HashMap<&'a str, &'a Candidate>,
    pub(crate) installed_pairs: &'a [(String, String, String)],
    pub(crate) extra_constraints: &'a [String],
    pub(crate) atom: &'a Atom,
    pub(crate) is_top_level: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) atom_str: &'a str,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) update: bool,
    pub(crate) empty: bool,
    pub(crate) selective: bool,
    pub(crate) candidates: &'a [Candidate],
    pub(crate) source: &'a SelectSource<'a>,
    pub(crate) opts: &'a SelectOptions<'a>,
}

/// Pick the best matched candidate, answering early when an excluded, avoid-update or installed one already settles the request.
pub(crate) fn select_best_candidate(
    args: SelectBestCandidateArgs<'_>,
) -> ControlFlow<Result<PretendOutcome, Error>, &Candidate> {
    let SelectBestCandidateArgs {
        excluded,
        matched,
        by_str,
        installed_pairs,
        extra_constraints,
        atom,
        is_top_level,
        repos,
        atom_str,
        config,
        root,
        update,
        empty,
        selective,
        candidates,
        source,
        opts,
    } = args;
    // --exclude/-X: an installed version matching an exclude atom is left as-is,
    // unconditionally, before --update/--newuse/--changed-use ever get a say.
    if !excluded.is_empty()
        && let Some(installed_best) = matched
            .iter()
            .filter_map(|m| by_str.get(m).copied())
            .filter(|c| candidate_is_installed(c, installed_pairs, extra_constraints, atom))
            .max_by(|a, b| {
                vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
            })
    {
        let installed_str = format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category,
            atom.package,
            installed_best.version,
            installed_best.slot,
            installed_best.sub_slot,
            installed_best.repo_name
        );
        if excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &installed_str))
        {
            return ControlFlow::Break(Ok(PretendOutcome::AlreadyInstalled {
                version: installed_best.version.clone(),
            }));
        }
    }

    // When only the *installed* instance satisfies the atom's `[use]` deps, the
    // first selection returns it and the autounmask flip path never runs; the tree
    // candidate's flip-away path below is what turns this into a `--newuse`
    // Reinstall otherwise. The ebuild matches against the profile-effective USE,
    // the installed instance against its own built USE.
    // Portage: _wrapped_select_pkg_highest_available_imp no-autounmask pass;
    // _iter_match_pkgs pkg.with_use(_pkg_use_enabled(pkg)); _want_installed_pkg
    if !is_top_level
        && atom.use_deps.as_ref().is_some_and(|d| !d.is_empty())
        && !atom_currently_satisfiable(repos, atom_str, config, extra_constraints)
        && let Some(version) = best_installed_matching(root, atom_str, config)
    {
        return ControlFlow::Break(Ok(PretendOutcome::AlreadyInstalled { version }));
    }

    // --update/-u: skipped entirely (not just outcome-adjusted) for a top-level atom
    // without `selective`, so selection falls through to the ordinary comparison
    // below. For a DEPENDENCY atom the early pre-visibility shortcut above already
    // handles the common case; it skips when `--exclude` is active, so this block
    // keeps its own `!is_top_level` branch (same broader lookup) for that one
    // combination. Under `--emptytree` the dependency branch is skipped too (no
    // installed candidates at all).
    // Portage: depgraph.py
    if !update && ((!is_top_level && !empty) || selective) {
        let installed_best = if !is_top_level {
            // Same `extra_constraints` gate as the early shortcut above.
            dependency_avoid_update_candidate(DependencyAvoidUpdateCandidateArgs {
                root,
                atom,
                atom_str,
                candidates,
                installed: installed_pairs,
                config,
            })
            .filter(|c| satisfies_extra_constraints(c, atom, extra_constraints))
        } else {
            matched
                .iter()
                .filter_map(|m| by_str.get(m).copied())
                .filter(|c| candidate_is_installed(c, installed_pairs, extra_constraints, atom))
                .max_by(|a, b| {
                    vercmp_ordering(&a.version, &b.version)
                        .then(a.repo_priority.cmp(&b.repo_priority))
                })
        };
        if let Some(installed_best) = installed_best {
            return ControlFlow::Break(already_installed_or_reinstall(
                source,
                opts,
                atom,
                installed_best,
                best_binary_build_time(candidates, &installed_best.version),
            ));
        }
    }

    // Ties on identical version (possible with several repos) break toward the
    // higher-priority repo.
    // Portage: portdbapi.cp_list (pkg.version, repo.priority) sort
    // --exclude/-X: an excluded candidate is never eligible as the New/Upgrade best
    // either. Installed-and-excluded versions were already returned above, so only a
    // not-yet-installed one can be filtered out here.
    let Some(best) = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .filter(|c| {
            let candidate_str = format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            );
            !excluded
                .iter()
                .any(|ex| matches_cli_option_atom(ex, &candidate_str))
        })
        .max_by(|a, b| {
            // At the winning version a surviving `--usepkg`/`--getbinpkg` binary beats
            // the same-version ebuild ("ebuild type is the last resort").
            // Portage: depgraph.py matched_packages[-1]
            let src_rank = |c: &Candidate| match c.source {
                CandidateSource::Binary => 1u8,
                CandidateSource::Ebuild => 0,
            };
            vercmp_ordering(&a.version, &b.version)
                .then(src_rank(a).cmp(&src_rank(b)))
                .then(a.repo_priority.cmp(&b.repo_priority))
        })
    else {
        return ControlFlow::Break(Ok(PretendOutcome::NoVisibleCandidate));
    };
    ControlFlow::Continue(best)
}

/// The values [`classify_best_against_installed`] reads from its caller.
pub(crate) struct ClassifyBestAgainstInstalledArgs<'a> {
    pub(crate) best: &'a Candidate,
    pub(crate) installed_pairs: &'a [(String, String, String)],
    pub(crate) extra_constraints: &'a [String],
    pub(crate) atom: &'a Atom,
    pub(crate) newuse: bool,
    pub(crate) changed_use: bool,
    pub(crate) root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) changed_deps: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) with_bdeps: bool,
    pub(crate) changed_slot: bool,
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) candidates: &'a [Candidate],
    pub(crate) rebuilt_binaries: bool,
    pub(crate) rebuilt_binaries_timestamp: Option<u64>,
    pub(crate) update: bool,
    pub(crate) newrepo: bool,
    pub(crate) empty: bool,
    pub(crate) is_top_level: bool,
    pub(crate) selective: bool,
}

/// Classify the chosen best candidate against what is installed: reinstall, already installed, upgrade, downgrade or new.
pub(crate) fn classify_best_against_installed(
    args: ClassifyBestAgainstInstalledArgs<'_>,
) -> Result<PretendOutcome, Error> {
    let ClassifyBestAgainstInstalledArgs {
        best,
        installed_pairs,
        extra_constraints,
        atom,
        newuse,
        changed_use,
        root,
        config,
        changed_deps,
        repos,
        with_bdeps,
        changed_slot,
        usepkg,
        usepkgonly,
        candidates,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        update,
        newrepo,
        empty,
        is_top_level,
        selective,
    } = args;
    if candidate_is_installed(best, installed_pairs, extra_constraints, atom) {
        let changed_flags = if newuse || changed_use {
            reinstall_flags_for_use_change(ReinstallFlagsForUseChangeArgs {
                root,
                category: &atom.category,
                package: &atom.package,
                candidate: best,
                config,
                newuse,
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        // A dependency atom `foo[bar]` whose installed `foo` was built WITHOUT `bar`
        // is not "already installed": Portage reinstalls it, checked against the
        // *vdb-recorded* USE (not the effective one). Forcing `Reinstall` here routes
        // the entry through the graph layer's child-flip block at all (the avoid-update
        // check above already returned `None`, so this `best` really is a
        // same-version installed one the atom's flag isn't satisfied by).
        let use_dep_needs_flip = atom
            .use_deps
            .as_deref()
            .filter(|d| !d.is_empty())
            .is_some_and(|use_deps| {
                let vdb_iuse =
                    read_vdb_flag_set(root, &atom.category, &atom.package, &best.version, "IUSE");
                let vdb_use =
                    read_vdb_flag_set(root, &atom.category, &atom.package, &best.version, "USE");
                let mut valid = valid_iuse(&vdb_iuse, config);
                valid.extend(vdb_use.iter().cloned());
                !portage_dep::use_deps_satisfied(use_deps, &valid, &vdb_use)
            });
        let deps_changed_flag = changed_deps
            && deps_changed(DepsChangedArgs {
                root,
                repos,
                category: &atom.category,
                package: &atom.package,
                version: &best.version,
                with_bdeps,
            });
        let slot_changed_flag =
            changed_slot && slot_changed(root, repos, &atom.category, &atom.package, &best.version);
        let rebuilt_binary_flag = (usepkg || usepkgonly)
            && binary_reinstall_warranted(BinaryReinstallWarrantedArgs {
                root,
                repos,
                config,
                category: &atom.category,
                package: &atom.package,
                version: &best.version,
                binary_build_time: best_binary_build_time(candidates, &best.version),
                rebuilt_binaries,
                rebuilt_binaries_timestamp,
                update,
            });
        let new_repo_flag = newrepo
            && new_repo_changed(
                root,
                &atom.category,
                &atom.package,
                &best.version,
                &best.repo_name,
            );
        // `is_top_level && !selective`: Portage's bare, reasonless `[ebuild R]`
        // (all other flags may still be empty/false here; that is the point).
        if empty
            || !changed_flags.is_empty()
            || deps_changed_flag
            || slot_changed_flag
            || rebuilt_binary_flag
            || new_repo_flag
            || use_dep_needs_flip
            || (is_top_level && !selective)
        {
            return Ok(PretendOutcome::Reinstall {
                version: best.version.clone(),
                changed_flags,
                deps_changed: deps_changed_flag,
                slot_changed: slot_changed_flag,
                rebuilt_binary: rebuilt_binary_flag,
                new_repo: new_repo_flag,
                slot_operator_rebuild: false,
            });
        }
        return Ok(PretendOutcome::AlreadyInstalled {
            version: best.version.clone(),
        });
    }

    // Upgrade/Downgrade/New is decided against only what's installed in `best`'s
    // *own* main slot. An installed version in another slot never makes this a
    // downgrade/upgrade; it is a `New` into a fresh slot (`[ebuild NS]`).
    // Portage: _get_installed_best myinslotlist = vardb.match(pkg.slot_atom)
    let installed_in_slot: Vec<&String> = installed_pairs
        .iter()
        // An `[installed]`-marked instance is hidden (see `candidate_is_installed`
        // above), so it neither anchors an upgrade/downgrade nor blocks a `New`.
        .filter(|(version, slot, _sub_slot)| {
            slot == &best.slot
                && !extra_constraints.iter().any(|ec| {
                    slot_operator_installed_masked_version(ec, &atom.category, &atom.package)
                        .is_some_and(|v| &v == version)
                })
        })
        .map(|(version, _slot, _sub_slot)| version)
        .collect();
    match installed_in_slot
        .iter()
        .copied()
        .max_by(|a, b| vercmp_ordering(a, b))
    {
        Some(current) => {
            if vercmp_ordering(&best.version, current) == Ordering::Less {
                Ok(PretendOutcome::Downgrade {
                    from: current.clone(),
                    to: best.version.clone(),
                })
            } else {
                Ok(PretendOutcome::Upgrade {
                    from: current.clone(),
                    to: best.version.clone(),
                })
            }
        }
        // The same version already installed in another slot (a slot move
        // that `--package-moves=n` left unapplied): under `--update` Portage
        // keeps the installed instance, as the cpv is equal.
        // Portage: depgraph.py, _wrapped_select_pkg_highest_available_imp
        None if update
            && !empty
            && !(newuse || changed_use || changed_deps || changed_slot || newrepo)
            && installed_pairs
                .iter()
                .any(|(v, slot, _)| *v == best.version && *slot != best.slot) =>
        {
            Ok(PretendOutcome::AlreadyInstalled {
                version: best.version.clone(),
            })
        }
        None => Ok(PretendOutcome::New {
            version: best.version.clone(),
        }),
    }
}

/// Whether the candidate satisfies every extra constraint of the request.
pub(crate) fn satisfies_extra_constraints(
    c: &Candidate,
    atom: &portage_dep::Atom,
    extra_constraints: &[String],
) -> bool {
    if extra_constraints.is_empty() {
        return true;
    }
    let s = format!(
        "{}/{}-{}:{}/{}::{}",
        atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
    );
    extra_constraints
        .iter()
        .all(|ec| match ec.strip_prefix('!') {
            // A per-instance negative hides no tree candidate (see
            // [`is_instance_only_mask`]).
            Some(_) if is_instance_only_mask(ec) => true,
            Some(neg) => {
                !portage_dep::match_from_list(neg, &[s.as_str()]).is_some_and(|r| !r.is_empty())
            }
            None => portage_dep::match_from_list(ec, &[s.as_str()]).is_some_and(|r| !r.is_empty()),
        })
}

/// Whether the candidate satisfies `atom_str` plus every extra constraint.
///
/// A fallback level is worth trying not just when *nothing* passes `is_visible`,
/// but when nothing visible satisfies the whole request: Portage weighs the whole
/// request before calling a version unavailable (e.g. `>=foo-2` where only `foo-2`
/// `~arch` is in range autounmasks its keyword). With no bound and no constraints
/// this is the old `is_empty()` gate.
/// Portage: depgraph.py, _select_pkg
fn usable(
    c: &Candidate,
    atom: &portage_dep::Atom,
    atom_str: &str,
    extra_constraints: &[String],
) -> bool {
    let s = format!(
        "{}/{}-{}:{}/{}::{}",
        atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
    );
    // The atom match sees the candidate's binary build-id, so an
    // `=`+build-id request is usable only by the equal instance
    // (dep:2994-3008, S9).
    if !portage_dep::match_from_list_with_build_id(atom_str, &[(s.as_str(), c.build_id.as_deref())])
        .is_some_and(|r| !r.is_empty())
    {
        return false;
    }
    extra_constraints.iter().all(|ec| {
        // A per-instance negative hides no tree candidate (see
        // [`is_instance_only_mask`]).
        if is_instance_only_mask(ec) {
            return true;
        }
        if let Some(neg) = ec.strip_prefix('!') {
            !portage_dep::match_from_list(neg, &[s.as_str()]).is_some_and(|r| !r.is_empty())
        } else {
            portage_dep::match_from_list(ec, &[s.as_str()]).is_some_and(|r| !r.is_empty())
        }
    })
}

/// True when no visible candidate satisfies the whole request.
fn need_fallback(
    vis: &[&Candidate],
    atom: &portage_dep::Atom,
    atom_str: &str,
    extra_constraints: &[String],
) -> bool {
    !vis.iter()
        .any(|c| usable(c, atom, atom_str, extra_constraints))
}

/// Whether the candidate's own main slot is installed at its version.
///
/// A matched candidate counts as already-installed only when its own main slot
/// is installed at that version (sub-slot ignored, like Portage's `slot_atom`).
fn candidate_is_installed(
    c: &Candidate,
    installed_pairs: &[(String, String, String)],
    extra_constraints: &[String],
    atom: &portage_dep::Atom,
) -> bool {
    installed_pairs.iter().any(|(version, slot, _sub_slot)| {
        version == &c.version
            && slot == &c.slot
            // An `[installed]`-marked negative hides the installed instance itself;
            // the same-version tree candidate stays eligible for merge.
            && !extra_constraints.iter().any(|ec| {
                slot_operator_installed_masked_version(ec, &atom.category, &atom.package)
                    .is_some_and(|v| &v == version)
            })
    })
}
