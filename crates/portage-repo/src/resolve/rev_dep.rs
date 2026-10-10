//! Constraints that installed reverse dependencies put on an upgrade.

use crate::*;

/// Real `_slot_operator_check_reverse_dependencies`'s own atom
/// normalisation, applied to one atom an *installed* consumer records
/// against a package this run wants to upgrade.
///
/// Real keeps the recorded atom as written, with one exception: a parent
/// whose atom is a **built** slot-operator atom (`cat/pkg:S/SS=` --
/// operator `=` with both slot and sub-slot present, real
/// `Atom.slot_operator_built`) "may need to be rebuilt, therefore
/// discard its soname and built slot operator dependency components
/// which are not necessarily relevant" (`depgraph.py:2494-2502`), which
/// it does with `atom.with_slot("=")` -- keeping the version bound and
/// dropping the slot/sub-slot. That distinction is the whole difference
/// between "this upgrade needs a consumer rebuild" (fine, and what
/// `slot_operator_rebuild_entries` already schedules) and "this upgrade
/// breaks a consumer outright" (not fine).
///
/// Portuale drops the slot component textually rather than rewriting it
/// to `:=`, which is equivalent for matching: `portage_dep`'s matcher
/// ignores a bare `:=` (no slot named) exactly as real does. `[use-dep]`
/// components are dropped too -- real evaluates them against the
/// candidate's own USE, which this vdb-only check has no resolved USE
/// for; a documented cut, and one that can only *allow* an upgrade real
/// would block, never the reverse.
/// A *built* slot-operator atom (`cat/pkg:S/SS=`, operator `=`, both
/// slot and sub-slot present, real `Atom.slot_operator_built`) -- the
/// shape real's `_slot_operator_check_reverse_dependencies` normalises.
pub(crate) fn is_built_slot_op(atom: &portage_dep::Atom) -> bool {
    atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
        && atom.slot.is_some()
        && atom.sub_slot.is_some()
}

/// Backlog #286: whether a newly recorded miss atom supersedes the one
/// already disclosed for the same `cat/pkg`. A live slot-operator atom
/// (`:=` family without a built `:S/SS=` binding) wins over a recorded
/// built one: real's fail-fast walk evaluates an installed parent's live
/// dep first (dynamic deps prepend it to the depstring --
/// `FakeVartree._apply_dynamic_deps`, confirmed on real's own
/// `--debug` for the #276 shape: the installed parent's depstring is
/// `<u276prov-2:= >=u276prov-1:0/1=` and the missing-dependency
/// backtrack fires on the live atom, never the built one), so the built
/// edge never reports when the live edge already failed. Portuale's BFS
/// evaluates both and records first-wins, which picks the built atom.
/// Scoped to exactly that pair (built recorded, live candidate) -- every
/// other combination keeps first-wins, matching real's first-failed-edge
/// report wherever only one shape exists.
pub(crate) fn live_miss_atom_supersedes(recorded: &str, candidate: &str) -> bool {
    let recorded_built = portage_dep::parse_atom(recorded).is_some_and(|a| is_built_slot_op(&a));
    if !recorded_built {
        return false;
    }
    if let Some(new) = portage_dep::parse_atom(candidate) {
        return new.slot_operator.is_some() && !is_built_slot_op(&new);
    }
    // The bare slot-operator suffix form (`<cat/pkg-2:=`: an operator
    // with no slot) never parses in `portage_dep` (its slot part hangs
    // off a `:`), but real `Atom` accepts it -- and it is always live
    // (no `:S/SS` binding to be built). Strip a trailing `[use]` group
    // first: use-deps never affect the operator shape.
    let bare = match candidate.rfind('[') {
        Some(i) if candidate.ends_with(']') => &candidate[..i],
        _ => candidate,
    };
    bare.ends_with(":=")
}

pub(crate) fn reverse_dep_constraint_atom(atom_str: &str, atom: &portage_dep::Atom) -> String {
    let head = atom_str.split('[').next().unwrap_or(atom_str);
    if is_built_slot_op(atom) {
        head.split(':').next().unwrap_or(head).to_string()
    } else {
        head.to_string()
    }
}

/// Real `Atom.with_slot("=")` as `_slot_operator_check_reverse_-
/// dependencies` applies it (`lib/_emerge/depgraph.py:2494-2502`): a
/// parent whose atom is a **built** slot-operator atom "may need to be
/// rebuilt, therefore discard its soname and built slot operator
/// dependency components" -- the recorded `:S/SS=` binding is relaxed
/// to bare `:=`, keeping the version operator, any `[use]` deps and
/// any `::repo`. The relaxed atom schedules a consumer rebuild, never
/// a withhold: `>=dev-libs/provpkg-1.0:0/1=` still accepts the
/// `0/2`-slot `provpkg-2.0` candidate, while `<dev-libs/r25lib-2.0:0/1=`
/// still rejects `r25lib-2.0`.
///
/// Unlike [`reverse_dep_constraint_atom`] (which drops the `=` as well
/// as the slot, equivalently, for *matching*), this keeps the operator
/// so the result stays a faithful `with_slot("=")` rendering.
/// A non-built or unparseable atom returns verbatim.
pub(crate) fn relax_built_slot_operator_atom(atom_str: &str) -> String {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return atom_str.to_string();
    };
    if !is_built_slot_op(&atom) {
        return atom_str.to_string();
    }
    // `is_built_slot_op` guarantees both are present; the `:S/SS=`
    // needle is parser-derived, so it occurs verbatim (slot names are
    // ASCII, and no `[use]`/`::repo` tail can contain that shape).
    let needle = format!(
        ":{}/{}=",
        atom.slot.as_deref().unwrap_or_default(),
        atom.sub_slot.as_deref().unwrap_or_default()
    );
    match atom_str.find(&needle) {
        Some(idx) => format!("{}:={}", &atom_str[..idx], &atom_str[idx + needle.len()..]),
        None => atom_str.to_string(),
    }
}

