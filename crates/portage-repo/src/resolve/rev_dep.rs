//! Constraints that installed reverse dependencies put on an upgrade.

use crate::*;
use std::ops::ControlFlow;

/// Whether `atom` is a **built** slot-operator atom (`cat/pkg:S/SS=`).
/// Slot dropped textually (equivalent); `[use-dep]` dropped too (allow-only).
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
pub(crate) fn is_built_slot_op(atom: &portage_dep::Atom) -> bool {
    atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
        && atom.slot.is_some()
        && atom.sub_slot.is_some()
}

/// Whether a new miss atom supersedes the disclosed one for the same `cat/pkg`:
/// a live slot-operator atom wins over a recorded built one. Scoped to that
/// pair; every other combination keeps first-wins.
pub(crate) fn live_miss_atom_supersedes(recorded: &str, candidate: &str) -> bool {
    let recorded_built = parse_dep_atom(recorded, "rev_dep").is_some_and(|a| is_built_slot_op(&a));
    if !recorded_built {
        return false;
    }
    if let Some(new) = parse_dep_atom(candidate, "rev_dep") {
        return new.slot_operator.is_some() && !is_built_slot_op(&new);
    }
    // The bare suffix form (`<cat/pkg-2:=`) never parses here but is always
    // live; strip a trailing `[use]` group first.
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

/// Portage `Atom.with_slot("=")`: a built atom's `:S/SS=` binding relaxes to
/// bare `:=` (rebuild, never withhold). Non-built/unparseable returns verbatim.
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
pub(crate) fn relax_built_slot_operator_atom(atom_str: &str) -> String {
    // drop-ok: unparseable atom returned verbatim, not dropped
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return atom_str.to_string();
    };
    if !is_built_slot_op(&atom) {
        return atom_str.to_string();
    }
    // Both slots are guaranteed present, so the needle occurs verbatim.
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

/// Whether an installed-settling edge's atom is a slot-operator rebuild trigger
/// (a built atom accepting the resolved merge once relaxed) rather than a slot
/// conflict. A version veto still records normally; unparseable never triggers.
// Portage: depgraph.py, _minimize_children (update probe)
pub(crate) fn built_slot_operator_rebuild_trigger(atom_str: &str, existing_str: &str) -> bool {
    let Some(atom) = parse_dep_atom(atom_str, "rev_dep") else {
        return false;
    };
    if !is_built_slot_op(&atom) {
        return false;
    }
    let relaxed = relax_built_slot_operator_atom(atom_str);
    portage_dep::match_from_list(&relaxed, &[existing_str]).is_some_and(|m| !m.is_empty())
}

/// One installed consumer's recorded atom broken by this pass's upgrade, with
/// the consumer attached (Portage's nomerge node, as a triple).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RevDepPin {
    /// The upgraded `category/package`.
    pub(crate) cp: (String, String),
    /// The consumer's recorded atom, normalised by
    /// `reverse_dep_constraint_atom`.
    pub(crate) atom: String,
    /// Verbatim text (with `[use]` deps), for notices only.
    pub(crate) raw_atom: String,
    /// Built `:S/SS=` atoms the consumer recorded against `cp`, verbatim.
    /// Never constrains (the stripped `atom` does that); only the
    /// skip-block printer renders them, one extra parent row each.
    pub(crate) built_render: Vec<String>,
    /// The installed consumer `(category, package, version)`.
    pub(crate) consumer: (String, String, String),
}

/// Arguments of [`rev_dep_pin_holdable`].
pub(crate) struct RevDepPinHoldableArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) cp: &'a (String, String),
    pub(crate) pin: &'a str,
    pub(crate) pin_slot: Option<&'a str>,
    pub(crate) upgraded_slots: &'a std::collections::HashSet<String>,
    pub(crate) hard_atoms: &'a [String],
    pub(crate) gate: CategoryGate<'a>,
}

