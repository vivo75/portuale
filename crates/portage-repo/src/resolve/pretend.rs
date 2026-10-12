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
    let mut candidates = if usepkgonly {
        Vec::new()
    } else {
        // Owned copy: the resolver's mutable pool (binaries are `extend`ed in and
        // `retain`ed below). One clone per resolved atom, cheap next to the walk.
        list_candidates(repos, &atom.category, &atom.package)?.to_vec()
    };
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
        let mut binary_candidates = dedup_binary_instances(binary_candidates, &atom, config);
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
    let satisfies_extra_constraints =
        |c: &Candidate| -> bool {
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
                    Some(neg) => !portage_dep::match_from_list(neg, &[s.as_str()])
                        .is_some_and(|r| !r.is_empty()),
                    None => portage_dep::match_from_list(ec, &[s.as_str()])
                        .is_some_and(|r| !r.is_empty()),
                })
        };
    // `--emptytree` skips installed packages as candidates entirely, so the
    // dependency `avoid_update` early return below can never fire (its pool holds no
    // installed instance).
    // Portage: depgraph.py
    if !update && !empty && !is_top_level && excluded.is_empty() {
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
            .filter(|c| satisfies_extra_constraints(c))
        {
            return already_installed_or_reinstall(
                source,
                opts,
                &atom,
                installed_best,
                best_binary_build_time(&candidates, &installed_best.version),
            );
        }
    }

    let mut visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    // Whether a candidate satisfies `atom_str` plus every `extra_constraints` entry.
    // A fallback level is worth trying not just when *nothing* passes `is_visible`,
    // but when nothing visible satisfies the whole request: Portage weighs the whole
    // request before calling a version unavailable (e.g. `>=foo-2` where only `foo-2`
    // `~arch` is in range autounmasks its keyword). With no bound and no constraints
    // this is the old `is_empty()` gate.
    // Portage: depgraph.py, _select_pkg
    let usable = |c: &Candidate| {
        let s = format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
        );
        // The atom match sees the candidate's binary build-id, so an
        // `=`+build-id request is usable only by the equal instance
        // (dep:2994-3008, S9).
        if !portage_dep::match_from_list_with_build_id(
            atom_str,
            &[(s.as_str(), c.build_id.as_deref())],
        )
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
    };
    let need_fallback = |vis: &[&Candidate]| !vis.iter().any(|c| usable(c));
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
        if !need_fallback(&visible) {
            break;
        }
        visible = candidates
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
    if need_fallback(&visible) {
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
            && (usepkg || usepkgonly)
            && let Some(version) =
                best_installed_for_atom(root, atom_str, &atom.category, &atom.package)
        {
            return Ok(PretendOutcome::AlreadyInstalled { version });
        }
        return Ok(PretendOutcome::NoVisibleCandidate);
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
        return Ok(PretendOutcome::NoVisibleCandidate);
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

    // `installed_pairs` carries each installed version's own main slot, so "already
    // installed" is answered against the resolved candidate's *own* main slot (not
    // merely "this version exists in some slot"); otherwise `emerge -p cat/foo:1`
    // with only `foo:0` installed mis-classifies a new-slot install as an upgrade.
    // Portage: output.py _get_installed_best; vardb.match(pkg.slot_atom)
    let installed_pairs = installed_candidates(root, &atom.category, &atom.package);
    // A matched candidate counts as already-installed only when its own main slot
    // is installed at that version (sub-slot ignored, like Portage's `slot_atom`).
    let candidate_is_installed = |c: &Candidate| -> bool {
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
    };

    // --exclude/-X: an installed version matching an exclude atom is left as-is,
    // unconditionally, before --update/--newuse/--changed-use ever get a say.
    if !excluded.is_empty()
        && let Some(installed_best) = matched
            .iter()
            .filter_map(|m| by_str.get(m).copied())
            .filter(|c| candidate_is_installed(c))
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
            return Ok(PretendOutcome::AlreadyInstalled {
                version: installed_best.version.clone(),
            });
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
        return Ok(PretendOutcome::AlreadyInstalled { version });
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
                atom: &atom,
                atom_str,
                candidates: &candidates,
                installed: &installed_pairs,
                config,
            })
            .filter(|c| satisfies_extra_constraints(c))
        } else {
            matched
                .iter()
                .filter_map(|m| by_str.get(m).copied())
                .filter(|c| candidate_is_installed(c))
                .max_by(|a, b| {
                    vercmp_ordering(&a.version, &b.version)
                        .then(a.repo_priority.cmp(&b.repo_priority))
                })
        };
        if let Some(installed_best) = installed_best {
            return already_installed_or_reinstall(
                source,
                opts,
                &atom,
                installed_best,
                best_binary_build_time(&candidates, &installed_best.version),
            );
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
        return Ok(PretendOutcome::NoVisibleCandidate);
    };

    if candidate_is_installed(best) {
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
                binary_build_time: best_binary_build_time(&candidates, &best.version),
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
        None => Ok(PretendOutcome::New {
            version: best.version.clone(),
        }),
    }
}