/// Whether an installed-settling edge's atom is a slot-operator rebuild
/// trigger rather than a genuine slot conflict: a **built**
/// slot-operator atom (`cat/pkg:S/SS=`, real `Atom.slot_operator_built`)
/// that accepts the already-resolved merge instance once relaxed the
/// way real's update probe relaxes it (`relax_built_slot_operator_atom`
/// -- real `Atom.with_slot("=")`,
/// `lib/_emerge/depgraph.py:2494-2502`).
///
/// Real's `_minimize_children` (`lib/_emerge/depgraph.py:4751-4840`)
/// never lets such an atom contend the selection against the probe's
/// upgrade -- both atoms collapse onto the installed instance first,
/// and the update probe then replaces it (scheduling consumer
/// rebuilds) exactly when this predicate holds. Recording it as a slot
/// conflict instead feeds the direct solve a removal verdict real never
/// renders (the slotop `@world` cells: no `provpkg-2.0`, no consumer
/// `rR` rows). A version veto (`<dev-libs/r25lib-2.0:0/1=`, the r25
/// consumer pin) still mismatches the merge under the relaxed form, so
/// that conflict records normally and the withhold survives.
/// An unparseable atom is never a trigger (fall back to recording,
/// today's behaviour).
pub(crate) fn built_slot_operator_rebuild_trigger(atom_str: &str, existing_str: &str) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if !is_built_slot_op(&atom) {
        return false;
    }
    let relaxed = relax_built_slot_operator_atom(atom_str);
    portage_dep::match_from_list(&relaxed, &[existing_str]).is_some_and(|m| !m.is_empty())
}

/// One installed consumer's recorded atom broken by this pass's
/// upgrade, with the consumer's identity attached -- real's complete
/// graph carries that consumer as a nomerge node (`_add_pkg` of the
/// installed package); portuale carries the `(cp, atom, consumer)`
/// triple instead, which is everything the backtrack loop and the
/// residual-conflict report below need from the node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RevDepPin {
    /// The upgraded `category/package`.
    pub(crate) cp: (String, String),
    /// The consumer's recorded atom, normalised by
    /// `reverse_dep_constraint_atom`.
    pub(crate) atom: String,
    /// The verbatim recorded atom text (with `[use]` deps), for the
    /// skip-conflict notice only -- matching always uses `atom`.
    pub(crate) raw_atom: String,
    /// The installed consumer `(category, package, version)`.
    pub(crate) consumer: (String, String, String),
}

/// Arguments of [`rev_dep_pin_holdable`].
pub(crate) struct RevDepPinHoldableArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) cp: &'a (String, String),
    pub(crate) pin: &'a str,
    pub(crate) pin_slot: Option<&'a str>,
    pub(crate) upgraded_slots: &'a std::collections::HashSet<String>,
    pub(crate) hard_atoms: &'a [String],
}