/// Whether `pin` on `cp` still holds alongside every hard requirement. A
/// jointly-unsatisfiable pin is DROPPED (Portage merges anyway). Per-slot;
/// unparseable or candidate-less falls back to enforcing.
// Portage: depgraph.py, _select_pkg_highest_available (whole atom set)
pub(crate) fn rev_dep_pin_holdable(args: RevDepPinHoldableArgs<'_>) -> bool {
    let RevDepPinHoldableArgs {
        root,
        cp,
        pin,
        pin_slot,
        upgraded_slots,
        hard_atoms,
        gate,
    } = args;
    // drop-ok: enforce
    if portage_dep::parse_atom(pin).is_none() {
        return true;
    }
    // An unparseable hard atom cannot constrain anything: enforce.
    let mut hard_parsed: Vec<(&str, portage_dep::Atom)> = Vec::new();
    for h in hard_atoms {
        // drop-ok: enforce
        match portage_dep::parse_atom(h) {
            Some(a) => hard_parsed.push((h.as_str(), a)),
            None => return true,
        }
    }
    let Ok(cands) = gate.repo_candidates(&cp.0, &cp.1) else {
        return true;
    };
    let constrained: Vec<&String> = upgraded_slots
        .iter()
        .filter(|s| pin_slot.is_none_or(|ps| ps == *s))
        .collect();
    if constrained.is_empty() {
        return true;
    }
    // Installed instances count too (withholding keeps them).
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
        let repo_cands: Vec<String> = cands
            .iter()
            .filter(|c| &c.slot == *slot)
            .map(|c| c.match_str(&cp.0, &cp.1))
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
                        // match-str-ok: installed side (inst_slot/installed_pkg_repo), not a Candidate
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

/// Whether a dep key is build-time: Portage empties those keys for built
/// packages, so only build-time pins are unconditionally ignorable.
pub(crate) fn is_build_time_dep_key(key: impl AsRef<str>) -> bool {
    matches!(key.as_ref(), "DEPEND" | "BDEPEND")
}

/// The dependency atoms an **installed** package records against a package this
/// run wants to upgrade, which that upgrade would break. The vdb is scanned
/// directly (no nomerge nodes); only `reachable` or in-graph consumers survive
/// as ordinary `slot_constraints`. Only upgrades/downgrades and slot-changing
/// reinstalls are checked; jointly-unsatisfiable pins return in `dropped`
/// for the residual report (see `rev_dep_pin_holdable`). Returns `(enforced,
/// dropped)`, sorted and deduped; empty when nothing is being upgraded.
// Portage: depgraph.py, _complete_graph / _slot_operator_check_reverse_dependencies
/// A same-version USE-change reinstall's decided merge state (candidate string,
/// effective USE, declared IUSE) for the USE-break check.
pub(crate) struct UseBreakSource {
    pub(crate) candidate: String,
    pub(crate) merge_use: HashSet<String>,
    pub(crate) merge_iuse: HashSet<String>,
}

/// The decided merge USE/IUSE for `category/package-version`. `None` when no
/// readable candidate exists; callers fall back to ignoring USE.
pub(crate) fn merge_use_state(
    gate: CategoryGate<'_>,
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(HashSet<String>, HashSet<String>)> {
    let cands = gate.repo_candidates(category, package).ok()?;
    let cand = cands.iter().find(|c| c.version == version)?;
    let (iuse, use_flags) = candidate_iuse_and_use(cand, category, package, config)?;
    Some(((*use_flags).clone(), iuse))
}

/// Arguments of [`merge_use_changed_vs_installed`].
pub(crate) struct MergeUseChangedVsInstalledArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) gate: CategoryGate<'a>,
}

