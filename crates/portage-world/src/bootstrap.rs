//! The charter 2a bootstrap of the fast plan: the reduction the resolver
//! would choose for the build-time cycles of a world, and the rows it
//! turns into (boot, middle, definitive).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use portage_profile::Config;
use portage_repo::{Candidate, ForcedOrMaskedFlagsArgs};
use portage_use_reduce::{BootstrapUse, reaches};

use crate::{Order, SlotSet, World, build_time_cycles};

type Guards = BTreeMap<(String, String), BTreeSet<String>>;

/// The single-flag reduction that breaks a world's build-time cycles.
pub struct Bootstrap {
    /// The slot built twice (`cat/pkg:slot`).
    pub node: String,
    /// The flag forced off for the boot build.
    pub flag: String,
    declared_use: Vec<String>,
    /// The flags guarding each edge between members of one cycle.
    guards: Guards,
}

impl Bootstrap {
    /// Whether the reduction drops an edge of `cycle`, so the report line
    /// can say the plan bootstraps there.
    #[must_use]
    pub fn breaks(&self, cycle: &[String]) -> bool {
        cycle.contains(&self.node)
            && self.guards.iter().any(|((from, to), flags)| {
                *from == self.node && cycle.contains(to) && flags.contains(&self.flag)
            })
    }

    fn drops(&self, from: &str, to: &str) -> bool {
        from == self.node
            && self
                .guards
                .get(&(from.to_string(), to.to_string()))
                .is_some_and(|flags| flags.contains(&self.flag))
    }
}

/// One row of the plan: the slot plus, on the boot row only, the reduction.
pub struct PlanRow<'a> {
    pub slot: &'a SlotSet,
    pub bootstrap: Option<BootstrapUse>,
}

/// The rows of a plan and the build-time cycles it reports.
pub struct BuildPlan<'a> {
    pub rows: Vec<PlanRow<'a>>,
    /// The genuine build-time cycles, members in walk order.
    pub cycles: Vec<Vec<String>>,
    pub bootstrap: Option<&'a Bootstrap>,
}

fn plain_rows<'a>(rows: &[&'a SlotSet]) -> Vec<PlanRow<'a>> {
    rows.iter()
        .map(|&slot| PlanRow {
            slot,
            bootstrap: None,
        })
        .collect()
}