/// Whether `pin` (a consumer atom on `cp`) can still hold alongside
/// every hard requirement pulling `cp` this pass -- real's
/// `_select_pkg_highest_available` seeing "the whole atom set for a
/// package, not just the first" (`depgraph.py`), with the installed
/// consumer's own atom among them.
///
/// A pin that is jointly unsatisfiable with the hard pullers is DROPPED,
/// not enforced: real merges the hard-required version anyway and
/// reports the broken consumer as a residual slot collision against the
/// installed instance (exit 1), instead of masking the version into
/// invisibility. Live shape (verified against real 3.0.82.2): an
/// installed `keeper-1.0` recording `=paired-1.0` while the run merges
/// `paired-2.0` for an explicit `=paired-2.0` argument or a
/// `>=paired-2.0` dependency -- real prints `U paired-2.0` plus the
/// conflict block pairing the merge instance against the installed
/// `paired-1.0` pulled in by keeper; portuale used to mask `2.0` away
/// entirely (`!!! no visible ebuild`, or a fatal top-level failure).
///
/// The check runs per upgraded slot: a pin (and each hard atom) applies
/// to a slot group only when its own slot is unset or equal, so a pin on
/// one slot never vetoes an upgrade in another. Anything unparseable, or
/// a `cp` with no readable candidates at all, falls back to enforcing
/// (today's behaviour) -- a drop happens only on proven joint
/// unsatisfiability.
pub(crate) fn rev_dep_pin_holdable(args: RevDepPinHoldableArgs<'_>) -> bool {
    let RevDepPinHoldableArgs {
        root,
        repos,
        cp,
        pin,
        pin_slot,
        upgraded_slots,
        hard_atoms,
    } = args;
    if portage_dep::parse_atom(pin).is_none() {
        return true;
    }
    // A hard atom that cannot even be parsed cannot constrain anything;
    // fall back to enforcing rather than reasoning about it.
    let mut hard_parsed: Vec<(&str, portage_dep::Atom)> = Vec::new();
    for h in hard_atoms {
        match portage_dep::parse_atom(h) {
            Some(a) => hard_parsed.push((h.as_str(), a)),
            None => return true,
        }
    }
    let Ok(cands) = list_candidates(repos, &cp.0, &cp.1) else {
        return true;
    };
    // Slot groups the pin actually constrains that this pass upgrades.
    let constrained: Vec<&String> = upgraded_slots
        .iter()
        .filter(|s| pin_slot.is_none_or(|ps| ps == *s))
        .collect();
    if constrained.is_empty() {
        return true;
    }
    // The pin holds iff some constrained slot still admits a candidate
    // satisfying the pin together with every slot-applicable hard atom.
    //
    // Backlog #210: withholding keeps the installed instance, so the
    // installed instances count as candidates too -- a same-version
    // slot-change reinstall has no *repo* candidate at the old slot
    // (only the installed instance still satisfies the pin), while an
    // upgrade always does (the old version stays visible). Without this
    // every slot-change pin would drop for the residual report instead
    // of withholding.
    let holds_for = |slot: &String, cands: &[String]| {
        cands.iter().any(|s| {
            let refs = [s.as_str()];
            if portage_dep::match_from_list(pin, &refs).is_none_or(|m| m.is_empty()) {
                return false;
            }
            hard_parsed.iter().all(|(text, h)| {
                if h.slot.is_some() && h.slot.as_deref() != Some(slot.as_str()) {
                    return true;
                }
                portage_dep::match_from_list(text, &refs).is_some_and(|m| !m.is_empty())
            })
        })
    };
    constrained.iter().any(|slot| {
        // Repo candidates at this slot.
        let repo_cands: Vec<String> = cands
            .iter()
            .filter(|c| &c.slot == *slot)
            .map(|c| {
                format!(
                    "{}/{}-{}:{}/{}::{}",
                    cp.0, cp.1, c.version, c.slot, c.sub_slot, c.repo_name
                )
            })
            .collect();
        if holds_for(slot, &repo_cands) {
            return true;
        }
        // Installed instances at this slot (withholding keeps them).
        let inst_cands: Vec<String> = all_installed_packages(root)
            .iter()
            .filter(|p| p.category == cp.0 && p.package == cp.1)
            .filter_map(|p| {
                let (inst_slot, inst_sub) =
                    read_vdb_slot(root, &p.category, &p.package, &p.version);
                (inst_slot == **slot).then(|| {
                    format!(
                        "{}/{}-{}:{}/{}::{}",
                        p.category,
                        p.package,
                        p.version,
                        inst_slot,
                        inst_sub,
                        installed_pkg_repo(root, &p.category, &p.package, &p.version)
                    )
                })
            })
            .collect();
        holds_for(slot, &inst_cands)
    })
}

/// Backlog #91 (S1): the dep-key axis of the reverse-dependency pin
/// scan. Real empties (or marks `optional`) the *build-time* keys of a
/// built package (`DEPEND`/`BDEPEND`, `depgraph.py:4194-4247`) while the
/// runtime keys (`RDEPEND`/`IDEPEND`/`PDEPEND`) always form hard parent
/// edges -- so only a build-time pin is unconditionally ignorable (D1);
/// a runtime pin from an in-graph consumer withholds (host qemu/edk2,
/// hermetic `whblocker` cell).
pub(crate) fn is_build_time_dep_key(key: impl AsRef<str>) -> bool {
    matches!(key.as_ref(), "DEPEND" | "BDEPEND")
}

