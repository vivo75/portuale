//! Selecting one atom's candidate (`resolve_pretend`).

use crate::*;

/// Real `_select_pkg_highest_available_imp`'s own early `avoid_update`
/// return for a DEPENDENCY atom (`lib/_emerge/depgraph.py` ~8440: `if
/// inst_pkg is not None and parent is not None and not self.
/// _want_update_pkg(parent, inst_pkg): return inst_pkg`) -- the highest
/// installed version of `category/package` that matches `atom_str`
/// (version/slot/repo, via the FULL `candidates` list, deliberately
/// NOT `is_visible`-filtered) and, if the atom carries a USE-dep
/// (`pkg[flag]`), satisfies it against that version's own real,
/// installed vdb `USE`/`IUSE` -- NOT the current tree's, matching real
/// `_iter_match_pkgs`'s own vardb-sourced USE-dep check for an already-
/// installed package. The valid-flag domain for that check follows real
/// `dbapi._iuse_implicit_cnstr` for a built package (recorded `IUSE` ∪
/// profile `IUSE_EFFECTIVE` ∪ the package's own recorded `USE`, bug
/// #640318 -- see the inline comment). `None` when no installed version
/// qualifies.
///
/// `installed` is the vdb's own `(version, slot, sub_slot)` triples
/// (`installed_candidates`): a repo candidate only counts as the
/// avoid-update package when that *exact `(version, slot)` pair* is
/// installed -- real `vardb.match(atom)` returns installed cpvs that
/// match the atom's slot too, so `cat/pkg-1.0` installed only at slot 0
/// never satisfies a `cat/pkg:2` dependency even when the repo offers
/// `cat/pkg-1.0` in slot 2.
///
/// Called from two places in `resolve_pretend` below: once *before*
/// visibility/USE-dep filtering against the tree even begins (so a
/// dependency reached only via a keyword-masked-but-installed version
/// never spuriously hits `NoVisibleCandidate` in the first place), and
/// once more from the ordinary `!update` shortcut further down (the
/// only place this can still matter once `--exclude` is in play -- see
/// that call site's own comment).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn dependency_avoid_update_candidate<'a>(
    root: &Path,
    atom: &portage_dep::Atom,
    atom_str: &str,
    candidates: &'a [Candidate],
    installed: &[(String, String, String)],
    config: &portage_profile::Config,
) -> Option<&'a Candidate> {
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
                // Real `dbapi._iuse_implicit_cnstr` for a *built* package
                // on an EAPI 5+ (all EAPIs here have `iuse_effective`):
                // the `is_valid_flag` domain is the recorded `IUSE`,
                // unioned with the profile's `IUSE_EFFECTIVE`
                // (`_iuse_effective_match` -- `elibc_*`/`kernel_*`/…, via
                // `valid_iuse`) AND every flag the package was actually
                // built with (`_iuse_implicit_built`'s own `flag in use`
                // clause, bug #640318: a built package's own `USE` is
                // authoritative for what counts as a valid flag,
                // independent of the profile's current `IUSE_IMPLICIT` --
                // e.g. an ebuild that has since dropped a flag from its
                // `IUSE`). Portuale doesn't persist a vdb
                // `IUSE_EFFECTIVE` file, but real `_match_use` recomputes
                // the domain this same way rather than reading it.
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

