//! Whether the order a fast plan produced builds a package before one of its
//! build dependencies is there (charter 2a, literal), and on which path.

use crate::bootstrap::BuildPlan;
use crate::order::build_path;
use crate::{DEP_KEYS, SlotSet, World, load_config, use_deps_hold};
use portage_dep::SlotOperator;
use portage_repo::{Candidate, EffectiveUseFlagsArgs, InstalledPackage};
use portage_use_reduce::{
    AltPreference, BUILD_KIND_MASK, MatchMode, UseReduceFlatDisjunctiveArgs, use_reduce_flat,
    use_reduce_flat_disjunctive,
};
use portage_vdb::EntryKey;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// A build dependency the produced order does not have in place when its
/// package is built.
pub struct LateNeed {
    /// `cat/pkg:slot` of the row built too early.
    pub row: String,
    /// The build atom.
    pub atom: String,
    /// `cat/pkg:slot` merged later.
    pub target: String,
    /// `row -> ... -> target -> ... -> row` along build edges, if one exists.
    pub path: Option<Vec<String>>,
    /// `(installed cpv, planned sub-slot)` when an installed version meets a
    /// `DEPEND` `:=` atom but the plan replaces it with another sub-slot.
    pub moved_binding: Option<(String, String)>,
}

/// How an atom stands at the start of a row.
enum Met {
    Yes,
    /// An installed version matches, but its `:=` binding goes stale.
    Moved(String, String),
    No,
}

struct Facts<'a> {
    world: &'a World,
    plan: &'a BuildPlan<'a>,
    root: &'a Path,
    config: portage_profile::Config,
    installed: Arc<Vec<InstalledPackage>>,
    db: Arc<dyn portage_vdb::InstalledDb>,
    rows_of: BTreeMap<String, Vec<usize>>,
    by_node: BTreeMap<String, &'a SlotSet>,
}

/// Every build atom of a plan row whose target is merged later (or is the
/// other row of a bootstrap pair) and that nothing installed, earlier or an
/// alternative `||` arm meets, in row order.
///
/// An atom no row aims at (nothing matches, or the best version is masked), a
/// target with a row before the need, and a kept slot (`emptytree`) are not
/// needs here: the checker and the "not checked" line own them.
///
/// # Errors
/// The repositories or the configuration cannot be read.
pub fn late_build_needs(
    world: &World,
    plan: &BuildPlan<'_>,
    root: &Path,
    config_root: &Path,
) -> Result<Vec<LateNeed>, String> {
    let repos = portage_repo::find_repos_quiet(config_root).map_err(|e| e.to_string())?;
    let config = load_config(config_root, root, &repos)?;
    let mut rows_of: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, row) in plan.rows.iter().enumerate() {
        rows_of.entry(row.slot.node()).or_default().push(i);
    }
    let facts = Facts {
        world,
        plan,
        root,
        config,
        installed: portage_repo::all_installed_packages(root),
        db: portage_vdb::for_root(root),
        rows_of,
        by_node: world.slots.iter().map(|s| (s.node(), s)).collect(),
    };
    let mut out = Vec::new();
    for (i, row) in plan.rows.iter().enumerate() {
        let from = row.slot.node();
        let Some(atoms) = world.node_atoms.get(&from) else {
            continue;
        };
        'atoms: for (atom, &bits) in atoms {
            if bits & BUILD_KIND_MASK == 0 {
                continue;
            }
            for to in world.atom_nodes.get(atom).into_iter().flatten() {
                if *to == from
                    || (row.bootstrap.is_some()
                        && plan.bootstrap.is_some_and(|b| b.drops(&from, to)))
                {
                    continue;
                }
                let Some(to_rows) = facts.rows_of.get(to) else {
                    continue;
                };
                if to_rows.iter().any(|&j| j < i) {
                    continue;
                }
                let moved = match facts.met_at(atom, i, bits & 1 != 0) {
                    Met::Yes => continue,
                    Met::Moved(installed, sub) => Some((installed, sub)),
                    Met::No if facts.or_displaced(i, atom, bits) => continue,
                    Met::No => None,
                };
                let path = build_path(&world.edges, &world.edge_kinds, to, &from).map(|back| {
                    let mut path = vec![from.clone()];
                    path.extend(back);
                    path
                });
                out.push(LateNeed {
                    row: from.clone(),
                    atom: atom.clone(),
                    target: to.clone(),
                    path,
                    moved_binding: moved,
                });
                continue 'atoms;
            }
        }
    }
    Ok(out)
}