/// Portage's USE-change trigger: merge IUSE or enabled-USE∩IUSE differs from
/// the installed instance's, independent of `--newuse`. `None` when unreadable.
// Portage: depgraph.py, _complete_graph (complete_if_new_use)
pub(crate) fn merge_use_changed_vs_installed(
    args: MergeUseChangedVsInstalledArgs<'_>,
) -> Option<bool> {
    let MergeUseChangedVsInstalledArgs {
        root,
        config,
        category,
        package,
        version,
        gate,
    } = args;
    let (merge_use, merge_iuse) = merge_use_state(gate, config, category, package, version)?;
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
    /// The required sets' installed closure for the report-only parent rows (#573);
    /// empty when the run has no seeds at all (unit tests, no CLI world).
    pub(crate) report_reachable: &'a HashSet<(String, String)>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) complete: bool,
    /// Portage entered complete mode in this pass: the `||` of a consumer the walk did
    /// not recurse into is evaluated only then.
    pub(crate) portage_complete: bool,
    /// The installed packages the walk recursed into, with the atoms its `||` groups chose.
    pub(crate) installed_walked: &'a InstalledWalked,
    pub(crate) update: bool,
    /// `selective` as `_want_installed_pkg` reads it: `--emptytree` already removed.
    pub(crate) selective: bool,
    pub(crate) backtrack_allowed: bool,
    /// The argument atoms (command line), for `_want_installed_pkg`.
    pub(crate) arg_atoms: &'a [String],
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) gate: CategoryGate<'a>,
}

/// (constrained cp, pin atom, pinning consumer) already emitted this pass.
type SeenPin = ((String, String), String, (String, String, String));

/// (upgraded `cat/pkg`, pinning consumer) -> verbatim built `:S/SS=` atoms
/// that consumer recorded against it, seen while scanning this pass.
type BuiltSeenMap = HashMap<((String, String), (String, String, String)), Vec<String>>;

/// (`cat/pkg`, slot) -> the candidate string this run would install, and whether it changes the slot.
type UpgradingMap = HashMap<((String, String), String), (String, bool)>;

/// The empty reverse-dependency want list shared by every consumer without one.
static EMPTY_WANT: Vec<String> = Vec::new();

pub(crate) fn reverse_dependency_constraints(
    args: ReverseDependencyConstraintsArgs<'_>,
) -> (Vec<RevDepPin>, Vec<RevDepPin>) {
    let (enforced, dropped, _report) = reverse_dependency_constraints_reported(args);
    (enforced, dropped)
}

