//! Masked-dependency reports.

use crate::*;

/// The single-atom v1 `emerge --pretend` decision: find the best visible
/// candidate matching `atom_str` across all of `repos` (the main repo and
/// any overlays -- see `find_repos`), compare it against what's
/// installed. `atom_str` may be a full atom (operator, slot -- anything
/// portage-dep's v1 grammar supports), not just a bare category/package:
/// this is what lets dependency atoms extracted from DEPEND/RDEPEND (see
/// `resolve_pretend_graph`) reuse the exact same resolution logic as the
/// top-level CLI atom. `newuse`/`changed_use` each enable their own
/// reinstall check (see `reinstall_flags_for_use_change`) for an
/// already-installed match, `newuse` winning if both are set; `false`
/// for both reproduces this function's pre-`--newuse`/`--changed-use`
/// behavior exactly.
///
/// `update` (`--update`/`-u`) gates real `_wrapped_select_pkg_highest_available_imp`'s
/// own `avoid_update`/`dont_miss_updates` behavior (`lib/_emerge/depgraph.py`,
/// lines 7814 and 8448): `"--update" not in myopts` is real portage's
/// *default*, and when it holds, an already-installed version that
/// itself still satisfies the atom is returned immediately, without ever
/// searching for a newer one -- real emerge does NOT offer an upgrade
/// just because `emerge cat/pkg` was run with no other flags; that's
/// what `--update`/`-u` is for. Ported below as an early return, checked
/// before the "always resolve to the single best visible candidate"
/// logic that already existed: if `!update` and some installed version
/// both matches `atom_str` and still has a visible candidate in `visible`
/// (mask/keyword-filtered above), the highest such version is used
/// as-is, exactly like the pre-existing "installed version equals best"
/// branch below, `newuse`/`changed_use` included. Requiring a *visible*
/// candidate (not just checking the vdb directly) is deliberate, not an
/// oversight: it's what lets an installed version that's since become
/// masked fall through to the ordinary best-visible-candidate path
/// below unchanged, matching real portage's own "enable upgrade or
/// downgrade to a version with visible KEYWORDS when the installed
/// version is masked" comment right above its own `avoid_update` check.
/// When `update` is true, or no installed version qualifies this way,
/// behavior is exactly as before this parameter existed.
///
/// `selective`/`is_top_level`: a real, previously-undiscovered gap this
/// portuale's own `update` handling above didn't capture, found by
/// comparing portuale's own output against the real, installed system
/// `emerge` on a real package (`sys-apps/portage`) and tracing real
/// portage's own decision live. Real portage's `avoid_update` shortcut
/// above (`!update`, ported as `update` here) is NOT sufficient on its
/// own for a **directly-requested (top-level) atom**: real
/// `_wrapped_select_pkg_highest_available_imp`'s own per-candidate loop
/// (`lib/_emerge/depgraph.py`) computes `want_reinstall = reinstall or
/// empty or (found_available_arg and not selective)`, and `if
/// want_reinstall and matched_packages: continue` -- for a "found via an
/// atom on the command line" (`found_available_arg`, real
/// `_iter_atoms_for_pkg`) candidate, this SKIPS ever re-adding the
/// already-installed `Package` object as a further candidate at all
/// whenever `selective` is absent, so the later `if avoid_update: ...
/// return pkg` shortcut (`lib/_emerge/depgraph.py` line ~8447) finds
/// nothing installed to return and falls through to picking the best
/// *available* (ebuild) candidate instead -- even when its version is
/// identical to what's already installed. The net real effect: a bare
/// `emerge <atom>` with no other flags, on an atom named directly (not
/// reached via a dependency string), always resolves against the best
/// *available* version (searching for a newer one exactly as `--update`
/// would), and even when nothing newer exists, still reports a bare
/// reinstall (real `[ebuild R] cat/pkg-ver`, no parenthetical reason at
/// all) rather than treating the identical installed version as
/// satisfying -- confirmed live: `--noreplace`/`--selective` (both of
/// which set real `myparams["selective"]`) restore the "nothing to do"
/// result. `selective` here mirrors real `create_depgraph_params.py`'s
/// own `myparams["selective"] = True` condition, computed from
/// whichever of its own real trigger flags portuale actually
/// implements: `update`, `newuse`, `changed_use` (real portage's own
/// `--changed-use`/`-U` rewrites to `--reinstall=changed-use` before
/// `create_depgraph_params` ever runs, `lib/_emerge/main.py`, and
/// `--reinstall` is itself constrained to that one literal choice in
/// real portage -- so `changed_use` alone covers portuale's whole
/// share of that real condition, no separate `--reinstall` flag
/// needed), `changed_deps` (any non-`"n"` value), `changed_slot`, plus
/// the two flags whose *entire* real effect is exactly this (see
/// `pretend.rs`'s own CLI parsing): `--noreplace`/`-n` and
/// `--selective[=y|n]` (`n` explicitly cancels `selective` even if one
/// of the other conditions set it, matching real `create_depgraph_
/// params.py`'s own `if myopts.get("--selective") == "n"`: pop
/// unconditionally). Real `--newrepo` (forces reinstall specifically on
/// an installed-vs-current repo mismatch, and separately contributes to
/// `selective`) is a documented, narrower scope cut, deliberately not
/// modeled: portuale has no vdb `REPOSITORY` reader (confirmed absent
/// during this same investigation -- the real vdb file is even
/// lowercase `repository`, unlike every other metadata key), the same
/// "no vdb-metadata reader" simplification already documented elsewhere
/// in this crate (e.g. `enqueue_dependencies`'s own doc comment).
///
/// `is_top_level` is portuale's own existing "argument" equivalent --
/// `resolve_pretend_graph`'s own `depth == 0`, the identical
/// equivalence already established for `--with-test-deps`'s own
/// `pkg.depth == 0 and self._is_argument(pkg)` gating -- since every
/// depth-0 atom here already came from `atoms` itself or a `@world`/
/// `@system` expansion of it, both of which real portage's own
/// `_iter_atoms_for_pkg` also counts as an "argument" for this exact
/// purpose. A dependency atom (`is_top_level = false`) is NEVER affected
/// by `selective` at all -- real `found_available_arg` is only ever set
/// for an argument-derived candidate in the first place, so a
/// dependency atom's own already-installed, still-satisfying version
/// keeps exactly its pre-existing `AlreadyInstalled` treatment,
/// unconditionally, matching real `_want_installed_pkg`'s own `return
/// not arg` fallback (empty `arg` for a non-argument package).
///
/// Applied at both places this function can otherwise decide an
/// installed version satisfies the atom "as is": the `!update`
/// shortcut immediately below (skipped entirely -- not just its
/// outcome adjusted -- whenever `is_top_level && !selective`, so
/// version selection also falls through to the ordinary "best across
/// everything visible" comparison below, exactly reproducing real
/// portage's own "searches for a newer version even without `--update`"
/// effect for this case) and the final "best visible candidate happens
/// to already be installed" comparison further down (where, instead,
/// the outcome is forced to `Reinstall` -- with whatever
/// `changed_flags`/`deps_changed`/`slot_changed` were independently
/// computed, all three possibly still empty/false, exactly matching
/// real portage's own bare, reasonless `[ebuild R]`).
///
/// `excluded` (`--exclude`/`-X`) is a list of raw atom/wildcard-atom
/// strings (real `WildcardPackageSet`, ported here as the same "try
/// `match_from_list`, fall back to `parse_wildcard_atom`" two-tier
/// matcher `matches_config_entry` already uses for `package.mask`/
/// `.unmask` -- both are real portage atom-set matchers with the
/// identical "plain atom or `*`-wildcard" grammar). Checked in two
/// places, mirroring real depgraph.py's own scattered
/// `excluded_pkgs.findAtomForPackage` call sites: (1) if an installed
/// version matches both `atom_str` and an exclude atom, it's returned
/// as `AlreadyInstalled` immediately, before `update`/`newuse`/
/// `changed_use` ever get a say -- ported from `_want_update_pkg`'s and
/// `_replace_installed_atom`'s own excluded-check-first pattern (real
/// portage: "the user has not explicitly requested for this package to
/// be replaced", so excluding it means never touching it, full stop);
/// (2) an excluded candidate is never eligible to be selected as the
/// New/Upgrade "best visible candidate" either, mirroring the
/// `excluded_pkgs`-gated candidate-selection loops elsewhere in
/// depgraph.py (e.g. around its own lines 2331 and 5544) -- if every
/// remaining candidate for this atom is excluded and none is already
/// installed, this resolves to `NoVisibleCandidate`, the same outcome
/// portuale already gives an atom with no eligible candidate for any
/// other reason. Deliberately NOT replicated: real depgraph.py's own
/// ~18 `excluded_pkgs` call sites cover many more specific interaction
/// points (autounmask, binpkg selection, `--complete-graph`, ...) this
/// portuale doesn't implement at all -- the two checks above cover the
/// dominant real-world use ("pin an installed package so `--update`/
/// `--deep` never touch it") and the New/Upgrade selection case, not
/// every real edge case.
/// Whether every atom in `atoms` currently has a satisfying candidate --
/// one input to the [`AltPreference`](portage_use_reduce::AltPreference)
/// rank `use_reduce_flat_disjunctive` uses to pick a `"||"` group's
/// alternative (the `Available` half; `atom_cp_installed` is the
/// installed-vs-not half). See `resolve_pretend_graph`'s own doc comment
/// for the full grounding. A blocker atom (`!foo/bar`/`!!foo/bar`) is always
/// satisfiable here, vacuously -- it isn't a dependency to *resolve* at
/// all, just a conflict to report (`enqueue_flat_deps` handles that
/// separately, unaffected by which `"||"` alternative was chosen), so
/// it never disqualifies an otherwise-fine alternative.
///
/// Deliberately the *early* half of `resolve_pretend`'s own logic only
/// (`list_candidates` -> filter `is_visible` -> `match_from_list` ->
/// USE-dep post-filter) -- not a call to `resolve_pretend` itself,
/// which also applies `--update`/`--newuse`/`--exclude`/reinstall
/// refinements that only matter once an alternative has already been
/// chosen and is actually being resolved, not for this "is it even
/// possible" probe. A minor, deliberate duplication of that logic
/// rather than a shared refactor -- the same "duplicate a small,
/// stable chunk rather than force two different call sites through one
/// function" precedent `keyword_masked_only` already set against
/// `is_visible` itself.
pub(crate) fn atom_currently_satisfiable(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    // The `'backtrack` loop's accumulated per-`cat/pkg` constraints for
    // this atom's package -- real backtracking's `runtime_pkg_mask` seen
    // by `dep_zapdeps` (`dep_check.py:449` `all_available`). A `!`-prefixed
    // entry is a negative (the candidate must NOT match it); a bare entry
    // is positive. Empty `&[]` at every non-disjunctive call site -- a
    // strict no-op, exactly the pre-`||`-backtrack behaviour.
    extra_constraints: &[String],
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return true;
    }
    let Ok(candidates) = list_candidates(repos, &atom.category, &atom.package) else {
        return false;
    };
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return false;
    }

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
    let Some(matched) = portage_dep::match_from_list(atom_str, &candidate_str_refs) else {
        return false;
    };
    // Apply the accumulated `runtime_pkg_mask` negatives/positives the
    // same way `resolve_pretend` filters its own `matched` list
    // (lib.rs ~7437) -- an alternative whose every satisfying candidate is
    // masked out counts as unsatisfiable, so the next `||` alternative
    // wins on the retry.
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

    let Some(use_deps) = atom.use_deps.as_ref().filter(|d| !d.is_empty()) else {
        return !matched.is_empty();
    };
    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }
    matched.into_iter().any(|m| {
        let Some(candidate) = by_str.get(m) else {
            return false;
        };
        let Some((iuse, use_flags)) =
            candidate_iuse_and_use(candidate, &atom.category, &atom.package, config)
        else {
            return false;
        };
        portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
    })
}