/// Shared by both of `resolve_pretend`'s own `!update` shortcut call
/// sites (the early, dependency-only one above `dependency_avoid_
/// update_candidate` feeds, and the later, `matched`-based one further
/// down `resolve_pretend` itself): once an installed version has been
/// chosen to keep, decides between `AlreadyInstalled` and `Reinstall`
/// exactly the same way -- `newuse`/`changed_use`/`changed_deps`/
/// `changed_slot`/`rebuilt_binaries`/`newrepo` each independently able
/// to trigger a reinstall even though no real version change is
/// happening at all.
pub(crate) fn already_installed_or_reinstall(
    source: &SelectSource,
    opts: &SelectOptions,
    atom: &portage_dep::Atom,
    installed_best: &Candidate,
    // `BUILD_TIME` of the binary candidate (local or remote) at
    // `installed_best.version`, for the `--rebuilt-binaries` check --
    // `None` when the pool has no binary for this version.
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
        // `--emptytree`/`-e` (real `create_depgraph_params.py:176` --
        // `myparams["empty"] = True`, `depgraph.py:7889` -- installed
        // packages are not selected as candidates, so every atom in the
        // deep tree resolves to a merge). Portuale's candidate pool is
        // already tree-only, so `empty` just forces the "would be
        // `AlreadyInstalled`" fallback to a bare `Reinstall` instead
        // (real `output.py`: `attr_display.replace` still set from
        // `vardb.cpv_exists`, no `[oldver]`, no reason -- exactly the
        // portuale's own reasonless `[ebuild R]`).
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

// Backlog #212 (v2 `#24c`): read one `slot_operator_mask_built`
// negative in its binary-marked form (`!=cat/pkg-ver[binary]`, see
// [`MaskReason::SlotOperatorBuilt`]). Returns the masked version when
// `neg` marks a binary of this `cat/pkg`, else `None`. Real keys the
// mask by package object so only the binary instance hides; portuale's
// string readers match binary and ebuild alike, so the marker keeps the
// two apart: every string-constraint reader below skips marked entries
// (see [`is_binary_only_mask`] -- `match_from_list` ignores USE-deps,
// so the marker would otherwise match the same-version ebuild too) and
// only the binary-candidate pool filter calls this.
pub(crate) fn slot_operator_binary_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[binary]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}

// Backlog #212 (v2 `#24c`): whether a string constraint is a
// binary-only `slot_operator_mask_built` negative
// (`!=cat/pkg-ver[binary]`). String-constraint readers (every
// `extra_constraints` filter and the missing-dep probe's masked
// negatives) match ebuild and binary candidates at one
// `cpv:slot::repo` key and must skip these -- the same-version ebuild
// stays visible (real keys by package object). Only the
// binary-candidate pool filter honours them (via
// [`slot_operator_binary_masked_version`]).
pub(crate) fn is_binary_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[binary]"))
}

// Backlog #276: the installed-only half of the same per-instance
// distinction. Real `_add_dep`'s missing-dependency mask
// (`depgraph.py:3456-3470`) keys by package object, so masking an
// installed parent hides only that instance -- the same-version ebuild
// stays visible for real's next backtracking try. Portuale's string
// readers match installed and ebuild at one key, so the rendered
// negative carries an `[installed]` marker
// (`!=cat/pkg-ver[installed]`, filed by the missing-dep trigger for an
// installed parent with no merge-bound version): every tree-candidate
// (`ebuild` or binary) string-constraint reader skips marked entries
// exactly like [`is_binary_only_mask`], and only the installed-instance
// selection honours them (via
// [`slot_operator_installed_masked_version`]). A marked negative never
// parses as a version (`split_cpv` rejects the brackets), so the
// missed-update collection skips it the same way.
pub(crate) fn is_installed_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[installed]"))
}

// Backlog #276: read one installed-only missing-dependency negative
// (`!=cat/pkg-ver[installed]`, see [`is_installed_only_mask`]).
// Returns the masked version when `neg` marks an installed instance of
// this `cat/pkg`, else `None`. Mirror of
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

// Every per-instance string negative (`[binary]` and `[installed]`
// marks): tree-candidate readers skip them all -- each hides only its
// own instance kind, never a tree candidate (real keys every such mask
// by package object). Instance pools keep honouring their own mark
// (see the two functions above).
pub(crate) fn is_instance_only_mask(constraint: &str) -> bool {
    is_binary_only_mask(constraint) || is_installed_only_mask(constraint)
}