/// [`reverse_dependency_constraints`] plus the report-only parent rows: atoms
/// Portage's missed-update tail records as parents of the skipped version
/// without making them constraints (`depgraph.py:2107-2121`).
pub(crate) fn reverse_dependency_constraints_reported(
    args: ReverseDependencyConstraintsArgs<'_>,
) -> (Vec<RevDepPin>, Vec<RevDepPin>, Vec<RevDepPin>) {
    let ReverseDependencyConstraintsArgs {
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
        report_reachable,
        config,
        complete,
        portage_complete,
        installed_walked,
        update,
        selective,
        backtrack_allowed,
        arg_atoms,
        root_deps_running_root,
        gate,
    } = args;
    // (`cat/pkg`, slot) -> candidate string this run would install.
    let mut upgrading: HashMap<((String, String), String), (String, bool)> = HashMap::new();
    let mut being_replaced: HashSet<(String, String)> = HashSet::new();
    // No-op top-level arguments (not digraph nodes).
    let mut already_installed_top_level: HashSet<(String, String)> = HashSet::new();
    // Removed consumers cannot veto their own blocker's upgrade.
    let mut being_removed: HashSet<(String, String)> = HashSet::new();
    // Every `cat/pkg` walked this pass (in-graph reachability).
    let mut graph_cps: HashSet<(String, String)> = HashSet::new();
    // (`cat/pkg`, slot) -> USE-reinstall merge state (complete mode only).
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
                merge_use_state(gate, config, &e.category, &e.package, version)
            && merge_use_changed_vs_installed(MergeUseChangedVsInstalledArgs {
                root,
                config,
                category: &e.category,
                package: &e.package,
                version,
                gate,
            })
            .unwrap_or(false)
        {
            let slot = e.slot.clone().unwrap_or_else(|| "0".to_string());
            use_breaks.insert(
                (cp.clone(), slot.clone()),
                UseBreakSource {
                    candidate: format!(
                        // match-str-ok: merge-list entry (Option slot/repo), not a Candidate
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
                    // match-str-ok: merge-list entry (Option slot/repo), not a Candidate
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
        return (Vec::new(), Vec::new(), Vec::new());
    }

    // No `DEPEND`/`BDEPEND` parent atoms from built packages unless `--with-bdeps`.
    let dep_keys: &[&str] = if with_bdeps {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND"]
    };

    let mut enforced: Vec<RevDepPin> = Vec::new();
    let mut dropped: Vec<RevDepPin> = Vec::new();
    let mut report: Vec<RevDepPin> = Vec::new();
    let mut seen: HashSet<SeenPin> = HashSet::new();
    let mut built_seen: BuiltSeenMap = HashMap::new();
    // Per-pass `Effective`-layer cache (one overlay per instance).
    let mut meta_memo: MetaMemo = HashMap::new();
    let or_scan = OrScan {
        portage_complete,
        installed_walked,
        update,
        selective,
        backtrack_allowed,
        excluded,
        arg_atoms,
        root_deps_running_root,
        post_merge: PostMergeSet::build(root, entries),
    };
    let scope = ConsumerScope {
        being_replaced: &being_replaced,
        being_removed: &being_removed,
        reachable,
        graph_cps: &graph_cps,
        already_installed_top_level: &already_installed_top_level,
        world_reachable,
        excluded,
    };
    let leaked = if or_scan.portage_complete || !installed_walked.is_empty() {
        sibling_virtual_leaks(SiblingVirtualLeaksArgs {
            scan: &or_scan,
            deps: DepSource {
                root,
                gate,
                dep_keys,
                dynamic_deps,
                ignore_built_slot_operator_deps,
            },
            meta_memo: &mut meta_memo,
            in_scope: &|c: &InstalledPackage| consumer_in_scope(&scope, c),
            entries,
            root,
            config,
            gate,
        })
    } else {
        HashMap::new()
    };
    'consumer: for consumer in all_installed_packages(root).iter() {
        if constraints_from_consumer(ConstraintsFromConsumerArgs {
            consumer,
            scope: &scope,
            or_scan: &or_scan,
            leaked: &leaked,
            entries,
            config,
            root,
            dep_keys,
            dynamic_deps,
            ignore_built_slot_operator_deps,
            meta_memo: &mut meta_memo,
            upgrading: &upgrading,
            use_breaks: &use_breaks,
            seen: &mut seen,
            built_seen: &mut built_seen,
            dropped: &mut dropped,
            report: &mut report,
            portage_complete,
            report_only: false,
            hard_want,
            enforced: &mut enforced,
            gate,
        })
        .is_break()
        {
            continue 'consumer;
        }
    }
    // Portage's "Record missed updates" tail takes the parent atoms of every
    // consumer the *completed* graph contains, including consumers this pass never
    // walked into (depgraph.py:2107-2121). They are report-only: the analysis below
    // never feeds `enforced`/`dropped`, so no merge decision can move.
    if portage_complete && !report_reachable.is_empty() {
        let report_scope = ConsumerScope {
            being_replaced: &being_replaced,
            being_removed: &being_removed,
            reachable: report_reachable,
            graph_cps: &graph_cps,
            already_installed_top_level: &already_installed_top_level,
            world_reachable,
            excluded,
        };
        for consumer in all_installed_packages(root).iter() {
            if consumer_in_scope(&scope, consumer) || !consumer_in_scope(&report_scope, consumer) {
                continue;
            }
            let _ = constraints_from_consumer(ConstraintsFromConsumerArgs {
                consumer,
                scope: &report_scope,
                or_scan: &or_scan,
                leaked: &leaked,
                entries,
                config,
                root,
                dep_keys,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                meta_memo: &mut meta_memo,
                upgrading: &upgrading,
                use_breaks: &use_breaks,
                seen: &mut seen,
                built_seen: &mut built_seen,
                dropped: &mut dropped,
                report: &mut report,
                portage_complete,
                report_only: true,
                hard_want,
                enforced: &mut enforced,
                gate,
            });
        }
    }
    // The built atoms rode along untouched by every mask/restart decision
    // above; attach them to their consumer's pins for the printer only. The
    // consumer's atoms come out of a hash set, so sort first: the same pin
    // takes the atoms on every run.
    enforced.sort();
    dropped.sort();
    for pin in enforced.iter_mut().chain(dropped.iter_mut()) {
        if let Some(raws) = built_seen.remove(&(pin.cp.clone(), pin.consumer.clone())) {
            raws.into_iter().for_each(|raw| {
                if !pin.built_render.contains(&raw) {
                    pin.built_render.push(raw);
                }
            });
            pin.built_render.sort();
        }
    }
    enforced.sort();
    dropped.sort();
    report.sort();
    report.dedup();
    (enforced, dropped, report)
}

/// The values [`constraints_from_consumer`] reads from its caller.
pub(crate) struct ConstraintsFromConsumerArgs<'a> {
    pub(crate) consumer: &'a InstalledPackage,
    pub(crate) scope: &'a ConsumerScope<'a>,
    pub(crate) or_scan: &'a OrScan<'a>,
    /// Per installed virtual: the atoms a sibling's choice leaked into it.
    pub(crate) leaked: &'a HashMap<(String, String, String), HashSet<String>>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) dynamic_deps: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) meta_memo: &'a mut MetaMemo,
    pub(crate) upgrading: &'a UpgradingMap,
    pub(crate) use_breaks: &'a HashMap<((String, String), String), UseBreakSource>,
    pub(crate) seen: &'a mut HashSet<SeenPin>,
    pub(crate) built_seen: &'a mut BuiltSeenMap,
    pub(crate) dropped: &'a mut Vec<RevDepPin>,
    /// Report-only parent rows for Portage's missed-update tail.
    pub(crate) report: &'a mut Vec<RevDepPin>,
    /// Portage entered complete mode on this pass.
    pub(crate) portage_complete: bool,
    /// This consumer is outside the pass's walk; its atoms become missed-update
    /// parent rows only, never pins.
    pub(crate) report_only: bool,
    pub(crate) hard_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) enforced: &'a mut Vec<RevDepPin>,
    pub(crate) gate: CategoryGate<'a>,
}