/// Real `atom.violated_conditionals(pkg_use_enabled(avail_pkg),
/// avail_pkg.iuse.is_valid_flag)` -- the `all_use_unmasked` boolean of
/// `dep_zapdeps`'s choice classification (`dep_check.py` soft 406-414,
/// 497-523). For an atom whose `[use]` deps NO candidate satisfies
/// (`avail_pkg_use` empty, the `if not avail_pkg_use:` branch at soft
/// 494), real computes which flags would have to change to satisfy the
/// atom, then checks those violated flags against the best candidate's
/// (`avail_pkg = mydbapi_match_pkgs(atom.without_use)[-1]`, see
/// `atom_all_use_unmasked`) own `use.mask` / `use.force`:
///   * an enabled-required flag that isn't enabled needs unmasking if it
///     sits in `use.mask` (global or package, `resolved_use_mask_or_force`)
///   * a disabled-required flag that is enabled needs unforcing if it
///     sits in `use.force` and is *not* also in `use.mask`
///     (one masked/forced violation makes the whole choice fall to `other`,
///     never selectable). Evaluated per single use-dep via
///     [`portage_dep::use_deps_satisfied`] so default-marker/missing-IUSE
///     semantics match real exactly (`(+)`-defaulted missing flags are "as if
///     enabled", `(-)`-defaulted missing flags "as if disabled", no-default
///     missing flags fail the atom outright -- the same classification real's
///     `violated_conditionals` derives from `is_valid_flag` +
///     `missing_enabled`/`missing_disabled`).
pub(crate) fn atom_all_use_unmasked(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    extra_constraints: &[String],
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return true;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return true;
    }
    let Some(use_deps) = atom.use_deps.as_deref().filter(|d| !d.is_empty()) else {
        return true;
    };
    let Some((candidate_str, keywords, iuse, enabled, _slot)) =
        highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
            repos,
            atom_str,
            category: &atom.category,
            package: &atom.package,
            config,
            extra_constraints,
        })
    else {
        // No visible candidate at all -- `all_available` already sent the
        // whole choice to `Unsatisfiable`; nothing to evaluate here.
        return true;
    };
    let valid_iuse = valid_iuse(&iuse, config);
    let stable = is_stable(IsStableArgs {
        keywords: &keywords,
        candidate_str: &candidate_str,
        category: &atom.category,
        package: &atom.package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    });
    let mask = resolved_use_mask_or_force(
        MaskOrForce::Mask,
        config,
        &candidate_str,
        &atom.category,
        &atom.package,
        stable,
    );
    let force = resolved_use_mask_or_force(
        MaskOrForce::Force,
        config,
        &candidate_str,
        &atom.category,
        &atom.package,
        stable,
    );
    let mut violated_enabled = Vec::new();
    let mut violated_disabled = Vec::new();
    for ud in use_deps {
        // Single-dep evaluation = "would *this* flag have to change", per
        // real `violated_conditionals`'s per-token classification.
        if !portage_dep::use_deps_satisfied(std::slice::from_ref(ud), &valid_iuse, &enabled) {
            match ud.op {
                portage_dep::UseDepOp::Enabled => violated_enabled.push(ud.flag.clone()),
                portage_dep::UseDepOp::Disabled => violated_disabled.push(ud.flag.clone()),
                // The four conditional forms impose no candidate
                // constraint in portuale's matching (see the
                // `use_deps_satisfied` doc), so they can never be
                // "violated" here either.
                _ => {}
            }
        }
    }
    if violated_enabled.iter().any(|f| mask.contains(f)) {
        return false;
    }
    if violated_disabled
        .iter()
        .any(|f| force.contains(f) && !mask.contains(f))
    {
        return false;
    }
    true
}