/// Real `depgraph._complete_graph` reaching
/// `_slot_operator_check_reverse_dependencies`: the dependency atoms an
/// **installed** package records against a package this run wants to
/// upgrade, which that upgrade would break.
///
/// A pin that no candidate can satisfy together with this pass's hard
/// requirements (`hard_want`: every atom text that targeted the package,
/// top-level arguments included) is NOT enforced -- it is returned in
/// `dropped` instead, for the residual installed-instance conflict
/// report (real merges the hard-required version and reports the broken
/// consumer against the installed instance, exit 1). See
/// `rev_dep_pin_holdable` for the joint-satisfiability rule and its
/// live grounding.
///
/// Real's graph does not stop at the packages its arguments reach. Once
/// any installed package would change version, slot or USE,
/// `_complete_graph` auto-enables complete mode
/// (`depgraph.py:8592-8648`) and re-seeds the walk from
/// `@world`/`@selected`/`@system` (`_initial_arg_list` plus the required
/// sets, `depgraph.py:8677-8731`), pulling every installed package
/// **reachable from those seeds over the installed dependency graph**
/// in as nomerge nodes -- not the whole installed universe (#54:
/// `keeper`'s `=paired-1.0` pin is outside every target closure in the
/// hermetic fixture and real never walks to it there; it only shows up
/// when a competing hard requirement keeps `paired`'s installed instance
/// itself in the graph, or when `keeper` is reachable from `@world`).
/// Each reachable consumer contributes its recorded `*DEPEND` atoms to
/// `_parent_atoms`, and a candidate has to satisfy every parent atom on
/// its package -- so an installed consumer nowhere near the requested
/// atom's own dependency tree can still veto an upgrade, as long as it
/// is in that reachable closure. Live example this exists for:
/// `net-libs/rest` does not reach `dev-libs/weston` through the
/// requested atom's own tree, but both are `@world`-reachable, and
/// weston's recorded `<media-libs/libdisplay-info-0.4.0:0/3=` is why
/// real leaves `media-libs/libdisplay-info` at `0.3.0` while portuale
/// used to upgrade it to `0.4.0`.
///
/// Portuale reaches the same answer without carrying the extra ~1400
/// installed nodes: it scans the vdb for the consumers of each
/// upgrade-bound `cat/pkg` directly, keeps only the ones `reachable`
/// names or that are themselves a graph entry this pass (real's "or is a
/// dep of a node already in the graph" case), and hands the surviving
/// atoms back to the `'backtrack` loop as ordinary `slot_constraints` --
/// which is exactly what those parent atoms are. The re-resolve then
/// picks the highest candidate that satisfies them (here: the installed
/// `0.3.0`, so the entry settles as `AlreadyInstalled`), and drops
/// whatever the rejected version had dragged in. `reachable` is
/// `ResolveCtx::slot_op_reachable` (`required_set_reachable_cps` over
/// `@world ∪ @selected ∪ @system ∪` this run's own argument cps) -- the
/// same reachable closure that gates the slot-operator-rebuild scan
/// (`slot_operator_rebuild_scan`), computed once per resolve, not a
/// second walker.
///
/// Only `Upgrade`/`Downgrade` entries are checked -- those are the only
/// ones that move a package's version out from under an existing
/// consumer -- plus a `Reinstall` whose slot or sub-slot changed without
/// a revbump (backlog #210): real `_complete_graph` treats that shape as
/// a version change (`depgraph.py:8611-8618`, "slot/sub-slot change
/// without revbump gets similar treatment to a version change") and
/// re-seeds the required sets so the consumers' recorded atoms constrain
/// selection. A `New` into a fresh slot leaves the old slot's consumers
/// alone, and any other `Reinstall` keeps the version.
///
/// Consumers real would not enforce are skipped: one whose own `cat/pkg`
/// this run is already replacing (real's "this parent may need to be
/// eliminated due to a slot conflict, so its dependencies aren't
/// necessarily relevant", `depgraph.py:2512-2522`), one this run
/// uninstalls to solve a blocker conflict (real's `parent.installed and
/// self._in_blocker_conflict(parent)`, bug 612772 -- its pin cannot
/// veto the upgrade whose blocker removes it), and one matched by
/// `--exclude`. Real's further `_upgrade_available` / direct-cycle
/// escapes sit behind `if not self._too_deep(parent.depth)`, and a
/// consumer that only entered via `_complete_graph` carries
/// `_UNREACHABLE_DEPTH`, for which `_too_deep` is unconditionally true
/// (`depgraph.py:7369`) -- so for exactly the consumers this function
/// finds, real skips those escapes too.
///
/// A top-level argument that settles `AlreadyInstalled` is never a
/// source (backlog #210): it is a no-op request, not a digraph node in
/// real, so its recorded atoms never become parent atoms -- even though
/// it carries a graph entry here (`graph_cps`) and complete-mode
/// arg-seeding pulls it into `reachable`. A consumer the required sets
/// (`@world ∪ @selected ∪ @system`) reach stays a source, whether or not
/// it was also requested.
///
/// Returns `(cat/pkg, atom)` pairs to add to `slot_constraints`; empty
/// (and the vdb scan skipped entirely) when nothing is being upgraded.
///
/// Returns `(enforced, dropped)` pin lists, each sorted and deduped by
/// `(cp, atom, consumer)`. Enforced pins feed `slot_constraints` exactly
/// as before; dropped pins (jointly unsatisfiable with this pass's hard
/// requirements -- see `rev_dep_pin_holdable`) feed the residual
/// installed-instance conflict report instead of masking the
/// hard-required version into invisibility.
/// A same-version USE-change reinstall's decided merge state, for the
/// USE-break check below: the candidate string this run installs (same
/// shape as `reverse_dependency_constraints`' own `upgrading` values),
/// plus the merge's own effective USE set and declared IUSE -- real's
/// `violated_conditionals(child_use, parent_use)` needs both sides.
pub(crate) struct UseBreakSource {
    pub(crate) candidate: String,
    pub(crate) merge_use: HashSet<String>,
    pub(crate) merge_iuse: HashSet<String>,
}

/// The decided merge USE/IUSE for `category/package-version`: the
/// candidate's own effective flags (`candidate_iuse_and_use` -- the
/// profile-resolved set this run merges with, or the baked set for a
/// binary). `None` when no readable candidate exists (an unreadable
/// md5-cache entry); callers fall back to ignoring USE, today's
/// behaviour.
pub(crate) fn merge_use_state(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(HashSet<String>, HashSet<String>)> {
    let cands = list_candidates(repos, category, package).ok()?;
    let cand = cands.iter().find(|c| c.version == version)?;
    let (iuse, use_flags) = candidate_iuse_and_use(cand, category, package, config)?;
    Some(((*use_flags).clone(), iuse))
}

/// Arguments of [`merge_use_changed_vs_installed`].
pub(crate) struct MergeUseChangedVsInstalledArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
}