/// The sets that say which installed consumers a pass's scan reads.
pub(crate) struct ConsumerScope<'a> {
    pub(crate) being_replaced: &'a HashSet<(String, String)>,
    pub(crate) being_removed: &'a HashSet<(String, String)>,
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) graph_cps: &'a HashSet<(String, String)>,
    pub(crate) already_installed_top_level: &'a HashSet<(String, String)>,
    pub(crate) world_reachable: &'a HashSet<(String, String)>,
    pub(crate) excluded: &'a [String],
}

/// Whether the consumer is a source of reverse-dependency pins at all.
pub(crate) fn consumer_in_scope(scope: &ConsumerScope<'_>, consumer: &InstalledPackage) -> bool {
    let consumer_cp = (consumer.category.clone(), consumer.package.clone());
    if scope.being_replaced.contains(&consumer_cp) || scope.being_removed.contains(&consumer_cp) {
        return false;
    }
    // Only required-set-reachable or in-graph consumers.
    if !scope.reachable.contains(&consumer_cp) && !scope.graph_cps.contains(&consumer_cp) {
        return false;
    }
    // A no-op top-level argument is never a source (unless required-set-reachable).
    if scope.already_installed_top_level.contains(&consumer_cp)
        && !scope.world_reachable.contains(&consumer_cp)
    {
        return false;
    }
    let consumer_str = format!(
        "{}/{}-{}",
        consumer.category, consumer.package, consumer.version
    );
    !scope
        .excluded
        .iter()
        .any(|ex| matches_cli_option_atom(ex, &consumer_str))
}