/// Real `avail_pkg = mydbapi_match_pkgs(atom.without_use)[-1]`
/// (`dep_check.py` soft 469-474): the single highest-sorted visible
/// candidate matching an atom's `[...]`-stripped form, plus its
/// candidate string, IUSE, effective enabled-use set and slot -- the
/// seeds of the `use.mask`/`use.force` probe, the `violated_conditionals`
/// argument of [`atom_all_use_unmasked`], and the slot
/// [`atom_installed_in_slot_of`] checks against the vdb.
pub(crate) type CandidateUseProbe = (
    String,
    Vec<String>,
    HashSet<String>,
    Rc<HashSet<String>>,
    String,
);

/// Arguments of [`highest_available_candidate_ignoring_use`].
pub(crate) struct HighestAvailableCandidateIgnoringUseArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) atom_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) extra_constraints: &'a [String],
}

/// `extra_constraints` is the same `'backtrack`-loop `runtime_pkg_mask`
/// filter [`atom_currently_satisfiable`] applies -- real's `mydbapi` in
/// `dep_zapdeps` is `_dep_check_composite_db`, which honours it too, so
/// once backtracking masks the highest version this must probe the
/// highest *unmasked* one instead. Empty `&[]` at every non-disjunctive
/// call site, a strict no-op.
pub(crate) fn highest_available_candidate_ignoring_use(
    args: HighestAvailableCandidateIgnoringUseArgs<'_>,
) -> Option<CandidateUseProbe> {
    let HighestAvailableCandidateIgnoringUseArgs {
        repos,
        atom_str,
        category,
        package,
        config,
        extra_constraints,
    } = args;
    let stripped = portage_dep::without_use(atom_str);
    let Ok(candidates) = list_candidates(repos, category, package) else {
        return None;
    };
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, category, package, config))
        .collect();
    if visible.is_empty() {
        return None;
    }
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{category}/{package}-{}:{}/{}::{}",
                c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(stripped, &candidate_str_refs)?;
    // Same `runtime_pkg_mask` filter as `atom_currently_satisfiable` --
    // see that function's own doc comment for the grounding.
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
    // Real `match_pkgs` returns matches ascending by version and takes
    // `[-1]`; `candidate_strs` is in repo/readdir order, so pick the
    // highest-VERSIONED match explicitly.
    let best = matched
        .iter()
        .filter_map(|m| by_str.get(m))
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))?;
    let (iuse, use_flags) = candidate_iuse_and_use(best, category, package, config)?;
    let best_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        best.version, best.slot, best.sub_slot, best.repo_name
    );
    Some((
        best_str,
        best.keywords.clone(),
        iuse,
        use_flags,
        best.slot.clone(),
    ))
}

