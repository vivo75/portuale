//! Skipped-update records and their grouping.

use crate::*;

/// One withheld upgrade for Portage's skipped-update block: same shape for the
/// direct solve's removal rows and the reverse-pin withholds. Rendered by
/// `pretend.rs`; the rc stays 0 either way.
// Portage: depgraph.py, _conflict_missed_update
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedUpdate {
    pub category: String,
    pub package: String,
    pub slot: String,
    pub skipped_version: String,
    pub skipped_sub_slot: String,
    pub skipped_repo: String,
    pub skipped_use: Vec<(String, String)>,
    /// `Some(build id)` when the skipped instance is a binary package (Portage
    /// prints `binary scheduled for merge` and the `-<build id>` suffix).
    pub skipped_build_id: Option<String>,
    pub atom: String,
    pub consumer_cpv: String,
    pub consumer_installed: bool,
    /// `Some(build id)` when the merge-bound consumer is a binary package.
    pub consumer_build_id: Option<String>,
    pub consumer_use: Vec<(String, String)>,
    /// Whether the missed instance resolves in the running root.
    pub missed_targets_running_root: bool,
    /// Whether a merge consumer resolves in the running root.
    pub consumer_targets_running_root: bool,
}

/// One slot for the abbreviated skipped-update tail: a masked version higher
/// than the chosen one whose missing-dependency mask still bites at settle
/// time. Only the slot renders.
// Portage: depgraph.py, _show_missed_update (check_backtrack probe)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedMissingDep {
    pub category: String,
    pub package: String,
    pub slot: String,
}

/// One update for the full per-update form: a masked higher version whose
/// missing-dependency mask no longer bites at settle time. Renders the
/// `selected:` / `skipped:` rows plus the missing atom's disclosure, with the
/// chain pinned to the single skipped parent (no `(argument)` line).
// Portage: depgraph.py, _show_missed_update_unsatisfied_dep
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedMissingDepFull {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The selected entry's display row (merge-bound) or installed instance.
    pub selected_display: String,
    /// The masked higher version's display row (always merge-scheduled).
    pub skipped_display: String,
    /// The missing atom's disclosure: masked candidates, or the plain miss.
    pub dep: SkippedDepDisclosure,
}

/// The missing atom's disclosure inside [`SkippedMissingDepFull`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkippedDepDisclosure {
    Masked(MaskedDepReport),
    Plain(PlainMissDepReport),
}

/// Arguments of [`missing_dep_full_row`].
pub(crate) struct MissingDepFullRowArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) cat: &'a str,
    pub(crate) pkg: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) ver: &'a str,
    pub(crate) sub: &'a str,
    pub(crate) repo: &'a str,
    pub(crate) atom: &'a str,
}

/// Builds the full-form row for the masked higher version `ver` of `cat/pkg`
/// at `slot`. `None` when no settled entry holds the slot or metadata fails.
pub(crate) fn missing_dep_full_row(
    args: MissingDepFullRowArgs<'_>,
) -> Option<SkippedMissingDepFull> {
    let MissingDepFullRowArgs {
        repos,
        root,
        config,
        entries,
        cat,
        pkg,
        slot,
        ver,
        sub,
        repo,
        atom,
    } = args;
    // The tracker's slot match: merge-bound entry first, else installed.
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
        // No settled entry holds the slot, but the vdb holds the masked version
        // itself: fall back to it, scoped to version equality only.
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
    // The chain is the single skipped node (not in the settled graph).
    let chain = vec![(format!("{cat}/{pkg}-{ver}::{repo}"), "ebuild".to_string())];
    let dep = match masked_candidates_for_atom(repos, atom, config) {
        Some(masked) => {
            // drop-ok: fallback to known cat/pkg, not dropped
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
            // drop-ok: fallback to known cat/pkg, not dropped
            let (acat, apkg) = portage_dep::parse_atom(atom)
                .map(|a| (a.category, a.package))
                .unwrap_or_else(|| (cat.to_string(), pkg.to_string()));
            SkippedDepDisclosure::Plain(PlainMissDepReport {
                category: acat,
                package: apkg,
                atom: atom.to_string(),
                chain,
                binary_only: false,
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

/// Collapses rows per `(category, package, slot)` to the highest
/// `skipped_version`; equal versions all stay, exact duplicates collapse to
/// one. A slot's survivors sit at the position of the slot's first row, and
/// the mask-rows-then-direct-rows-then-withholds relative order is preserved.
// Portage: depgraph.py, _get_missed_updates
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
    // A replacing version renders where the lower was first recorded.
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

/// Groups rows naming the same missed package into one block, preserving
/// first-seen group order and row order within.
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

/// Whether `atom` (USE-deps stripped) matches a version the settled backtrack
/// state still masks: then the missed update renders in the abbreviated tail.
// Portage: depgraph.py, _show_unsatisfied_dep (check_backtrack probe)
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
