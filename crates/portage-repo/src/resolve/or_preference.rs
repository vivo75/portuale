//! Which alternative of an `||` group to prefer.

use crate::*;

/// Real `dep_zapdeps`'s two bug-531656 guards (`dep_check.py` soft 476-490
/// `conflict_downgrade`, 531-541 `installed_downgrade`): an otherwise
/// available `||` alternative is demoted to the `other` bin when one of its
/// atoms would pull in a *lower* version of a package whose slot the live
/// graph already holds at a higher version, unless that downgrade is
/// desirable ([`downgrade_probe`]). This is what makes
/// `|| ( dev-ml/labltk:= <dev-lang/ocaml-4.02 )` take `labltk` once the
/// `ocaml-4.02.1` update is in the graph (upstream
/// `test_or_choices.py::testConflictMissedUpdate`) instead of the
/// `<ocaml-4.02` arm that forces the update back out.
///
/// Real's `graph_db` is the depgraph's live package set; portuale's is the
/// resolver's in-progress `entries` (both `||` call sites pass
/// `state.entries`/`entries` as they stand at that instant), which is the
/// same set real consults at the same moment.
///
/// Port notes, each a deliberate narrowing:
///   - `avail_pkg` is the highest visible tree match of the atom without its
///     `[use]` block (real then re-selects with USE when that succeeds; the
///     version compared below can only differ when a USE-satisfying lower
///     version exists, not a shape this guard targets).
///   - `highest_in_slot` counts only as "in graph" when an entry for that
///     `cat/pkg` slot exists (merge-bound, or an `AlreadyInstalled` node).
///     Real's `_select_package` would also return a bare installed instance
///     that is not yet a graph node when `--update` is off; that variant
///     needs the update mode threaded to this depth and is left out.
///   - `replacing` (`will_replace_child`): an atom of the entry's own
///     `cat/pkg` (`self_cp`) is never demoted.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn alternative_downgrade_demoted(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    entries: &[GraphEntry],
    self_cp: &RootCp,
    constraints: &HashMap<(String, String), Vec<String>>,
    atoms: &[String],
    queued: &[QueueItem],
    update: bool,
) -> bool {
    atoms.iter().any(|a| {
        let Some(at) = portage_dep::parse_atom(a) else {
            return false;
        };
        if at.blocker != portage_dep::Blocker::None
            || (at.category == self_cp.1 && at.package == self_cp.2)
        {
            return false;
        }
        let extra = constraints
            .get(&(at.category.clone(), at.package.clone()))
            .map_or(&[][..], Vec::as_slice);
        let Some((avail_version, avail_slot)) = visible_tree_matches(repos, a, config, extra)
            .into_iter()
            .max_by(|x, y| vercmp_ordering(&x.0, &y.0))
        else {
            return false;
        };
        // Graph nodes in `cat/pkg:avail_slot`: merge-bound versions, and the
        // installed version when an `AlreadyInstalled` node holds the slot.
        let mut in_slot: Vec<String> = entries
            .iter()
            .filter(|e| {
                e.category == at.category
                    && e.package == at.package
                    && e.slot.as_deref() == Some(avail_slot.as_str())
            })
            .filter_map(|e| match &e.outcome {
                PretendOutcome::AlreadyInstalled { version }
                | PretendOutcome::New { version }
                | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some(to.clone())
                }
                // #72 B3: a removal installs nothing into the slot.
                PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
            })
            .collect();
        // Backlog #90 (S1): real selects every world/argv atom inline
        // and forward before walking anything, so by the time a
        // `||` alternative is judged the pending update is already a
        // graph node. Portuale walks last-declared-first and judges
        // eagerly: a queued atom on the same cp stands in for the
        // not-yet-graphed selection. The hypothetical is what that
        // selection would pick -- tree-best under `--update`
        // (the update real already graphed), installed-best otherwise
        // (real selected the installed instance) -- evaluated against
        // the queued atom's own text, with that cp's hard constraints.
        if in_slot.is_empty() {
            let mut hypothetical: Vec<String> = Vec::new();
            for q in queued {
                let Some(qat) = portage_dep::parse_atom(&q.atom) else {
                    continue;
                };
                if qat.category != at.category || qat.package != at.package {
                    continue;
                }
                let qextra = constraints
                    .get(&(qat.category.clone(), qat.package.clone()))
                    .map_or(&[][..], Vec::as_slice);
                if update {
                    hypothetical.extend(
                        visible_tree_matches(repos, &q.atom, config, qextra)
                            .into_iter()
                            .map(|(v, _)| v),
                    );
                } else {
                    hypothetical.extend(
                        installed_candidates(root, &qat.category, &qat.package)
                            .into_iter()
                            .filter_map(|(v, _, _)| {
                                let probe = format!("{}/{}-{v}", qat.category, qat.package);
                                if portage_dep::match_from_list(&q.atom, &[probe.as_str()])
                                    .is_some_and(|m| !m.is_empty())
                                {
                                    Some(v)
                                } else {
                                    None
                                }
                            }),
                    );
                }
            }
            in_slot = hypothetical;
        }
        let Some(highest_in_graph) = in_slot.iter().max_by(|x, y| vercmp_ordering(x, y)) else {
            return false;
        };
        if vercmp_ordering(&avail_version, highest_in_graph) != Ordering::Less {
            return false;
        }
        let slot_atom = format!("{}/{}:{avail_slot}", at.category, at.package);
        // `conflict_downgrade` (two or more graph packages in the slot) and
        // `installed_downgrade` (the slot's highest is in the graph) share the
        // same "lower than what the graph holds, and not a desirable
        // downgrade" test once `highest_in_slot` is a graph node.
        !downgrade_probe(repos, config, &slot_atom, &avail_version, extra)
    })
}