/// Real `Package._validate_deps`'s own IUSE-missing-conditional mask
/// (`_emerge/Package.py`, backlog #153): every `flag? ( … )` group and
/// every `[flag?]`/`[!flag?]`/`[flag=]`/`[!flag=]` use-dep conditional in
/// the five dep keys must reference a flag valid for the *atom-owning*
/// package's own effective IUSE (`is_valid_flag`, i.e. explicit IUSE ∪
/// implicit: arch.list, use.mask/force, `build`/`bootstrap`,
/// `iuse_effective` -- [`implicit_iuse_set`]), or the whole candidate is
/// masked `invalid`. Skipped for installed packages in real
/// (`dep_valid_flag = None` when `self.installed`); portuale only ever
/// calls this for repo/binary candidates, never vdb ones, so the
/// exemption holds structurally. The check runs with `matchall=True`
/// semantics: every conditional is validated regardless of the
/// candidate's own USE state (an inactive `off? ( … )` group with an
/// undeclared flag is still invalid -- real validates at group close,
/// not at evaluation).
///
/// The scan mirrors real `use_reduce(..., matchall=True,
/// token_class=Atom, flat=True)`'s own single left-to-right pass: an
/// atom token is validated the moment it is scanned (real constructs
/// the `Atom`, whose `__init__` runs `_validate_conditional_flags`
/// immediately), a group conditional when its own `)` closes
/// (innermost group first, emerging naturally from the stack). First
/// failure per key wins (real raises, aborting that key's reduction);
/// keys run in real's own `_dep_keys` order and every key still runs
/// after a failure, so a candidate can carry several `invalid: …`
/// reasons at once.
///
/// Message shapes, both confirmed live against real 3.0.82.2 on the
/// fixture tree (via a `cp -a` copy, never the live checkout):
/// - group: `{KEY}: USE flag 'soflag' referenced in conditional
///   'soflag?' is not in IUSE` (the raw conditional token, no atom part
///   -- `dev-vcs/somercurial-5.5.1`'s own `RDEPEND`);
/// - atom: `{KEY}: USE flag 'x' referenced in conditional 'x?' in atom
///   'dev-libs/fucyclea[x?]' is not in IUSE` (conditional from real's
///   own `_conditional_strings`: `f?`/`!f?`/`f=`/`!f=`; the atom exactly
///   as written, defaults like `[baz(+)?]` kept --
///   `dev-libs/usedeppkg-1.0`'s own `RDEPEND`).
///
/// Within one atom, real checks its `_conditionals_class` slot order
/// (`disabled`, `enabled`, `equal`, `not_equal`); the scan does the same.
///
/// Deliberate narrowings, all without a fixture instance: EAPI gating
/// (real passes `dep_eapi`, so an ancient EAPI reports `EAPI.incompatible`
/// instead -- portuale's own atom grammar already accepts what it
/// accepts, independently of this check); `LICENSE`/`PROPERTIES`/`RESTRICT`
/// group conditionals and `SRC_URI` (real validates those too, but the
/// `LICENSE` shortcut's own doc comment parks that pathway separately);
/// and syntactically malformed dep strings (real reports those as
/// `*.syntax` invalid instead -- an unparseable atom token is skipped
/// here, an unbalanced bracket ends the scan silently).
pub(crate) fn invalid_use_conditional_reasons(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Vec<String> {
    let ebuild_map;
    let deps: &HashMap<String, String> = match candidate.source {
        CandidateSource::Ebuild => {
            let stem = format!("{package}-{}", candidate.version);
            let Ok(map) = repo_aux_metadata(&candidate.repo_location, category, &stem) else {
                return Vec::new();
            };
            ebuild_map = map;
            &ebuild_map
        }
        // A `Packages` index carries the binpkg's own dep keys (real
        // validates a built, non-installed binary exactly like an
        // ebuild here); there is no fixture binpkg oracle for the
        // message text, so this arm is fidelity without a pin.
        CandidateSource::Binary => &candidate.binary_deps,
    };
    let valid = implicit_iuse_set(&candidate.iuse, config);
    let mut out = Vec::new();
    for key in ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"] {
        let Some(dep) = deps.get(key) else { continue };
        if dep.trim().is_empty() {
            continue;
        }
        if let Some(msg) = first_invalid_conditional(dep, &valid) {
            out.push(format!("invalid: {key}: {msg}"));
        }
    }
    out
}