/// Collect the reverse-dependency pins one installed consumer places on the packages being upgraded.
pub(crate) fn constraints_from_consumer(args: ConstraintsFromConsumerArgs<'_>) -> ControlFlow<()> {
    let ConstraintsFromConsumerArgs {
        consumer,
        scope,
        or_scan,
        leaked,
        entries,
        config,
        root,
        dep_keys,
        dynamic_deps,
        ignore_built_slot_operator_deps,
        meta_memo,
        upgrading,
        use_breaks,
        seen,
        built_seen,
        dropped,
        report,
        portage_complete,
        report_only,
        hard_want,
        enforced,
        gate,
    } = args;
    if !consumer_in_scope(scope, consumer) {
        return ControlFlow::Break(());
    }
    let deps = consumer_deps(
        DepSource {
            root,
            gate,
            dep_keys,
            dynamic_deps,
            ignore_built_slot_operator_deps,
        },
        meta_memo,
        &consumer.category,
        &consumer.package,
        &consumer.version,
    );
    let leaked_here = leaked.get(&(
        consumer.category.clone(),
        consumer.package.clone(),
        consumer.version.clone(),
    ));
    for (dep_key, depstr) in &deps.strings {
        let dep_key = &dep_key.as_str();
        let use_flags = &deps.use_flags;
        let Some(atoms) = flat_dep_atoms(depstr, use_flags) else {
            continue;
        };
        // A `||` arm `dep_zapdeps` does not select is no parent atom at all.
        // A report-only consumer was never walked, and Portage's `||` choice for it
        // keeps the installed arm (`dep_zapdeps`'s installed preference), which is
        // the view without the post-merge set.
        let post_merge = (or_scan.portage_complete
            && !report_only
            && !or_scan.installed_walked.contains_key(&(
                consumer.category.clone(),
                consumer.package.clone(),
                consumer.version.clone(),
            )))
        .then_some(or_scan);
        // A report-only consumer was never walked, and Portage keeps the arm its
        // installed record selects (`dep_zapdeps`'s installed preference here, not
        // the post-merge view the walk uses): the same demotion `#569` applies.
        let demoted = if report_only {
            let mut demoted = or_branch_vdb_deselected_set(root, depstr, use_flags);
            if let Some(leaked) = leaked_here {
                demoted.retain(|a| !leaked.contains(a));
            }
            demoted
        } else {
            consumer_or_demotions(ConsumerOrDemotionsArgs {
                scan: or_scan,
                consumer,
                depstr,
                use_flags,
                leaked: leaked_here,
                entries,
                root,
                config,
                gate,
            })
        };
        'atom: for atom_str in atoms {
            if constraints_from_atom(ConstraintsFromAtomArgs {
                atom_str: &atom_str,
                demoted: &demoted,
                post_merge,
                config,
                upgrading,
                dep_key,
                use_breaks,
                use_flags,
                consumer,
                seen,
                built_seen,
                dropped,
                report,
                portage_complete,
                report_only,
                hard_want,
                root,
                enforced,
                gate,
            })
            .is_break()
            {
                continue 'atom;
            }
        }
    }
    ControlFlow::Continue(())
}

/// The values [`constraints_from_atom`] reads from its caller.
pub(crate) struct ConstraintsFromAtomArgs<'a> {
    pub(crate) atom_str: &'a str,
    /// `||`-arm texts `dep_zapdeps` does not select for this consumer
    /// (`consumer_or_demotions`): never parent atoms, never pins.
    pub(crate) demoted: &'a HashSet<String>,
    /// The post-merge set when the consumer is a complete-graph one (not walked, no
    /// `--update`): a built slot-operator atom it cannot satisfy conflicts.
    pub(crate) post_merge: Option<&'a OrScan<'a>>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) upgrading: &'a UpgradingMap,
    pub(crate) dep_key: &'a &'a str,
    pub(crate) use_breaks: &'a HashMap<((String, String), String), UseBreakSource>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) consumer: &'a InstalledPackage,
    pub(crate) seen: &'a mut HashSet<SeenPin>,
    pub(crate) built_seen: &'a mut BuiltSeenMap,
    pub(crate) dropped: &'a mut Vec<RevDepPin>,
    /// Report-only parent rows: atoms Portage records in the missed-update tail
    /// without making them constraints (see [`constraints_from_atom`]).
    pub(crate) report: &'a mut Vec<RevDepPin>,
    /// Portage entered complete mode on this pass (the missed-update tail's
    /// parents come from the completed graph; `depgraph.py:2107-2121`).
    pub(crate) portage_complete: bool,
    /// Report-only: every pin this atom would make becomes a `report` row.
    pub(crate) report_only: bool,
    pub(crate) hard_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) root: &'a Path,
    pub(crate) enforced: &'a mut Vec<RevDepPin>,
    pub(crate) gate: CategoryGate<'a>,
}