/// The single probe closure `use_reduce_flat_disjunctive` calls for every
/// `||` alternative -- real `dep_zapdeps`'s whole choice-bin
/// classification (`dep_check.py` soft 449-523, 599-618) collapsed into
/// one [`AltPreference`](portage_use_reduce::AltPreference) rank. Shared
/// by both of portuale's `||`-resolution call sites (the main New/
/// Upgrade/Reinstall walk in `backtracking_resolve`, and the `--deep`
/// AlreadyInstalled recursion in `enqueue_dependencies`) so a fix to one
/// can never silently miss the other again (2026-09-11 review finding
/// F1/F2: `enqueue_dependencies` had kept its own, unfixed copy of this
/// closure after the fine `unsat_use_*` bins shipped).
///
/// `constraints` is the `'backtrack`-loop's accumulated per-`cat/pkg`
/// `runtime_pkg_mask` (empty at every non-disjunctive call site, a strict
/// no-op) -- kept as the raw map rather than a `constraints_for(atom)`
/// closure parameter (as first drafted): a `&dyn Fn(&str) -> &[String]`
/// trait object needs the elided-lifetime signature `for<'a> Fn(&'a str)
/// -> &'a [String]`, which a closure whose return actually borrows a
/// captured map (not the input `&str`) can't satisfy without the borrow
/// checker extending the map's borrow for the trait object's whole
/// lifetime -- exactly the `slot_constraints` "does not live long enough"
/// / "also borrowed as mutable" conflict this hit against the
/// `'backtrack` loop's later `slot_constraints.entry(...)` mutations.
/// `self_cp` is the entry currently being resolved, for the
/// circular-self-dep check; `circular_dependency` is the `'backtrack`
/// loop's map (owner `cat/pkg` → circular-child records; empty on the
/// first pass, a strict no-op) -- real `dep_zapdeps`' `circular_atom`
/// demotion (`dep_check.py:673-691`), backlogs #216/#221. `root_deps_running_root` is the `--root-deps`
/// feed-in (`None` = strict no-op). See `atom_currently_satisfiable`'s,
/// `atom_all_use_unmasked`'s, `atom_cp_installed`'s and
/// `atom_installed_in_slot_of`'s own doc comments for the per-predicate
/// grounding; this function only sequences them the way real
/// `dep_zapdeps`'s per-atom loop does.
///
/// `circular_dependency` is backlog #221's `backtrack_infos["config"]
/// ["circular_dependency"] for this pass (empty on the first attempt):
/// the previous attempt's cycle edges, parent `cat/pkg` to in-cycle
/// children. An alternative with an atom matching one of `self_cp`'s
/// recorded children -- real's `circular_atom`
/// (`lib/portage/dep/dep_check.py:673-691`) -- ranks `Other`, so the
/// re-walk takes the branch that breaks the cycle (bugs 703440, 705986).
/// The installed-walk call sites (`collect_unwalked_installed_blockers`,
/// `enqueue_dependencies`) pass an empty map: real keys exact package
/// nodes (`Package.__hash__ = Task.__hash__`, no `__eq__` override), so
/// an installed parent -- never a merge-bound cycle node -- can never
/// hit. The argument is node-identity keying, not cp-disjointness:
/// portuale's cp-keying would over-demote on an installed-v1/merging-v2
/// cp collision real never exhibits (the blocker scan skips walked cps,
/// but `enqueue_dependencies` parents are `AlreadyInstalled` entries
/// whose cp may coincide with a merging cp).
///
/// Real's per-atom loop probes each atom's availability once and
/// short-circuits (`if not avail_pkg: ... break`, soft 469): review
/// finding F3 -- for an atom with no `[use]` block (the overwhelming
/// majority), `atom.without_use` and `atom` are the same string, so
/// real's separate `all_available`/`all_use_satisfied` probes would do
/// byte-identical candidate-listing work twice. This loop computes each
/// atom's availability once and reuses it for both facts when there is no
/// `[use]` block to strip.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn disjunction_preference(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    root: &Path,
    entries: &[GraphEntry],
    self_cp: &RootCp,
    constraints: &HashMap<(String, String), Vec<String>>,
    circular_dependency: &HashMap<RootCp, Vec<CircularDepChild>>,
    root_deps_running_root: Option<&Path>,
    atoms: &[String],
    queued: &[QueueItem],
    update: bool,
    // Track X Slice C (#242): the whole-graph views for the
    // circular-self-dep gate below. `entries`/`queued` above are the
    // group's own root scope (real's group-rooted `graph_db` match);
    // the gate asks whether the owner's cp is in play *at all* (real
    // has no self-exclusion -- the package is already graphed when its
    // own `||` group pops, whichever root it sits in), so it reads
    // these unscoped views instead. Same slices as `entries`/`queued`
    // on every same-root call -- a strict no-op there.
    gate_entries: &[GraphEntry],
    gate_queued: &[QueueItem],
) -> portage_use_reduce::AltPreference {
    let constraints_for = |a: &str| -> &[String] {
        portage_dep::parse_atom(a)
            .and_then(|at| constraints.get(&(at.category, at.package)))
            .map_or(&[][..], Vec::as_slice)
    };
    let mut all_available = true;
    let mut all_use_satisfied = true;
    let mut all_use_unmasked = true;
    // Portuale's own bolt-on (real has no equivalent): once true, the
    // alternative is hard `Unsatisfiable` in BOTH passes below, never
    // promoted into one of the `other_*` bins the way a genuine
    // `!all_available` atom is.
    //
    // Backlog #216: narrowed to real's actual pass-1 rule. Real has no
    // self-exclusion at all -- `_create_graph` adds the package to the
    // digraph before walking its deps (`_add_pkg` at select time), so by
    // the time the owner's own `||` group pops off
    // `_dep_disjunctive_stack` the self-naming atom matches an in-graph
    // node and the alternative ranks `preferred_in_graph` (bin 0): the
    // cycle forms, then `_serialize_tasks` dead-ends and the
    // `circular_dependency` map re-resolves it (see
    // `BacktrackParams::circular_dependency`). Only a self atom matching
    // nothing in-graph (and nothing installed) stays `Unsatisfiable` --
    // selecting it even as a last resort would let a package
    // self-satisfy its own not-yet-resolved atom.
    let mut circular = false;
    // Backlog #90 (S1)'s drain-state approximation (see
    // `atoms_all_in_graph`), built lazily: only a self-naming atom needs
    // it, and most alternatives have none.
    let mut queued_cps: Option<HashSet<(String, String)>> = None;
    for a in atoms {
        let self_naming = portage_dep::parse_atom(a).is_some_and(|at| {
            at.blocker == portage_dep::Blocker::None
                && at.category == self_cp.1
                && at.package == self_cp.2
        });
        if self_naming
            && !atom_cp_installed(root, a)
            && !atom_cp_present_in_graph(
                a,
                gate_entries,
                config,
                queued_cps.get_or_insert_with(|| {
                    gate_queued
                        .iter()
                        .filter_map(|q| {
                            portage_dep::parse_atom(&q.atom)
                                .map(|at| (at.category.clone(), at.package.clone()))
                        })
                        .collect()
                }),
            )
        {
            all_available = false;
            circular = true;
            break;
        }
        let stripped = portage_dep::without_use(a);
        let extra_constraints = constraints_for(a);
        // Real `all_available`'s per-atom probe: `mydbapi_match_pkgs(
        // atom.without_use)` -- USE settings never affect `||` preference
        // evaluation at this stage (dep_check.py soft 469).
        let avail = atom_currently_satisfiable(repos, stripped, config, extra_constraints)
            || root_deps_running_root
                .is_some_and(|running_root| running_root_satisfies_atom(a, running_root, config));
        if !avail {
            all_available = false;
            all_use_satisfied = false;
            break;
        }
        // No `[use]` block: real's `if atom.use:` is false, so this atom
        // trivially satisfies its own (nonexistent) use-deps -- reuse
        // `avail` (`== true`) instead of re-probing the identical
        // candidate list a second time (F3).
        let use_ok = if stripped.len() == a.len() {
            true
        } else {
            atom_currently_satisfiable(repos, a, config, extra_constraints)
                // #74 S1 (T10): real's `mydbapi_match_pkgs(atom)` in
                // `dep_zapdeps` is `_dep_check_composite_db` over the
                // graph/installed/ebuild dbs, and a *built* package's
                // `[use]` deps are matched against its recorded vdb USE
                // (`dbapi._match_use`'s `not self._use_mutable` branch,
                // `portage/dbapi/__init__.py:298-327`) -- not against
                // the tree candidate's profile-derived USE. Without this
                // disjunct an alternative satisfied by an installed
                // instance whose flag the current profile does not
                // enable (`dev-lang/rust:1.94.1[llvm_slot_21]`,
                // `dev-libs/altprov:1.1[flip]`) failed
                // `all_use_satisfied`, was demoted to `unsat_use_*`,
                // and lost bin 0 to the *uninstalled* slot's cp-level
                // `Installed` -- real keeps the installed alternative
                // (fixture `altprov`/`altconsumer` and the #74 host
                // probe).
                || atom_matches_installed(root, a, config)
                || root_deps_running_root
                    .is_some_and(|running_root| running_root_satisfies_atom(a, running_root, config))
        };
        if !use_ok {
            all_use_satisfied = false;
            // Real's bug-515584 probe: only evaluated for an atom whose
            // own USE-match actually failed (dep_check.py soft 493-523).
            if !atom_all_use_unmasked(repos, a, config, extra_constraints) {
                all_use_unmasked = false;
            }
        }
    }
    if !all_available {
        if circular {
            return portage_use_reduce::AltPreference::Unsatisfiable;
        }
        // Real's `other_installed`/`other_installed_some`/
        // `other_installed_any_slot`/`other` classification
        // (dep_check.py soft 715-724): reached when some atom has no
        // tree candidate at all, even ignoring USE. Blockers are
        // excluded from the fold entirely (real's own `if not
        // atom.blocker:` guard), not merely vacuously satisfied.
        let non_blocker: Vec<&String> = atoms
            .iter()
            .filter(|a| {
                portage_dep::parse_atom(a)
                    .is_some_and(|at| at.blocker == portage_dep::Blocker::None)
            })
            .collect();
        let all_installed = non_blocker
            .iter()
            .all(|a| atom_matches_installed(root, a, config));
        if all_installed {
            return portage_use_reduce::AltPreference::OtherInstalled;
        }
        let some_installed = non_blocker
            .iter()
            .any(|a| atom_matches_installed(root, a, config));
        if some_installed {
            return portage_use_reduce::AltPreference::OtherInstalledSome;
        }
        // Real's own fuzzy cp-level fallback (soft 720-724, bug 522652)
        // -- plain `vardb.match(Atom(atom.cp))`, deliberately NOT
        // `atom_cp_installed` (that helper's `virtual/` zero-cost
        // exemption doesn't apply to this specific real code path).
        let any_cp_installed = non_blocker.iter().any(|a| {
            portage_dep::parse_atom(a)
                .is_some_and(|at| !installed_candidates(root, &at.category, &at.package).is_empty())
        });
        return if any_cp_installed {
            portage_use_reduce::AltPreference::OtherInstalledAnySlot
        } else {
            portage_use_reduce::AltPreference::Other
        };
    }
    // Real demotes an otherwise-`all_available` choice straight to `other`
    // (skipping bin 0/1 entirely) when `conflict_downgrade` or
    // `installed_downgrade` fires (dep_check.py soft 634, bug 531656) --
    // backlog #35, see `alternative_downgrade_demoted`. `entries` is the
    // resolver's live in-progress set at both call sites.
    if alternative_downgrade_demoted(
        repos,
        root,
        config,
        entries,
        self_cp,
        constraints,
        atoms,
        queued,
        update,
    ) {
        return portage_use_reduce::AltPreference::Other;
    }
    // Backlog #221: real's `circular_atom` demotion
    // (`lib/portage/dep/dep_check.py:673-691`, bugs 703440, 705986): on a
    // backtrack restart after a stranded merge-order walk, an alternative
    // with an atom matching one of the resolving parent's recorded
    // in-cycle children ranks `other`, so the re-walk takes the branch
    // that breaks the cycle. Before the USE binning below because real
    // demotes regardless of USE-satisfaction (`if circular_atom is not
    // None: other.append(this_choice)` ahead of the `all_use_satisfied`
    // split). Two exemptions straight from real: blocker atoms never
    // match, and an atom satisfied by an installed version is not a
    // circular dep (`if vardb.match(atom): continue`). The
    // `parent.onlydeps` half of the `circular_atom` site (soft 649-671)
    // stays a documented cut: `--onlydeps` is not threaded to this call
    // depth, and its demotion (avoiding the direct self-cycle for
    // `--onlydeps` arguments) is independent of this backtracking one.
    if let Some(children) = circular_dependency.get(self_cp) {
        let circular_hit = atoms.iter().any(|a| {
            let Some(atom) = portage_dep::parse_atom(a) else {
                return false;
            };
            if atom.blocker != portage_dep::Blocker::None {
                return false;
            }
            if atom_matches_installed(root, a, config) {
                return false;
            }
            children.iter().any(|child| {
                let candidate = circular_child_candidate(child);
                portage_dep::match_from_list(a, &[candidate.as_str()])
                    .is_some_and(|m| !m.is_empty())
            })
        });
        if circular_hit {
            return portage_use_reduce::AltPreference::Other;
        }
    }
    if all_use_satisfied {
        // Real `dep_zapdeps` choice bin 0 -- the single list
        // `preferred_in_graph` / `preferred_installed` / `preferred_any_slot`
        // all alias to when `graph_db` is present. An alternative every
        // non-blocker atom of which is either already installed
        // (`all_installed`, cp-level -- makes `virtual/wine`'s
        // `|| ( wine-vanilla wine-staging … )` pick the installed
        // `wine-staging`) OR already a merge-bound node this run
        // (`all_in_graph`, `[use]`-checked -- makes `virtual/secret-service`
        // pick the KDE-stack-pulled `kwallet-runtime[keyring]` over the
        // first-listed `gnome-keyring`) ranks above one that would need a
        // fresh merge.
        return if atoms.iter().all(|a| atom_cp_installed(root, a))
            || atoms_all_in_graph(atoms, entries, config, queued)
        {
            portage_use_reduce::AltPreference::Installed
        } else {
            portage_use_reduce::AltPreference::Available
        };
    }
    // `!all_use_satisfied`: real's `unsat_use_*` bins -- the alternative
    // exists but can only be merged with USE changes. First the
    // bug-515584 gate (soft 705): if the flags that would have to change
    // sit in `use.mask` / `use.force`, the choice is demoted to plain
    // `other` (dep_check.py soft 705-706 -- no installed-variant
    // sub-classification, unlike the `!all_available` branch above);
    // otherwise it keeps the fine-bin ordering `unsat_use_in_graph` >
    // `unsat_use_installed` > `unsat_use_non_installed`.
    if !all_use_unmasked {
        return portage_use_reduce::AltPreference::Other;
    }
    if atoms_all_in_graph(atoms, entries, config, queued) {
        return portage_use_reduce::AltPreference::UnsatUseInGraph;
    }
    // Real `unsat_use_installed`'s `all_installed_slots` (dep_check.py
    // soft 599-607): cp-level installed is not enough -- the *slot* the
    // best USE-ignoring candidate would pull in must also be installed
    // (F4). A cp installed only in a different slot than this alternative
    // targets still counts as `unsat_use_non_installed`.
    if atoms.iter().all(|a| atom_cp_installed(root, a))
        && atoms
            .iter()
            .all(|a| atom_installed_in_slot_of(root, repos, config, a, constraints_for(a)))
    {
        portage_use_reduce::AltPreference::UnsatUseInstalled
    } else {
        portage_use_reduce::AltPreference::UnsatUseNonInstalled
    }
}