// 11 args trips clippy::too_many_arguments; a bundled options struct
// would touch every one of this function's own call sites (production
// and test) for a single-slice-sized addition of one more CLI flag
// alongside six already threaded the same way -- not worth it, same
// reasoning as resolve_pretend_graph's own identical allow below.
/// Where [`resolve_pretend`] selects from: the repos, the target root,
/// its config and the local binary-package index.
#[derive(Clone, Copy)]
pub struct SelectSource<'a> {
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub config: &'a portage_profile::Config,
    /// The run's already-built local `$PKGDIR` binary index (backlog #123).
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
    // Real `create_depgraph_params.py:179`: `--emptytree` does
    // `myparams.pop("selective", None)`.
    let selective = selective && !empty;
    let atom = portage_dep::parse_atom(atom_str).ok_or_else(|| Error::InvalidAtom {
        atom: atom_str.to_string(),
    })?;

    // --usepkg/--usepkgonly (real depgraph.py's own `dbs` candidate-pool
    // construction, `if "--usepkgonly" not in myopts: dbs.append(("ebuild"
    // ...)); if myopts.get("--usepkg") is True: dbs.append(("binary"
    // ...))`, confirmed by reading it -- see `list_binary_candidates`'s
    // own doc comment): `--usepkgonly` excludes ebuild candidates from
    // the pool entirely; either flag alone makes binary candidates
    // (`<PKGDIR>/Packages`) eligible alongside them. Binary candidates
    // reuse `is_visible` completely unchanged -- it only ever consults
    // fields every `Candidate` carries regardless of `source`
    // (package.mask/license/keywords/properties/restrict), never
    // anything ebuild-specific. `--usepkg-exclude`/`--usepkg-include`
    // (see `filter_usepkg_exclude_include`'s own doc comment) drop a
    // binary candidate from the pool entirely before it's ever
    // considered alongside ebuilds, matching real depgraph.py's own
    // `break`-out-of-the-loop rejection.
    let mut candidates = if usepkgonly {
        Vec::new()
    } else {
        // Owned copy: this is the resolver's mutable candidate pool
        // (binaries are `extend`ed in and `retain`ed below). One clone
        // per resolved atom -- cheap next to the graph walk.
        list_candidates(repos, &atom.category, &atom.package)?.to_vec()
    };
    if usepkg || usepkgonly {
        let mut binary_candidates =
            list_binary_candidates(local_binpkg, &atom.category, &atom.package);
        // `--getbinpkg`/`-g`: remote binary candidates from each
        // `config.binrepos` binrepo's own on-disk `Packages` index. A
        // remote build of a cpv the local `$PKGDIR` also has is treated
        // as already-downloaded (real `bintree.isremote` -> not remote
        // once in `_additional_pkgs`), so `list_remote_binary_candidates`
        // drops it -- see its own doc comment.
        if getbinpkg {
            binary_candidates.extend(list_remote_binary_candidates(
                &config.binrepos,
                root,
                local_binpkg,
                &atom.category,
                &atom.package,
            ));
        }
        // `--binpkg-respect-use` (real: reject a binary built with the
        // wrong USE and keep searching -- `depgraph.py:8259-8288`). A
        // *candidate*-level filter, not a matched-string one: an ebuild
        // and a binary at the same `cpv:slot::repo` collapse to one key
        // downstream (`by_str`), so a USE-mismatched binary left in the
        // pool would take that key and, when it's dropped, take the
        // same-version ebuild with it.
        // #69 S1: real's gate is `reinstall_use or (not installed and
        // respect_use)` -- `--newuse`/`--reinstall` open it on their
        // own, which is what makes a `newflag`-only IUSE difference
        // reject the binary under `--newuse` even with
        // `--binpkg-respect-use=n`. A same-version ebuild (when the
        // pool has one) is the comparison's `cur_` side.
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
        // `--binpkg-changed-deps` (real `create_depgraph_params.py:196-203`:
        // auto-enabled whenever `--usepkgonly` is not given): reject a
        // binhost binary whose recorded `*DEPEND` differs from the current
        // ebuild's (real `depgraph.py:8288` -> `ignored_binaries[pkg]
        // ["changed_deps"] = True; continue`), so a package built against
        // a since-changed ebuild is rebuilt from source rather than
        // merged stale. An explicit `--binpkg-changed-deps=y|n` forces
        // the check on (even under `--usepkgonly`) or off (even for an
        // ordinary `--getbinpkg`) -- `binpkg_changed_deps_active`.
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
        // `_equiv_ebuild_visible` (real `depgraph.py:8015-8025`): once a
        // visible ebuild has matched the atom (`dbs` order is ebuild-first,
        // so real's `matched_packages` is non-empty by the time binaries
        // are weighed), a binary is only eligible if a visible ebuild
        // exists at its *exact* version -- a binhost package whose ebuild
        // was dropped from the tree, or masked, since it was built is not
        // merged stale (the ebuild at another version is rebuilt instead).
        // Skipped for a binhost-only atom (no ebuild matches at all ->
        // real's `(use_ebuild_visibility or matched_packages)` gate is
        // falsy) and under `--usepkgonly` (real: ebuild status is ignored
        // there) -- `--use-ebuild-visibility` overrides both of those
        // skips (real `depgraph.py:8027`'s `not use_ebuild_visibility and
        // (usepkgonly or useoldpkg)` guard), enforcing the check even
        // under `--usepkgonly` / for a `--useoldpkg-atoms` match.
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
                // Real's `(usepkgonly or useoldpkg)` exemption: a binary
                // matching `--useoldpkg-atoms` is not subjected to the
                // ebuild-visibility check -- unless `--use-ebuild-
                // visibility` was given.
                binary_candidates.retain(|c| {
                    ebuild_visible_at(&c.version)
                        || (!uev
                            && useoldpkg_atom_matches(&atom.category, &atom.package, &c.version))
                });
            }
        }
        // binpkg-multi-instance: keep only the highest-`BUILD_TIME`
        // surviving build of each `cpv:slot::repo` (real
        // `_iter_match_pkgs` yields instances newest-first and the
        // selection loop `break`s on the first acceptable one).
        let mut binary_candidates = dedup_binary_instances(binary_candidates, &atom, config);
        // #212 (v2 `#24c`): real `_slot_change_backtrack`'s
        // `slot_operator_mask_built` (`depgraph.py:2383`) steers the
        // backtrack-config restart away from the stale binary
        // (`resolver/backtracking.py::_feedback_config:237-242` files
        // it under `runtime_pkg_mask`, and `_iter_similar_available`
        // (`:3034`) skips masked packages on the re-pass). A
        // candidate-level filter, like every other binary rejection
        // above: the same-version ebuild must survive (real masks the
        // binary package object only), so the marked negative must not
        // reach the matched-string pipeline below.
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

    // Real avoid_update's own EARLY return for a dependency atom (see
    // `dependency_avoid_update_candidate`'s own doc comment for the
    // full citation) genuinely happens before real portage ever tries
    // to find a "best available" candidate at all -- so it's checked
    // here too, before portuale's own visibility/USE-dep-against-the-
    // tree filtering below gets a chance to (wrongly) bail out with
    // `NoVisibleCandidate` for an atom whose installed version already
    // satisfies it. Confirmed live: `sys-fs/fuse`'s own real
    // `sys-libs/liburing:=[abi_x86_64(-)?,...]` dependency needs
    // exactly this -- the tree's only *visible* `liburing` candidate
    // doesn't even have the right USE profile to satisfy the atom
    // (nothing enables it there), while the real, installed version
    // does (its own real vdb `USE`). `--exclude` deliberately keeps
    // portuale's own pre-existing, narrower behavior instead (see the
    // later, `!is_top_level`-aware `!update` shortcut's own comment) --
    // skipped here so that block still gets a chance to run.
    //
    // #57: `extra_constraints` gates the two `avoid_update` shortcuts
    // (this one and the `!update` block further down) as well as the
    // `matched` list they bypass. The constraints are every *other*
    // parent atom that targeted this `cat/pkg:slot` (real
    // `_select_pkg_highest_available` weighs the whole atom set, not
    // just the one being resolved), so an installed version one of them
    // rejects is not a version real would "avoid updating" to -- it
    // falls through to the ordinary highest-available search, which is
    // what makes a *solvable* installed-vs-merge slot collision
    // reconcile on the backtracking retry (`dev-libs/paired` staying on
    // installed 1.0 while a sibling's `>=paired-2.0` forces 2.0) instead
    // of re-deriving the same conflict on every attempt.
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
                    // #212/#276: a per-instance negative (`[binary]`
                    // `slot_operator_mask_built`, `[installed]`
                    // missing-dependency) hides no tree candidate (see
                    // [`is_instance_only_mask`]).
                    Some(_) if is_instance_only_mask(ec) => true,
                    Some(neg) => !portage_dep::match_from_list(neg, &[s.as_str()])
                        .is_some_and(|r| !r.is_empty()),
                    None => portage_dep::match_from_list(ec, &[s.as_str()])
                        .is_some_and(|r| !r.is_empty()),
                })
        };
    // #157: `--emptytree` skips installed packages as candidates entirely
    // (real `depgraph.py:7888-7899`, `if empty and pkg.installed ...:
    // continue`), so the dependency `avoid_update` early return below
    // (`depgraph.py:8453-8458`, `return inst_pkg`) can never fire -- its
    // `matched_packages` holds no installed instance. Gating on `!empty`
    // keeps portuale from reinstalling a keyword-masked installed
    // dependency that real downgrades to the best visible stable.
    if !update && !empty && !is_top_level && excluded.is_empty() {
        let installed = installed_candidates(root, &atom.category, &atom.package);
        if let Some(installed_best) = dependency_avoid_update_candidate(
            root,
            &atom,
            atom_str,
            &candidates,
            &installed,
            config,
        )
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
    // Whether a candidate satisfies `atom_str` itself *and* every
    // `extra_constraints` entry (positive: must match; `!`-prefixed: must
    // not) -- the same per-atom tests the `matched` pipeline below runs. A
    // `*_masked_only` fallback level is worth trying not just when
    // *nothing* passes `is_visible`, but when nothing visible actually
    // satisfies what this atom (and any slot-conflict constraints folded in
    // with it) asks for: real `_select_pkg` weighs the whole request before
    // calling a version unavailable, so `>=dev-libs/foo-2` where `foo-1` is
    // stable but only `foo-2` (`~arch`) is in range autounmasks `foo-2`'s
    // keyword rather than reporting the dep missing. With no version bound
    // and no `extra_constraints` this is exactly the old `is_empty()` gate.
    let usable = |c: &Candidate| {
        let s = format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
        );
        if !portage_dep::match_from_list(atom_str, &[s.as_str()]).is_some_and(|r| !r.is_empty()) {
            return false;
        }
        extra_constraints.iter().all(|ec| {
            // #212/#276: a per-instance negative (`[binary]`
            // `slot_operator_mask_built`, `[installed]`
            // missing-dependency) hides no tree candidate (see
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
    // Real `_autounmask_levels` (`depgraph.py:7446`) + `_select_pkg_
    // highest_available_imp`'s per-level loop: try each allowed relaxation
    // set from least to most invasive, re-scanning EVERY version at each
    // level (real re-runs its highest-first match), and stop at the first
    // level that yields a candidate satisfying the atom + slot
    // constraints. `_AutounmaskLevel` is a single mutable object that's
    // re-yielded, so a later level relaxes `LICENSE` only if level 1 was
    // allowed to (`allow_license_changes` sticks once set). portuale skips
    // real's `missing keywords` steps (levels 3 & 6) -- it has no `**`
    // suggestion. `(relax_license, relax_keywords, relax_masks)`:
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
        // 4. +masks, keywords still respected (real bug #463394).
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
        // Real `_select_pkg_highest_available_imp`'s installed fallback
        // (`depgraph.py` `_iter_match_pkgs(root_config, "installed", atom)`
        // after the merge-candidate search comes up empty): in `selective`
        // mode (the default -- `--emptytree` clears it) an already-
        // installed version that satisfies the atom is accepted as-is
        // (`operation="nomerge"`). `dependency_avoid_update_candidate`
        // above only reaches atoms that still have *some* repo candidate
        // (e.g. keyword-masked-but-present); this vdb-only check also
        // covers a dependency whose ebuild has left the tree and -- the
        // common case -- `--usepkgonly` with no binary package for an
        // installed dependency (real `--usepkgonly` restricts what may be
        // newly *merged*, not what counts as already *satisfied*). A
        // top-level atom still reports the missing candidate: real
        // requires something mergeable for a package named on the command
        // line.
        //
        // Scoped to `--usepkg`/`--usepkgonly` and a dependency (not
        // `--emptytree`, which must rebuild everything): `--usepkgonly`
        // empties the ebuild candidate pool wholesale (above), so a
        // dependency whose only satisfier is an installed version with no
        // binary package would otherwise wrongly go `NoVisibleCandidate`
        // and, in a real `-K` merge, land on the resume list ("no visible
        // ebuild to merge"). Real `--usepkgonly` restricts the *merge*
        // pool, not installed-satisfaction. The non-`--usepkg` path keeps
        // its existing behaviour here (a dependency whose ebuild left the
        // tree entirely is still reported) -- narrowing this fix to the
        // case the L1 test bed actually turned up (L1-a). Runtime-keyed
        // deps get their installed hearing one layer up instead (the
        // queue loop's #233 rewrite, which sees the edge kind this
        // function never receives).
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

    // Reuses portage-dep's already-verified match_from_list rather than
    // re-deriving version/slot matching rules here; candidate strings are
    // round-tripped back to their Candidate via `by_str` below.
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
    let matched = portage_dep::match_from_list(atom_str, &candidate_str_refs).ok_or_else(|| {
        Error::InvalidAtom {
            atom: atom_str.to_string(),
        }
    })?;

    // Slot-conflict reconciliation (real `_process_slot_conflicts` /
    // `_select_pkg`'s "all the atoms pulling this slot", not just the
    // first): when `resolve_pretend_graph` re-resolves a package that hit
    // a slot conflict, it passes every *other* parent atom that targeted
    // the same `cat/pkg:slot` here -- the winning candidate must satisfy
    // all of them at once. An empty list (the default at every ordinary
    // call site) is a strict no-op.
    //
    // A constraint that begins with `!` is a *negative* constraint (real
    // backtracking's `runtime_pkg_mask`): the winning candidate must NOT
    // match the atom after the `!` -- `resolve_pretend_graph`'s unsolvable
    // slot-conflict handler adds `!=cat/pkg-<ver>` entries here to hide a
    // specific version (of the conflicted package or of a parent that
    // pulled the losing atom) and force a lower one on the retry.
    let matched: Vec<&str> = if extra_constraints.is_empty() {
        matched
    } else {
        matched
            .into_iter()
            .filter(|m| {
                extra_constraints.iter().all(|c| {
                    // #212/#276: a per-instance negative (`[binary]`
                    // `slot_operator_mask_built`, `[installed]`
                    // missing-dependency) hides no tree candidate (see
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

    // USE deps (`dev-libs/foo[bar]`/`[-bar]`, `(+)`/`(-)` defaults --
    // PMS 8.3.4): a post-filter on top of match_from_list's own version/
    // slot/repo matching, exactly where real portage's own
    // `match_from_list` applies its equivalent USE-dep post-pass too --
    // see `use_deps_satisfied`'s own doc comment (portage-dep) for the
    // ported algorithm and why match_from_list itself doesn't do this.
    // Each surviving candidate's own current-tree IUSE/effective-USE
    // (`candidate_iuse_and_use`) decides it -- a candidate whose own
    // metadata can't even be read is dropped, same "can't tell, so
    // exclude" precedent `reinstall_flags_for_use_change` already sets.
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
                // Real `--autounmask-use`: a plain USE-dep mismatch a
                // `package.use` flip would fix (`suggested_use_flip` is
                // `Some`) keeps the candidate -- the graph layer applies
                // the flip and records the change. `use_masked_only`
                // already re-checks `is_visible` (redundant here, this
                // candidate is in `visible`) and the same
                // "only-a-flip-away" test.
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

    // (`--binpkg-respect-use` was applied as a candidate-level filter
    // above, before `by_str` -- see there for why it can't be a
    // matched-string filter.)
    if matched.is_empty() {
        return Ok(PretendOutcome::NoVisibleCandidate);
    }

    // `--useoldpkg-atoms ATOMS` (real `depgraph.py:7936` +
    // `matched_oldpkg`/`visible_matches`): for a package matching one of
    // these atoms, prefer a *built* (binary) candidate over any newer
    // *unbuilt* ebuild. Real portage collects the built matches into
    // `matched_oldpkg` and, when non-empty, restricts `visible_matches`
    // to them -- so every downstream check here (the `--exclude` /
    // `--update` already-installed lookups and the final best-version
    // pick) sees only the old binaries. Empty `--useoldpkg-atoms` (the
    // default) is a strict no-op; so is a run with no binary candidates
    // in the pool at all (no `--usepkg`/`--getbinpkg`). See
    // `set_useoldpkg_atoms`'s own doc comment.
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

    // `installed_pairs` carries each installed version's own main slot,
    // so "is this candidate already installed" can be answered the way
    // real `output.py::_get_installed_best` does -- against
    // `vardb.match(pkg.slot_atom)` (the resolved candidate's *own* main
    // slot), not merely "this version exists in some slot". Without the
    // slot filter, `emerge -p cat/foo:1` with only `foo:0` installed
    // mis-classifies a new-slot install as an `Upgrade`/`Downgrade`
    // (real portage: `[ebuild NS]`). `installed` (version-only, all
    // slots) is still what `dependency_avoid_update_candidate` consumes
    // -- its own `avoid_update` grounding is a separate concern.
    let installed_pairs = installed_candidates(root, &atom.category, &atom.package);
    // A matched candidate counts as already-installed only when its own
    // main slot is installed at that version (sub-slot ignored, exactly
    // like real `pkg.slot_atom`).
    let candidate_is_installed = |c: &Candidate| -> bool {
        installed_pairs.iter().any(|(version, slot, _sub_slot)| {
            version == &c.version
                && slot == &c.slot
                // Backlog #276: an `[installed]`-marked
                // missing-dependency negative hides the installed
                // instance itself (real keys the mask by package
                // object) -- the same-version tree candidate stays
                // eligible for merge.
                && !extra_constraints.iter().any(|ec| {
                    slot_operator_installed_masked_version(ec, &atom.category, &atom.package)
                        .is_some_and(|v| &v == version)
                })
        })
    };

    // --exclude/-X: an installed version matching an exclude atom is
    // left exactly as-is, unconditionally, before --update/--newuse/
    // --changed-use ever get a say -- see this function's own doc
    // comment.
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
            .any(|ex| matches_config_entry(ex, &installed_str, &atom.category, &atom.package))
        {
            return Ok(PretendOutcome::AlreadyInstalled {
                version: installed_best.version.clone(),
            });
        }
    }

    // #74 S1, selection half (real `_wrapped_select_pkg_highest_available_imp`'s
    // no-autounmask pass): the ebuild candidate is matched against the
    // profile-effective USE and the installed instance against its own
    // built USE (`_iter_match_pkgs`'s `pkg.with_use(self._pkg_use_enabled(pkg))`,
    // `dbapi._match_use`'s built-package branch). When only the *installed*
    // instance satisfies the atom's `[use]` deps, real's first selection
    // returns it (`_want_installed_pkg` keeps an installed package the user
    // did not name on the command line as a nomerge selection) and the
    // autounmask flip path never runs; the tree candidate's flip-away path
    // below is what turns this into a `--newuse` Reinstall otherwise.
    // Host probe: `dev-lang/rust:1.94.1[llvm_slot_21]` (installed with the
    // flag; the current profile's `LLVM_SLOT` expand turns it off for the
    // ebuild) must stay `AlreadyInstalled` under `-puDvN`, exactly as real
    // keeps it.
    if !is_top_level
        && atom.use_deps.as_ref().is_some_and(|d| !d.is_empty())
        && !atom_currently_satisfiable(repos, atom_str, config, extra_constraints)
        && let Some(version) = best_installed_matching(root, atom_str, config)
    {
        return Ok(PretendOutcome::AlreadyInstalled { version });
    }

    // --update/-u: see this function's own doc comment. Skipped
    // entirely (not just its outcome adjusted) for a top-level atom
    // without `selective` -- see the doc comment's own `selective`/
    // `is_top_level` paragraph -- so version selection falls through to
    // the ordinary best-visible-candidate comparison below too.
    //
    // For a DEPENDENCY atom, the early, pre-visibility-filtering
    // shortcut above (`dependency_avoid_update_candidate`) already
    // handles the common case (see its own doc comment for the full
    // real-portage citation). It deliberately skips when `--exclude` is
    // active, though, to preserve portuale's own pre-existing
    // `--exclude`-vs-`matched` interaction exactly -- so this block
    // still needs its own `!is_top_level` branch, reusing the same
    // broader (not `is_visible`-filtered) lookup, for that one
    // remaining combination.
    // #157: under `--emptytree` the dependency branch is skipped for the
    // same reason as the early shortcut above -- real's installed
    // candidates are dropped wholesale (`depgraph.py:7888-7899`), so the
    // `inst_pkg` return it mirrors cannot apply. The top-level `selective`
    // arm is already false under `empty` (`selective = selective && !empty`).
    if !update && ((!is_top_level && !empty) || selective) {
        let installed_best = if !is_top_level {
            // #57: same `extra_constraints` gate as the early shortcut
            // above (`matched`, which the `is_top_level` arm below reads,
            // is already filtered by them).
            dependency_avoid_update_candidate(
                root,
                &atom,
                atom_str,
                &candidates,
                &installed_pairs,
                config,
            )
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

    // Ties on identical version (possible once more than one repo can
    // provide it) are broken toward the higher-priority repo, matching
    // real portage's own `(pkg.version, repo.priority)` sort in
    // `portdbapi.cp_list`.
    // --exclude/-X: an excluded candidate is never eligible to become
    // the New/Upgrade "best visible candidate" either -- see this
    // function's own doc comment. Any already-installed match was
    // already handled (and returned) above, so nothing here can
    // silently drop an installed-and-excluded version -- only a
    // not-yet-installed one can end up filtered out entirely.
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
                .any(|ex| matches_config_entry(ex, &candidate_str, &atom.category, &atom.package))
        })
        .max_by(|a, b| {
            // Real `_wrapped_select_pkg_highest_available_imp` builds
            // `matched_packages` in `dbs` order -- `ebuild` before
            // `binary` -- and returns `matched_packages[-1]`, "ebuild
            // type is the last resort" (`depgraph.py:8492`). So at the
            // winning version a `--usepkg`/`--getbinpkg` binary that
            // survived the `binpkg-respect-use` filter above beats the
            // same-version ebuild. (`--usepkgonly` has no ebuilds in the
            // pool; without `--usepkg` there are no binaries -- moot
            // either way.)
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
        // Real: a dependency atom `foo[bar]` whose already-installed
        // `foo` was built WITHOUT `bar` is not "already installed" --
        // real reinstalls `foo` to satisfy the atom, whatever `foo`'s
        // current effective USE happens to be. The check is against the
        // *vdb-recorded* USE, not the profile-resolved one: `policykit`
        // (a global flag `systemd`'s ebuild now defaults on) shows
        // `[ebuild R] … policykit*` at rc 0 -- the graph layer's own
        // child-flip block records an autounmask change only when the
        // *effective* USE still can't satisfy the atom (`mime-types
        // [nginx]` -> rc 1). Forcing a `Reinstall` here (rather than
        // `AlreadyInstalled`) is what routes the entry through that
        // block at all. `dependency_avoid_update_candidate` above already
        // made the same vdb-USE check and returned `None`, so a `best`
        // that reaches here really is a same-version installed one the
        // atom's `[flag]` isn't satisfied by.
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
        // `is_top_level && !selective`: real portage's own bare,
        // reasonless `[ebuild R]` -- see this function's own doc
        // comment's `selective`/`is_top_level` paragraph. `changed_flags`/
        // `deps_changed_flag`/`slot_changed_flag`/`rebuilt_binary_flag`/
        // `new_repo_flag` may all still be empty/false here; that's the
        // whole point of this case.
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

    // Upgrade/Downgrade/New is decided against only what's installed in
    // `best`'s *own* main slot (real `_get_installed_best`'s `myinslotlist
    // = vardb.match(pkg.slot_atom)`). An installed version in a different
    // slot never makes this a downgrade/upgrade -- it's a `New` into a
    // fresh slot (the renderer's `[ebuild NS]`, see `GraphEntry::new_slot`).
    let installed_in_slot: Vec<&String> = installed_pairs
        .iter()
        // Backlog #276: an `[installed]`-marked instance is hidden
        // (see `candidate_is_installed` above), so it neither anchors
        // an upgrade/downgrade nor blocks a `New`.
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