/// Real `_complete_graph`'s own USE-change trigger
/// (`depgraph.py:8620-8637`): the merge node's IUSE, or its enabled USE
/// intersected with IUSE, differs from the installed instance's. This is
/// a node-vs-vdb comparison real makes unconditionally -- it does NOT go
/// through `--newuse`/`--changed-use` (a bare, reasonless `[ebuild R]`
/// for an explicitly-requested installed package carries no
/// `changed_flags` yet still flips USE and still enables complete
/// mode). `None` when the merge side is unreadable (no md5-cache
/// entry); callers fall back to the entry's own `changed_flags`,
/// today's behaviour.
pub(crate) fn merge_use_changed_vs_installed(
    args: MergeUseChangedVsInstalledArgs<'_>,
) -> Option<bool> {
    let MergeUseChangedVsInstalledArgs {
        root,
        repos,
        config,
        category,
        package,
        version,
    } = args;
    let (merge_use, merge_iuse) = merge_use_state(repos, config, category, package, version)?;
    let inst_use = read_vdb_flag_set(root, category, package, version, "USE");
    let inst_iuse = read_vdb_flag_set(root, category, package, version, "IUSE");
    if merge_iuse != inst_iuse {
        return Some(true);
    }
    let merge_enabled: HashSet<&String> = merge_use.intersection(&merge_iuse).collect();
    let inst_enabled: HashSet<&String> = inst_use.intersection(&inst_iuse).collect();
    Some(merge_enabled != inst_enabled)
}
/// Arguments of [`reverse_dependency_constraints`].
pub(crate) struct ReverseDependencyConstraintsArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) hard_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) dynamic_deps: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) world_reachable: &'a HashSet<(String, String)>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) complete: bool,
}