/// Collect the reverse-dependency pins one dependency atom of a consumer places on the packages being upgraded.
pub(crate) fn constraints_from_atom(args: ConstraintsFromAtomArgs<'_>) -> ControlFlow<()> {
    let ConstraintsFromAtomArgs {
        atom_str,
        demoted,
        post_merge,
        config,
        upgrading,
        dep_key,
        use_breaks,
        use_flags,
        consumer,
        seen,
        built_seen,
        dropped,
        report,
        portage_complete,
        report_only,
        hard_want,
        root,
        enforced,
        gate,
    } = args;
    let Some(atom) = parse_dep_atom(atom_str, "rev_dep") else {
        return ControlFlow::Break(());
    };
    if atom.blocker != portage_dep::Blocker::None {
        return ControlFlow::Break(());
    }
    let cp = (atom.category.clone(), atom.package.clone());
    let built_slot_op = is_built_slot_op(&atom);
    let constraint = reverse_dep_constraint_atom(atom_str, &atom);
    // Demoted `||` branches are no parent atoms: neither constraint nor
    // rendering may see them.
    if demoted.contains(atom_str) || demoted.contains(constraint.as_str()) {
        return ControlFlow::Break(());
    }
    // The built atom rides to the printer verbatim; the stripped constraint
    // below still drives every mask/restart decision, exactly as before.
    if built_slot_op && upgrading.keys().any(|(ucp, _)| *ucp == cp) {
        built_seen
            .entry((
                cp.clone(),
                (
                    consumer.category.clone(),
                    consumer.package.clone(),
                    consumer.version.clone(),
                ),
            ))
            .or_default()
            .push(atom_str.split('[').next().unwrap_or(atom_str).to_string());
    }
    // Collect every slot whose upgrade breaks the pin. Slot-change
    // reinstalls probe the verbatim atom; Upgrade/Downgrade the
    // stripped constraint (built bindings are rebuild triggers).
    let verbatim_head = atom_str.split('[').next().unwrap_or(atom_str);
    // A built atom the merge leaves unsatisfied while an installed instance matches it:
    // Portage adds that instance to the graph, a slot conflict (D:8803-8834), unless the
    // rebuild probe applies (backtracking allowed, `_want_installed_pkg` false).
    // A stripped constraint that the merge already breaks keeps its old pin text.
    let stripped_breaks = upgrading.iter().any(|((ucp, _), (candidate, _))| {
        *ucp == cp
            && portage_dep::match_from_list(constraint.as_str(), &[candidate.as_str()])
                .is_none_or(|m| m.is_empty())
    });
    let built_conflict = built_slot_op
        && !stripped_breaks
        && post_merge.is_some_and(|scan| !scan.post_merge.matches(atom_str))
        && best_installed_matching(root, atom_str, config).is_some_and(|version| {
            !(scan_rebuild_probe_applies(post_merge, root, &atom.category, &atom.package, &version))
        });
    let mut failing: Vec<(String, bool)> = Vec::new();
    for ((ucp, slot), (candidate, reinstall_slot_change)) in upgrading {
        if *ucp != cp {
            continue;
        }
        // An explicit-slot atom constrains only that slot's instance.
        if atom.slot.as_deref().is_some_and(|ps| ps != slot.as_str()) {
            continue;
        }
        let probe = if *reinstall_slot_change || built_conflict {
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
        // Portage's "Record missed updates" tail keeps every parent atom that
        // does not match the skipped package, including a built `:S/SS=` atom
        // the *kept* instance still satisfies (`depgraph.py:2107-2121`). The
        // completed graph is where those parents come from, so the row is
        // report-only: never a constraint, never a new mask.
        if (report_only || (portage_complete && built_slot_op))
            && upgrading.iter().any(|((ucp, _), (candidate, _))| {
                *ucp == cp
                    && portage_dep::match_from_list(verbatim_head, &[candidate.as_str()])
                        .is_none_or(|m| m.is_empty())
            })
        {
            let pin = RevDepPin {
                cp: cp.clone(),
                atom: verbatim_head.to_string(),
                raw_atom: atom_str.to_string(),
                built_render: Vec::new(),
                consumer: (
                    consumer.category.clone(),
                    consumer.package.clone(),
                    consumer.version.clone(),
                ),
            };
            if !report.contains(&pin) {
                report.push(pin);
            }
        }
        // A same-version USE-change reinstall can still break the `[use]`
        // bound: such a pin is dropped for the residual report, never
        // enforced. The stripped probe must still hold the merge;
        // built pins and build-time keys stay out.
        let use_deps = atom.use_deps.clone().unwrap_or_default();
        if !use_deps.is_empty() && !is_build_time_dep_key(dep_key) && !built_slot_op {
            let pin_slot = atom.slot.as_deref();
            let mut broken = false;
            for ((ucp, uslot), source) in use_breaks {
                if *ucp != cp {
                    continue;
                }
                if pin_slot.is_some_and(|ps| ps != uslot.as_str()) {
                    continue;
                }
                if portage_dep::match_from_list(constraint.as_str(), &[source.candidate.as_str()])
                    .is_none_or(|m| m.is_empty())
                {
                    continue;
                }
                if portage_dep::use_deps_violated(
                    &use_deps,
                    use_flags,
                    &source.merge_use,
                    &source.merge_iuse,
                ) {
                    broken = true;
                    break;
                }
            }
            if broken {
                // The pin carries parent-USE-evaluated text.
                let evaluated = portage_dep::evaluate_atom_conditionals(atom_str, use_flags)
                    .unwrap_or_else(|| atom_str.to_string());
                let pin = RevDepPin {
                    cp: cp.clone(),
                    atom: evaluated,
                    raw_atom: atom_str.to_string(),
                    built_render: Vec::new(),
                    consumer: (
                        consumer.category.clone(),
                        consumer.package.clone(),
                        consumer.version.clone(),
                    ),
                };
                if seen.insert((pin.cp.clone(), pin.atom.clone(), pin.consumer.clone())) {
                    if report_only {
                        report.push(pin);
                    } else {
                        dropped.push(pin);
                    }
                }
            }
        }
        return ControlFlow::Break(());
    }
    // Verbatim for built bindings via slot-change reinstalls, else stripped.
    let pin_atom = if built_slot_op
        && (built_conflict
            || failing
                .iter()
                .any(|(_, reinstall_slot_change)| *reinstall_slot_change))
    {
        verbatim_head.to_string()
    } else {
        constraint
    };
    let pin = RevDepPin {
        cp: cp.clone(),
        atom: pin_atom,
        raw_atom: atom_str.to_string(),
        built_render: Vec::new(),
        consumer: (
            consumer.category.clone(),
            consumer.package.clone(),
            consumer.version.clone(),
        ),
    };
    if !seen.insert((pin.cp.clone(), pin.atom.clone(), pin.consumer.clone())) {
        return ControlFlow::Break(());
    }
    if report_only {
        report.push(pin);
        return ControlFlow::Break(());
    }
    // A stripped pin constrains no slot; a plain pin stays scoped.
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
            cp: &cp,
            pin: &pin.atom,
            pin_slot,
            upgraded_slots: &slots,
            hard_atoms: hard,
            gate,
        })
    });
    // A holdable plain pin from a build-time key is ignored (optional
    // edges, never withholding). A runtime-key pin IS a parent edge
    // once in-graph. Built and not-holdable pins still enforce/drop.
    if holds && !built_slot_op && is_build_time_dep_key(dep_key) {
        return ControlFlow::Break(());
    }
    if holds {
        enforced.push(pin);
    } else {
        dropped.push(pin);
    }
    ControlFlow::Continue(())
}

/// Whether Portage tries the slot-operator rebuild probe instead of adding the installed
/// instance: backtracking is allowed and the instance is not wanted (D:8816-8832).
fn scan_rebuild_probe_applies(
    scan: Option<&OrScan<'_>>,
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    scan.is_some_and(|s| {
        s.backtrack_allowed && !s.want_installed_pkg(root, category, package, version)
    })
}