/// First IUSE-missing conditional in one dep string, in real's own
/// encounter order -- see [`invalid_use_conditional_reasons`].
pub(crate) fn first_invalid_conditional(dep: &str, valid: &HashSet<String>) -> Option<String> {
    /// What the current level's last token was: only a `?`-suffixed
    /// token can open a group, so only that needs remembering.
    enum Last {
        None,
        Cond(String),
        Other,
    }
    let mut stack: Vec<Last> = vec![Last::None];
    for tok in dep.split_whitespace() {
        match tok {
            "(" => stack.push(Last::None),
            ")" => {
                let _closed = stack.pop();
                let Some(top) = stack.last_mut() else {
                    continue;
                };
                let raw = match top {
                    Last::Cond(raw) => std::mem::take(raw),
                    _ => continue,
                };
                *top = Last::Other;
                let inner = raw.strip_suffix('?').unwrap_or(raw.as_str());
                let flag = inner.strip_prefix('!').unwrap_or(inner);
                if !valid.contains(flag) {
                    return Some(format!(
                        "USE flag '{flag}' referenced in conditional '{raw}' is not in IUSE"
                    ));
                }
            }
            "||" => {
                if let Some(top) = stack.last_mut() {
                    *top = Last::Other;
                }
            }
            t if t.ends_with('?') => {
                if let Some(top) = stack.last_mut() {
                    *top = Last::Cond(t.to_string());
                }
            }
            t => {
                if let Some(top) = stack.last_mut() {
                    *top = Last::Other;
                }
                let Some(atom) = portage_dep::parse_atom(t) else {
                    continue;
                };
                let Some(use_deps) = atom.use_deps else {
                    continue;
                };
                // Real `_conditionals_class` slot order.
                for op in [
                    portage_dep::UseDepOp::IfParentDisabled,
                    portage_dep::UseDepOp::IfParentEnabled,
                    portage_dep::UseDepOp::EqualParent,
                    portage_dep::UseDepOp::OppositeParent,
                ] {
                    let (prefix, suffix) = match op {
                        portage_dep::UseDepOp::IfParentDisabled => ("!", "?"),
                        portage_dep::UseDepOp::IfParentEnabled => ("", "?"),
                        portage_dep::UseDepOp::EqualParent => ("", "="),
                        portage_dep::UseDepOp::OppositeParent => ("!", "="),
                        portage_dep::UseDepOp::Enabled | portage_dep::UseDepOp::Disabled => {
                            continue;
                        }
                    };
                    for ud in &use_deps {
                        if ud.op == op && !valid.contains(&ud.flag) {
                            return Some(format!(
                                "USE flag '{f}' referenced in conditional '{prefix}{f}{suffix}' in atom '{t}' is not in IUSE",
                                f = ud.flag
                            ));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Real `getmaskingstatus._getmaskingstatus`'s human reason strings for a
/// candidate that is *not* [`is_visible`] -- the `(masked by: …)` text
/// [`all_masked_report`] puts after each `- <cpv>` line. Reason order
/// mirrors real: `package.mask`, `LICENSE`, `PROPERTIES`, `RESTRICT`,
/// `KEYWORDS`, then `invalid` (real appends the invalid reasons after
/// `_getmaskingstatus` in `_get_masking_status`). `package.mask` and the `KEYWORDS` string (`"~<arch>
/// keyword"` / `"-<arch> keyword"` / `"missing keyword"`) and the
/// `LICENSE` string (`"<names> license(s)"`) match real verbatim;
/// `PROPERTIES`/`RESTRICT` are the bare key name (real names the specific
/// tokens -- portuale has no missing-token list for those two, and
/// neither the L0 comparator nor the contract suite checks this line
/// against real anyway; the two `!!!` header lines are what matter).
pub(crate) fn candidate_masking_reasons(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Vec<String> {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let mut reasons = Vec::new();

    if any_config_entry_matches(&config.package_mask, &candidate_str, category, package)
        && !any_config_entry_matches(&config.package_unmask, &candidate_str, category, package)
    {
        reasons.push("package.mask".to_string());
    }

    if !license_accepted(candidate, category, package, &candidate_str, config) {
        let missing = missing_licenses(candidate, category, package, &candidate_str, config);
        reasons.push(if missing.is_empty() {
            "LICENSE".to_string()
        } else {
            format!("{} license(s)", missing.join(" "))
        });
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        reasons.push("PROPERTIES".to_string());
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        reasons.push("RESTRICT".to_string());
    }

    if !keywords_accepted(KeywordsAcceptedArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    }) {
        let kmask = candidate
            .keywords
            .iter()
            .find_map(|k| {
                k.strip_prefix('~')
                    .filter(|a| config.accept_keywords.contains(*a))
                    .map(|a| format!("~{a}"))
                    .or_else(|| {
                        k.strip_prefix('-')
                            .filter(|a| config.accept_keywords.contains(*a))
                            .map(|a| format!("-{a}"))
                    })
            })
            .unwrap_or_else(|| "missing".to_string());
        reasons.push(format!("{kmask} keyword"));
    }

    // Backlog #153: real appends the `invalid` reasons after
    // `_getmaskingstatus` in `_get_masking_status` -- same position here.
    reasons.extend(invalid_use_conditional_reasons(
        candidate, category, package, config,
    ));

    if reasons.is_empty() {
        reasons.push("masked".to_string());
    }
    reasons
}

/// One dependency atom's masked-only disclosure: real
/// `_show_unsatisfied_dep`'s "All ebuilds that could satisfy … have been
/// masked" block for a *dependency* (the Tier 2.20 half of finding S's
/// top-level report). The resolver attaches these to the pass result;
/// the caller (`pretend.rs`) renders each in place of the bare
/// `!!! no visible ebuild for dependency` line, followed by the
/// `(dependency required by …)` chain, then the docs footer -- real
/// `depgraph.py:6992-7030`'s shape for a non-`AtomArg` parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom text (the unevaluated form as queued, so a
    /// USE-conditional dep shows as written).
    pub atom: String,
    /// `(cpv::repo, reasons)` descending version, cpv-deduped -- exactly
    /// the `- <cpv> (masked by: …)` lines the renderer prints.
    pub masked: Vec<(String, Vec<String>)>,
    /// Real `_get_dep_chain` for this disclosure: `(node, type)` pairs,
    /// innermost parent first, up to the top-level argument(s) -- e.g.
    /// `("dev-libs/n1-1.0::probe", "ebuild")` then `("dev-libs/n1",
    /// "argument")`. An installed intermediate renders its vdb repo
    /// (`__unknown__` fallback, like real) with type `"installed"`; a
    /// binary parent renders `"binary"`. Computed post-loop out of the
    /// final entries (see `masked_dep_chain`); one chain per direct
    /// parent (real shows only its DFS-first abandon -- portuale never
    /// abandons, so all failing branches disclose, convergence with #19
    /// parked).
    pub chain: Vec<(String, String)>,
}

/// Real `_show_unsatisfied_dep`'s separate "no ebuilds built with USE
/// flags to satisfy" disclosure (backlog #20) for a *dependency*'s
/// `[use]`-unsatisfied atom: among the ebuilds an atom names by version
/// (USE ignored), the visible ones whose own IUSE is missing a required
/// flag (`Missing IUSE: …`) or whose effective USE cannot satisfy the
/// atom's unconditional use-deps (`Change USE: +… -…`). Rendered instead
/// of the bare `!!! no visible ebuild for dependency` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseUnsatDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom as queued, unevaluated (real
    /// `atom.unevaluated_atom`), so a `[flag]` dep shows as written.
    pub atom: String,
    /// The `- <cpv>::<repo> (<reason>)` rows in real's display order: the
    /// latest `Change USE:`-adjustable candidate only (real
    /// `unmasked_use_reasons`'s "Only show the latest version"), else
    /// every `Missing IUSE:` candidate in descending-version order. Never
    /// empty (the report exists only when a row does).
    pub rows: Vec<(String, Vec<String>)>,
    /// Real `_get_dep_chain`, walked post-pass exactly like
    /// [`MaskedDepReport::chain`].
    pub chain: Vec<(String, String)>,
}

/// Real `_show_unsatisfied_dep`'s plain-miss disclosure — the
/// `emerge: there are no ebuilds to satisfy "<unevaluated atom>"`
/// block plus the `(dependency required by …)` chain — for a dependency
/// `NoVisibleCandidate` that is neither masked-only nor `[use]`-
/// unsatisfied (nothing matches the atom by version at all, or every
/// version-matching candidate is unusable for a reason no
/// `Change USE:`/`Missing IUSE:` row can name, e.g. a profile-forced
/// flag). Rendered instead of the bare `!!! no visible ebuild for
/// dependency "<cp>"` line (#135 (d); oracle bed
/// `l0-fx-20260922T192350Z` arm D / `libcnoisepkg`). The staging
/// `for <root>.` suffix real appends when the target root is not `/`
/// stays absent, matching the top-level miss and the two sibling arms
/// (the `fixture-miss-message-unsuffixed` class).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainMissDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom as queued, unevaluated (real
    /// `atom.unevaluated_atom`), so a `[flag=]` dep shows as written.
    pub atom: String,
    /// Real `_get_dep_chain`, walked post-pass exactly like
    /// [`MaskedDepReport::chain`].
    pub chain: Vec<(String, String)>,
}

/// Real `_show_unsatisfied_dep`'s version match: the candidates an atom
/// names by version, visibility and USE ignored (real `db.match(atom.
/// without_use)`). `None` when the atom matches no ebuild at all by
/// version (real's plain "there are no ebuilds to satisfy" case) or when
/// some matching ebuild is actually `is_visible` (e.g. only a `[use]`-dep
/// mismatch -- real's separate "no ebuilds built with USE flags" path,
/// left to the autounmask-use machinery here).
pub(crate) fn masked_candidates_for_atom(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
) -> Option<Vec<(String, Vec<String>)>> {
    let atom = portage_dep::parse_atom(atom_str)?;
    let candidates = list_candidates(repos, &atom.category, &atom.package).ok()?;

    let strs: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    // Real `db.match(atom.without_use)` -- version/slot filter, USE and
    // visibility ignored.
    let matched: std::collections::HashSet<&str> = portage_dep::match_from_list(atom_str, &refs)?
        .into_iter()
        .collect();

    let mut masked: Vec<&Candidate> = candidates
        .iter()
        .zip(strs.iter())
        .filter(|(_, s)| matched.contains(s.as_str()))
        .map(|(c, _)| c)
        .collect();
    if masked.is_empty()
        || masked
            .iter()
            .any(|c| is_visible(c, &atom.category, &atom.package, config))
    {
        return None;
    }
    // Real `cpv_list.reverse()` -> descending version.
    masked.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));

    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in masked {
        let output_cpv = format!(
            "{}/{}-{}::{}",
            atom.category, atom.package, c.version, c.repo_name
        );
        if !seen.insert(output_cpv.clone()) {
            continue;
        }
        let reasons = candidate_masking_reasons(c, &atom.category, &atom.package, config);
        out.push((output_cpv, reasons));
    }
    Some(out)
}

pub(crate) fn all_masked_report(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    xinfo: &str,
) -> Option<String> {
    // The header/footer live here (top-level shape); the masked list
    // itself is shared with the dependency disclosure below.
    let masked = masked_candidates_for_atom(repos, atom_str, config)?;

    let mut out = format!(
        "\n!!! All ebuilds that could satisfy {xinfo} have been masked.\n\
         !!! One of the following masked packages is required to complete your request:\n"
    );
    for (output_cpv, reasons) in masked {
        out.push_str(&format!(
            "- {output_cpv} (masked by: {})\n",
            reasons.join(", ")
        ));
    }
    out.push_str(
        "\nFor more information, see the MASKED PACKAGES section in the emerge\n\
         man page or refer to the Gentoo Handbook.\n",
    );
    Some(out)
}
