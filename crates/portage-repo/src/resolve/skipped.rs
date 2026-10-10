//! Skipped-update records and their grouping.

use crate::*;

/// Backlog #90 (S2): one withheld upgrade for real's
/// `WARNING: One or more updates/rebuilds have been skipped due to a
/// dependency conflict:` block (`depgraph.py`'s `_conflict_missed_update[
/// "slot conflict"]`, rendered after the merge list, rc 0). Two
/// producers feed it: the direct solve below (a formed slot conflict
/// whose non-forced instance is removed) and the reverse-pin withhold
/// (an enforced holdable pin that settles the installed version while a
/// higher visible candidate exists). Both render through the same
/// `pretend.rs` block, suppressed under `--quiet` unless `--debug`
/// (real `_show_missed_update`, `depgraph.py:1576-1581`); `--json`
/// returns before the block and `--columns` has no gate. The rc stays
/// 0 either way since no `SlotConflict` survives).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedUpdate {
    pub category: String,
    pub package: String,
    pub slot: String,
    pub skipped_version: String,
    pub skipped_sub_slot: String,
    pub skipped_repo: String,
    pub skipped_use: Vec<(String, String)>,
    pub atom: String,
    pub consumer_cpv: String,
    pub consumer_installed: bool,
    pub consumer_use: Vec<(String, String)>,
    /// Backlog #242 Slice D: whether the missed instance resolves in the
    /// running root (real's per-node `to '<root>'` + per-root USE paint,
    /// [`skipped_update_roots`]). `false` on every single-root run and on
    /// the target-walk-only withhold path.
    pub missed_targets_running_root: bool,
    /// Backlog #242 Slice D: whether a merge consumer resolves in the
    /// running root (its entry's root; installed consumers always resolve
    /// from the walk root's own vdb — the target walk — so `false`).
    pub consumer_targets_running_root: bool,
}

/// Backlog #129 (S1): one slot real reports through the abbreviated
/// `!!! The following update(s) have been skipped due to unsatisfied
/// dependencies / !!! triggered by backtracking:` tail
/// (`depgraph.py:1638-1649`) -- a masked version higher than the chosen
/// one whose missing-dependency mask still bites at settle time (real's
/// `check_backtrack` probe raises `_backtrack_mask`). Only the slot
/// renders (no versions, no parents); the full per-update form real
/// prints when the probe does *not* raise is out of scope (see
/// `backtrack_missed_updates`: those records are skipped, not rendered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedMissingDep {
    pub category: String,
    pub package: String,
    pub slot: String,
}

/// Backlog #198: one update real reports through the full per-update
/// form of `_show_missed_update_unsatisfied_dep`
/// (`lib/_emerge/depgraph.py:1592-1636`) -- a masked version higher
/// than the chosen one whose missing-dependency mask no longer bites
/// at settle time (real's `check_backtrack` probe does *not* raise
/// `_backtrack_mask`, i.e. the missing atom names no backtrack-masked
/// version). Renders the `cat/pkg:slot` header, the `selected:` /
/// `skipped: ... (see unsatisfied dependency below)` rows (real
/// `str(Package)`, destinations omitted like every other portuale
/// notice row), then the missing atom's own disclosure -- the masked
/// block or the plain-miss block, with a single-parent
/// `(dependency required by ...)` chain: the skipped package is not in
/// the settled graph, so real's `_get_dep_chain` stops there
/// (`depgraph.py:6397`, "not in the graph ... break") with no
/// `(argument)` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedMissingDepFull {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// `(cpv:slot/sub::repo, ebuild scheduled for merge)` for a
    /// merge-bound selected entry, `(cpv:slot/sub::repo, installed)`
    /// for an installed one (vdb repo, `__unknown__` fallback).
    pub selected_display: String,
    /// `(cpv:slot/sub::repo, ebuild scheduled for merge)` for the
    /// masked higher version (never installed: real excludes
    /// `pkg.installed` from missed updates, `:1539-1542`).
    pub skipped_display: String,
    /// The missing atom's disclosure: masked-only candidates, or the
    /// plain miss when the atom names nothing at all.
    pub dep: SkippedDepDisclosure,
}