pub(crate) fn reverse_dependency_constraints(
    args: ReverseDependencyConstraintsArgs<'_>,
) -> (Vec<RevDepPin>, Vec<RevDepPin>) {
    let ReverseDependencyConstraintsArgs {
        repos,
        root,
        entries,
        with_bdeps,
        excluded,
        hard_want,
        reachable,
        dynamic_deps,
        ignore_built_slot_operator_deps,
        top_level_cps,
        world_reachable,
        config,
        complete,
    } = args;
    // (`cat/pkg`, slot) -> (the candidate string this run would install,
    // whether it comes from a same-version slot-change reinstall) for
    // every entry that replaces an installed version in its own slot.
    let mut upgrading: HashMap<((String, String), String), (String, bool)> = HashMap::new();
    let mut being_replaced: HashSet<(String, String)> = HashSet::new();
    // Backlog #210: every `cat/pkg` this pass leaves installed that was
    // directly requested -- a no-op top-level argument, not a digraph
    // node in real (see the function doc comment).
    let mut already_installed_top_level: HashSet<(String, String)> = HashSet::new();
    // Backlog #91 (S1): consumers this pass uninstalls (bug 612772 --
    // real skips a parent that `is installed and _in_blocker_conflict`,
    // since it will be uninstalled to solve the conflict). A removal
    // entry lands in `entries` through the blocker phase before this
    // scan runs, which would otherwise make the removed consumer look
    // "walked" (`graph_cps`) and let its runtime pin veto the very
    // upgrade whose blocker removes it (the `whpuller`-only twin:
    // `whtarget-1.0`'s `Uninstall` entry vs its `~whblocker-1.0` pin).
    let mut being_removed: HashSet<(String, String)> = HashSet::new();
    // Every `cat/pkg` this pass walked, any outcome -- real's "or is a dep
    // of a node already in the graph" reachability path (`_complete_graph`
    // deep-walks the required sets, but a package the requested atoms'
    // own tree already pulled in is a graph node regardless of that walk).
    let mut graph_cps: HashSet<(String, String)> = HashSet::new();
    // Backlog #222: (`cat/pkg`, slot) -> decided merge state for every
    // same-version USE-change reinstall -- a `Reinstall` whose version
    // never moves, so the stripped-constraint check below always holds,
    // but whose USE flip can still break an installed consumer's `[use]`
    // bound (upstream `test_complete_graph.py` pg0: the `icu` flip
    // breaks installed `cgp0q`'s `[-icu]` dep). Real fails exactly that
    // shape in its complete-mode end-of-walk unsatisfied-dep loop
    // (`depgraph.py:8751-8791`), having enabled complete mode on the USE
    // change (`complete_if_new_use`, `:8620-8637` -- portuale's
    // `complete_graph_auto_enable` already re-resolves with
    // `complete = true` for it, so these sources are collected only in
    // complete mode). Upgrade/Downgrade/slot-change entries are
    // deliberately NOT sources: their version machinery owns the
    // break, and a USE interplay there (the #91 qemu shapes) belongs to
    // the slot-operator probe's rebuild domain, not to a failure pin.
    let mut use_breaks: HashMap<((String, String), String), UseBreakSource> = HashMap::new();
    for e in entries {
        let cp = (e.category.clone(), e.package.clone());
        graph_cps.insert(cp.clone());
        if merge_bound_cpv(e).is_some() {
            being_replaced.insert(cp.clone());
        }
        if matches!(e.outcome, PretendOutcome::Uninstall { .. }) {
            being_removed.insert(cp.clone());
        }
        if matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. })
            && top_level_cps.contains(&cp)
        {
            already_installed_top_level.insert(cp.clone());
        }
        if complete
            && let PretendOutcome::Reinstall {
                version,
                slot_changed: false,
                ..
            } = &e.outcome
            && let Some((merge_use, merge_iuse)) =
                merge_use_state(repos, config, &e.category, &e.package, version)
            && merge_use_changed_vs_installed(MergeUseChangedVsInstalledArgs {
                root,
                repos,
                config,
                category: &e.category,
                package: &e.package,
                version,
            })
            .unwrap_or(false)
        {
            let slot = e.slot.clone().unwrap_or_else(|| "0".to_string());
            use_breaks.insert(
                (cp.clone(), slot.clone()),
                UseBreakSource {
                    candidate: format!(
                        "{}/{}-{version}:{slot}/{}::{}",
                        e.category,
                        e.package,
                        e.sub_slot.as_deref().unwrap_or("0"),
                        e.repo_name.as_deref().unwrap_or_default()
                    ),
                    merge_use,
                    merge_iuse,
                },
            );
        }
        let (to, reinstall_slot_change) = match &e.outcome {
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                (to, false)
            }
            PretendOutcome::Reinstall {
                version,
                slot_changed: true,
                ..
            } => (version, true),
            _ => continue,
        };
        upgrading.insert(
            (cp, e.slot.clone().unwrap_or_else(|| "0".to_string())),
            (
                format!(
                    "{}/{}-{to}:{}/{}::{}",
                    e.category,
                    e.package,
                    e.slot.as_deref().unwrap_or("0"),
                    e.sub_slot.as_deref().unwrap_or("0"),
                    e.repo_name.as_deref().unwrap_or_default()
                ),
                reinstall_slot_change,
            ),
        );
    }
    if upgrading.is_empty() && use_breaks.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // Real `_add_pkg_dep_string` empties `DEPEND`/`BDEPEND` for a *built*
    // package (an installed one always is) unless `--with-bdeps` asks for
    // them, so those keys contribute no parent atom either.
    let dep_keys: &[&str] = if with_bdeps {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND"]
    };

    static EMPTY_WANT: Vec<String> = Vec::new();
    let mut enforced: Vec<RevDepPin> = Vec::new();
    let mut dropped: Vec<RevDepPin> = Vec::new();
    // (constrained cp, pin atom, pinning consumer) already emitted this
    // pass.
    type SeenPin = ((String, String), String, (String, String, String));
    let mut seen: HashSet<SeenPin> = HashSet::new();
    // Per-pass `Effective`-layer cache, mirroring real's once-per-instance
    // `FakeVartree` overlay (`installed_dep_string`'s own doc comment).
    let mut meta_memo: HashMap<(String, String, String, String), String> = HashMap::new();
    for consumer in all_installed_packages(root) {
        let consumer_cp = (consumer.category.clone(), consumer.package.clone());
        if being_replaced.contains(&consumer_cp) || being_removed.contains(&consumer_cp) {
            continue;
        }
        // #54: real only ever sees this consumer if `_complete_graph`'s
        // required-set walk reaches it, or it is itself a dep of a node
        // already in the graph -- not merely because it is installed.
        if !reachable.contains(&consumer_cp) && !graph_cps.contains(&consumer_cp) {
            continue;
        }
        // Backlog #210: a no-op top-level argument settles
        // `AlreadyInstalled` -- not a digraph node in real, so never a
        // source -- unless the required sets reach it too (a world
        // member stays a source even when also requested).
        if already_installed_top_level.contains(&consumer_cp)
            && !world_reachable.contains(&consumer_cp)
        {
            continue;
        }
        let consumer_str = format!(
            "{}/{}-{}",
            consumer.category, consumer.package, consumer.version
        );
        if excluded.iter().any(|ex| {
            matches_config_entry(ex, &consumer_str, &consumer.category, &consumer.package)
        }) {
            continue;
        }
        let use_flags = read_vdb_flag_set(
            root,
            &consumer.category,
            &consumer.package,
            &consumer.version,
            "USE",
        );
        // Backlog #91 (S1): real walks an installed consumer's
        // *effective* deps -- the live ebuild metadata (plus the vdb's
        // own built `:=` atoms, backlog #25 S1b) when the installed
        // version is still in the tree
        // (`FakeVartree._apply_dynamic_deps`, bug #368725), the raw vdb
        // record when it is gone or EAPI-gated out -- so a pin the live
        // ebuild no longer carries is invisible to real (the #79 `revdepconsumer`
        // shape: vdb `RDEPEND="<revdeptarget-2.0"`, live ebuild empty,
        // real upgrades). Same layer choice as
        // `collect_unwalked_installed_blockers` (#77).
        let live = live_metadata_for_installed(
            repos,
            root,
            &consumer.category,
            &consumer.package,
            &consumer.version,
        );
        for dep_key in dep_keys {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            let depstr = installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category: &consumer.category,
                package: &consumer.package,
                version: &consumer.version,
                live: live.as_deref(),
                key: dep_key,
                layer,
                memo: &mut meta_memo,
            });
            if depstr.trim().is_empty() {
                continue;
            }
            let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            for atom_str in atoms {
                let Some(atom) = portage_dep::parse_atom(&atom_str) else {
                    continue;
                };
                if atom.blocker != portage_dep::Blocker::None {
                    continue;
                }
                let cp = (atom.category.clone(), atom.package.clone());
                let built_slot_op = is_built_slot_op(&atom);
                let constraint = reverse_dep_constraint_atom(&atom_str, &atom);
                // The pin fires when the upgrade in some slot breaks it;
                // collect every failing slot for the per-slot holdability
                // check below.
                //
                // Backlog #210: a same-version slot-change reinstall
                // re-checks the recorded atom verbatim -- real's
                // `_complete_graph` re-seed constrains
                // `_select_pkg_highest_available` with the required sets'
                // atoms as-is (`depgraph.py:8677-8754`), built `:S/SS=`
                // bindings included. The Upgrade/Downgrade path keeps the
                // stripped constraint (the update probe's rule,
                // `_slot_operator_check_reverse_dependencies`,
                // `depgraph.py:2494-2502`, backlog #24 S5: a built binding
                // is a rebuild trigger there, never a withhold).
                let verbatim_head = atom_str.split('[').next().unwrap_or(atom_str.as_str());
                let mut failing: Vec<(String, bool)> = Vec::new();
                for ((ucp, slot), (candidate, reinstall_slot_change)) in &upgrading {
                    if *ucp != cp {
                        continue;
                    }
                    // #296: an atom with an explicit slot constrains that
                    // slot's instance only; an upgrade in another slot
                    // leaves it standing (`bdhold`'s `bdprov:0` is not
                    // broken by `bdprov-3` arriving in slot 1).
                    if atom.slot.as_deref().is_some_and(|ps| ps != slot.as_str()) {
                        continue;
                    }
                    let probe = if *reinstall_slot_change {
                        verbatim_head
                    } else {
                        constraint.as_str()
                    };
                    let satisfied = portage_dep::match_from_list(probe, &[candidate.as_str()])
                        .is_some_and(|m| !m.is_empty());
                    if !satisfied {
                        failing.push((slot.clone(), *reinstall_slot_change));
                    }
                }
                if failing.is_empty() {
                    // Backlog #222: no version in any upgraded slot breaks
                    // the stripped constraint -- but a same-version
                    // USE-change reinstall can still break the pin's
                    // `[use]` bound, which real's complete-mode
                    // end-of-walk loop fails on (`depgraph.py:8751-8791`:
                    // an unsatisfied dep the vdb still matches pulls the
                    // installed package in as a nomerge node, i.e. a slot
                    // collision). The merge's USE is already decided --
                    // real fails rather than reflipping it (the EAPI-8
                    // oracle answers `success=False` with no
                    // `slot_collision_solutions`) -- so a broken USE pin
                    // is dropped for the residual installed-instance
                    // report, never enforced (enforcing the verbatim atom
                    // would let the re-resolve silently flip the USE back
                    // and succeed where real fails). The stripped probe
                    // must still hold the merge: otherwise the break is
                    // not USE at all (and a consumer already broken before
                    // the run is real's `initially_unsatisfied` skip).
                    // Built slot-operator pins stay out: their domain
                    // owns the version/USE interplay through rebuilds
                    // (#24), and build-time keys stay out like the D0
                    // ignore below (real empties a built package's
                    // `DEPEND`/`BDEPEND` before registering any parent
                    // atom, `depgraph.py:4194-4247`).
                    let use_deps = atom.use_deps.clone().unwrap_or_default();
                    if !use_deps.is_empty() && !is_build_time_dep_key(dep_key) && !built_slot_op {
                        let pin_slot = atom.slot.as_deref();
                        let mut broken = false;
                        for ((ucp, uslot), source) in &use_breaks {
                            if *ucp != cp {
                                continue;
                            }
                            if pin_slot.is_some_and(|ps| ps != uslot.as_str()) {
                                continue;
                            }
                            if portage_dep::match_from_list(
                                constraint.as_str(),
                                &[source.candidate.as_str()],
                            )
                            .is_none_or(|m| m.is_empty())
                            {
                                continue;
                            }
                            if portage_dep::use_deps_violated(
                                &use_deps,
                                &use_flags,
                                &source.merge_use,
                                &source.merge_iuse,
                            ) {
                                broken = true;
                                break;
                            }
                        }
                        if broken {
                            // Real stores the parent atom evaluated
                            // against the parent's own USE (real
                            // `Atom.evaluate_conditionals` at edge time,
                            // `lib/portage/dep/__init__.py:1387`): the
                            // collision renderer classifies USE reasons
                            // from the unconditional forms alone (real
                            // `slot_collision.py:331-389`), so the pin
                            // carries the evaluated text (`[-icu]`, not
                            // `[!icu?]`) while `raw_atom` keeps the
                            // recorded verbatim form for notices.
                            let evaluated = portage_dep::evaluate_atom_conditionals(
                                atom_str.as_str(),
                                &use_flags,
                            )
                            .unwrap_or_else(|| atom_str.clone());
                            let pin = RevDepPin {
                                cp: cp.clone(),
                                atom: evaluated,
                                raw_atom: atom_str.clone(),
                                consumer: (
                                    consumer.category.clone(),
                                    consumer.package.clone(),
                                    consumer.version.clone(),
                                ),
                            };
                            if seen.insert((pin.cp.clone(), pin.atom.clone(), pin.consumer.clone()))
                            {
                                dropped.push(pin);
                            }
                        }
                    }
                    continue;
                }
                // The enforced/dropped atom text: verbatim when a built
                // slot-operator binding fired through a slot-change
                // reinstall (withholding the reinstall keeps the recorded
                // binding satisfied); the stripped constraint otherwise.
                let pin_atom = if built_slot_op
                    && failing
                        .iter()
                        .any(|(_, reinstall_slot_change)| *reinstall_slot_change)
                {
                    verbatim_head.to_string()
                } else {
                    constraint
                };
                let pin = RevDepPin {
                    cp: cp.clone(),
                    atom: pin_atom,
                    // The verbatim recorded atom (with `[use]` deps):
                    // `atom` above is normalised for matching, but the
                    // skip-conflict notice renders what real renders --
                    // the consumer's own recorded text (S2/#92 oracle:
                    // qemu's `~...-202408[qemu_softmmu_targets_x86_64(+)]`).
                    raw_atom: atom_str.clone(),
                    consumer: (
                        consumer.category.clone(),
                        consumer.package.clone(),
                        consumer.version.clone(),
                    ),
                };
                if !seen.insert((pin.cp.clone(), pin.atom.clone(), pin.consumer.clone())) {
                    continue;
                }
                // Enforce when some failing slot can still hold the pin
                // alongside the hard requirements; otherwise drop it for
                // the residual report (the hard-required upgrade merges
                // anyway). `hard_want` is every atom text that targeted
                // the package this pass, top-level arguments included --
                // real's whole-atom-set selection. A slot-stripped
                // (built-`:=`) pin constrains no slot in particular, like
                // its normalised form; a plain slot pin stays scoped.
                let pin_slot = if built_slot_op {
                    None
                } else {
                    atom.slot.as_deref()
                };
                let hard = hard_want.get(&cp).map_or(&EMPTY_WANT, |v| v);
                let holds = failing.iter().any(|(slot, _)| {
                    let mut slots = std::collections::HashSet::new();
                    slots.insert(slot.clone());
                    rev_dep_pin_holdable(RevDepPinHoldableArgs {
                        root,
                        repos,
                        cp: &cp,
                        pin: &pin.atom,
                        pin_slot,
                        upgraded_slots: &slots,
                        hard_atoms: hard,
                    })
                });
                // Backlog #79 (D0 + the shared S0 with #78): real only
                // ever consults such a pin from the slot-operator update
                // probe (`_slot_operator_check_reverse_dependencies`,
                // `depgraph.py:2472-2538` -- which parents that probe
                // visits, which it skips). A holdable *plain* pin (no
                // built `:=`) from a *build-time* key (`DEPEND`/`BDEPEND`)
                // is therefore ignored entirely: a built package's
                // build-time deps are `optional` edges (real
                // `depgraph.py:4194-4247`, `optional=(pkg.built or ...)`
                // at `:4277-4287`), emptied outright when `bdeps` is off,
                // and never withhold an upgrade -- real merges without
                // filtering on the pin and without reporting it (D1:
                // `rdctarget-1.0`'s recorded `~rdcblocker-1.0` neither
                // drops `rdcblocker-2.0` nor leaves a residual row -- the
                // only `B` is the new version's own `RDEPEND` blocker).
                // Enforcing it here is what silently settled the old
                // version (D0's trace).
                //
                // Backlog #91 (S1) narrows that ignore to build-time
                // keys: a holdable plain pin from a *runtime* key
                // (`RDEPEND`/`IDEPEND`/`PDEPEND`, never emptied as a
                // class) IS a slot-conflict parent edge once its
                // consumer is in the graph, and real's direct solve
                // keeps the installed instance (host qemu/edk2:
                // `Slot conflict ... remove: ...-202608 / keep:
                // ...-202408-5`; hermetic oracle: the `whblocker`/
                // `whtarget`/`whpuller` fixtures -- runtime `~` pin from
                // a non-replaced consumer withholds `whblocker-2.0`,
                // while the unreachable-consumer twin merges with the
                // uninstall/`b` rows). Enforcing it here as an ordinary
                // slot constraint settles the same answer through the
                // re-resolve. The three rejections of #91 §1 stand: no
                // use-dep gating (qemu's own pin carries use-deps and
                // withholds), no operator narrowing (any operator holds
                // when runtime + in-graph), only the key axis moves.
                // Still enforced: built slot-operator pins (the probe's
                // own domain, #24's rebuilds) and every not-holdable pin
                // (dropped for the residual report, keeper's shape).
                if holds && !built_slot_op && is_build_time_dep_key(dep_key) {
                    continue;
                }
                if holds {
                    enforced.push(pin);
                } else {
                    dropped.push(pin);
                }
            }
        }
    }
    enforced.sort();
    dropped.sort();
    (enforced, dropped)
}