impl Facts<'_> {
    /// Whether an earlier row than `i` replaces the slot of `p`.
    fn replaced_before(&self, p: &InstalledPackage, i: usize) -> bool {
        self.rows_of
            .get(&format!("{}/{}:{}", p.category, p.package, p.slot))
            .is_some_and(|rows| rows.iter().any(|&j| j < i))
    }

    /// The installed `p` matches `atom`: version, slot, sub-slot, and the
    /// `[flag]` dependencies against its recorded `USE`.
    fn installed_matches(&self, atom: &str, p: &InstalledPackage) -> bool {
        let m = p.match_str(self.root);
        if !portage_dep::match_from_list(atom, &[m.as_str()]).is_some_and(|v| !v.is_empty()) {
            return false;
        }
        if !atom.contains('[') {
            return true;
        }
        let key = EntryKey::new(p.category.clone(), format!("{}-{}", p.package, p.version));
        let read = |field: &str| {
            self.db
                .aux_get(&key, field)
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        let enabled: HashSet<String> = read("USE")
            .split_whitespace()
            .map(|f| f.trim_start_matches(['+', '-']).to_string())
            .collect();
        use_deps_hold(atom, &read("IUSE"), &enabled)
    }

    /// The USE a plan row builds with: the planned USE of its slot, the
    /// boot row's reduced flag removed.
    fn row_use(&self, j: usize, c: &Candidate) -> HashSet<String> {
        let row = &self.plan.rows[j];
        let node = row.slot.node();
        let mut flags: HashSet<String> = match self.world.planned_use.get(&node) {
            Some(set) => set.iter().cloned().collect(),
            None => {
                let (category, package) = row.slot.cp.split_once('/').unwrap_or_default();
                self.use_of(c, category, package)
            }
        };
        if let Some(b) = &row.bootstrap {
            flags.remove(&b.reduced_flag);
        }
        flags
    }

    fn use_of(&self, c: &Candidate, category: &str, package: &str) -> HashSet<String> {
        portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
            config: &self.config,
            iuse: &c.iuse,
            keywords: &c.keywords,
            candidate_str: &c.match_str(category, package),
            build_id: None,
            category,
            package,
        })
        .iter()
        .cloned()
        .collect()
    }

    /// Whether the earlier row `j` meets `atom`.
    fn row_meets(&self, atom: &str, j: usize) -> bool {
        let row = &self.plan.rows[j];
        let Some(c) = row.slot.chosen() else {
            return false;
        };
        let (category, package) = row.slot.cp.split_once('/').unwrap_or_default();
        let s = c.match_str(category, package);
        if !portage_dep::match_from_list(atom, &[s.as_str()]).is_some_and(|v| !v.is_empty()) {
            return false;
        }
        !atom.contains('[') || use_deps_hold(atom, &c.iuse, &self.row_use(j, c))
    }

    /// Whether `atom` is met when row `i` starts: by an earlier row, or by an
    /// installed version no earlier row replaces. A `DEPEND` `:=` atom
    /// (`in_depend`) whose installed provider the plan moves to another
    /// sub-slot is bound to the old one, so it is `Moved`, not met.
    fn met_at(&self, atom: &str, i: usize, in_depend: bool) -> Met {
        if (0..i).any(|j| self.row_meets(atom, j)) {
            return Met::Yes;
        }
        let Some(parsed) = portage_dep::parse_atom(atom) else {
            return Met::No;
        };
        let binding = in_depend && parsed.slot_operator == Some(SlotOperator::Equals);
        let mut moved = None;
        for p in self.installed.iter().filter(|p| {
            p.category == parsed.category
                && p.package == parsed.package
                && !self.replaced_before(p, i)
                && self.installed_matches(atom, p)
        }) {
            let planned = self
                .by_node
                .get(&format!("{}/{}:{}", p.category, p.package, p.slot))
                .and_then(|s| s.chosen());
            let Some(planned) = planned.filter(|_| binding) else {
                return Met::Yes;
            };
            let installed_sub = p.match_str(self.root);
            if installed_sub.rsplit_once('/').map(|(_, sub)| sub) == Some(planned.sub_slot.as_str())
            {
                return Met::Yes;
            }
            moved.get_or_insert_with(|| (p.cpv(), planned.sub_slot.clone()));
        }
        moved.map_or(Met::No, |(cpv, sub)| Met::Moved(cpv, sub))
    }

    /// Whether re-reducing every `||` group of the build text of row `i` that
    /// holds `atom`, the arms met at the row preferred, leaves `atom` out of
    /// each of them: another arm is met, so the plan did not need this one.
    fn or_displaced(&self, i: usize, atom: &str, bits: u8) -> bool {
        let row = &self.plan.rows[i];
        let Some(c) = row.slot.chosen() else {
            return false;
        };
        let (category, package) = row.slot.cp.split_once('/').unwrap_or_default();
        let pf = format!("{package}-{}", c.version);
        let Ok(meta) = portage_repo::repo_aux_metadata(&c.repo_location, category, &pf) else {
            return false;
        };
        let use_set = self.use_of(c, category, package);
        let norm = |tok: &str| {
            portage_dep::evaluate_atom_conditionals(tok, &use_set)
                .unwrap_or_else(|| tok.to_string())
        };
        let (mut any_text, mut all_displaced) = (false, true);
        for (bit, key) in DEP_KEYS.iter().enumerate() {
            let mask = 1u8 << bit;
            if bits & mask == 0 || mask & BUILD_KIND_MASK == 0 {
                continue;
            }
            let Some(text) = meta.get(*key) else { continue };
            let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
            let in_text = use_reduce_flat(&tokens, &use_set, MatchMode::Normal)
                .is_ok_and(|flat| flat.iter().any(|t| norm(t) == atom));
            if !in_text {
                continue;
            }
            any_text = true;
            let in_depend = *key == "DEPEND";
            let mut tie = |lists: &[Vec<String>]| {
                lists
                    .iter()
                    .position(|l| l.iter().any(|t| norm(t) == atom))
                    .unwrap_or(0)
            };
            let reduced = use_reduce_flat_disjunctive(
                UseReduceFlatDisjunctiveArgs {
                    tokens: &tokens,
                    uselist: &use_set,
                    mode: MatchMode::Normal,
                    empty_groups_always_true: true,
                    tie_break: &mut tie,
                },
                &mut |branch: &[String]| {
                    let met = branch
                        .iter()
                        .filter_map(|t| {
                            let a = portage_repo::parse_dep_atom(t, "portage-world")?;
                            (a.blocker == portage_dep::Blocker::None).then(|| norm(t))
                        })
                        .all(|a| matches!(self.met_at(&a, i, in_depend), Met::Yes));
                    if met {
                        AltPreference::BestVisibleInstalled
                    } else {
                        AltPreference::Available
                    }
                },
            );
            // A text that does not reduce leaves the atom a need.
            if reduced.map_or(true, |flat| flat.iter().any(|t| norm(t) == atom)) {
                all_displaced = false;
            }
        }
        any_text && all_displaced
    }
}