/// The missing atom's disclosure inside [`SkippedMissingDepFull`]:
/// reuses the settled-pass report shapes, with the chain pinned to the
/// single skipped parent (see above).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkippedDepDisclosure {
    Masked(MaskedDepReport),
    Plain(PlainMissDepReport),
}

/// Backlog #198: build the full-form row for a missing-dependency mask
/// whose atom bites nothing backtrack-masked at settle time (real
/// `_show_missed_update_unsatisfied_dep`, `depgraph.py:1592-1636`).
/// `cat/pkg` at `slot` is the masked higher version `ver` (tree
/// sub-slot `sub`, tree repo `repo`); `atom` is the missing atom text
/// as queued. Returns `None` when no settled entry holds the slot
/// (real's `any_selected` gate) or the masked version's tree metadata
/// cannot name the skipped node.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn missing_dep_full_row(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    entries: &[GraphEntry],
    cat: &str,
    pkg: &str,
    slot: &str,
    ver: &str,
    sub: &str,
    repo: &str,
    atom: &str,
) -> Option<SkippedMissingDepFull> {
    // Real's `selected_pkg`: the tracker's match for the slot -- a
    // merge-bound entry first, else the installed instance.
    let mut selected_merge: Option<(&str, &str, &str, &str)> = None;
    let mut selected_installed: Option<&str> = None;
    for e in entries {
        if e.category != cat || e.package != pkg {
            continue;
        }
        match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                if e.slot.as_deref() == Some(slot) {
                    selected_merge = Some((
                        version.as_str(),
                        e.slot.as_deref().unwrap_or(slot),
                        e.sub_slot.as_deref().unwrap_or(sub),
                        e.repo_name.as_deref().unwrap_or(repo),
                    ));
                    break;
                }
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                if e.slot.as_deref() == Some(slot) {
                    selected_merge = Some((
                        to.as_str(),
                        e.slot.as_deref().unwrap_or(slot),
                        e.sub_slot.as_deref().unwrap_or(sub),
                        e.repo_name.as_deref().unwrap_or(repo),
                    ));
                    break;
                }
            }
            PretendOutcome::AlreadyInstalled { version } => {
                if selected_installed.is_none() && read_vdb_slot(root, cat, pkg, version).0 == slot
                {
                    selected_installed = Some(version.as_str());
                }
            }
            PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => {}
        }
    }
    let selected_display = if let Some((v, s, ss, r)) = selected_merge {
        format!("({cat}/{pkg}-{v}:{s}/{ss}::{r}, ebuild scheduled for merge)")
    } else if let Some(v) = selected_installed {
        let (s, ss) = read_vdb_slot(root, cat, pkg, v);
        let r = installed_pkg_repo(root, cat, pkg, v);
        format!("({cat}/{pkg}-{v}:{s}/{ss}::{r}, installed)")
    } else {
        // Backlog #286: no settled entry holds the slot, but the vdb
        // holds the masked version itself in it -- the dropped set-arg
        // shape (real's tracker still selects the installed instance:
        // the `@world` argument is dropped from the graph, but the
        // installed package stays visible through the complete walk).
        // Scoped to version equality: any other installed version has
        // no proven graph presence (real's `any_selected` gate), so it
        // stays silent like before.
        let v = installed_candidates(root, cat, pkg)
            .into_iter()
            .find(|(iv, _, _)| iv.as_str() == ver && read_vdb_slot(root, cat, pkg, iv).0 == slot)
            .map(|(iv, _, _)| iv)?;
        let (s, ss) = read_vdb_slot(root, cat, pkg, &v);
        let r = installed_pkg_repo(root, cat, pkg, &v);
        format!("({cat}/{pkg}-{v}:{s}/{ss}::{r}, installed)")
    };
    let skipped_display =
        format!("({cat}/{pkg}-{ver}:{slot}/{sub}::{repo}, ebuild scheduled for merge)");
    // The skipped package is not in the settled graph, so the chain is
    // the single skipped node (real stops there, no `(argument)` line).
    let chain = vec![(format!("{cat}/{pkg}-{ver}::{repo}"), "ebuild".to_string())];
    let dep = match masked_candidates_for_atom(repos, atom, config) {
        Some(masked) => {
            let (acat, apkg) = portage_dep::parse_atom(atom)
                .map(|a| (a.category, a.package))
                .unwrap_or_else(|| (cat.to_string(), pkg.to_string()));
            SkippedDepDisclosure::Masked(MaskedDepReport {
                category: acat,
                package: apkg,
                atom: atom.to_string(),
                masked,
                chain,
            })
        }
        None => {
            let (acat, apkg) = portage_dep::parse_atom(atom)
                .map(|a| (a.category, a.package))
                .unwrap_or_else(|| (cat.to_string(), pkg.to_string()));
            SkippedDepDisclosure::Plain(PlainMissDepReport {
                category: acat,
                package: apkg,
                atom: atom.to_string(),
                chain,
            })
        }
    };
    Some(SkippedMissingDepFull {
        category: cat.to_string(),
        package: pkg.to_string(),
        slot: slot.to_string(),
        selected_display,
        skipped_display,
        dep,
    })
}