impl World {
    /// The rows in `how` order, with the bootstrap applied: the boot row
    /// takes the earliest of the boot slot's own place and its dependents',
    /// the definitive row follows the last of them.
    #[must_use]
    pub fn build_plan(&self, how: &Order<'_>) -> BuildPlan<'_> {
        let rows = self.ordered(how);
        let cycles = build_time_cycles(&self.edges, &self.edge_kinds);
        let boot = self.bootstrap.as_ref();
        let Some((b, at)) =
            boot.and_then(|b| Some((b, rows.iter().position(|s| s.node() == b.node)?)))
        else {
            return BuildPlan {
                rows: plain_rows(&rows),
                cycles,
                bootstrap: boot,
            };
        };
        let keep = |from: &str, to: &str| !b.drops(from, to);
        let dependents: Vec<usize> = (0..rows.len())
            .filter(|&i| i != at && reaches(&self.edges, &rows[i].node(), &keep, &|n| n == b.node))
            .collect();
        let shift = |i: usize| if i > at { i - 1 } else { i };
        let boot_at = dependents
            .iter()
            .map(|&i| shift(i))
            .min()
            .unwrap_or(shift(at))
            .min(shift(at));
        let mut out = plain_rows(&rows);
        out.remove(at);
        out.insert(
            boot_at,
            PlanRow {
                slot: rows[at],
                bootstrap: Some(BootstrapUse {
                    reduced_flag: b.flag.clone(),
                    declared_use: b.declared_use.clone(),
                }),
            },
        );
        let last = dependents
            .iter()
            .map(|&i| shift(i))
            .max()
            .map(|p| if p >= boot_at { p + 1 } else { p });
        let definitive_at = last.map_or(out.len(), |p| p + 1).min(out.len());
        out.insert(
            definitive_at,
            PlanRow {
                slot: rows[at],
                bootstrap: None,
            },
        );
        BuildPlan {
            rows: out,
            cycles,
            bootstrap: boot,
        }
    }
}

/// The metadata of the version a plan would install in `slot`.
fn chosen_metadata(slot: &SlotSet) -> Option<(&Candidate, Arc<HashMap<String, String>>)> {
    let best = slot.chosen()?;
    let (category, package) = slot.cp.split_once('/')?;
    let pf = format!("{package}-{}", best.version);
    let meta = portage_repo::repo_aux_metadata(&best.repo_location, category, &pf).ok()?;
    Some((best, meta))
}

/// The USE flags guarding each edge between members of one cycle: the flags
/// whose removal drops every build-time arm naming the dependency's `cp`
/// (read from the raw `*DEPEND` strings; the edge's existence already
/// encodes the evaluated USE, so the relation is purely syntactic).
fn cycle_guards(
    cycles: &[Vec<String>],
    world: &World,
    by_node: &BTreeMap<String, &SlotSet>,
) -> Guards {
    let mut out = Guards::new();
    for (from, to) in cycles.iter().flat_map(|c| {
        c.iter().flat_map(|m| {
            world
                .edges
                .get(m)
                .into_iter()
                .flatten()
                .filter(|n| c.contains(n))
                .map(move |n| (m, n))
        })
    }) {
        let Some((_, meta)) = by_node.get(from.as_str()).and_then(|s| chosen_metadata(s)) else {
            continue;
        };
        let dep_cp = to.rsplit_once(':').map_or(to.as_str(), |(cp, _)| cp);
        let mut candidates: BTreeSet<String> = BTreeSet::new();
        let mut arms: Vec<BTreeSet<String>> = Vec::new();
        for text in ["DEPEND", "BDEPEND", "IDEPEND"]
            .iter()
            .filter_map(|k| meta.get(*k))
        {
            for (atom, guards) in portage_use_reduce::conditional_guards(text) {
                candidates.extend(guards.iter().cloned());
                let named = portage_dep::parse_atom(&atom).is_some_and(|a| {
                    a.blocker == portage_dep::Blocker::None
                        && format!("{}/{}", a.category, a.package) == dep_cp
                });
                if named {
                    arms.push(guards);
                }
            }
        }
        // Universally quantified: an unguarded arm naming the dep keeps the
        // edge, so no single flag drops it.
        let dropping: BTreeSet<String> = candidates
            .into_iter()
            .filter(|f| !arms.is_empty() && arms.iter().all(|g| g.contains(f)))
            .collect();
        if !dropping.is_empty() {
            out.insert((from.clone(), to.clone()), dropping);
        }
    }
    out
}

/// The bootstrap for `world`'s build-time cycles, chosen by the resolver's
/// own rule ([`portage_repo::bootstrap_flag_eligible`]), or `None` when no
/// single-flag reduction applies (no flag breaks a cycle, the boot build
/// still needs a member, or more than 3 rows would be affected).
pub(crate) fn select(world: &World, config: &Config) -> Option<Bootstrap> {
    let cycles = build_time_cycles(&world.edges, &world.edge_kinds);
    if cycles.is_empty() {
        return None;
    }
    let by_node: BTreeMap<String, &SlotSet> = world.slots.iter().map(|s| (s.node(), s)).collect();
    let guards = cycle_guards(&cycles, world, &by_node);
    let eligible = |node: &str, flag: &str, keep_required_use: bool| {
        let (Some(use_set), Some((best, meta))) = (
            world.planned_use.get(node),
            by_node.get(node).and_then(|s| chosen_metadata(s)),
        ) else {
            return false;
        };
        let (category, package) = node
            .split_once(':')
            .map_or(node, |(cp, _)| cp)
            .split_once('/')
            .unwrap_or_default();
        let forced = portage_repo::forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: &best.iuse,
            keywords: &best.keywords,
            candidate_str: &best.match_str(category, package),
            build_id: None,
            category,
            package,
            config,
        });
        let use_flags: HashSet<String> = use_set.iter().cloned().collect();
        portage_repo::bootstrap_flag_eligible(&meta, &use_flags, &forced, flag, keep_required_use)
    };
    let (node, flag) = portage_use_reduce::bootstrap_reduction_strict_first(
        &cycles,
        &world.edges,
        &world.edge_kinds,
        &guards,
        &eligible,
    )?;
    let members: BTreeSet<String> = cycles.iter().flatten().cloned().collect();
    if members.len() + 1 > 3
        || portage_use_reduce::boot_closure_reaches_member(
            &world.edges,
            &world.edge_kinds,
            &guards,
            &node,
            &flag,
            &members,
        )
    {
        return None;
    }
    let declared_use = world.planned_use.get(&node)?.iter().cloned().collect();
    Some(Bootstrap {
        node,
        flag,
        declared_use,
        guards,
    })
}