/// Real `mydbapi_match_pkgs(atom.without_use)[-1]`'s `version` field,
/// parsed back out of a `highest_available_candidate_ignoring_use`
/// candidate string (`"{cp}-{version}:{slot}/{sub_slot}::{repo}"`) --
/// the per-atom seed of a `||` alternative's own `cp_map`
/// (`promote_tied_alternative`'s slice-2 in-bin ordering).
pub(crate) fn candidate_version(
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = candidate_str.strip_prefix(&format!("{category}/{package}-"))?;
    rest.split(':').next().map(str::to_string)
}

/// Real `dep_zapdeps`'s in-bin upgrade-preference ordering pass
/// (`dep_check.py` soft 738-802), plugged in as `use_reduce_flat_disjunctive`'s
/// `tie_break` -- called only when 2+ alternatives tie at the best
/// selectable [`AltPreference`](portage_use_reduce::AltPreference) rank
/// (`disjunction_preference`'s own return). Real processes `choices[1:]`
/// (by object identity, in original left-to-right order) against the
/// CURRENT, being-mutated bin list, promoting `choice_1` in front of the
/// first `choice_2` it beats and stopping the moment it would reach its
/// own current position unpromoted; ported as a literal remove+insert
/// over an `id`-tagged `Vec` (not simplified to a stable sort -- the
/// promotion isn't transitive/associative in general, which is the
/// whole point of real's own "sorting is done separately for each bin,
/// so as not to interfere with the ordering ... specified in the
/// ebuild" comment) so this matches real byte-for-byte rather than an
/// approximation of it. Returns the WINNING alternative's original
/// index into `alts` (`tie_break`'s own contract).
///
/// Two of real's per-choice facts are deliberate, documented cuts (§2 of
/// the design brief, `docs/history/022-agent-task-22-zapdeps.fable.md`):
/// `minimize_slots` (only applies after `_overlap_dnf`'s DNF rewrite,
/// which portuale doesn't do -- skipped entirely, matching the brief's
/// own "cut it and document" fallback for slice 2) and `want_update`
/// (`graph_interface.want_update_pkg`, needing the `--update`/`--newuse`
/// "wanted" set threaded down to this call depth, which portuale
/// doesn't do) -- pinned to real's own default (`False`), which only
/// ever WIDENS when the `all_installed_slots` promotion applies (a true
/// `want_update` on `choice_2` would SUPPRESS it): never picks a
/// DIFFERENT winner than real would for a fixture where `want_update` is
/// genuinely false, only potentially a fixture real would need
/// `want_update` to decide -- none of which are targeted here.
///
/// Also deliberately simplified vs `dep_check.py`'s own `cp_map`
/// construction (soft 552-568, "bug 600346"): when an alternative's OWN
/// atoms name the SAME `cp` more than once, real re-checks whether the
/// higher-version candidate still satisfies every atom that already
/// matched the previous one before overwriting `cp_map[cp]` (internal
/// consistency); this always just keeps the higher version. A narrow
/// cut -- it only differs from real when a single `||` alternative
/// contains two atoms on the identical package where the higher version
/// satisfies one atom but not the other, which the brief's own slice-2
/// fixtures don't exercise -- documented in `scope-backlog.md`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn promote_tied_alternative(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    root: &Path,
    entries: &[GraphEntry],
    constraints: &HashMap<(String, String), Vec<String>>,
    alts: &[Vec<String>],
    queued: &[QueueItem],
) -> usize {
    struct Choice {
        id: usize,
        all_installed_slots: bool,
        all_in_graph: bool,
        cp_map: HashMap<String, String>,
    }
    let constraints_for = |a: &str| -> &[String] {
        portage_dep::parse_atom(a)
            .and_then(|at| constraints.get(&(at.category, at.package)))
            .map_or(&[][..], Vec::as_slice)
    };
    let mut choices: Vec<Choice> = alts
        .iter()
        .enumerate()
        .map(|(id, atoms)| {
            let mut cp_map: HashMap<String, String> = HashMap::new();
            for a in atoms {
                let Some(atom) = portage_dep::parse_atom(a) else {
                    continue;
                };
                if atom.blocker != portage_dep::Blocker::None {
                    continue;
                }
                let cp = format!("{}/{}", atom.category, atom.package);
                if let Some((candidate_str, .., _slot)) = highest_available_candidate_ignoring_use(
                    HighestAvailableCandidateIgnoringUseArgs {
                        repos,
                        atom_str: a,
                        category: &atom.category,
                        package: &atom.package,
                        config,
                        extra_constraints: constraints_for(a),
                    },
                ) && let Some(version) =
                    candidate_version(&candidate_str, &atom.category, &atom.package)
                {
                    let better = cp_map
                        .get(&cp)
                        .is_none_or(|existing| vercmp_ordering(&version, existing).is_gt());
                    if better {
                        cp_map.insert(cp, version);
                    }
                }
            }
            Choice {
                id,
                all_installed_slots: atoms.iter().all(|a| atom_cp_installed(root, a))
                    && atoms.iter().all(|a| {
                        atom_installed_in_slot_of(root, repos, config, a, constraints_for(a))
                    }),
                all_in_graph: atoms_all_in_graph(atoms, entries, config, queued),
                cp_map,
            }
        })
        .collect();

    if choices.len() < 2 {
        return 0;
    }
    // Real `for choice_1 in choices[1:]:` -- a snapshot of every choice
    // BUT the first, by identity, taken before any mutation.
    let snapshot_ids: Vec<usize> = choices[1..].iter().map(|c| c.id).collect();
    for id1 in snapshot_ids {
        let Some(pos1) = choices.iter().position(|c| c.id == id1) else {
            continue;
        };
        let choice1_all_installed_slots = choices[pos1].all_installed_slots;
        let choice1_all_in_graph = choices[pos1].all_in_graph;
        let choice1_cp_map = choices[pos1].cp_map.clone();
        let mut promote_before: Option<usize> = None;
        for (pos2, choice2) in choices.iter().enumerate() {
            if choice2.id == id1 {
                // Reached choice_1's own current position without a
                // promotion firing -- real's `if choice_1 is choice_2: break`.
                break;
            }
            if choice1_all_installed_slots && !choice2.all_installed_slots {
                promote_before = Some(pos2);
                break;
            }
            let mut has_upgrade = false;
            let mut has_downgrade = false;
            for (cp, v1) in &choice1_cp_map {
                if let Some(v2) = choice2.cp_map.get(cp) {
                    if vercmp_ordering(v1, v2).is_gt() {
                        has_upgrade = true;
                    } else if vercmp_ordering(v1, v2).is_lt() {
                        has_downgrade = true;
                    }
                }
            }
            if (has_upgrade && !has_downgrade)
                || (choice1_all_in_graph
                    && !choice2.all_in_graph
                    && !(has_downgrade && !has_upgrade))
            {
                promote_before = Some(pos2);
                break;
            }
        }
        if let Some(pos2) = promote_before {
            // `pos2 < pos1` always (the scan breaks the moment it would
            // reach `pos1`), so removing `pos1` first never shifts `pos2`.
            let choice1 = choices.remove(pos1);
            choices.insert(pos2, choice1);
        }
    }
    choices[0].id
}