/// Backlog #129 (review): real `_get_missed_updates` chains
/// `_runtime_pkg_mask` before `_conflict_missed_update`
/// (`lib/_emerge/depgraph.py:1533-1536`) and keeps per
/// `(root, slot_atom)` the highest missed pkg (`:1553-1562`).
/// Portuale's two `SkippedUpdate` sources -- the backtrack-mask rows
/// (first-seen order) and the direct-solve/withhold rows (sorted) --
/// collapse the same way: per `(category, package, slot)` (a single
/// root, so root is constant) keep only the rows naming the highest
/// `skipped_version` (`vercmp_ordering`, the same comparison the
/// derivation uses). Equal versions all stay (the established
/// one-row-per-parent shape -- real keeps one record per slot with
/// every parent inside, portuale one row per parent, grouped into
/// one block at render by [`group_skipped_updates`]); exact
/// duplicates collapse to one. Relative order is preserved: mask
/// rows lead, then the direct solve's rows in first-seen block order,
/// then the pin withholds (sorted) -- except that a slot's surviving
/// rows sit at the position of the slot's first row (#292, real's dict
/// key order), however the higher version arrived later.
pub(crate) fn collapse_skipped_updates(rows: Vec<SkippedUpdate>) -> Vec<SkippedUpdate> {
    let mut best: HashMap<(String, String, String), String> = HashMap::new();
    for s in &rows {
        let key = (s.category.clone(), s.package.clone(), s.slot.clone());
        let replace = match best.get(&key) {
            Some(kept) => vercmp_ordering(&s.skipped_version, kept) == std::cmp::Ordering::Greater,
            None => true,
        };
        if replace {
            best.insert(key, s.skipped_version.clone());
        }
    }
    // Backlog #292: real's `missed_updates[k] = ...` overwrites the value
    // of an existing `(root, slot_atom)` key, and a Python dict keeps the
    // key's *first* insertion position (`_get_missed_updates`,
    // `depgraph.py:1544-1560`): a higher pkg replacing a lower one of the
    // same slot renders where the lower was first recorded, not where the
    // higher row sits. Record each slot's first row index over the
    // pre-collapse rows and order the survivors by it (stable).
    let mut first_pos: HashMap<(String, String, String), usize> = HashMap::new();
    for (i, s) in rows.iter().enumerate() {
        first_pos
            .entry((s.category.clone(), s.package.clone(), s.slot.clone()))
            .or_insert(i);
    }
    let slot_pos = |s: &SkippedUpdate| -> usize {
        first_pos[&(s.category.clone(), s.package.clone(), s.slot.clone())]
    };
    let mut rows = rows;
    rows.sort_by_key(slot_pos);
    let mut seen: HashSet<SkippedUpdateKey> = HashSet::new();
    rows.into_iter()
        .filter(|s| {
            best.get(&(s.category.clone(), s.package.clone(), s.slot.clone()))
                .is_some_and(|v| v == &s.skipped_version)
        })
        .filter(|s| {
            seen.insert((
                (
                    s.category.clone(),
                    s.package.clone(),
                    s.slot.clone(),
                    s.skipped_version.clone(),
                    s.skipped_sub_slot.clone(),
                    s.skipped_repo.clone(),
                ),
                s.skipped_use.clone(),
                s.atom.clone(),
                s.consumer_cpv.clone(),
                s.consumer_installed,
                s.consumer_use.clone(),
                s.missed_targets_running_root,
                s.consumer_targets_running_root,
            ))
        })
        .collect()
}

/// Backlog #129 (review round 2): real `_get_missed_updates` keeps one
/// entry per `(root, slot_atom)`
/// (`lib/_emerge/depgraph.py:1553-1562`) -- the highest missed pkg with
/// its whole `(parent, atom)` set (`:2090-2106` accumulates into a set
/// per pkg) -- and `_show_missed_update_slot_conflicts` (`:1650+`)
/// renders one block per entry with every parent line inside.
/// Portuale's rows stay one per rejecting parent (the established #90
/// S2 shape, which `collapse_skipped_updates` also assumes), so the
/// renderer groups rows naming the same missed pkg into one block
/// (`GraphResult::skipped_updates` order is preserved: groups keep
/// first-seen order, parents keep row order -- mask rows arrive in
/// first-seen mask order, direct rows in first-seen block order). Real itself iterates an
/// unordered set -- `Package` keeps identity hash
/// (`_emerge/Package.py:27`) -- so its within-block order is hash
/// order; the `blk0b blk0c blk0a` oracle's b-then-c order is what the
/// sorted direct rows give here.
pub fn group_skipped_updates(rows: &[SkippedUpdate]) -> Vec<Vec<&SkippedUpdate>> {
    type MissedPkgKey = (
        (String, String, String, String, String, String),
        Vec<(String, String)>,
    );
    let key_of = |s: &SkippedUpdate| -> MissedPkgKey {
        (
            (
                s.category.clone(),
                s.package.clone(),
                s.slot.clone(),
                s.skipped_version.clone(),
                s.skipped_sub_slot.clone(),
                s.skipped_repo.clone(),
            ),
            s.skipped_use.clone(),
        )
    };
    let mut index: HashMap<MissedPkgKey, usize> = HashMap::new();
    let mut groups: Vec<Vec<&SkippedUpdate>> = Vec::new();
    for s in rows {
        let key = key_of(s);
        if !index.contains_key(&key) {
            index.insert(key.clone(), groups.len());
            groups.push(Vec::new());
        }
        groups[index[&key]].push(s);
    }
    groups
}

/// Backlog #129 (S1): real `_show_unsatisfied_dep(..., check_backtrack=
/// True)` (`depgraph.py:6471-6484`) -- no output, just the verdict:
/// `true` when the atom matches a version the settled backtrack state
/// still masks (real raises `_backtrack_mask`, and the missed update
/// renders in the abbreviated tail). USE-deps are stripped for the
/// match, exactly like the missed-update recording's plain-match form
/// (`direct_solve_atom_matches`' `for_skipped` arm, `:2083-2094`).
pub(crate) fn backtrack_masked_match(
    repos: &[RepoConfig],
    params: &BacktrackParams,
    atom: &str,
) -> bool {
    let probe_atom = portage_dep::without_use(atom).to_string();
    for ((cat, pkg), bucket) in &params.runtime_pkg_mask {
        for entry in bucket {
            let Some(stripped) = entry.neg.strip_prefix("!=") else {
                continue;
            };
            let Some((c, p, ver)) = split_cpv(stripped) else {
                continue;
            };
            if c != *cat || p != *pkg {
                continue;
            }
            let (_, _, slot) = slot_conflict_meta(repos, &c, &p, &ver);
            let probe = if slot.is_empty() {
                format!("{c}/{p}-{ver}")
            } else {
                format!("{c}/{p}-{ver}:{slot}")
            };
            if portage_dep::match_from_list(&probe_atom, &[probe.as_str()])
                .is_some_and(|m| !m.is_empty())
            {
                return true;
            }
        }
    }
    false
}
