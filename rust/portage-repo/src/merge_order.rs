//! Real `_emerge/depgraph.py::_serialize_tasks` -- portage's own merge-order
//! scheduler -- ported over a typed dependency digraph.
//!
//! The pre-2026-09-06 implementation approximated this with a batched
//! Kahn's walk over cp-level `required_by` edges plus a discovery-rank
//! tie-break. That reproduced the *set* of merge tasks but not their
//! order, because real's scheduler is not a topological sort at all: it
//! is a repeated "pop every currently-eligible leaf" loop over a digraph
//! whose edges carry a real `DepPriority` (build-time / run-time /
//! run-time-post / slot-operator / optional / already-satisfied), with a
//! priority-relaxation ladder for cycles, a `_merge_order_bias`
//! pre-sort, and an `asap_nodes` fast path for freshly-unblocked
//! `PDEPEND` children.
//!
//! Every piece of that is needed for the order to come out right -- the
//! ladder decides *which* nodes are eligible in a round, the bias
//! decides their sequence within it, and `asap_nodes` overrides both for
//! `PDEPEND` children. Validated end to end against real portage's own
//! `emerge -p --debug` digraph dump on a live ~1850-node Gentoo system:
//! feeding this algorithm real's dumped graph reproduces real's merge
//! list exactly.
//!
//! The one deliberate scope reduction, also verified against that dump:
//! the graph only needs the *forward transitive dependency closure of
//! the merge-bound packages* (461 nodes for the validation case), not
//! real's full `@world`/`@system` universe (1854 nodes). Real prunes its
//! own graph down before scheduling ("Prune 'nomerge' root nodes if
//! nothing depends on them", `depgraph.py:9509-9518`), and re-running
//! the algorithm on just the closure yields the identical merge list.
//!
//! The selection loop runs over an incrementally-maintained leaf
//! frontier (`SerializeFrontier` below, a port of real
//! `_emerge/_serialize_frontier.py`): per-node, per-filter
//! surviving-child counts plus per-filter ready heaps, so each ladder
//! rung enumerates its leaves without an O(V+E) scan and each removal
//! only decrements its parents. `PORTAGE_SERIALIZE_FRONTIER_DISABLE`
//! falls back to the plain scans (real's own escape hatch). Pure
//! plumbing: leaf sets, order, and every downstream decision are
//! identical with the frontier on or off (pinned by unit tests).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::Path;

use crate::{
    BlockerSatisfiedBy, CandidateSource, GraphEntry, PretendOutcome, RepoConfig,
    VisibilityProvenance, all_installed_packages, read_vdb_flag_set, read_vdb_slot,
};

/// Real `_emerge/DepPriority.py::DepPriority` -- the per-edge dependency
/// classification that drives every `ignore_priority` decision in
/// `_serialize_tasks`.
///
/// `cross` (a dependency crossing into a different `ROOT`) is always
/// `false` here: portuale resolves one root at a time, so real's
/// `self._cross(pkg.root)` is constantly false and the two
/// `runtime_slot_op and not priority.cross` guards below collapse to
/// plain `runtime_slot_op`. `ignored` is likewise not modelled -- real
/// only ever sets it for `--root-deps=rdeps` / `--with-bdeps=n` on a
/// built package, both of which portuale expresses by dropping the
/// `DEPEND`/`BDEPEND` keys from the walk entirely (see
/// `dep_edges_from_metadata`'s callers), which is what real's own
/// `edepend["DEPEND"] = ""` does one line later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DepPriority {
    pub buildtime: bool,
    pub runtime: bool,
    pub runtime_post: bool,
    pub buildtime_slot_op: bool,
    pub runtime_slot_op: bool,
    pub optional: bool,
    /// Real `mypriority.satisfied = inst_pkg` -- an already-installed
    /// package matches this atom, so the edge can be relaxed to break a
    /// cycle (`DepPrioritySatisfiedRange`). Filled in by
    /// `build_digraph`, which is the first point at which every entry's
    /// resolved slot is known.
    pub satisfied: bool,
}

/// One resolved dependency of a `GraphEntry`, in real
/// `_add_pkg_dep_string`'s own `deps`-tuple key order (`RDEPEND`,
/// `IDEPEND`, `PDEPEND`, `DEPEND`, `BDEPEND` -- `depgraph.py:4253-4289`).
///
/// The atom string is kept alongside the resolved `category`/`package`
/// so `build_digraph` can decide `DepPriority::satisfied` against the
/// installed vdb once the whole graph is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepEdge {
    pub atom: String,
    /// The token with its use-deps evaluated against the parent's
    /// USE (`portage_dep::evaluate_atom_conditionals`), for real's
    /// `--debug` `Candidates:` line (#138; oracle
    /// `logs/l111-s0-20260921/real-rest-debug.log`). Note the token
    /// itself is already reduced-raw, not true-raw:
    /// `dep_edges_from_metadata` runs `use_reduce_structured` first,
    /// which evaluates `flag?()` conditionals away -- so real's first,
    /// true-raw `Depstring:` stanza (`Depstring: plasma? ( ... )`) has
    /// no portuale counterpart (known remainder, unnumbered display
    /// gap). Falls back to the token when evaluation returns `None`.
    pub evaluated: String,
    pub category: String,
    pub package: String,
    pub priority: DepPriority,
    /// Real `depgraph._queue_disjunctive_deps`: this atom sits inside a
    /// `|| ( … )` group, or is itself a `virtual/*` atom (which real's
    /// `dep_check` expands into one). Real does **not** walk these
    /// inline -- it collects each dep key's whole disjunctive set into
    /// one bundle on `_dep_disjunctive_stack`, which `_create_graph`
    /// only pops once the ordinary `_dep_stack` has drained completely.
    /// That deferral is load-bearing for `.order`, and therefore for
    /// merge order: it is why a `virtual/*` dependency lands hundreds of
    /// positions later in real's graph than the sibling atoms declared
    /// next to it.
    pub disjunctive: bool,
    /// For a `disjunctive` atom that came from an actual `|| ( … )`
    /// group: `(group, branch)` -- `group` counts the `||` groups in
    /// this dep key, `branch` the alternative within one group.
    /// `build_digraph` keeps one branch per group (`dep_zapdeps`'
    /// `choice_bins`: all-in-graph, then all-installed, then first) and
    /// suppresses the rest. `None` for an inline atom or a bare deferred
    /// `virtual/*`.
    pub alt: Option<(u32, u32)>,
    /// Which of real's own `deps`-tuple keys this came from
    /// (0=`RDEPEND`, 1=`IDEPEND`, 2=`PDEPEND`, 3=`DEPEND`, 4=`BDEPEND`).
    /// Disjunctive bundles are queued one per key, so the walk needs the
    /// key boundaries.
    pub key: u8,
}

/// Real `depgraph._queue_disjunctive_deps`, run over one USE-reduced dep
/// key: returns `(inline, disjunctive)` atom lists.
///
/// Real walks the `opconvert=True` reduced struct and, for each element:
/// a `|| ( … )` group goes wholesale into the deferred bundle; a plain
/// all-of group recurses (contributing to that same bundle); a bare atom
/// is deferred too when its category is `virtual` ("Eventually this will
/// check for PROPERTIES=virtual"), and yielded inline otherwise.
///
/// Portuale keeps every branch of a `||` group rather than resolving one
/// up front; `build_digraph` then picks `dep_zapdeps`' first-satisfiable
/// branch (see its own doc). Each disjunctive atom is tagged with the
/// `(group, branch)` of the `|| ( … )` alternative it belongs to --
/// `group` counts the distinct `||` groups reached in this key,
/// `branch` the alternative within one group (a bare atom is its own
/// single-atom branch; a nested `( … )` is a multi-atom branch). A
/// `virtual/*` atom outside any `||` is deferred too (like real) with
/// `group`/`branch` `u32::MAX` so `build_digraph` never groups it with a
/// real alternative.
fn split_disjunctive(tokens: &[String]) -> (Vec<String>, Vec<(String, u32, u32)>) {
    let mut inline: Vec<String> = Vec::new();
    let mut disjunctive: Vec<(String, u32, u32)> = Vec::new();
    // Depth of the innermost enclosing `|| ( … )`, if any: everything
    // below one is disjunctive, however deeply nested.
    let mut any_of_depth: Option<usize> = None;
    let mut depth: usize = 0;
    let mut pending_any_of = false;
    let mut group: u32 = 0;
    let mut next_group: u32 = 0;
    let mut branch: u32 = 0;
    // Depth at which the current `branch`'s nested `( … )` opened, if any;
    // the branch stays fixed for every atom until that group's `)`.
    let mut branch_group_depth: Option<usize> = None;
    for tok in tokens {
        match tok.as_str() {
            "||" => pending_any_of = true,
            "(" => {
                depth += 1;
                if pending_any_of && any_of_depth.is_none() {
                    any_of_depth = Some(depth);
                    group = next_group;
                    next_group += 1;
                    branch = 0;
                    branch_group_depth = None;
                } else if any_of_depth.is_some()
                    && Some(depth) == any_of_depth.map(|d| d + 1)
                    && branch_group_depth.is_none()
                {
                    // A nested `( … )` alternative directly inside the `||`.
                    branch_group_depth = Some(depth);
                }
                pending_any_of = false;
            }
            ")" => {
                if branch_group_depth == Some(depth) {
                    branch_group_depth = None;
                    branch += 1;
                }
                if any_of_depth == Some(depth) {
                    any_of_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {
                pending_any_of = false;
                let is_virtual =
                    portage_dep::parse_atom(tok).is_some_and(|a| a.category == "virtual");
                if let Some(aod) = any_of_depth {
                    disjunctive.push((tok.clone(), group, branch));
                    if depth == aod {
                        // A bare atom directly in the `||` is its own branch.
                        branch += 1;
                    }
                } else if is_virtual {
                    disjunctive.push((tok.clone(), u32::MAX, u32::MAX));
                } else {
                    inline.push(tok.clone());
                }
            }
        }
    }
    (inline, disjunctive)
}

/// The dependency keys real walks, paired with the `DepPriority` each
/// contributes -- real `depgraph.py:4253-4289`'s own `deps` tuple.
/// `optional` is `pkg.built` for the two build-time keys (a built
/// package's build deps are informational only); `IDEPEND` is a
/// `runtime` priority like `RDEPEND` (real also sets `installtime`,
/// which no `ignore_priority` predicate reads).
fn key_priority(key: &str, built: bool) -> DepPriority {
    match key {
        "RDEPEND" | "IDEPEND" => DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        "PDEPEND" => DepPriority {
            runtime_post: true,
            ..DepPriority::default()
        },
        // "DEPEND" | "BDEPEND"
        _ => DepPriority {
            buildtime: true,
            optional: built,
            ..DepPriority::default()
        },
    }
}

/// `GraphEntry::deps`: flattens `metadata`'s own dep-key strings, one key
/// at a time in `keys`' given order (so cross-key token order can't leak
/// in), against `use_flags`. `use_reduce_flat` transparently drops
/// `||`/`(`/`)` markers, listing every branch's atoms in written order --
/// the *other* branches simply never correspond to a real entry, so the
/// digraph's own lookup skips them for free.
///
/// One `DepEdge` per distinct `(atom, category, package, priority)`: real
/// records a *list* of priorities per digraph edge (`digraph.add`'s
/// `bisect.insort`), and `leaf_nodes`/`child_nodes` need every one of
/// them, so an atom named by both `RDEPEND` and `DEPEND` contributes two
/// edges here rather than being deduped to the first. The `atom` is part
/// of the dedup key (not just `cat/pkg`) so the distinct branches of a
/// `|| ( >=foo-2:2 >=foo-1:1 )` group both survive -- `build_digraph`
/// matches each atom against the resolved entries individually, so the
/// branch that doesn't resolve is dropped there, not here.
///
/// `built` is real `pkg.built` -- true for a binary candidate and for an
/// installed package -- and only affects whether the build-time keys are
/// marked `optional`. Callers that drop `DEPEND`/`BDEPEND` from `keys`
/// entirely (real's `edepend["DEPEND"] = ""` for a built package under
/// `--with-bdeps=n`) never reach that branch.
pub(crate) fn dep_edges_from_metadata(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    keys: &[&str],
    built: bool,
) -> Vec<DepEdge> {
    let mut edges: Vec<DepEdge> = Vec::new();
    let mut seen: HashSet<(String, String, String, DepPriority)> = HashSet::new();
    for dep_key in keys {
        let Some(d) = metadata.get(*dep_key) else {
            continue;
        };
        let base = key_priority(dep_key, built);
        let key_index = match *dep_key {
            "RDEPEND" => 0,
            "IDEPEND" => 1,
            "PDEPEND" => 2,
            "DEPEND" => 3,
            _ => 4,
        };
        let toks: Vec<String> = d.split_whitespace().map(String::from).collect();
        // The structured reduction keeps the `||`/`( … )` boundaries
        // `split_disjunctive` needs; real reduces the same string with
        // `opconvert=True` before handing it to
        // `_queue_disjunctive_deps`.
        let Ok(structured) = portage_use_reduce::use_reduce_structured(
            &toks,
            use_flags,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };
        let (inline, deferred) = split_disjunctive(&structured);
        let flat: Vec<(String, Option<(u32, u32)>)> = inline
            .into_iter()
            .map(|a| (a, None))
            .chain(deferred.into_iter().map(|(a, g, b)| (a, Some((g, b)))))
            .collect();
        for (t, alt) in flat {
            if t == "||" {
                continue;
            }
            let disjunctive = alt.is_some();
            let Some(dep_atom) = portage_dep::parse_atom(&t) else {
                crate::note_unparsed_dep_token(&t, "merge-order digraph");
                continue;
            };
            if dep_atom.blocker != portage_dep::Blocker::None {
                continue;
            }
            // Real `_wrapped_add_pkg_dep_string`: a `:=`/`:slot=` atom
            // promotes its own key's priority to the slot-operator
            // variant, which the `ignore_priority` ladder refuses to
            // relax as readily as a plain one.
            let mut priority = base;
            if dep_atom.slot_operator == Some(portage_dep::SlotOperator::Equals) {
                if priority.buildtime {
                    priority.buildtime_slot_op = true;
                }
                if priority.runtime {
                    priority.runtime_slot_op = true;
                }
            }
            let key = (
                t.clone(),
                dep_atom.category.clone(),
                dep_atom.package.clone(),
                priority,
            );
            if seen.insert(key) {
                edges.push(DepEdge {
                    atom: t.clone(),
                    evaluated: portage_dep::evaluate_atom_conditionals(&t, use_flags)
                        .unwrap_or_else(|| t.clone()),
                    category: dep_atom.category,
                    package: dep_atom.package,
                    priority,
                    disjunctive,
                    alt,
                    key: key_index,
                });
            }
        }
    }
    edges
}

// ---------------------------------------------------------------------
// `ignore_priority` predicates
// ---------------------------------------------------------------------

type Ignore = fn(&DepPriority) -> bool;

// `_emerge/DepPriorityNormalRange.py`.
fn n_ignore_optional(p: &DepPriority) -> bool {
    p.optional
}
fn n_ignore_runtime_post(p: &DepPriority) -> bool {
    p.optional || p.runtime_post
}
fn n_ignore_runtime(p: &DepPriority) -> bool {
    !p.runtime_slot_op && (p.optional || !p.buildtime)
}

// `_emerge/DepPrioritySatisfiedRange.py`.
fn s_ignore_optional(p: &DepPriority) -> bool {
    p.optional
}
fn s_ignore_satisfied_runtime_post(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if !p.satisfied {
        return false;
    }
    if p.buildtime || p.runtime {
        return false;
    }
    p.runtime_post
}
fn s_ignore_runtime_post(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime || p.runtime {
        return false;
    }
    p.runtime_post
}
fn s_ignore_satisfied_runtime(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime {
        return false;
    }
    if !p.runtime {
        return true;
    }
    p.satisfied
}
fn s_ignore_satisfied_buildtime(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime_slot_op {
        return false;
    }
    p.satisfied
}
fn s_ignore_satisfied_buildtime_slot_op(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.satisfied {
        return true;
    }
    !p.buildtime && !p.runtime
}
fn s_ignore_runtime(p: &DepPriority) -> bool {
    (!p.runtime_slot_op || p.satisfied) && (p.satisfied || p.optional || !p.buildtime)
}

/// Real `ignore_priority.__name__` for the `_serialize_tasks` trace
/// (`PORTUALE_MO_SEL`), so a portuale `MO_SEL` line lines up field-for-
/// field with the `RT_SEL` line `TEST/scripts/mo-trace/real-trace.py`
/// injects into real's loop. `None` is real's `ignore_priority = None`.
fn ignore_name(ig: Option<Ignore>) -> &'static str {
    let Some(f) = ig else {
        return "none";
    };
    let addr = f as usize;
    for (pred, name) in [
        (n_ignore_optional as Ignore, "ignore_optional"),
        (n_ignore_runtime_post as Ignore, "ignore_runtime_post"),
        (n_ignore_runtime as Ignore, "ignore_runtime"),
        (s_ignore_optional as Ignore, "ignore_optional"),
        (
            s_ignore_satisfied_runtime_post as Ignore,
            "ignore_satisfied_runtime_post",
        ),
        (s_ignore_runtime_post as Ignore, "ignore_runtime_post"),
        (
            s_ignore_satisfied_runtime as Ignore,
            "ignore_satisfied_runtime",
        ),
        (
            s_ignore_satisfied_buildtime as Ignore,
            "ignore_satisfied_buildtime",
        ),
        (
            s_ignore_satisfied_buildtime_slot_op as Ignore,
            "ignore_satisfied_buildtime_slot_op",
        ),
        (s_ignore_runtime as Ignore, "ignore_runtime"),
    ] {
        if addr == pred as usize {
            return name;
        }
    }
    "unknown"
}

/// Real's two `DepPriority*Range` classes: an `ignore_priority` ladder
/// indexed `NONE=0 .. MEDIUM`, plus the named rungs `_serialize_tasks`
/// reaches for directly.
struct PriorityRange {
    ignore: &'static [Option<Ignore>],
    medium: usize,
    medium_soft: usize,
    medium_post: usize,
}

static NORMAL: PriorityRange = PriorityRange {
    ignore: &[
        None,
        Some(n_ignore_optional),
        Some(n_ignore_runtime_post),
        Some(n_ignore_runtime),
    ],
    medium: 3,
    medium_soft: 2,
    medium_post: 2,
};

static SATISFIED: PriorityRange = PriorityRange {
    ignore: &[
        None,
        Some(s_ignore_optional),
        Some(s_ignore_satisfied_runtime_post),
        Some(s_ignore_runtime_post),
        Some(s_ignore_satisfied_runtime),
        Some(s_ignore_satisfied_buildtime),
        Some(s_ignore_satisfied_buildtime_slot_op),
        Some(s_ignore_runtime),
    ],
    medium: 7,
    medium_soft: 6,
    medium_post: 3,
};

impl PriorityRange {
    fn ig(&self, i: usize) -> Option<Ignore> {
        self.ignore[i]
    }
    fn ig_medium(&self) -> Option<Ignore> {
        self.ignore[self.medium]
    }
    fn ig_medium_soft(&self) -> Option<Ignore> {
        self.ignore[self.medium_soft]
    }
}

// ---------------------------------------------------------------------
// the digraph
// ---------------------------------------------------------------------

/// A `portage.util.digraph` restricted to what `_serialize_tasks` reads:
/// per-node child/parent adjacency with a priority *list* per edge, plus
/// `order` (the sequence nodes were added in, which real's own
/// `leaf_nodes()` iterates and `_merge_order_bias` re-sorts in place).
struct Digraph {
    /// Node `i` is `entries[i]`.
    n: usize,
    /// `children[i]` = `(child, priorities)`, in edge-creation order.
    children: Vec<Vec<(usize, Vec<DepPriority>)>>,
    parents: Vec<Vec<usize>>,
    order: Vec<usize>,
    /// Real `node.installed` -- a "nomerge" node, present only to
    /// constrain and inform merge order, never emitted.
    installed: Vec<bool>,
    alive: Vec<bool>,
}

impl Digraph {
    /// Real `digraph.child_nodes(node, ignore_priority)`: a child counts
    /// when *at least one* of the edge's priorities survives the filter.
    fn child_nodes(&self, i: usize, ig: Option<Ignore>) -> Vec<usize> {
        self.children[i]
            .iter()
            .filter(|(c, prios)| self.alive[*c] && ig.is_none_or(|f| prios.iter().any(|p| !f(p))))
            .map(|(c, _)| *c)
            .collect()
    }

    fn is_leaf(&self, i: usize, ig: Option<Ignore>) -> bool {
        !self.children[i]
            .iter()
            .any(|(c, prios)| self.alive[*c] && ig.is_none_or(|f| prios.iter().any(|p| !f(p))))
    }

    fn has_parents(&self, i: usize) -> bool {
        self.parents[i].iter().any(|&p| self.alive[p])
    }

    /// Real `digraph.leaf_nodes(ignore_priority)` -- every alive leaf, in
    /// `order` sequence.
    fn leaf_nodes(&self, ig: Option<Ignore>) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|&i| self.alive[i] && self.is_leaf(i, ig))
            .collect()
    }

    fn add_edge(&mut self, parent: usize, child: usize, priority: DepPriority) {
        if let Some(slot) = self.children[parent].iter_mut().find(|(c, _)| *c == child) {
            if !slot.1.contains(&priority) {
                slot.1.push(priority);
            }
            return;
        }
        self.children[parent].push((child, vec![priority]));
        self.parents[child].push(parent);
    }
}

// ---------------------------------------------------------------------
// the leaf frontier (real `_emerge/_serialize_frontier.py`)
// ---------------------------------------------------------------------

/*
 * Incrementally-maintained leaf frontier for the `select_nodes` loop,
 * a port of real `_emerge/_serialize_frontier.py` (`_SerializeFrontier`;
 * the `_FrontierDigraph` wrapper subclass has no portuale equivalent
 * because removals here go through the one `alive[i] = false` site in
 * `select_nodes`, which notifies the frontier explicitly instead).
 *
 * The selection loop used to ask `Digraph::leaf_nodes` which nodes are
 * leaves under an `ignore_priority` filter on every ladder rung of every
 * iteration -- one O(V+E) scan per query. The frontier keeps, per node
 * and per filter level, a count of how many children have an edge
 * surviving that level's filter; the node is a leaf under level L iff
 * its count for L is zero. Removing a node decrements its parents'
 * counts; each level owns a min-heap of order-indices of its current
 * leaves, so leaves enumerate in `order` without a walk. Heaps use lazy
 * deletion: an entry is valid iff the node is still alive and still a
 * leaf under the level.
 *
 * Two deliberate narrowings vs real, both forced by portuale's model:
 * nodes here are dense `usize` indices (not opaque `Package` objects),
 * so counts/index maps are `Vec`s, and nothing is ever re-added -- the
 * loop only ever clears `alive` -- so there is no `_assign_index` fresh
 * path and no index-identity check on pop (an order-index is assigned
 * once at build and never changes; `order.retain` preserves relative
 * order, so heap order and scan order agree by construction).
 * `add_edge` below exists for parity with real's mutation surface and
 * is pinned by its own unit test; the selection loop never calls it
 * (real only grows the graph mid-loop for uninstall reversal and
 * blocker edges, and a `--pretend` merge graph has neither -- see the
 * scope-backlog entry for this slice).
 */

/// Real `_SerializeFrontier`: per-node, per-level surviving-child counts
/// plus per-level ready heaps. A node is a leaf under level L iff
/// `surv[node][L] == 0`.
struct SerializeFrontier {
    /// Level -> filter (`None` at level 0: a leaf iff it has no children
    /// at all). The deduped union of both `PriorityRange` ladders, so
    /// every filter the loop can ask about has a level.
    levels: Vec<Option<Ignore>>,
    /// Per-node surviving-child count per level.
    surv: Vec<Vec<u32>>,
    /// `(parent, child)` -> survival bitmask over levels.
    edge_mask: HashMap<(usize, usize), u64>,
    /// Node -> order-index; order-index -> node. Assigned once at build
    /// from `order`; never changes afterwards.
    index: Vec<usize>,
    by_index: Vec<usize>,
    /// Per-level min-heap of order-indices currently believed leaf.
    ready: Vec<BinaryHeap<Reverse<usize>>>,
}

impl SerializeFrontier {
    /// The filter union both ladders query: `None` first, then every
    /// rung of `NORMAL` and `SATISFIED` in ladder order, deduplicated by
    /// function identity (real deduplicates by object identity too).
    /// Textually identical rungs share a level when the toolchain folds
    /// them to one address (observed: the two `p.optional` rungs) --
    /// behavior-preserving, since identical bodies filter identically
    /// and every consumer only ever asks "leaves under filter F".
    ///
    /// On the `unpredictable_function_pointer_comparisons` lint this
    /// relies on below: a conflated level still computes the exact same
    /// leaf sets for the reason above, and every consumer only ever asks
    /// "leaves under filter F", never "which level index". The only
    /// observable would be fewer internal levels, which no output
    /// depends on (and the equivalence tests pin the leaf sets, not the
    /// level count).
    #[allow(unpredictable_function_pointer_comparisons)]
    fn build_levels() -> Vec<Option<Ignore>> {
        let mut levels: Vec<Option<Ignore>> = vec![None];
        for f in NORMAL
            .ignore
            .iter()
            .chain(SATISFIED.ignore.iter())
            .copied()
            .flatten()
        {
            if !levels.contains(&Some(f)) {
                levels.push(Some(f));
            }
        }
        levels
    }

    fn build(g: &Digraph) -> Self {
        let levels = Self::build_levels();
        debug_assert!(levels.len() <= 64, "edge masks are u64");
        let nlevels = levels.len();
        let n = g.n;
        let mut index = vec![0usize; n];
        let mut by_index = vec![0usize; g.order.len()];
        for (pos, &node) in g.order.iter().enumerate() {
            index[node] = pos;
            by_index[pos] = node;
        }
        let mut edge_mask: HashMap<(usize, usize), u64> = HashMap::new();
        let mut surv = vec![vec![0u32; nlevels]; n];
        for &node in &g.order {
            for (child, prios) in &g.children[node] {
                let mask = Self::compute_mask(&levels, prios);
                edge_mask.insert((node, *child), mask);
                let mut m = mask;
                while m != 0 {
                    let l = m.trailing_zeros() as usize;
                    surv[node][l] += 1;
                    m &= m - 1;
                }
            }
        }
        // Seed each level's ready heap with its initial leaves, in
        // order-index order -- ascending pushes need no heapify.
        let mut ready: Vec<BinaryHeap<Reverse<usize>>> =
            (0..nlevels).map(|_| BinaryHeap::new()).collect();
        for &node in &g.order {
            let idx = index[node];
            for l in 0..nlevels {
                if surv[node][l] == 0 {
                    ready[l].push(Reverse(idx));
                }
            }
        }
        Self {
            levels,
            surv,
            edge_mask,
            index,
            by_index,
            ready,
        }
    }

    /// Bitmask of the levels whose filter an edge with `prios`
    /// survives. Level 0 (`None`) is always set; for level L > 0 the
    /// edge survives iff some priority passes the filter -- exactly
    /// `Digraph::is_leaf`'s per-edge test, as in real `_compute_mask`.
    fn compute_mask(levels: &[Option<Ignore>], prios: &[DepPriority]) -> u64 {
        let mut mask = 1u64;
        for (l, f) in levels.iter().enumerate().skip(1) {
            let f = f.expect("levels past 0 always hold a filter");
            if prios.iter().any(|p| !f(p)) {
                mask |= 1 << l;
            }
        }
        mask
    }

    /// Level index for a filter, or `None` if untracked (every filter
    /// the loop uses is tracked; the fallback is defensive, mirroring
    /// real's `level_of` returning `None`). See `build_levels` on why
    /// pointer comparison is sound here.
    #[allow(unpredictable_function_pointer_comparisons)]
    fn level_of(&self, ig: Option<Ignore>) -> Option<usize> {
        self.levels.iter().position(|&f| f == ig)
    }

    fn is_leaf(&self, node: usize, level: usize) -> bool {
        self.surv.get(node).is_some_and(|counts| counts[level] == 0)
    }

    /// The alive leaves under `level`, in `order` sequence -- the
    /// frontier equivalent of `Digraph::leaf_nodes`. Drains the level's
    /// heap, discarding stale entries (dead nodes and nodes that
    /// stopped being leaves), and rebuilds it from the survivors;
    /// entries pop in ascending order-index order, so the result is in
    /// `order` and the rebuilt heap needs no heapify. `alive` is the
    /// graph's liveness vector (nodes are never deleted here, only
    /// flagged, unlike real's dict-keyed graph).
    fn ready_nodes(&mut self, level: usize, alive: &[bool]) -> Vec<usize> {
        let mut result: Vec<usize> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        while let Some(Reverse(idx)) = self.ready[level].pop() {
            if !seen.insert(idx) {
                continue;
            }
            let Some(&node) = self.by_index.get(idx) else {
                continue;
            };
            if !alive.get(node).copied().unwrap_or(false) {
                continue;
            }
            if !self.is_leaf(node, level) {
                continue;
            }
            result.push(node);
        }
        self.ready[level] = BinaryHeap::from(
            result
                .iter()
                .map(|&node| Reverse(self.index[node]))
                .collect::<Vec<_>>(),
        );
        result
    }

    /// Account for `node` leaving the graph. `children`/`parents` are
    /// its adjacency as it stands (only the adjacency and the masks
    /// matter here, so call before or after flipping `alive`). Each
    /// removed edge decrements the parent's per-level counts wherever
    /// the edge had survived; a count hitting zero pushes the parent's
    /// index (lazily validated on pop). A removed node's own counts are
    /// poisoned so `is_leaf` stays false for it, mirroring real's
    /// `surv.pop`.
    fn remove(&mut self, node: usize, children: &[(usize, Vec<DepPriority>)], parents: &[usize]) {
        for &parent in parents {
            let Some(mask) = self.edge_mask.remove(&(parent, node)) else {
                continue;
            };
            let Some(pcounts) = self.surv.get_mut(parent) else {
                continue;
            };
            let Some(&pidx) = self.index.get(parent) else {
                continue;
            };
            let mut m = mask;
            while m != 0 {
                let l = m.trailing_zeros() as usize;
                pcounts[l] = pcounts[l].saturating_sub(1);
                if pcounts[l] == 0 {
                    self.ready[l].push(Reverse(pidx));
                }
                m &= m - 1;
            }
        }
        for (child, _) in children {
            self.edge_mask.remove(&(node, *child));
        }
        if let Some(counts) = self.surv.get_mut(node) {
            counts.fill(u32::MAX);
        }
    }

    /// Account for `digraph.add(node, parent, priority)`: an edge from
    /// `parent` to `node` whose priority list just grew to `priorities`.
    /// Adding a priority only makes an edge survive more levels, so the
    /// parent's counts only increase; a parent leaving a level's ready
    /// set is handled lazily on pop. Not exercised by the selection
    /// loop (which never adds edges); pinned by unit test for parity
    /// with real's mutation surface.
    #[allow(dead_code)]
    fn add_edge(&mut self, node: usize, parent: Option<usize>, priorities: &[DepPriority]) {
        let ensure = |slf: &mut Self, n: usize| {
            if n >= slf.surv.len() {
                let idx = slf.by_index.len();
                slf.by_index.push(n);
                slf.index.resize(n + 1, 0);
                slf.index[n] = idx;
                slf.surv.resize(n + 1, vec![0u32; slf.levels.len()]);
                for heap in slf.ready.iter_mut() {
                    heap.push(Reverse(idx));
                }
            }
        };
        ensure(self, node);
        let Some(parent) = parent else { return };
        ensure(self, parent);
        let new_mask = Self::compute_mask(&self.levels, priorities);
        let old_mask = self.edge_mask.get(&(parent, node)).copied().unwrap_or(0);
        if new_mask == old_mask {
            return;
        }
        self.edge_mask.insert((parent, node), new_mask);
        let mut delta = new_mask & !old_mask;
        while delta != 0 {
            let l = delta.trailing_zeros() as usize;
            self.surv[parent][l] += 1;
            delta &= delta - 1;
        }
    }
}

/// Real `PORTAGE_SERIALIZE_FRONTIER_DISABLE`: fall back to the plain
/// `leaf_nodes()`/`is_leaf()` scans (frontier never built). An escape
/// hatch for debugging and for A/B perf comparison, same as upstream.
fn frontier_enabled() -> bool {
    std::env::var_os("PORTAGE_SERIALIZE_FRONTIER_DISABLE").is_none()
}

/// `PORTUALE_MO_SEL=1`: emit one `MO_SEL ` line per `select_nodes`
/// iteration to stderr -- the portuale half of the real-vs-portuale
/// merge-order trace harness (`TEST/scripts/mo-trace/`). The real half is
/// `real-trace.py`, which injects an `RT_SEL` line with the same fields
/// into real's `_serialize_tasks`; `align-traces.py` walks the two streams
/// in step and reports the first iteration whose state diverges. Never
/// on by default, so a normal run is byte-identical (there is no other
/// output change).
fn mo_sel_enabled() -> bool {
    std::env::var_os("PORTUALE_MO_SEL").is_some_and(|v| v != "0")
}

/// The version an entry is resolved at (the merge target for an
/// upgrade/downgrade, the current version otherwise). `None` for
/// `NoVisibleCandidate` and for a `Uninstall` removal (#72 B3), which is
/// never a merge target.
fn outcome_version(e: &GraphEntry) -> Option<&str> {
    match &e.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::AlreadyInstalled { version } => Some(version),
        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
    }
}

/// Real `Package`'s `cat/pkg-ver` label for the trace `pick=` field.
/// Prefixed `m:` for a merge-bound node (real `operation == "merge"`) or
/// `n:` for a nomerge/installed one, so the aligner can see *which kind*
/// of node each side removed when the frontiers differ -- the iter-1
/// gnuconfig/elt-patches batch vs real's installed-nomerge batch is
/// exactly the difference this harness exists to localize.
fn mo_sel_cpv(e: &GraphEntry, installed: bool) -> String {
    let kind = if installed { 'n' } else { 'm' };
    match outcome_version(e) {
        Some(v) => format!("{kind}:{}/{}-{}", e.category, e.package, v),
        None => format!("{kind}:{}/{}", e.category, e.package),
    }
}

/// One `MO_SEL ` line (`PORTUALE_MO_SEL`) -- pure so the format is
/// unit-pinned. Field order is the shared harness contract:
/// `iter retlist alive asap prefer_asap drop_satisfied ig pick`; `asap`
/// is the bracketed list of `asap_nodes` cpvs (a count was not enough:
/// firefox's clang-runtime divergence is exactly one extra lingering
/// asap node on portuale's side).
#[allow(clippy::too_many_arguments)]
fn mo_sel_trace_line(
    iter: usize,
    retlist: usize,
    alive: usize,
    asap: &[String],
    prefer_asap: bool,
    drop_satisfied: bool,
    ig: Option<Ignore>,
    pick: &[String],
) -> String {
    format!(
        "MO_SEL iter={iter} retlist={retlist} alive={alive} asap=[{}] \
         prefer_asap={} drop_satisfied={} ig={} pick={}",
        asap.join(" "),
        prefer_asap as u8,
        drop_satisfied as u8,
        ignore_name(ig),
        pick.join(" ")
    )
}

/// `Digraph::leaf_nodes` through the frontier when built, plain scan
/// otherwise. The two agree by construction (same order, same filter);
/// the equivalence unit tests pin it.
fn leaves_via(
    frontier: Option<&mut SerializeFrontier>,
    g: &Digraph,
    ig: Option<Ignore>,
) -> Vec<usize> {
    match frontier {
        Some(fr) => match fr.level_of(ig) {
            Some(level) => fr
                .ready_nodes(level, &g.alive)
                .into_iter()
                .filter(|&i| g.alive[i])
                .collect(),
            None => g.leaf_nodes(ig),
        },
        None => g.leaf_nodes(ig),
    }
}

/// `Digraph::is_leaf` through the frontier when built, direct check
/// otherwise.
fn is_leaf_via(
    frontier: Option<&SerializeFrontier>,
    g: &Digraph,
    node: usize,
    ig: Option<Ignore>,
) -> bool {
    match frontier {
        Some(fr) => match fr.level_of(ig) {
            Some(level) => fr.is_leaf(node, level),
            None => g.is_leaf(node, ig),
        },
        None => g.is_leaf(node, ig),
    }
}

/// Real `mypriority.satisfied = inst_pkg`: whether an installed package
/// matches `edge`'s atom (`installed_by_cp` is `installed_candidates_by_cp`
/// -- real `vardb.match_pkgs(atom)`), with the `:=`/`:*`-style narrowing
/// `build_digraph` documents: for a `buildtime_slot_op`/`runtime_slot_op`
/// edge the match must carry the resolved child's own slot/sub-slot, so a
/// sub-slot bump doesn't read as satisfied. `child_slot` `None` means the
/// caller has no resolved child (plain atom check).
///
/// Extracted from `build_digraph` so the #68/#72 B2 satisfied-blocker
/// predicate (`portuale`'s `replacement_wait_index`) reads exactly the
/// same rule instead of re-deriving it (T4).
fn edge_satisfied_with(
    installed_by_cp: &HashMap<(String, String), Vec<String>>,
    edge: &DepEdge,
    child_slot: Option<(&str, &str)>,
) -> bool {
    let Some(cands) = installed_by_cp.get(&(edge.category.clone(), edge.package.clone())) else {
        return false;
    };
    let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(&edge.atom, &refs) else {
        return false;
    };
    if matched.is_empty() {
        return false;
    }
    if edge.priority.buildtime_slot_op || edge.priority.runtime_slot_op {
        let Some((slot, sub_slot)) = child_slot else {
            return true;
        };
        return matched.iter().any(|m| {
            portage_dep::parse_candidate(m).is_some_and(|c| {
                c.slot.as_deref() == Some(slot) && c.sub_slot.as_deref() == Some(sub_slot)
            })
        });
    }
    true
}

/// The [`edge_satisfied_with`] rule against the live vdb, for callers that
/// did not already cache `installed_candidates_by_cp`. #68/#72 B2 is the
/// first such caller: `GraphEntry::deps` never carries the computed
/// `DepPriority::satisfied` bit (`build_digraph` sets it on its own edge
/// copies only), so the satisfied-blocker predicate must ask this itself.
pub fn dep_edge_satisfied_by_installed(
    root: &Path,
    edge: &DepEdge,
    child: Option<&GraphEntry>,
) -> bool {
    edge_satisfied_with(
        &installed_candidates_by_cp(root),
        edge,
        child.and_then(|c| c.slot.as_deref().zip(c.sub_slot.as_deref())),
    )
}

/// Every installed package's `cat/pkg-version:slot/sub_slot` candidate
/// string, grouped by `cat/pkg` -- the input `DepPriority::satisfied`
/// needs (real `vardb.match_pkgs(atom)`).
fn installed_candidates_by_cp(root: &Path) -> HashMap<(String, String), Vec<String>> {
    let mut by_cp: HashMap<(String, String), Vec<String>> = HashMap::new();
    for p in all_installed_packages(root) {
        let (slot, sub_slot) = read_vdb_slot(root, &p.category, &p.package, &p.version);
        by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push(format!(
                "{}/{}-{}:{slot}/{sub_slot}",
                p.category, p.package, p.version
            ));
    }
    by_cp
}

/// A synthetic `AlreadyInstalled` graph node for an installed package
/// that the resolve never created an entry for -- pulled in only to
/// complete real `_complete_graph`'s installed-dependency tree for merge
/// ordering. `serialize_merge_order` filters every index `>= real_n`
/// (these) back out of the scheduled list before returning.
fn synthetic_installed_entry(
    category: String,
    package: String,
    version: String,
    deps: Vec<DepEdge>,
) -> GraphEntry {
    GraphEntry {
        category,
        package,
        outcome: PretendOutcome::AlreadyInstalled { version },
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name: None,
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps,
    }
}

/// Real `_complete_graph`'s effect on `_serialize_tasks`: every installed
/// "nomerge" node in the digraph carries its own **recorded vdb
/// dependency tree**, recursively -- so leaf selection clears a shallow
/// installed subtree (`x11-libs/xtrans` -> `app-portage/elt-patches` ->
/// `sys-apps/gentoo-functions`) before a deep one (`x11-base/xorg-proto`
/// -> `dev-build/meson` -> the whole Python build stack) and merges the
/// package sitting on the shallow one first.
///
/// `build_digraph` only follows `GraphEntry::deps`, and an
/// `AlreadyInstalled` entry has none -- so portuale's graph truncated
/// every installed node at depth 1 (`(no children)` in the `--debug`
/// dump) and treated `meson`/`elt-patches` alike as instant leaves. This
/// walks the forward transitive closure: fill `deps` on every installed
/// entry from its vdb `*DEPEND` (USE-reduced against its recorded `USE`,
/// with real `pkg.built` priorities so build deps are `optional`), and
/// append a [`synthetic_installed_entry`] for each installed dependency
/// not already present, to a fixpoint.
///
/// Bounded by the installed set. In complete mode this also seeds from
/// the whole `@system` set (real `_complete_graph` seeds `@world` /
/// `@system` as `SetArg`s and walks them deep): an installed `@system`
/// package like `app-shells/bash` or `dev-libs/gmp` -- reached from
/// `sys-libs/glibc`'s optional `sys-devel/gcc` edge -- is a graph node
/// with its own tree even when nothing being merged reaches it. That
/// installed bulk is `_ignore_optional` ballast: real drains it one node
/// at a time in `DepPriorityNormalRange`, which paces the frontier so a
/// package sitting on a real runtime cycle (`dev-lang/perl` on the
/// `glibc`/`libcrypt` cycle) is not front-loaded past an unrelated merge
/// (`dev-cpp/eigen`) via a premature `drop_satisfied`. The nodes real's
/// own "Prune 'nomerge' root nodes" step then drops are removed again in
/// `serialize_merge_order` right after `build_digraph`.
#[allow(clippy::too_many_arguments)]
fn add_installed_dependency_closure(
    entries: &mut Vec<GraphEntry>,
    root: &Path,
    repos: &[RepoConfig],
    system_atoms: &[String],
    virtuals_only: bool,
    dynamic_deps: bool,
) {
    // `virtuals_only` (a plain `[ebuild N]` resolve, real not in complete
    // mode): real still expands an installed `virtual/*` node to its
    // provider (`virtual/pkgconfig` -> `dev-util/pkgconf`) -- new-style
    // virtuals are "free" and always walked -- but leaves every
    // non-virtual installed node a `(no children)` leaf. So only a node
    // whose own category is `virtual` gets its deps walked; the provider
    // it names is added as a leaf, not queued (unless it is itself a
    // virtual).
    let installed = all_installed_packages(root);
    // B1: every installed *version* of a cp, not one (a `cp -> single pkg`
    // map silently dropped all but the last-installed slot; real's
    // `_complete_graph` keeps every installed slot -- gtk:4's missing
    // `docbook-xml-dtd-{4.2,4.4,4.5}` nodes were exactly this).
    let by_cp: HashMap<(&str, &str), Vec<&crate::InstalledPackage>> = {
        let mut m: HashMap<(&str, &str), Vec<&crate::InstalledPackage>> = HashMap::new();
        for p in installed.iter() {
            m.entry((p.category.as_str(), p.package.as_str()))
                .or_default()
                .push(p);
        }
        m
    };
    // Pick the installed version an edge's atom names -- the highest
    // matching version when several match, the highest overall when the
    // atom is absent/unparseable. The candidate string carries the main
    // slot (`:4.2` style atoms are what pull sibling docbook slots in).
    let pick_installed =
        |atom: Option<&str>, cat: &str, pkg: &str| -> Option<&crate::InstalledPackage> {
            let cands = by_cp.get(&(cat, pkg))?;
            let mut best: Option<&crate::InstalledPackage> = None;
            for p in cands {
                if let Some(a) = atom
                    && !portage_dep::match_from_list(
                        a,
                        &[format!("{cat}/{pkg}-{}:{}", p.version, p.slot).as_str()],
                    )
                    .is_some_and(|m| !m.is_empty())
                {
                    continue;
                }
                if best.is_none_or(|b| {
                    portage_versions::vercmp(&b.version, &p.version).is_some_and(|o| o < 0)
                }) {
                    best = Some(p);
                }
            }
            best.or_else(|| {
                cands.iter().copied().max_by(|a, b| {
                    portage_versions::vercmp(&a.version, &b.version)
                        .unwrap_or(0)
                        .cmp(&0)
                })
            })
        };

    // A2 (#26): the closure edges come from the raw vdb snapshot
    // (`installed_dep_string`, `InstalledMetaLayer::Raw`) -- deliberately
    // NOT the same view the walk uses (the walk reads the *current
    // ebuild*'s deps under `--dynamic-deps`). Phase 5 S1 removed the vdb
    // built-`:=` append as a documented cut, so the ebuild view would
    // lose the vdb's built :S/SS= atoms; the pre-A2 Raw graph (minus the
    // injected libc) stays the closer approximation, exactly as under
    // the old gate-off default. The injected-libc strip below is therefore only
    // sound on the Raw path (Gate G0.3): `_inject_libc_dep` appends a
    // bare `>=<libc-provider>-<version>` to every installed package's vdb
    // `RDEPEND` (bug #753500), which the ebuild never declared -- real's
    // default walk never sees it, but the `=n` snapshot does, and
    // portuale keeps the historical strip there. A genuine ebuild-written
    // `>=sys-libs/glibc-x` must NOT be stripped now that the default path
    // reads the ebuild.
    let libc_cps = crate::libc_provider_cps(root);
    let is_injected_libc = |atom: &str| -> bool {
        let Some(a) = portage_dep::parse_atom(atom) else {
            return false;
        };
        a.blocker == portage_dep::Blocker::None
            && a.operator == portage_dep::Operator::Ge
            && a.version.is_some()
            && a.slot.is_none()
            && a.sub_slot.is_none()
            && a.use_deps.as_ref().is_none_or(|u| u.is_empty())
            && libc_cps.contains(&(a.category.clone(), a.package.clone()))
    };

    let vdb_edges = |cat: &str, pkg: &str, ver: &str| -> Vec<DepEdge> {
        // Live md5-cache metadata for this exact installed cpv, from the
        // vdb-recorded repo (backlog #86: never a priority search) --
        // `None` when the version is gone there (the Raw fallback).
        let live = crate::live_metadata_for_installed(repos, root, cat, pkg, ver);
        let mut md: HashMap<String, String> = HashMap::new();
        let mut memo: HashMap<(String, String, String, String), String> = HashMap::new();
        // A2 follow-up, settled by Phase 5 S1: with the built-`:=`
        // append removed, the closure stays on Raw under every option
        // set -- the pre-A2 scheduler graph (raw minus the injected
        // libc), which is closer to real's effective view than the
        // ebuild alone.
        let layer = crate::InstalledMetaLayer::Raw;
        for k in ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            let s = crate::installed_dep_string(
                root,
                dynamic_deps,
                cat,
                pkg,
                ver,
                live.as_deref(),
                k,
                layer,
                &mut memo,
            );
            if !s.trim().is_empty() {
                md.insert(k.to_string(), s);
            }
        }
        let use_flags = read_vdb_flag_set(root, cat, pkg, ver, "USE");
        dep_edges_from_metadata(
            &md,
            &use_flags,
            &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"],
            true,
        )
        .into_iter()
        .filter(|e| layer != crate::InstalledMetaLayer::Raw || !is_injected_libc(&e.atom))
        .collect()
    };

    let mut present: HashSet<(String, String, String)> = entries
        .iter()
        .map(|e| {
            (
                e.category.clone(),
                e.package.clone(),
                outcome_version(e).unwrap_or("").to_string(),
            )
        })
        .collect();
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    // Seed 1: every installed dependency named by an *already-resolved*
    // entry (merge-bound or installed) that the resolve never made a
    // node for -- an installed package satisfying a dep is not walked, so
    // `dev-perl/common-sense`'s edge to the installed `dev-lang/perl`
    // (and perl's own deep tree) was simply missing. Real
    // `_complete_graph` has every one of these.
    // `virtuals_only`: a node just added is queued for its own dep walk
    // only if the whole closure is wanted, or it is itself a `virtual/*`.
    // B1: `present` is keyed by cpv, and `add_node` selects the installed
    // version the edge's own atom names -- real keeps every installed
    // slot, and keying by cp alone dropped all but the first
    // (`docbook-xml-dtd:4.2`/`:4.4`/`:4.5` vs the already-present 4.1.2).
    let expandable = |cat: &str| !virtuals_only || cat == "virtual";
    let add_node = |entries: &mut Vec<GraphEntry>,
                    present: &mut HashSet<(String, String, String)>,
                    queue: &mut std::collections::VecDeque<usize>,
                    atom: Option<&str>,
                    cat: &str,
                    pkg: &str| {
        let Some(p) = pick_installed(atom, cat, pkg) else {
            return;
        };
        let key = (p.category.clone(), p.package.clone(), p.version.clone());
        if !present.insert(key) {
            return;
        }
        entries.push(synthetic_installed_entry(
            p.category.clone(),
            p.package.clone(),
            p.version.clone(),
            Vec::new(),
        ));
        if expandable(cat) {
            queue.push_back(entries.len() - 1);
        }
    };

    let seed_targets: Vec<(String, String, String)> = entries
        .iter()
        .flat_map(|e| e.deps.iter())
        .map(|d| (d.atom.clone(), d.category.clone(), d.package.clone()))
        .collect();
    for (atom, cat, pkg) in seed_targets {
        add_node(entries, &mut present, &mut queue, Some(&atom), &cat, &pkg);
    }

    // Seed 1b (complete mode only): real `_complete_graph` seeds a
    // `SetArg` for `@world`/`@system` and adds every atom in them, then
    // walks the lot deep. Add each installed `@system` package as a node
    // and queue its own dep walk. `serialize_merge_order`'s prune drops
    // the ones no merge-bound package reaches -- but the deep-only
    // members (`dev-libs/gmp` under `glibc` -> `gcc`, `sys-libs/readline`
    // under `bash`, ...) stay, exactly as real's own post-prune graph
    // keeps them.
    //
    // R5 (#17): real `_resolve` (`depgraph.py:5500`) processes this
    // `SetArg`'s own atom list `sorted(arg.pset.getAtoms(), key=str)` --
    // the same seed-order fact R3b already fixed for an *explicit*
    // `@world`/`@system` top-level target (`pretend.rs`'s
    // `expand_top_level_atoms`). This is the other place the identical
    // unsorted-profile-order seed survives: *every* complete-mode probe
    // (not just an explicit `@system`/`@world` argument) walks
    // `system_atoms` here to build the installed closure, and an
    // unsorted seed skews the DFS discovery order the pre-bias sort
    // preserves -- exactly the `_serialize_tasks` "installed-chain"
    // front-load family (`pyproject-metadata`, `freetype`, …) `TEST/
    // findings/l0.md` "## I" names as the outstanding wall.
    let mut system_atoms_sorted = system_atoms.to_vec();
    system_atoms_sorted.sort();
    if !virtuals_only {
        for atom_str in &system_atoms_sorted {
            let Some(atom) = portage_dep::parse_atom(atom_str) else {
                continue;
            };
            add_node(
                entries,
                &mut present,
                &mut queue,
                Some(atom_str),
                &atom.category,
                &atom.package,
            );
        }
    }

    // Seed 2: every installed-outcome entry whose deps were never filled
    // (and, in `virtuals_only` mode, is a `virtual/*`).
    for (i, e) in entries.iter().enumerate() {
        if matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. })
            && e.deps.is_empty()
            && expandable(&e.category)
        {
            queue.push_back(i);
        }
    }

    while let Some(i) = queue.pop_front() {
        let PretendOutcome::AlreadyInstalled { version } = entries[i].outcome.clone() else {
            continue;
        };
        if !entries[i].deps.is_empty() {
            continue;
        }
        let (cat, pkg) = (entries[i].category.clone(), entries[i].package.clone());
        let edges = vdb_edges(&cat, &pkg, &version);
        for e in &edges {
            add_node(
                entries,
                &mut present,
                &mut queue,
                Some(&e.atom),
                &e.category,
                &e.package,
            );
        }
        entries[i].deps = edges;
    }
}

/// Shared prelude of `build_digraph` and the public
/// [`kept_alt_branches`]: `cat/pkg` -> entry indices, the installed-node
/// flags (`AlreadyInstalled`/`NoVisibleCandidate`), and the per-entry
/// candidate strings `edge_matches` narrows atoms against.
struct DigraphPrelude<'a> {
    cp_indices: HashMap<(&'a str, &'a str), Vec<usize>>,
    installed: Vec<bool>,
    entry_candidate: Vec<Option<String>>,
}

impl DigraphPrelude<'_> {
    /// Real `_create_graph` resolves every dep atom to a single package
    /// (`_select_pkg_highest_available`) before `_add_pkg` records the
    /// edge. Portuale looked each atom's `cat/pkg` up in `cp_indices` and
    /// connected it to *every* scheduled instance of that `cp` -- so a
    /// slot-qualified atom (`app-text/docbook-sgml-dtd:3.0`) wrongly
    /// gained an edge to a sibling slot also being merged, and the extra
    /// parent skewed `_merge_order_bias`'s parent-count ordering. Narrow
    /// every edge to the entries its atom actually matches (version /
    /// slot / repo); keep the edge whenever the candidate string can't be
    /// built or the atom won't parse, so this only ever removes a
    /// provably-wrong edge.
    fn edge_matches(&self, atom: &str, j: usize) -> bool {
        let Some(cand) = self.entry_candidate[j].as_deref() else {
            return true;
        };
        match portage_dep::match_from_list(atom, &[cand]) {
            Some(m) => !m.is_empty(),
            None => true,
        }
    }

    /// B2: real resolves every dep atom to a *single* package
    /// (`_select_pkg_highest_available`) before `_add_pkg` records the
    /// edge. A bare multi-slot atom (`llvm-runtimes/clang-runtime[...]`
    /// in clang-common's PDEPEND) matches every scheduled slot, and
    /// edging it to all of them gave the older slot an extra
    /// `runtime_post` parent -- which promoted it into `asap` and split
    /// the clang-runtime drain. Among the matches prefer a merge-bound
    /// entry (the node real's scheduler graph actually edges to when the
    /// cp is being rebuilt/updated), then the highest version by
    /// `vercmp`; first on ties. A `Uninstall` removal (#72 B3) is never a
    /// merge target: its only ordering edge is `build_digraph`'s
    /// dedicated one.
    ///
    /// `merge_bound_only` is the **one** difference between this
    /// function's two callers (#82). `build_digraph` says `false`: real's
    /// scheduler digraph holds nomerge nodes, and an installed target is
    /// a legitimate edge there. `pretend.rs::print_tree` says `true`: it
    /// renders no line at all for an `AlreadyInstalled` /
    /// `NoVisibleCandidate` entry, so an edge to one would open a hole in
    /// the tree. Everything else -- the candidate strings, the atom
    /// narrowing and the ranking -- is literally this code for both, so
    /// the ranking cannot drift between them again (#82: `print_tree`'s
    /// private copy ranked with a **string** compare, under which `1.9`
    /// outranks `1.10` and slot `9` outranks slot `10`).
    ///
    /// #85: the ranking above runs *after* real's `_minimize_children`
    /// (`depgraph.py:4751-4856`). When several of one parent's atoms
    /// select different instances of the same cp, real eliminates the
    /// redundant selections first: installed instances first, then
    /// ascending version, dropping a package every one of whose atoms is
    /// also matched by another surviving package. The surviving target
    /// per atom is then the highest-ranked remainder -- which is NOT
    /// always the vercmp-highest match. Live shape: `treeslotuser`'s
    /// `:0` selects 1.9 while the bare `treeslotpkg` atom selects 1.10;
    /// real eliminates 1.10 (`:0` matches only 1.9, the bare atom matches
    /// both), so the scheduler graph holds no `user → 1.10` edge at all
    /// (verified in real's own `--debug` digraph dump) and the bias
    /// parent-count ties 1-1, settling by discovery order `1.9, 1.10`.
    /// Without the elimination the extra edge doubles 1.10's parent
    /// count and the bias flips the pair. Fully-redundant atom sets
    /// collapse to the highest version, exactly today's ranking, so only
    /// partially-overlapping shapes like this one move.
    fn select_dep_target(
        &self,
        entries: &[GraphEntry],
        from: usize,
        from_deps: &[DepEdge],
        ei: usize,
        suppressed: &HashSet<usize>,
        merge_bound_only: bool,
    ) -> Option<usize> {
        let edge = &from_deps[ei];
        // Sibling edges selecting the same cp: the minimize universe.
        // Suppressed `||` branches never become edges, so they don't
        // participate (same exclusion the edge loops apply).
        let cp = (edge.category.as_str(), edge.package.as_str());
        let sib_eis: Vec<usize> = from_deps
            .iter()
            .enumerate()
            .filter(|(si, se)| {
                (se.category.as_str(), se.package.as_str()) == cp && !suppressed.contains(si)
            })
            .map(|(si, _)| si)
            .collect();
        let sib_sets: Vec<Vec<usize>> = sib_eis
            .iter()
            .map(|&si| self.match_candidates(entries, from, &from_deps[si], merge_bound_only))
            .collect();
        let own = sib_eis
            .iter()
            .position(|&si| si == ei)
            .map(|pos| &sib_sets[pos]);
        let own = own?;
        if own.len() < 2 {
            return Self::rank_best(entries, &self.installed, own.iter().copied());
        }
        // A `NoVisibleCandidate` entry is never a *selected* package:
        // real's `_select_package` returns None for its atom and
        // `_minimize_children` yields `(atom, None)` without any
        // elimination. So NVC entries neither eliminate other
        // candidates nor are eliminated themselves here -- but they
        // stay rankable below, preserving the pre-#85 fallback that
        // picked them (e.g. the `opartlya` NVC disclosure row, whose
        // dep-string order the contract suite pins). Without this,
        // the ascending-version elimination order drops an NVC entry
        // before a same-cp installed one and flips disclosure order.
        let is_nvc = |j: &usize| matches!(entries[*j].outcome, PretendOutcome::NoVisibleCandidate);
        // Real's elimination order (bug 631894 determinism note):
        // installed instances first, then ascending version.
        let mut union: Vec<usize> = Vec::new();
        for set in &sib_sets {
            for &j in set {
                if !union.contains(&j) {
                    union.push(j);
                }
            }
        }
        union.sort_by(|&a, &b| {
            // Installed sorts before merge-bound so installed
            // candidates are eliminated first, like real.
            match (self.installed[a], self.installed[b]) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let av = outcome_version(&entries[a]).unwrap_or("");
                    let bv = outcome_version(&entries[b]).unwrap_or("");
                    portage_versions::vercmp(av, bv)
                        .map(|o| o.cmp(&0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        });
        let mut alive: HashSet<usize> = union.iter().copied().collect();
        for &p in &union {
            if is_nvc(&p) || !alive.contains(&p) {
                continue;
            }
            let mut shared_by_all = false;
            for set in &sib_sets {
                if set.contains(&p) {
                    if set
                        .iter()
                        .filter(|q| alive.contains(q) && !is_nvc(q))
                        .count()
                        < 2
                    {
                        shared_by_all = false;
                        break;
                    }
                    shared_by_all = true;
                }
            }
            if shared_by_all {
                alive.remove(&p);
            }
        }
        Self::rank_best(
            entries,
            &self.installed,
            own.iter().copied().filter(|j| alive.contains(j)),
        )
    }

    /// The per-atom candidate entries `select_dep_target` narrows and
    /// ranks: every same-cp entry the atom matches (version / slot /
    /// repo -- plus the #148 USE narrowing below), minus `Uninstall`
    /// removals, minus (under `merge_bound_only`) the parent itself
    /// and installed nodes.
    fn match_candidates(
        &self,
        entries: &[GraphEntry],
        from: usize,
        edge: &DepEdge,
        merge_bound_only: bool,
    ) -> Vec<usize> {
        let Some(idxs) = self
            .cp_indices
            .get(&(edge.category.as_str(), edge.package.as_str()))
        else {
            return Vec::new();
        };
        // Uninstall entries are skipped below, so among the survivors
        // "merge-bound" is exactly "not an installed node".
        let merge_bound = |j: usize| !self.installed[j];
        let cands: Vec<usize> = idxs
            .iter()
            .copied()
            .filter(|&j| {
                if !self.edge_matches(&edge.atom, j) {
                    return false;
                }
                if matches!(entries[j].outcome, PretendOutcome::Uninstall { .. }) {
                    return false;
                }
                if merge_bound_only && (j == from || !merge_bound(j)) {
                    return false;
                }
                true
            })
            .collect();
        self.narrow_by_use(entries, &edge.atom, cands)
    }

    /// Backlog #148: USE-aware narrowing of multi-instance matches.
    /// Real matches dep atoms against pkgs *with* use (`atom.match(pkg.
    /// with_use(...))`), so `>=slotusetarget-1.0[x]` never edges to an
    /// x-less instance -- without this, the x/y parents all edge to the
    /// vercmp-highest instance and `_merge_order_bias`'s parent-count
    /// ordering prints the surviving conflict's rows in the wrong order
    /// (2.0 before real's 1.0-first). Applies only when it can decide
    /// fairly, and only ever removes a provably-wrong edge (same rule
    /// as `edge_matches`): unparseable atoms, atoms without use-deps,
    /// singleton sets, any candidate without display USE, and a
    /// narrowing that would empty the set all keep the unfiltered set.
    ///
    /// Deliberate narrowing vs the walk: `valid_iuse`'s
    /// profile-effective union is not applied (no config reaches this
    /// seam), so a use-dep on an effective-only flag over sibling
    /// versions with split IUSE declarations could mis-drop. The
    /// empty-fallback keeps the worst case at status quo; the
    /// contract suite and the L0 bed gate the rest.
    fn narrow_by_use(&self, entries: &[GraphEntry], atom: &str, cands: Vec<usize>) -> Vec<usize> {
        if cands.len() < 2 {
            return cands;
        }
        let Some(parsed) = portage_dep::parse_atom(atom) else {
            return cands;
        };
        let use_deps = parsed.use_deps.unwrap_or_default();
        if use_deps.is_empty() {
            return cands;
        }
        let mut kept = Vec::new();
        for &j in &cands {
            let display = &entries[j].use_flags_display;
            if display.is_empty() {
                return cands;
            }
            let declared: HashSet<String> = display.iter().map(|(flag, _)| flag.clone()).collect();
            let enabled: HashSet<String> = display
                .iter()
                .filter(|(_, on)| *on)
                .map(|(flag, _)| flag.clone())
                .collect();
            if portage_dep::use_deps_satisfied(&use_deps, &declared, &enabled) {
                kept.push(j);
            }
        }
        if kept.is_empty() { cands } else { kept }
    }

    /// `select_dep_target`'s ranking over a fixed candidate set: prefer a
    /// merge-bound entry, then the highest version by `vercmp`; first on
    /// ties.
    fn rank_best(
        entries: &[GraphEntry],
        installed: &[bool],
        cands: impl Iterator<Item = usize>,
    ) -> Option<usize> {
        let merge_bound = |j: usize| !installed[j];
        let mut best: Option<usize> = None;
        for j in cands {
            best = Some(match best {
                None => j,
                Some(b) => {
                    let better = match (merge_bound(j), merge_bound(b)) {
                        (true, false) => true,
                        (false, true) => false,
                        _ => {
                            let bv = outcome_version(&entries[b]).unwrap_or("");
                            let jv = outcome_version(&entries[j]).unwrap_or("");
                            portage_versions::vercmp(jv, bv).is_some_and(|o| o > 0)
                        }
                    };
                    if better { j } else { b }
                }
            });
        }
        best
    }
}

fn digraph_prelude<'a>(entries: &'a [GraphEntry], root: &Path) -> DigraphPrelude<'a> {
    let mut cp_indices: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        cp_indices
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(i);
    }
    let installed: Vec<bool> = entries
        .iter()
        .map(|e| {
            matches!(
                e.outcome,
                PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
            )
        })
        .collect();
    let entry_candidate: Vec<Option<String>> = entries
        .iter()
        .map(|e| {
            let ver = entry_version(e)?;
            let (slot, sub_slot) = match (e.slot.as_deref(), e.sub_slot.as_deref()) {
                (Some(s), Some(ss)) => (s.to_string(), ss.to_string()),
                _ => read_vdb_slot(root, &e.category, &e.package, ver),
            };
            let repo = e.repo_name.as_deref().unwrap_or("gentoo");
            Some(format!(
                "{}/{}-{ver}:{slot}/{sub_slot}::{repo}",
                e.category, e.package
            ))
        })
        .collect();
    DigraphPrelude {
        cp_indices,
        installed,
        entry_candidate,
    }
}

/// Real `dep_zapdeps` (`dep_check.py`): a `|| ( … )` group resolves to
/// one alternative, not all. Portuale keeps every branch's atoms in
/// `GraphEntry::deps` and picks here, per `(key, group)`, following
/// real's `choice_bins` ordering: the first branch (written order)
/// *all* of whose atoms already match a merge-bound graph node
/// (`preferred_in_graph`, and real's line-793 promotion of the
/// all-in-graph choice ahead of an all-installed one in the same bin),
/// else the first all of whose atoms match an installed entry
/// (`preferred_installed`), else the first all of whose atoms match
/// anything at all. Returns, per entry, the `deps` indices of the
/// branches **not** picked. If *no* branch fully resolves, nothing is
/// suppressed (keep the over-inclusive stopgap).
fn suppressed_alt_edges(entries: &[GraphEntry], pre: &DigraphPrelude<'_>) -> Vec<HashSet<usize>> {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut groups: HashMap<(u8, u32), Vec<(u32, usize)>> = HashMap::new();
            for (ei, edge) in e.deps.iter().enumerate() {
                if let Some((g, b)) = edge.alt
                    && g != u32::MAX
                {
                    groups.entry((edge.key, g)).or_default().push((b, ei));
                }
            }
            let mut suppressed = HashSet::new();
            for members in groups.values() {
                // Per branch: does *every* atom match a merge-bound node
                // / an installed entry / anything?
                let mut all_graph: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                let mut all_inst: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                let mut all_any: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                for &(b, ei) in members {
                    let edge = &e.deps[ei];
                    let (mut graph_m, mut inst_m, mut any_m) = (false, false, false);
                    if let Some(idxs) = pre
                        .cp_indices
                        .get(&(edge.category.as_str(), edge.package.as_str()))
                    {
                        for &j in idxs {
                            // Real `dep_zapdeps` treats a `||` alternative
                            // satisfied only by the package currently
                            // being resolved as unavailable -- a circular
                            // self-dependency (the `circular_self` check
                            // in lib.rs's actual resolve, b5b256f, L0
                            // finding D). Without this guard, an
                            // alternative whose only tree match is the
                            // owner's own not-yet-merged graph node (`j
                            // == i`) trivially counts as "in graph" and
                            // can win over the branch the resolver
                            // actually chose (e.g. a bootstrap branch),
                            // adding a phantom self-edge that can never
                            // be satisfied and stalls the owner's own
                            // merge-order position (#53).
                            if j == i && !pre.installed[j] {
                                continue;
                            }
                            if pre.edge_matches(&edge.atom, j) {
                                any_m = true;
                                if pre.installed[j] {
                                    inst_m = true;
                                } else {
                                    graph_m = true;
                                }
                            }
                        }
                    }
                    *all_graph.entry(b).or_insert(true) &= graph_m;
                    *all_inst.entry(b).or_insert(true) &= inst_m;
                    *all_any.entry(b).or_insert(true) &= any_m;
                }
                let pick = all_graph
                    .iter()
                    .find(|(_, v)| **v)
                    .map(|(b, _)| *b)
                    .or_else(|| all_inst.iter().find(|(_, v)| **v).map(|(b, _)| *b))
                    .or_else(|| all_any.iter().find(|(_, v)| **v).map(|(b, _)| *b));
                if let Some(pick) = pick {
                    for &(b, ei) in members {
                        if b != pick {
                            suppressed.insert(ei);
                        }
                    }
                }
            }
            suppressed
        })
        .collect()
}

/// #76 B1: the `GraphEntry::deps` indices of every `|| ( … )` alternative
/// this run **kept** -- the branch `dep_zapdeps` resolved the group to,
/// per `suppressed_alt_edges`' real `choice_bins` ranking. Exposed for
/// the satisfied-blocker wait predicate (`pretend.rs::
/// replacement_wait_index`): the serializer runs over the collapsed
/// graph, so a wait chain through a disjunctive edge is a wait exactly
/// when that edge's branch was the kept one (`TEST/findings/l0.md`
/// "#76 B0"). The #53 circular-self-branch exclusion is preserved
/// because both callers share `suppressed_alt_edges`.
pub fn kept_alt_branches(entries: &[GraphEntry], root: &Path) -> Vec<HashSet<usize>> {
    let pre = digraph_prelude(entries, root);
    let suppressed = suppressed_alt_edges(entries, &pre);
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.deps
                .iter()
                .enumerate()
                .filter(|(ei, edge)| edge.alt.is_some() && !suppressed[i].contains(ei))
                .map(|(ei, _)| ei)
                .collect()
        })
        .collect()
}

/// #82: for every entry, the entry index each of its `GraphEntry::deps`
/// edges resolves to -- `None` when the atom has no target in this
/// graph, when the target is the entry itself, or when the edge is a
/// `|| ( … )` alternative this run did not keep (`suppressed_alt_edges`,
/// the #76 B1 set, so a caller needs no second `kept_alt_branches` pass).
///
/// Exposed for `pretend.rs::print_tree`, whose tree display needs the
/// same "one package per atom" narrowing real's `_create_graph` does.
/// It shares `DigraphPrelude::select_dep_target` with `build_digraph`
/// (see that function's doc for the single `merge_bound_only`
/// difference and for why the sharing matters).
pub fn resolved_dep_targets(entries: &[GraphEntry], root: &Path) -> Vec<Vec<Option<usize>>> {
    let pre = digraph_prelude(entries, root);
    let suppressed = suppressed_alt_edges(entries, &pre);
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.deps
                .iter()
                .enumerate()
                .map(|(ei, _edge)| {
                    if suppressed[i].contains(&ei) {
                        return None;
                    }
                    pre.select_dep_target(entries, i, &e.deps, ei, &suppressed[i], true)
                })
                .collect()
        })
        .collect()
}

/// Builds the merge-order digraph out of the resolved `entries`.
///
/// Nodes are the entries themselves (portuale's own graph is already
/// one node per resolved `cat/pkg` slot); edges come from
/// `GraphEntry::deps`, which carries real's own per-key `DepPriority`.
/// `required_by` supplies a fallback edge for any owner relationship the
/// forward `deps` walk didn't record (a diamond dependency's second
/// owner, a synthetic rebuild entry, an entry whose metadata was
/// unreadable) so the new scheduler is never *less* constrained than the
/// `required_by`-only one it replaces.
fn build_digraph(entries: &[GraphEntry], top_level_atoms: &[String], root: &Path) -> Digraph {
    let n = entries.len();
    let pre = digraph_prelude(entries, root);
    let cp_indices = &pre.cp_indices;
    let installed = &pre.installed;

    let mut g = Digraph {
        n,
        children: vec![Vec::new(); n],
        parents: vec![Vec::new(); n],
        order: Vec::new(),
        installed: installed.clone(),
        alive: vec![true; n],
    };

    // Real `mypriority.satisfied`: an installed package matching the
    // atom, via the shared `edge_satisfied_with` rule (see its doc).
    let installed_by_cp = installed_candidates_by_cp(root);
    let satisfied = |edge: &DepEdge, child: Option<usize>| -> bool {
        edge_satisfied_with(
            &installed_by_cp,
            edge,
            child.and_then(|ci| {
                entries[ci]
                    .slot
                    .as_deref()
                    .zip(entries[ci].sub_slot.as_deref())
            }),
        )
    };

    // The per-entry candidate strings `edge_matches` narrows atoms
    // against -- see `digraph_prelude` for the full real grounding.
    let edge_matches = |atom: &str, j: usize| pre.edge_matches(atom, j);

    // Real `dep_zapdeps` (`dep_check.py`): a `|| ( … )` group resolves to
    // one alternative, not all. The derivation lives in
    // `suppressed_alt_edges` so the public `kept_alt_branches` used by the
    // #76 wait predicate cannot drift from this internal use.
    let alt_suppressed = suppressed_alt_edges(entries, &pre);

    // Real `_create_graph`: an explicit LIFO `dep_stack` seeded from the
    // top-level atoms. A node is recorded into `.order` the moment its
    // parent's dep string first names it (forward, before any recursion),
    // then the *last*-pushed node is expanded first -- so each node's own
    // direct children land as one contiguous forward-order run, but the
    // deepest-declared sibling's subtree is numbered before its earlier
    // siblings'.
    let mut discovered = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let discover = |i: usize, discovered: &mut Vec<bool>, order: &mut Vec<usize>| -> bool {
        if discovered[i] {
            return false;
        }
        discovered[i] = true;
        order.push(i);
        true
    };
    for atom_str in top_level_atoms {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        if let Some(idxs) = cp_indices.get(&(atom.category.as_str(), atom.package.as_str())) {
            for &i in idxs {
                if !edge_matches(atom_str, i) {
                    continue;
                }
                if discover(i, &mut discovered, &mut g.order) {
                    stack.push(i);
                }
            }
        }
    }
    // Real `_create_graph`'s own two-stack outer loop
    // (`depgraph.py:3254-3269`): the ordinary `_dep_stack` is drained
    // completely, and only then is the *last*-queued disjunctive bundle
    // popped off `_dep_disjunctive_stack` and expanded -- which pushes
    // fresh nodes back onto `_dep_stack`, so the whole thing repeats.
    // Each bundle is one dep key's worth of `|| ( … )` / `virtual/*`
    // atoms (`_queue_disjunctive_deps` queues at most one per key,
    // before that key's inline atoms are added).
    let mut disjunctive_stack: Vec<(usize, u8)> = Vec::new();
    let expand = |i: usize,
                  disjunctive: bool,
                  key_filter: Option<u8>,
                  discovered: &mut Vec<bool>,
                  order: &mut Vec<usize>,
                  stack: &mut Vec<usize>| {
        for (ei, edge) in entries[i].deps.iter().enumerate() {
            if edge.disjunctive != disjunctive {
                continue;
            }
            if key_filter.is_some_and(|k| k != edge.key) {
                continue;
            }
            if alt_suppressed[i].contains(&ei) {
                continue;
            }
            let Some(idxs) = cp_indices.get(&(edge.category.as_str(), edge.package.as_str()))
            else {
                continue;
            };
            for &j in idxs {
                if !edge_matches(&edge.atom, j) {
                    continue;
                }
                if discover(j, discovered, order) {
                    stack.push(j);
                }
            }
        }
    };
    loop {
        while let Some(i) = stack.pop() {
            let mut keys: Vec<u8> = entries[i]
                .deps
                .iter()
                .filter(|e| e.disjunctive)
                .map(|e| e.key)
                .collect();
            keys.dedup();
            for k in keys {
                disjunctive_stack.push((i, k));
            }
            expand(i, false, None, &mut discovered, &mut g.order, &mut stack);
        }
        let Some((i, k)) = disjunctive_stack.pop() else {
            break;
        };
        expand(i, true, Some(k), &mut discovered, &mut g.order, &mut stack);
    }
    // An entry this walk never reaches -- a synthetic slot-operator /
    // `--rebuild-if-*` entry (no `deps` of its own), or an `--nodeps`
    // run -- keeps its original array position, appended after every
    // genuinely-discovered node.
    for (i, seen) in discovered.iter().enumerate() {
        if !seen {
            g.order.push(i);
        }
    }

    // Edges. Forward first, so `children` keeps real's own
    // dep-key/atom order (which `asap_nodes` and the cycle harvester
    // both read).
    for (i, entry) in entries.iter().enumerate() {
        for (ei, edge) in entry.deps.iter().enumerate() {
            if alt_suppressed[i].contains(&ei) {
                continue;
            }
            // The single package the atom resolves to -- shared with
            // `pretend.rs::print_tree` through `resolved_dep_targets`
            // (#82), so the ranking cannot drift between the scheduling
            // graph and the tree display. `merge_bound_only = false`:
            // real's scheduler digraph edges to nomerge nodes too.
            if let Some(j) =
                pre.select_dep_target(entries, i, &entry.deps, ei, &alt_suppressed[i], false)
            {
                // Real `_add_pkg`: a direct self-edge is dropped unless
                // it is an unsatisfied build-time dependency, "since
                // otherwise it can skew the merge order calculation in
                // an unwanted way" (`depgraph.py:3765-3770`).
                let sat = satisfied(edge, Some(j));
                if i == j && !(edge.priority.buildtime && !sat) {
                    continue;
                }
                let mut priority = edge.priority;
                priority.satisfied = sat;
                g.add_edge(i, j, priority);
            }
        }
    }
    // Fallback edges from `required_by` for owner relationships the
    // forward walk has no `deps` entry for.
    for (j, e) in entries.iter().enumerate() {
        // #72 B3: a `Uninstall` removal's `required_by` names the blocker
        // owner it removes *for*, which real orders the removal **after**
        // (u1/u3: the owner's row, then `[uninstall]`). The generic
        // fallback would add owner -> removal (removal first), so the
        // dedicated pass below adds the reversed edge instead and this
        // loop skips the removal.
        if matches!(e.outcome, PretendOutcome::Uninstall { .. }) {
            for owner in &e.required_by {
                let Some(owner_indices) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str()))
                else {
                    continue;
                };
                for &i in owner_indices {
                    if i == j || g.children[i].iter().any(|(c, _)| *c == j) {
                        continue;
                    }
                    // `add_edge(j, i)`: the owner is the removal's child,
                    // so the owner is selected first (real's
                    // `_serialize_tasks` defers every uninstall node until
                    // it is scheduled, `get_nodes` `depgraph.py:9539-9549`).
                    g.add_edge(
                        j,
                        i,
                        DepPriority {
                            buildtime: true,
                            runtime: true,
                            satisfied: false,
                            ..DepPriority::default()
                        },
                    );
                }
            }
            continue;
        }
        for owner in &e.required_by {
            let Some(owner_indices) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str())) else {
                continue;
            };
            for &i in owner_indices {
                if i == j || g.children[i].iter().any(|(c, _)| *c == j) {
                    continue;
                }
                // `required_by` is keyed by `cat/pkg` only, so a
                // multi-slot dependency hands every scheduled slot the
                // same owner set. If the forward dep walk already gave
                // this owner a real edge to *another* slot of `j`'s
                // `cat/pkg`, that atom was slot-qualified and resolved
                // elsewhere -- don't synthesize a fallback edge to this
                // slot too (real resolves each atom to one package).
                if g.children[i].iter().any(|(c, _)| {
                    entries[*c].category == e.category && entries[*c].package == e.package
                }) {
                    continue;
                }
                g.add_edge(
                    i,
                    j,
                    DepPriority {
                        runtime: true,
                        satisfied: g.installed[j],
                        ..DepPriority::default()
                    },
                );
            }
        }
    }
    g
}

/// Real `_serialize_tasks`' own "Prune 'nomerge' root nodes if nothing
/// depends on them, since otherwise they slow down merge order
/// calculation" loop (`depgraph.py:9509-9518`) is deliberately **not**
/// ported, and this note records why.
///
/// Real's graph is seeded from `DependencyArg` nodes (`@world`,
/// `@selected`, `@system`, each explicit target), so before scheduling it
/// carries the entire installed universe those sets reach -- 1854 nodes
/// for the live case this module was validated against, of which the
/// prune removes 977. Portuale's graph is built from resolved
/// `GraphEntry`s instead: it has no arg nodes and never contains that
/// universe in the first place (461 nodes for the same case, which is
/// exactly real's own post-prune reachable-from-merge-tasks closure --
/// re-running the algorithm on just that closure reproduces real's merge
/// list identically).
///
/// What the prune *would* still remove here is a top-level
/// `AlreadyInstalled` entry -- a nomerge node with no parents. Real drops
/// those because it never displays one at all (confirmed live: `emerge -p
/// --noreplace <already-installed pkg>` prints nothing whatsoever).
/// Portuale does display them, and its own `--tree` / "package is already
/// installed" rendering wants them ordered after the dependencies they
/// pull in -- which is what leaving them in the scheduler produces, since
/// such a node only becomes a leaf once its whole subtree has been
/// emitted. Keeping them costs nothing in fidelity for the merge tasks
/// themselves: a parentless node is skipped by real's own "removing a
/// root node will not produce a leaf node, so avoid it" preference at
/// every relaxed `ignore_priority` rung anyway.
/// Real `_emerge/_find_deep_system_runtime_deps.py`: every `@system`-set
/// member in the graph, plus everything reachable from one by following
/// only *runtime*-priority edges (`RDEPEND`/`IDEPEND`/`PDEPEND`;
/// `DEPEND`/`BDEPEND` are dropped). Feeds `merge_order_bias`'s own
/// "system deps first" tier -- real: "promote deep system runtime
/// deps... for optimal leaf node selection".
fn deep_system_deps(
    g: &Digraph,
    entries: &[GraphEntry],
    config: &portage_profile::Config,
) -> Vec<bool> {
    let system_cps: HashSet<(String, String)> = config
        .system_packages
        .iter()
        .filter_map(|a| portage_dep::parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();
    let mut deep = vec![false; g.n];
    let mut stack: Vec<usize> = Vec::new();
    for &i in &g.order {
        if system_cps.contains(&(entries[i].category.clone(), entries[i].package.clone())) {
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        if deep[i] {
            continue;
        }
        deep[i] = true;
        for (c, prios) in &g.children[i] {
            if g.alive[*c] && prios.iter().any(|p| p.runtime || p.runtime_post) {
                stack.push(*c);
            }
        }
    }
    deep
}

/// Real `depgraph._merge_order_bias` (`depgraph.py:9274-9307`): re-sorts
/// `mygraph.order` in place so that, among simultaneously-eligible
/// leaves, `@system`-deep runtime deps come first and the rest go from
/// highest to lowest reference count. Real's own uninstalls-last rule
/// has nothing to apply to here (a `--pretend` merge graph has no
/// uninstall nodes).
///
/// `implicit_system_deps` is real `myparams["implicit_system_deps"]`
/// (`create_depgraph_params.py:120`, default on): `false` (real
/// `--implicit-system-deps=n`) takes real's own early return
/// (`depgraph.py:9279`) and leaves discovery order alone.
fn merge_order_bias(
    g: &mut Digraph,
    entries: &[GraphEntry],
    config: &portage_profile::Config,
    implicit_system_deps: bool,
) {
    if !implicit_system_deps {
        return;
    }
    let deep = deep_system_deps(g, entries, config);
    let parent_count: Vec<usize> = (0..g.n)
        .map(|i| g.parents[i].iter().filter(|&&p| g.alive[p]).count())
        .collect();
    // Stable, so a tie keeps discovery order -- real's own
    // `cmp_sort_key` comparator returns 0 for a tie and `list.sort` is
    // stable too.
    g.order
        .sort_by_key(|&i| (!deep[i], std::cmp::Reverse(parent_count[i])));
}

// ---------------------------------------------------------------------
// the scheduler
// ---------------------------------------------------------------------

/// Real `_serialize_tasks`' own libc / os-headers `asap_nodes` seeding
/// (`depgraph.py:9608-9638`, bug #303567 / #328317): merge libc as early
/// as possible -- almost every ebuild has an implicit build-time
/// dependency on it that real strips (`strip_libc_deps`) and re-expresses
/// as this ordering preference -- and pull an `os-headers` upgrade in
/// first when there is one.
///
/// Real walks `_expand_virt_from_graph(root, virtual/libc)` -> the
/// graphed `virtual/libc`'s own `RDEPEND` provider atoms ->
/// `_package_tracker.match` -> the provider package, keeping only
/// `pkg.operation == "merge"` (not already installed at that cpv).
/// Portuale's equivalent: find the graphed `virtual/libc` /
/// `virtual/os-headers` entry, take its `RDEPEND` (`key == 0`) provider
/// `cat/pkg`s, and return the merge-bound entries with those `cp`s.
/// `os-headers` providers first (bug #328317), then libc.
fn seed_toolchain_asap(entries: &[GraphEntry]) -> Vec<usize> {
    // Real: `pkg.operation == "merge" and not vardb.cpv_exists(pkg.cpv)`
    // -- a genuine new-version/upgrade merge, NOT a bare `[ebuild R]`
    // reinstall at a cpv already in the vdb. A `@world` re-emerge where
    // `virtual/libc` / `virtual/os-headers` just reinstall must not seed
    // `asap`, or they front-load past dep-free `@system` leaves like
    // `sys-devel/gnuconfig`.
    let new_cpv_merge = |i: usize| {
        matches!(
            entries[i].outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        )
    };
    let providers = |virt_pkg: &str| -> Vec<usize> {
        let Some(vi) = entries
            .iter()
            .position(|e| e.category == "virtual" && e.package == virt_pkg)
        else {
            return Vec::new();
        };
        let mut out: Vec<usize> = Vec::new();
        // The virtual itself, if it is a genuine merge (real
        // `_package_tracker.match` returns it too).
        if new_cpv_merge(vi) {
            out.push(vi);
        }
        for edge in entries[vi].deps.iter().filter(|d| d.key == 0) {
            if let Some(pi) = entries
                .iter()
                .position(|e| e.category == edge.category && e.package == edge.package)
                && new_cpv_merge(pi)
                && !out.contains(&pi)
            {
                out.push(pi);
            }
        }
        out
    };
    let mut asap: Vec<usize> = Vec::new();
    for i in providers("os-headers").into_iter().chain(providers("libc")) {
        if !asap.contains(&i) {
            asap.push(i);
        }
    }
    asap
}

/// The version a `GraphEntry` resolves to, whatever its outcome -- real
/// `Package.version`, which `find_smallest_cycle`'s `sorted(nodes)`
/// compares after `cp`. A `Uninstall` removal (#72 B3) carries the
/// installed version it removes, like real's uninstall `Package`.
pub(crate) fn entry_version(e: &GraphEntry) -> Option<&str> {
    match &e.outcome {
        PretendOutcome::New { version }
        | PretendOutcome::Reinstall { version, .. }
        | PretendOutcome::AlreadyInstalled { version }
        | PretendOutcome::Uninstall { version } => Some(version),
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::NoVisibleCandidate => None,
    }
}

/// Real `_serialize_tasks`' `gather_deps`: the closure of `node` under
/// `ig`, or `None` when it escapes `mergeable`. "Recursively gather a
/// group of nodes that RDEPEND on eachother. This ensures that they are
/// merged as a group and get their RDEPENDs satisfied as soon as
/// possible."
fn gather_deps(
    g: &Digraph,
    node: usize,
    ig: Option<Ignore>,
    mergeable: &HashSet<usize>,
) -> Option<HashSet<usize>> {
    let mut sel: HashSet<usize> = HashSet::new();
    let mut stack = vec![node];
    while let Some(x) = stack.pop() {
        if sel.contains(&x) {
            continue;
        }
        if !mergeable.contains(&x) {
            return None;
        }
        sel.insert(x);
        stack.extend(g.child_nodes(x, ig));
    }
    Some(sel)
}

/// Real `find_smallest_cycle`: the smallest `gather_deps` closure among
/// the currently-mergeable nodes, searched from the lowest
/// `ignore_priority` rung upward so as few dependencies as possible are
/// relaxed. "In the case of multiple runtime cycles, where some cycles
/// may depend on smaller independent cycles, it's optimal to merge
/// smaller independent cycles before other cycles that depend on them."
fn find_smallest_cycle(
    g: &Digraph,
    frontier: Option<&mut SerializeFrontier>,
    entries: &[GraphEntry],
    range: &PriorityRange,
    asap: &[usize],
    prefer_asap: bool,
    // #142 S2 F4b: nodes temporarily ineligible for harvesting —
    // owners waiting on their unadmitted removal (real's validation
    // edge keeps them out of the leaf set, so real's cycle search
    // never sees them either). Empty everywhere else, including all
    // unit tests.
    ineligible: &HashSet<usize>,
) -> Option<(HashSet<usize>, Option<Ignore>)> {
    let mergeable: HashSet<usize> = leaves_via(frontier, g, range.ig_medium())
        .into_iter()
        .filter(|i| !ineligible.contains(i))
        .collect();
    if mergeable.is_empty() {
        return None;
    }
    let mut cand: Vec<usize> = if prefer_asap && !asap.is_empty() {
        asap.iter()
            .copied()
            .filter(|i| mergeable.contains(i))
            .collect()
    } else {
        mergeable.iter().copied().collect()
    };
    // Real "Sort nodes for deterministic results" -- `sorted(nodes)`,
    // i.e. `Package.__lt__`: `cp` then version.
    cand.sort_by(|&a, &b| {
        (entries[a].category.as_str(), entries[a].package.as_str())
            .cmp(&(entries[b].category.as_str(), entries[b].package.as_str()))
            .then_with(|| {
                let ver = |i: usize| entry_version(&entries[i]).unwrap_or_default().to_string();
                portage_versions::vercmp(&ver(a), &ver(b))
                    .unwrap_or(0)
                    .cmp(&0)
            })
    });
    let mut best: Option<(HashSet<usize>, Option<Ignore>)> = None;
    for i in range.medium_post..=range.medium_soft {
        let ig = range.ig(i);
        for &node in &cand {
            if !g.has_parents(node) {
                continue;
            }
            let Some(cl) = gather_deps(g, node, ig, &mergeable) else {
                continue;
            };
            if best.as_ref().is_none_or(|(b, _)| cl.len() < b.len()) {
                best = Some((cl, ig));
            }
        }
        // "Exit this loop with the lowest possible priority, which
        // minimizes the use of installed packages to break cycles."
        if best.is_some() {
            break;
        }
    }
    best
}

/// Real `_serialize_tasks`' own final block for a multi-node cycle:
/// harvest one node at a time from the induced subgraph, always taking a
/// leaf at the lowest `ignore_priority` that produces one, and
/// preferring an installed leaf "in order to avoid merging something too
/// early as in bug 917259".
fn harvest_cycle(g: &Digraph, sub: &HashSet<usize>) -> Vec<usize> {
    let ladder: Vec<Option<Ignore>> = NORMAL
        .ignore
        .iter()
        .chain(SATISFIED.ignore.iter())
        .filter(|f| f.is_some())
        .copied()
        .collect();
    let mut remaining = sub.clone();
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = Vec::new();
        for ig in &ladder {
            leaves = g
                .order
                .iter()
                .copied()
                .filter(|i| {
                    remaining.contains(i)
                        && !g.children[*i].iter().any(|(c, prios)| {
                            remaining.contains(c) && ig.is_none_or(|f| prios.iter().any(|p| !f(p)))
                        })
                })
                .collect();
            if !leaves.is_empty() {
                break;
            }
        }
        if leaves.is_empty() {
            // Real `leaves = [cycle_digraph.order[-1]]`.
            leaves = vec![
                *g.order
                    .iter()
                    .rev()
                    .find(|i| remaining.contains(i))
                    .expect("remaining is non-empty"),
            ];
        }
        let installed_leaves: Vec<usize> =
            leaves.iter().copied().filter(|&i| g.installed[i]).collect();
        let pick = *installed_leaves.first().unwrap_or(&leaves[0]);
        remaining.remove(&pick);
        out.push(pick);
    }
    out
}

/// Real `depgraph._serialize_tasks`' own selection loop, restricted to
/// what a `--pretend` merge graph contains: no `Uninstall` tasks, no
/// blocker nodes (portuale renders blockers separately), and therefore
/// none of real's `myblocker_uninstalls` scheduling. What remains is the
/// whole of real's leaf-selection machinery:
///
/// * the `NONE .. MEDIUM_SOFT` `ignore_priority` ladder, with real's
///   "greedily pop all of these nodes since no relationship has been
///   ignored" batch at rung `NONE` and one-node-at-a-time selection
///   (preferring a node that actually has a parent) at every relaxed rung;
/// * `asap_nodes` -- real's `PDEPEND`-promotion path (bug #180045): every
///   unsatisfied `runtime_post` child freed by a relaxed-rung selection
///   is queued to be merged as soon as it becomes a leaf, ahead of the
///   ordinary bias order, and is scanned under `DepPrioritySatisfiedRange`;
/// * `find_smallest_cycle` for a genuine runtime cycle, with the
///   `drop_satisfied` escalation to `DepPrioritySatisfiedRange` that lets
///   an already-installed dependency break one.
///
/// Port of real `portage.util.digraph.get_cycles` (+ `shortest_path` +
/// `bfs`): every node's shortest child→node paths (all ties), each a
/// recorded cycle. Real feeds this the stuck remainder with the
/// `medium_soft` rung and counts the records (`large_cycle_count =
/// len(cycles) > 3`); rotations of one ring count separately (a square
/// records four), which is exactly what makes the count a "lot of
/// cycles" signal rather than a ring census.
///
/// `ig` is the survival filter (real's `ignore_priority`); pass `None`
/// for the unfiltered graph. Nodes with no path back to themselves
/// contribute nothing. Only the count and the member union are consumed
/// downstream, so traversal order is fixed (sorted indices) for
/// determinism -- real keeps every tie, which makes its result set
/// order-independent too (same reasoning as its PYTHONHASHSEED comment).
fn elementary_cycles(g: &Digraph, ig: Option<Ignore>) -> Vec<Vec<usize>> {
    use std::collections::VecDeque;
    /// BFS shortest path, real `digraph.shortest_path`: first visit wins
    /// per node (BFS order), returned on first reaching `end`. `None`
    /// when `end` is unreachable (real raises `KeyError` for unknown
    /// nodes instead -- every node here is known by construction).
    fn shortest_path(
        g: &Digraph,
        start: usize,
        end: usize,
        ig: Option<Ignore>,
    ) -> Option<Vec<usize>> {
        let mut paths: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut queue: VecDeque<(Option<usize>, usize)> = VecDeque::from([(None, start)]);
        let mut enqueued: HashSet<usize> = HashSet::from([start]);
        while let Some((parent, n)) = queue.pop_front() {
            let mut path = match parent {
                None => Vec::new(),
                Some(p) => paths.get(&p).cloned().unwrap_or_default(),
            };
            path.push(n);
            paths.insert(n, path);
            if n == end {
                return paths.remove(&end);
            }
            let mut fresh: Vec<usize> = g.children[n]
                .iter()
                .filter(|(c, prios)| {
                    !enqueued.contains(c) && ig.is_none_or(|f| prios.iter().any(|p| !f(p)))
                })
                .map(|(c, _)| *c)
                .collect();
            fresh.sort_unstable();
            for c in fresh {
                enqueued.insert(c);
                queue.push_back((Some(n), c));
            }
        }
        None
    }

    let mut all: Vec<Vec<usize>> = Vec::new();
    for &node in &g.order {
        // Shortest child→node path length seen for this node; every
        // path tied at the minimum is recorded (real appends paths not
        // longer than the running minimum, then filters by the final
        // minimum -- same set, same child order).
        let mut min_len: Option<usize> = None;
        let mut cands: Vec<Vec<usize>> = Vec::new();
        for (child, prios) in &g.children[node] {
            if !ig.is_none_or(|f| prios.iter().any(|p| !f(p))) {
                continue;
            }
            let Some(path) = shortest_path(g, *child, node, ig) else {
                continue;
            };
            if min_len.is_none_or(|m| path.len() <= m) {
                if min_len.is_none_or(|m| path.len() < m) {
                    min_len = Some(path.len());
                }
                cands.push(path);
            }
        }
        if let Some(m) = min_len {
            all.extend(cands.into_iter().filter(|p| p.len() == m));
        }
    }
    all
}

/// Port of real `circular_dependency_handler._prepare_reduced_merge_list`
/// over an explicit drain set: leaf-drain (no filter, like real's plain
/// `leaf_nodes()`), falling back to the lowest-order remaining node when
/// nothing is a leaf. Real drains its whole stuck remainder -- cycle
/// members plus everything left unscheduled downstream of them; the
/// caller (`cycle_report`) passes exactly that set: members plus
/// transitive requirers. A drained child frees its parents (checked
/// against the shrinking remainder, not the static set), matching
/// real's shrinking copy.
fn reduced_merge_order(g: &Digraph, drain: &HashSet<usize>) -> Vec<usize> {
    // A node is a leaf when it has no still-remaining child in the
    // drain set -- self-edges count, exactly like real's plain
    // `leaf_nodes()` on its shrinking copy (an unsatisfied buildtime
    // self-loop is the only self-edge `build_digraph` keeps). Checking
    // a static set instead would pin every ring member forever, since
    // a drained child must free its parents.
    let mut remaining: HashSet<usize> = drain.clone();
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = g
            .order
            .iter()
            .copied()
            .filter(|i| {
                remaining.contains(i) && !g.children[*i].iter().any(|(c, _)| remaining.contains(c))
            })
            .collect();
        if leaves.is_empty() {
            // Real `node = tempgraph.order[0]` -- lowest-order remaining
            // node in insertion order (the fallback only fires inside
            // a ring, where every remaining node has a remaining child).
            leaves = g
                .order
                .iter()
                .copied()
                .filter(|i| remaining.contains(i))
                .take(1)
                .collect();
        }
        for i in leaves {
            remaining.remove(&i);
            out.push(i);
        }
    }
    out
}

/// The `medium_soft` rung of the satisfied range -- the filter real's
/// cycle handler passes to `get_cycles`. (Kept as an accessor because
/// `PriorityRange`/`SATISFIED` stay private to this module.)
fn satisfied_medium_soft_rung() -> Option<Ignore> {
    SATISFIED.ig_medium_soft()
}

/// Cycle report over a freshly built scheduling graph: all elementary
/// cycles (entry indices) plus the reduced display order. The drain set
/// is the cycle members plus everything transitively requiring them --
/// real drains its whole stuck remainder (members plus everything left
/// unscheduled downstream); portuale schedules everything, so the
/// downstream cone is re-derived here from `required_by` (cp-level, like
/// everywhere else). Built on demand -- callers only pay for it when a
/// hard cycle was already reported (the only path that consumes either
/// number). `top_level_atoms`/`root` feed `build_digraph` exactly as
/// `serialize_merge_order` passes them.
pub(crate) fn cycle_report(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let g = build_digraph(entries, top_level_atoms, root);
    let cycles = elementary_cycles(&g, satisfied_medium_soft_rung());
    let members: HashSet<usize> = cycles.iter().flatten().copied().collect();
    let mut cp_indices: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        cp_indices
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(i);
    }
    let mut drain: HashSet<usize> = members.clone();
    let mut stack: Vec<usize> = members.iter().copied().collect();
    while let Some(i) = stack.pop() {
        for owner in &entries[i].required_by {
            if let Some(idxs) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str())) {
                for &j in idxs {
                    if drain.insert(j) {
                        stack.push(j);
                    }
                }
            }
        }
    }
    let display = reduced_merge_order(&g, &drain);
    (cycles, display)
}

/// Returns the alive nodes in scheduling order (installed "nomerge"
/// nodes included -- the caller drops them).
/// Backlog #81: tree-mode stuck-branch state for [`select_nodes`].
///
/// Real's tree serializer keeps `scheduled_uninstalls` as live scheduler
/// state: when a round selects nothing and blockers are pending, it
/// schedules an uninstall for the blocked package, reverses the edges so
/// the blocked package merges on top of it, and appends the solved
/// blocker when the blocked package (or the uninstall itself) is later
/// selected (`depgraph.py:9998`, `:10190`, `:10315-10358`). The flat
/// greedy pop drains past the same shape, so the row stays hidden there.
///
/// Portuale's scheduler graph has no uninstall nodes, so a scheduled
/// uninstall is a synthetic node: childless, with the blocked
/// (replacement) entry as its one parent and a hard (never-ignored)
/// edge. Selection and removal flow through the normal loop; only the
/// verdicts (`solved`) escape.
struct TreeStuck {
    /// One pending scheduling per unsatisfied Replacement row:
    /// the replacement entry, the installed instance it replaces,
    /// and the row's address for the verdict.
    pending: Vec<PendingUninstall>,
    /// Synthetic uninstall node indices already scheduled.
    scheduled: HashSet<usize>,
    /// Verdicts: `(owner entry, blocker index)` rows tree mode solves.
    solved: Vec<(usize, usize)>,
    /// Set by [`tree_schedule_stuck`] when it schedules something, so
    /// the loop resets state and retries selection like real's
    /// `continue`.
    progressed: bool,
}

/// One install-replacing satisfied row awaiting tree-mode scheduling.
#[derive(Debug)]
struct PendingUninstall {
    /// The merge-bound replacement entry.
    repl: usize,
    /// The installed instance it replaces (cp, slot, version).
    inst_cp: (String, String),
    inst_slot: String,
    inst_version: String,
    /// Verdict address.
    owner: usize,
    blocker: usize,
    /// Real applies overlap guards to strong (`!!`) blockers
    /// (`forbid_overlap`, portage-essential); v1 schedules soft (`!`)
    /// rows only and leaves strong rows hidden (current behavior).
    strong: bool,
    /// Synthetic uninstall node once scheduled.
    node: Option<usize>,
}

/// The stuck branch: schedule one pending uninstall.
///
/// Mirrors real's choice loop in narrowed form: among pending rows whose
/// uninstall is not yet scheduled, whose installed instance is not
/// itself a graph node (real `digraph.contains(inst_pkg)` skips those),
/// and whose blocker is soft, take the first in entry order (real picks
/// by fewest parent-deps with an early break at one; single-pending
/// shapes -- every oracle here -- decide identically). Creates the
/// synthetic uninstall node as the replacement's child with a hard edge
/// (real's edge reversal: the blocked package merges on top of it).
fn tree_schedule_stuck(g: &mut Digraph, entries: &[GraphEntry], ts: &mut TreeStuck) {
    for p in ts.pending.iter_mut() {
        if p.node.is_some() {
            continue;
        }
        if p.strong {
            continue;
        }
        let walked_installed = entries.iter().any(|e| {
            (e.category.as_str(), e.package.as_str()) == (p.inst_cp.0.as_str(), p.inst_cp.1.as_str())
                && e.slot.as_deref() == Some(p.inst_slot.as_str())
                && matches!(&e.outcome, crate::PretendOutcome::AlreadyInstalled { version } if version == &p.inst_version)
        });
        if walked_installed {
            continue;
        }
        // Synthetic uninstall node: no children of its own (a leaf), the
        // replacement as its one parent. Appended to `order` like real's
        // `mygraph.add` appends new nodes.
        let u = g.n;
        g.n += 1;
        g.children.push(Vec::new());
        g.parents.push(vec![p.repl]);
        g.order.push(u);
        g.installed.push(true);
        g.alive.push(true);
        g.children[p.repl].push((u, vec![DepPriority::default()]));
        p.node = Some(u);
        ts.scheduled.insert(u);
        ts.progressed = true;
        return;
    }
}

/// Record solved verdicts at selection time.
///
/// Real appends a solved blocker when the blocked package merges while
/// its uninstall is scheduled, and discards it when the uninstall node
/// itself is selected (`:10315-10358`). Either selection orders the same
/// verdict here.
fn tree_note_selection(
    g: &mut Digraph,
    entries: &[GraphEntry],
    ts: &mut TreeStuck,
    selected: usize,
) {
    let _ = entries;
    for p in &ts.pending {
        let Some(u) = p.node else {
            continue;
        };
        if !ts.scheduled.contains(&u) {
            continue;
        }
        if selected == p.repl || selected == u {
            if selected == p.repl {
                // The replacement merged on top: the uninstall task is
                // gone (real `mygraph.remove`), like the flat loop's
                // removal site.
                g.alive[u] = false;
            }
            if !ts.solved.contains(&(p.owner, p.blocker)) {
                ts.solved.push((p.owner, p.blocker));
            }
        }
    }
}

/// Backlog #81 S1: which satisfied Replacement rows tree-mode
/// serialization solves.
///
/// Runs the shared scheduler loop in tree mode (greedy pop disabled,
/// one non-root leaf per round) with the stuck branch enabled, and
/// returns the solved `(owner entry, blocker index)` rows. Callers pass
/// resolver-indexed `entries`; verdicts address the same indexing.
///
/// Shares `schedule_graph` (closure, prune, bias) and every selection
/// helper with the flat path -- the only differences from
/// [`serialize_merge_order`] are the `not tree_mode` gate and the stuck
/// branch, exactly like real. Returns empty without running the loop
/// when no Replacement row is pending (the common case pays nothing).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tree_solved_replacements(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    config: &portage_profile::Config,
    root: &Path,
    implicit_system_deps: bool,
    repos: &[RepoConfig],
    dynamic_deps: bool,
) -> Vec<(usize, usize)> {
    // Pending rows: unsatisfied Replacement arms (the uninstall-solving
    // Uninstall arms and the unsolvable rows already have display homes
    // everywhere; strong blockers stay hidden per the v1 narrowing).
    let mut pending = Vec::new();
    for (owner, entry) in entries.iter().enumerate() {
        for (blocker, b) in entry.blockers.iter().enumerate() {
            if b.unsolvable {
                continue;
            }
            let Some(crate::BlockerSatisfiedBy::Replacement { cp, slot }) = &b.satisfied_by else {
                continue;
            };
            let Some(repl) = entries.iter().position(|e| {
                (e.category.as_str(), e.package.as_str()) == (cp.0.as_str(), cp.1.as_str())
                    && e.slot.as_deref() == Some(slot.as_str())
                    && crate::merge_bound_version(&e.outcome)
                        .is_some_and(|v| v != b.matched_version.as_str())
            }) else {
                continue;
            };
            pending.push(PendingUninstall {
                repl,
                inst_cp: (b.matched_category.clone(), b.matched_package.clone()),
                inst_slot: slot.clone(),
                inst_version: b.matched_version.clone(),
                owner,
                blocker,
                strong: b.strong,
                node: None,
            });
        }
    }
    if pending.is_empty() {
        return Vec::new();
    }
    let (ext, mut g, _real_n, _discovery_rank) = schedule_graph(
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
    );
    // NOTE: `ext` may append synthetic closure entries; `pending`
    // addresses resolver indexing, which is a prefix of `ext` (the
    // closure only appends), so indices stay valid.
    let _ = &ext;
    let mut ts = TreeStuck {
        pending,
        scheduled: HashSet::new(),
        solved: Vec::new(),
        progressed: false,
    };
    select_nodes(&mut g, &ext, root, Some(&mut ts));
    ts.solved
}

fn select_nodes(
    g: &mut Digraph,
    entries: &[GraphEntry],
    root: &Path,
    mut tree: Option<&mut TreeStuck>,
) -> Vec<usize> {
    let mut retlist: Vec<usize> = Vec::new();
    let mut asap: Vec<usize> = seed_toolchain_asap(entries);
    let mut prefer_asap = true;
    let mut drop_satisfied = false;
    // The incremental leaf frontier (see `SerializeFrontier`): built
    // once from the biased order, kept in sync at the one removal site
    // below. `None` when `PORTAGE_SERIALIZE_FRONTIER_DISABLE` falls back
    // to the plain scans.
    let mut frontier: Option<SerializeFrontier> =
        frontier_enabled().then(|| SerializeFrontier::build(g));
    let mut mo_iter: usize = 0;

    // #142 S2 F4b: real's uninstall-scheduling dance
    // (`depgraph.py`, `_validate_blockers` through `_serialize_tasks`):
    // every uninstall task is ordered after its blocker owner (the
    // validation edge — the owner is not a leaf while its removal is
    // unscheduled), an empty selection round admits removals (edges
    // reversed: the removal then waits on its owner), and a round
    // mixing merges and admitted uninstalls takes the uninstalls
    // first (`good_uninstalls`). Portuale files removals only for
    // solved blockers (real's schedulable uninstalls) with the
    // execution edge already in the graph, so the dance is virtualized
    // as two leaf-selection rules instead of graph surgery (the
    // frontier never sees an edge change): an owner of a live
    // unadmitted removal waits (filtered out of every leaf set), and
    // the first stuck round admits removals. No removal present: both
    // rules are vacuous and selection is byte-identical to before.
    //
    // Waiting fires only for the merging arm — the anchor entry
    // carrying the `Uninstall` row naming this removal. A nomerge-arm
    // anchor (B0b/#77 A1: the blocked merge instance) never waits,
    // exactly like real, whose validation edge runs owner-side.
    let removals: Vec<usize> = (0..entries.len())
        .filter(|&i| matches!(entries[i].outcome, PretendOutcome::Uninstall { .. }))
        .collect();
    // Static (removal, merging-arm owner) pairs; liveness is checked
    // per round below.
    let removal_owners: Vec<(usize, usize)> =
        removals
            .iter()
            .filter_map(|&r| {
                let rcpv = match &entries[r].outcome {
                    PretendOutcome::Uninstall { version } => {
                        format!("{}/{}-{version}", entries[r].category, entries[r].package)
                    }
                    _ => return None,
                };
                entries
                    .iter()
                    .position(|e| {
                        matches!(
                    e.outcome,
                    PretendOutcome::New { .. }
                        | PretendOutcome::Upgrade { .. }
                        | PretendOutcome::Downgrade { .. }
                        | PretendOutcome::Reinstall { .. }
                ) && entries[r].required_by.iter().any(|(cc, pp)| {
                    *cc == e.category && *pp == e.package
                }) && e.blockers.iter().any(|b| {
                    matches!(
                        b.satisfied_by,
                        Some(BlockerSatisfiedBy::Uninstall { ref cpv, .. }) if cpv == &rcpv
                    )
                })
                    })
                    .map(|o| (r, o))
            })
            .collect();
    let mut scheduled: HashSet<usize> = HashSet::new();

    // B1: one `MO_NODES` snapshot of the post-prune graph, so the aligner
    // can name a membership difference (gtk:4 is alive=398 vs 395 at
    // iteration 1) instead of only reporting the count.
    if mo_sel_enabled() {
        let nodes: Vec<String> = g
            .order
            .iter()
            .filter(|&&i| g.alive[i])
            .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
            .collect();
        eprintln!("MO_NODES count={} {}", nodes.len(), nodes.join(" "));
    }

    while g.order.iter().any(|&i| g.alive[i]) {
        mo_iter += 1;
        let mut selected: Option<Vec<usize>> = None;
        let mut used_ig: Option<Ignore> = None;
        let asap_active = prefer_asap && !asap.is_empty();
        let range: &PriorityRange = if asap_active { &SATISFIED } else { &NORMAL };
        // F4b: owners waiting on their unadmitted removal (the
        // validation edge, virtualized). Recomputed per round: admission
        // below lifts entries out of this set.
        let waiting: HashSet<usize> = removal_owners
            .iter()
            .filter(|(r, _)| !scheduled.contains(r) && g.alive[*r])
            .map(|(_, o)| *o)
            .filter(|o| g.alive[*o])
            .collect();
        // Cycle search must not see waiting owners either (real's
        // validation edge keeps them out of its leaf set), nor a
        // removal that was never admitted (defensive: one is only a
        // leaf once its owner is gone, which admission precedes).
        let ineligible: HashSet<usize> = waiting
            .iter()
            .copied()
            .chain(
                removals
                    .iter()
                    .copied()
                    .filter(|r| !scheduled.contains(r) && g.alive[*r]),
            )
            .collect();

        if asap_active {
            asap.retain(|&i| g.alive[i]);
            'asap: for i in 1..=range.medium_soft {
                let ig = range.ig(i);
                for (pos, &node) in asap.iter().enumerate() {
                    if !waiting.contains(&node) && is_leaf_via(frontier.as_ref(), g, node, ig) {
                        selected = Some(vec![node]);
                        used_ig = ig;
                        asap.remove(pos);
                        break 'asap;
                    }
                }
            }
        }

        if selected.is_none() && !(prefer_asap && !asap.is_empty()) {
            for i in 0..=range.medium_soft {
                let ig = range.ig(i);
                let nodes = leaves_via(frontier.as_mut(), g, ig);
                // F4b: waiting owners never compete (real's validation
                // edge keeps them out of the leaf set), and an
                // unadmitted removal is only a leaf once its owner is
                // gone — which admission below always precedes, so
                // filter defensively all the same.
                let rest: Vec<usize> = nodes
                    .iter()
                    .copied()
                    .filter(|n| {
                        !waiting.contains(n) && (scheduled.contains(n) || !removals.contains(n))
                    })
                    .collect();
                if rest.is_empty() {
                    continue;
                }
                // Real: "Greedily pop all of these nodes since no
                // relationship has been ignored." Real additionally
                // gates this on `not tree_mode`, because the batch
                // "destroys --tree output" (`depgraph.py:9764-9777`).
                // The flat caller (`tree == None`) keeps portuale's
                // standing port (its `--tree` re-derives nesting from
                // `required_by` top-down instead of consuming the
                // serialized list); the #81 tree simulation passes
                // `Some` and takes real's gate, popping one non-root
                // node per round so the stuck branch below can fire.
                //
                // #142 S2 F4: "If there is a mixture of merges and
                // uninstalls, do the uninstalls first"
                // (`depgraph.py`, same selection-round block). Every
                // removal reaching this filter is admitted (F4b
                // admission runs before any owner it waits on can
                // schedule), i.e. one of real's `scheduled_uninstalls`
                // — bed blk0 cells schedule `[X-1, uninstall, blocks,
                // ...]`; the blockerorderpkg T7 pin already expects
                // post-owner placement.
                let mut rest = rest;
                let good_uninstalls: Vec<usize> = if rest.len() > 1 {
                    rest.iter()
                        .copied()
                        .filter(|&node| {
                            matches!(entries[node].outcome, PretendOutcome::Uninstall { .. })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let has_good_uninstalls = !good_uninstalls.is_empty();
                if has_good_uninstalls {
                    rest = good_uninstalls;
                }
                if has_good_uninstalls
                    || rest.len() == 1
                    || (ig.is_none() && asap.is_empty() && tree.is_none())
                {
                    selected = Some(rest);
                } else {
                    // "For optimal merge order: only pop one node;
                    // removing a root node (node without a parent) will
                    // not produce a leaf node, so avoid it." Real first
                    // prefers a node whose parent is itself an asap node.
                    let mut picked = None;
                    if !asap.is_empty() {
                        picked = rest.iter().copied().find(|&node| {
                            g.parents[node].iter().any(|&p| {
                                g.alive[p]
                                    && asap.contains(&p)
                                    && g.children[p].iter().any(|(c, prios)| {
                                        *c == node
                                            && range
                                                .ig_medium_soft()
                                                .is_none_or(|f| prios.iter().any(|q| !f(q)))
                                    })
                            })
                        });
                    }
                    if picked.is_none() {
                        picked = rest.iter().copied().find(|&node| g.has_parents(node));
                    }
                    selected = picked.map(|p| vec![p]);
                }
                if selected.is_some() {
                    used_ig = ig;
                    break;
                }
            }
        }

        if selected.is_none() {
            let mut ranges: Vec<&PriorityRange> = Vec::new();
            if !std::ptr::eq(range, &NORMAL) {
                ranges.push(&NORMAL);
            }
            ranges.push(range);
            if drop_satisfied && !std::ptr::eq(range, &SATISFIED) {
                ranges.push(&SATISFIED);
            }
            for lr in ranges {
                if let Some((sub, ig)) = find_smallest_cycle(
                    g,
                    frontier.as_mut(),
                    entries,
                    lr,
                    &asap,
                    prefer_asap,
                    &ineligible,
                ) {
                    used_ig = ig;
                    // `emerge --pretend --debug`: real
                    // `depgraph.py:9917-9930`'s `\nruntime cycle digraph
                    // (<n> nodes):\n\n` + `debug_print()` of the induced
                    // subgraph, then `runtime cycle leaf: <pkg>` -- to
                    // stderr, once per relaxed cycle.
                    if crate::resolver_debug() && sub.len() > 1 {
                        debug_dump_cycle(g, entries, &sub, root);
                    }
                    // "NOTE: This case should only be triggered when
                    // prefer_asap is True... select only one node here,
                    // so that merge order accounts for as many
                    // dependencies as possible."
                    let sub_leaves: Vec<usize> = g
                        .order
                        .iter()
                        .copied()
                        .filter(|i| {
                            sub.contains(i) && !g.children[*i].iter().any(|(c, _)| sub.contains(c))
                        })
                        .collect();
                    selected = Some(if let Some(&first) = sub_leaves.first() {
                        vec![first]
                    } else if sub.len() > 1 {
                        harvest_cycle(g, &sub)
                    } else {
                        sub.into_iter().collect()
                    });
                    break;
                }
            }
            if selected.is_none() && prefer_asap && !asap.is_empty() {
                // "We failed to find any asap nodes to merge, so ignore
                // them for the next iteration."
                prefer_asap = false;
                continue;
            }
        }

        // "Try to merge neglected medium_post deps as soon as possible if
        // they're not satisfied by installed packages." -- real's
        // `PDEPEND`-asap promotion, bug #180045.
        if let (Some(sel), Some(_)) = (selected.as_ref(), used_ig) {
            let mut promoted: Vec<usize> = Vec::new();
            for &node in sel {
                for (c, prios) in &g.children[node] {
                    if !g.alive[*c] {
                        continue;
                    }
                    let is_medium_post = !prios.iter().any(|p| !s_ignore_runtime_post(p));
                    let is_satisfied_medium_post =
                        !prios.iter().any(|p| !s_ignore_satisfied_runtime_post(p));
                    if is_medium_post && !is_satisfied_medium_post {
                        promoted.push(*c);
                    }
                }
            }
            for c in promoted {
                if sel.contains(&c) || asap.contains(&c) {
                    continue;
                }
                asap.push(c);
            }
        }

        // Backlog #81: the stuck branch (`depgraph.py:9998`), tree-mode
        // only. When no leaf is selectable and replacement rows are still
        // pending, real schedules an uninstall for the blocked package
        // (`scheduled_uninstalls`, `:10190`) and reverses its edges so the
        // blocked package merges on top of it; the solved blocker is
        // appended when the blocked package is later selected
        // (`:10351-10358`). It sits here -- after the cycle harvest and
        // the PDEPEND promotion, before the roots-last-resort -- exactly
        // like real's. The flat caller never takes it (`tree == None`).
        if selected.is_none()
            && let Some(ts) = tree.as_mut()
        {
            tree_schedule_stuck(g, entries, ts);
            if ts.progressed {
                ts.progressed = false;
                prefer_asap = true;
                drop_satisfied = false;
                continue;
            }
        }

        // #142 S2 F4b admission: real's uninstall-scheduling round
        // (`depgraph.py:9998-10020`, "An Uninstall task needs to be
        // executed in order to avoid conflict if possible"). No merge
        // node is selectable anywhere on the ladder or the cycle paths
        // above, and a live unadmitted removal exists: admit the first
        // in graph order (real admits one uninstall per stuck round —
        // its min-parent_deps choice approximated here by graph order;
        // single-removal runs are exact), reset the round state like
        // real, and re-select. The admitted removal's owner stops
        // waiting (the validation edge reversed); the execution edge
        // already in the graph still orders the removal after it.
        // Sits after the cycle harvest and before the roots
        // last-resort, exactly like real's: a waiting owner must not
        // be picked as a last-resort root while its removal is still
        // unadmitted.
        if selected.is_none()
            && let Some(&r) = removals
                .iter()
                .find(|r| !scheduled.contains(r) && g.alive[**r])
        {
            scheduled.insert(r);
            prefer_asap = true;
            drop_satisfied = false;
            continue;
        }

        // Real `_serialize_tasks` (`depgraph.py:10230-10231`): "Only
        // select root nodes as a last resort. This case should only
        // trigger when the graph is nearly empty and the only remaining
        // nodes are isolated (no parents or children)." -- but it also
        // catches parentless plain leaves the one-node-at-a-time scan
        // above skipped (it only ever picks a node *with* parents, and
        // once `asap` is non-empty its rung-`NONE` batch is disabled).
        // Real runs this *before* escalating to `drop_satisfied`, so a
        // stub whose only dep is a satisfied-runtime edge is not freed
        // early via `s_ignore_satisfied_runtime`.
        if selected.is_none() {
            // Same set the order scan collected (alive `None`-leaves),
            // via the level-0 heap instead.
            let roots: Vec<usize> = leaves_via(frontier.as_mut(), g, None);
            if !roots.is_empty() {
                // Real leaves `ignore_priority` at `None` here, so the
                // `medium_post` PDEPEND-asap promotion above (gated on
                // `ignore_priority is not None`) does not apply -- and it
                // has already run for this iteration regardless.
                selected = Some(roots);
            }
        }

        if selected.is_none() && !drop_satisfied {
            drop_satisfied = true;
            continue;
        }
        let selected = match selected {
            Some(s) => s,
            // Real raises `_unknown_internal_error` here (an unresolved
            // blocker, or a cycle none of the `ignore_priority` rungs
            // could break). Portuale reports a genuinely unbreakable
            // cycle separately via `find_hard_cycles` and still has to
            // produce a list either way, so it keeps going instead:
            // prefer a node whose every remaining dependency is one the
            // *widest* filter (`DepPrioritySatisfiedRange.ignore_medium`,
            // i.e. everything except an unsatisfied build-time dep) would
            // drop -- the same "break the cycle at a run-time edge"
            // preference the pre-digraph implementation used -- and only
            // fall back to plain bias order when even that finds nothing.
            None => vec![
                g.order
                    .iter()
                    .copied()
                    .find(|&i| g.alive[i] && g.is_leaf(i, SATISFIED.ig_medium()))
                    .or_else(|| g.order.iter().copied().find(|&i| g.alive[i]))
                    .expect("loop condition guarantees one"),
            ],
        };

        if mo_sel_enabled() {
            let retlist_merges = retlist
                .iter()
                .filter(|&&i| {
                    !matches!(
                        entries[i].outcome,
                        PretendOutcome::AlreadyInstalled { .. }
                            | PretendOutcome::NoVisibleCandidate
                    )
                })
                .count();
            let alive = g.order.iter().filter(|&&i| g.alive[i]).count();
            let pick: Vec<String> = selected
                .iter()
                .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
                .collect();
            let asap_cpvs: Vec<String> = asap
                .iter()
                .filter(|&&i| g.alive[i])
                .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
                .collect();
            eprintln!(
                "{}",
                mo_sel_trace_line(
                    mo_iter,
                    retlist_merges,
                    alive,
                    &asap_cpvs,
                    prefer_asap,
                    drop_satisfied,
                    used_ig,
                    &pick,
                )
            );
        }
        prefer_asap = true;
        drop_satisfied = false;
        for i in selected {
            if !g.alive[i] {
                continue;
            }
            g.alive[i] = false;
            // Keep the frontier in sync at the loop's one removal site
            // (real `_FrontierDigraph.remove`); counts/edges are read off
            // the adjacency as it stands.
            if let Some(fr) = frontier.as_mut() {
                fr.remove(i, &g.children[i], &g.parents[i]);
            }
            // Backlog #81: selecting the blocked package while its
            // uninstall is scheduled solves the blocker (real
            // `:10351-10358` appends it to the retlist here); selecting
            // the synthetic uninstall itself discards it the same way
            // (`myblocker_uninstalls.remove`). Either way the row's
            // verdict is recorded once.
            if let Some(ts) = tree.as_mut() {
                tree_note_selection(g, entries, ts, i);
            }
            retlist.push(i);
        }
        g.order.retain(|&i| g.alive[i]);
    }
    retlist
}

impl std::fmt::Display for DepPriority {
    /// Real `_emerge/DepPriority.py::DepPriority.__str__` -- the single
    /// highest classification. `Priority:` lines and `digraph:` edge
    /// labels both use it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::resolver_trace::dep_priority_str(self))
    }
}

/// Dumps the freshly-built digraph in real portage's own
/// `digraph.debug_print()` format (`portage/util/digraph.py:349`),
/// preceded by the `\ndigraph:\n\n` header -- to **stderr**, under
/// `emerge --pretend --debug` (or the legacy `PORTUALE_DEBUG_MERGE_GRAPH`
/// env var). This is how the module was validated and the fastest way to
/// localise a merge-order divergence: dump both, diff the node sets, then
/// the edge sets, then the priorities, then `.order` (per this module's
/// header).
///
/// Real's format, per node in insertion order (`g.order` before
/// `merge_order_bias` re-sorts it -- real `debug_print` iterates
/// `self.nodes`, not `.order`):
///
/// ```text
/// (cat/pkg-ver:slot/sub_slot::repo, <state>) depends on
///   (child..., <state>) (<max-priority>)
/// (leaf..., <state>) (no children)
/// ```
///
/// **Divergences from real** (see `resolver_trace`'s header):
/// plain-text node labels; portuale's post-prune merge closure as the
/// node set, with the top-level atoms appended as pseudo-`DependencyArg`
/// nodes (`<atom> depends on` / `  <resolved-node> (soft)`) so a diff
/// against real still lines up, rather than real's full `@world`/
/// `@system` universe (proven unnecessary for ordering by the
/// `_serialize_tasks` port).
/// Real `depgraph.py:9917-9930`'s `\nruntime cycle digraph (<n> nodes):
/// \n\n` + `debug_print()` of the cycle's induced subgraph + `runtime
/// cycle leaf: <pkg>\n\n` -- to stderr, under `--pretend --debug`, once
/// per relaxed runtime cycle. Same node/edge formatting as
/// `debug_dump_graph`.
fn debug_dump_cycle(g: &Digraph, entries: &[GraphEntry], sub: &HashSet<usize>, root: &Path) {
    let label = |i: usize| crate::resolver_trace::node_label(&entries[i], root, g.installed[i]);
    crate::resolver_trace::err(format_args!(
        "\nruntime cycle digraph ({} nodes):\n\n",
        sub.len()
    ));
    for &i in g.order.iter().filter(|i| sub.contains(i)) {
        let kids: Vec<&(usize, Vec<DepPriority>)> = g.children[i]
            .iter()
            .filter(|(c, _)| sub.contains(c))
            .collect();
        if kids.is_empty() {
            crate::resolver_trace::err(format_args!("{} (no children)\n", label(i)));
        } else {
            crate::resolver_trace::err(format_args!("{} depends on\n", label(i)));
            for (c, prios) in kids {
                let max = crate::resolver_trace::max_priority(prios);
                crate::resolver_trace::err(format_args!("  {} ({})\n", label(*c), max));
            }
        }
    }
    // Real `cycle_digraph.order[-1]` -- the last node in `.order` within
    // the subgraph is the leaf real reports.
    if let Some(&leaf) = g.order.iter().rev().find(|i| sub.contains(i)) {
        crate::resolver_trace::err(format_args!("runtime cycle leaf: {}\n\n", label(leaf)));
    }
}

/// Build the digraph and dump it -- for `topological_merge_order`'s
/// single-package short-circuit, where `serialize_merge_order` (and thus
/// the normal `debug_dump_graph` call) never runs.
pub(crate) fn debug_dump_graph_only(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
) {
    let g = build_digraph(entries, top_level_atoms, root);
    debug_dump_graph(&g, entries, top_level_atoms, root);
}

fn debug_dump_graph(g: &Digraph, entries: &[GraphEntry], top_level_atoms: &[String], root: &Path) {
    let env = std::env::var_os("PORTUALE_DEBUG_MERGE_GRAPH").is_some();
    if !env && !crate::resolver_debug() {
        return;
    }
    // The env var predates the `--debug` wiring and went to stderr
    // directly; keep it working even without `--debug`.
    let emit = |s: String| {
        if crate::resolver_debug() {
            crate::resolver_trace::err(format_args!("{s}"));
        } else {
            eprint!("{s}");
        }
    };
    emit("\ndigraph:\n\n".to_string());

    let label = |i: usize| crate::resolver_trace::node_label(&entries[i], root, g.installed[i]);

    for &i in &g.order {
        if g.children[i].is_empty() {
            emit(format!("{} (no children)\n", label(i)));
        } else {
            emit(format!("{} depends on\n", label(i)));
            for (c, prios) in &g.children[i] {
                let max = crate::resolver_trace::max_priority(prios);
                emit(format!("  {} ({})\n", label(*c), max));
            }
        }
    }

    // Pseudo-`DependencyArg` nodes for the top-level atoms, appended
    // after the real graph (real interleaves them; portuale flattens set
    // expansion before the resolver, so it only has the concrete atoms).
    let mut cp_idx: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (j, e) in entries.iter().enumerate() {
        cp_idx
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(j);
    }
    for atom_str in top_level_atoms {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        match cp_idx.get(&(atom.category.as_str(), atom.package.as_str())) {
            Some(idxs) if !idxs.is_empty() => {
                emit(format!("{atom_str} depends on\n"));
                for &j in idxs {
                    emit(format!("  {} (soft)\n", label(j)));
                }
            }
            _ => emit(format!("{atom_str} (no children)\n")),
        }
    }
}

/// The whole pipeline: build the digraph out of `entries`, prune it the
/// way real does, bias it, schedule it, and weave portuale's own
/// non-merge-bound entries back in.
///
/// `implicit_system_deps` (real `--implicit-system-deps`, default on)
/// gates the bias re-sort only -- the digraph build, prune, schedule,
/// and weave-back are identical either way.
///
/// Returns a permutation of `0..entries.len()` in merge order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn serialize_merge_order(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    config: &portage_profile::Config,
    root: &Path,
    implicit_system_deps: bool,
    repos: &[RepoConfig],
    dynamic_deps: bool,
) -> Vec<usize> {
    let (ext, mut g, real_n, discovery_rank) = schedule_graph(
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
    );
    let entries: &[GraphEntry] = &ext;

    debug_dump_graph(&g, entries, top_level_atoms, root);

    // B3: post-prune, pre-bias insertion order -- the tie-break real's
    // stable `_merge_order_bias` preserves when parent counts are equal.
    if mo_sel_enabled() {
        let nodes: Vec<String> = g
            .order
            .iter()
            .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
            .collect();
        eprintln!("MO_ORDER count={} {}", nodes.len(), nodes.join(" "));
    }

    merge_order_bias(&mut g, entries, config, implicit_system_deps);
    let scheduled: Vec<usize> = select_nodes(&mut g, entries, root, None)
        .into_iter()
        .filter(|&i| i < real_n)
        .collect();

    // Real's own retlist skips every "nomerge" node (`if node.operation
    // == "nomerge": continue`), because real never displays one. Portuale
    // keeps its `AlreadyInstalled`/`NoVisibleCandidate` entries -- its
    // `--tree`/`--emptytree`/"already installed" rendering needs them --
    // so a nomerge node that survived the prune keeps the position the
    // scheduler gave it: it is a genuine graph node there, ordered ahead
    // of everything that depends on it, exactly as real orders it before
    // dropping it from the display.
    //
    // Only the entries the scheduler never saw are woven back in
    // afterwards: the pruned "nomerge" *roots* (real drops those from
    // scheduling outright -- a top-level already-installed target) and
    // any entry with no node at all. Each goes on its own unbiased
    // discovery rank, so such an entry is never bias-promoted past a
    // merge task it was behind in plain discovery order.
    let placed: HashSet<usize> = scheduled.iter().copied().collect();
    let mut leftover: Vec<usize> = (0..real_n).filter(|i| !placed.contains(i)).collect();
    leftover.sort_by_key(|&i| discovery_rank[i]);

    let mut out: Vec<usize> = Vec::with_capacity(real_n);
    let mut ti = 0;
    for &m in &scheduled {
        while ti < leftover.len() && discovery_rank[leftover[ti]] < discovery_rank[m] {
            out.push(leftover[ti]);
            ti += 1;
        }
        out.push(m);
    }
    out.extend(&leftover[ti..]);
    debug_assert_eq!(out.len(), real_n);
    out
}
/// The shared scheduler-graph setup for [`serialize_merge_order`]
/// and the #81 tree-mode simulation: the installed-dependency closure,
/// `build_digraph`, the nomerge-root prune and `_merge_order_bias`.
/// Returns the extended entries, the biased graph, the real-entry count
/// and the unbiased discovery rank. Diagnostics (`debug_dump_graph`,
/// the `MO_ORDER` print) stay with the flat caller, not here.
#[allow(clippy::too_many_arguments)]
fn schedule_graph(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    config: &portage_profile::Config,
    root: &Path,
    implicit_system_deps: bool,
    repos: &[RepoConfig],
    dynamic_deps: bool,
) -> (Vec<GraphEntry>, Digraph, usize, Vec<usize>) {
    let real_n = entries.len();
    // Real `_complete_graph` auto-enables (a merge changes an
    // already-installed package -> real re-walks the whole `@world` /
    // `@system` universe as nomerge nodes carrying their full recorded
    // dependency trees). In that mode leaf selection clears a shallow
    // installed subtree (`xtrans` -> `elt-patches` -> `gentoo-functions`)
    // before a deep one (`xorg-proto` -> `meson` -> the Python stack).
    // Portuale gave every installed node `(no children)`, so both freed
    // together and sorted by bias alone. `add_installed_dependency_
    // closure` supplies those trees -- but only when a real
    // `_complete_graph` would run: a plain all-`[ebuild N]` resolve keeps
    // installed nodes as leaves. The trigger is the same set real's
    // `complete_graph_auto_enable` checks (in-slot version/USE change, or
    // a new-slot install of an already-installed `cp`); computed here
    // straight off `entries` rather than threaded from the CLI, because
    // the CLI's own `want_complete` misses a reason-less `[ebuild R]`
    // whose displayed USE still differs from the vdb. Synthetic nodes get
    // indices `>= real_n` and are filtered out of `scheduled` below.
    let complete = entries.iter().any(|e| {
        matches!(
            e.outcome,
            PretendOutcome::Reinstall { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        ) || (matches!(e.outcome, PretendOutcome::New { .. }) && e.new_slot)
    });
    let mut ext = entries.to_vec();
    add_installed_dependency_closure(
        &mut ext,
        root,
        repos,
        &config.system_packages,
        !complete,
        dynamic_deps,
    );
    let entries: &[GraphEntry] = &ext;
    let n = entries.len();

    let mut g = build_digraph(entries, top_level_atoms, root);
    // Unbiased discovery rank, kept before the bias re-sorts `g.order` --
    // it is what the trivial (non-merge-bound) entries are woven back in
    // on, so that a "package is already installed" notice never gets
    // bias-promoted past a merge task it was behind in plain discovery
    // order.
    let mut discovery_rank = vec![0usize; n];
    for (pos, &i) in g.order.iter().enumerate() {
        discovery_rank[i] = pos;
    }

    debug_dump_graph(&g, entries, top_level_atoms, root);

    // Real `_serialize_tasks`: after stripping its `DependencyArg` nodes
    // (portuale has none), "Prune 'nomerge' root nodes if nothing depends
    // on them, since otherwise they slow down merge order calculation"
    // (`depgraph.py:9505-9518`) -- iterated to a fixpoint. This is what
    // turns the `@system` seed above into real's actual post-prune
    // selection graph: the deep-only members survive (a parent inside the
    // closure), the top-level `@system` leaves (`net-misc/wget` and its
    // ilk, whose only parent was the pruned `SetArg`) do not.
    //
    // Restricted to the synthetic `i >= real_n` nodes. A real
    // `AlreadyInstalled` *entry* -- a top-level `--noreplace`/`--deep`
    // target -- is also a nomerge root real would drop, but portuale
    // *displays* it (real never does) and its `--tree` / "already
    // installed" rendering wants it after the deps it pulls in. Leaving
    // it in the scheduler produces exactly that (it only leafs once its
    // subtree has); pruning it instead drops it into `leftover`, which
    // re-weaves it on its *parent-first* discovery rank -- wrong order.
    loop {
        let mut removed = false;
        for i in real_n..n {
            if !g.alive[i] || !g.installed[i] {
                continue;
            }
            if g.parents[i].iter().any(|&p| g.alive[p]) {
                continue;
            }
            g.alive[i] = false;
            removed = true;
        }
        if !removed {
            break;
        }
    }
    g.order.retain(|&i| g.alive[i]);
    merge_order_bias(&mut g, entries, config, implicit_system_deps);
    (ext, g, real_n, discovery_rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mo_sel_trace_line_pins_the_harness_format() {
        // The shared contract with `TEST/scripts/mo-trace/real-trace.py`
        // and `align-traces.py`: one line per `select_nodes` iteration,
        // fields `iter retlist alive asap prefer_asap drop_satisfied ig
        // pick`. Changing this string breaks the aligner, so it is
        // pinned.
        let line = mo_sel_trace_line(
            7,
            3,
            42,
            &["m:llvm-runtimes/clang-runtime-22".to_string()],
            false,
            true,
            Some(n_ignore_runtime),
            &["m:dev-libs/a-1".to_string(), "n:dev-libs/b-2".to_string()],
        );
        assert_eq!(
            line,
            "MO_SEL iter=7 retlist=3 alive=42 asap=[m:llvm-runtimes/clang-runtime-22] \
             prefer_asap=0 drop_satisfied=1 ig=ignore_runtime pick=m:dev-libs/a-1 n:dev-libs/b-2"
        );
        assert_eq!(
            mo_sel_trace_line(1, 0, 5, &[], true, false, None, &[]),
            "MO_SEL iter=1 retlist=0 alive=5 asap=[] prefer_asap=1 \
             drop_satisfied=0 ig=none pick="
        );
    }

    #[test]
    fn dep_edges_carry_the_evaluated_atom_for_candidates_display() {
        // #138 (Phase 5b S3): real's `--debug` prints `Depstring:` raw
        // and `Candidates:` evaluated -- the *evaluated* form must ride
        // the edge beside the raw token. The parent was built -flip, so
        // `~dev-libs/x-1.0[flip=]` evaluates to `[-flip]` (oracle:
        // `logs/l111-s0-20260921/real-rest-debug.log`, bed
        // `l0-fx-20260922T192350Z` arm A).
        let mut metadata = HashMap::new();
        metadata.insert(
            "RDEPEND".to_string(),
            "~dev-libs/deepusedepchild-1.0[flip=]".to_string(),
        );
        let use_flags: HashSet<String> = HashSet::new();
        let edges = dep_edges_from_metadata(&metadata, &use_flags, &["RDEPEND"], false);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].atom, "~dev-libs/deepusedepchild-1.0[flip=]");
        assert_eq!(edges[0].evaluated, "~dev-libs/deepusedepchild-1.0[-flip]");
    }

    fn prio(word: u64) -> DepPriority {
        DepPriority {
            buildtime: word & 1 != 0,
            runtime: word & 2 != 0,
            runtime_post: word & 4 != 0,
            buildtime_slot_op: word & 8 != 0,
            runtime_slot_op: word & 16 != 0,
            optional: word & 32 != 0,
            satisfied: word & 64 != 0,
        }
    }

    /// A `Digraph` over `0..n` with deduped edges, mirroring
    /// `Digraph::add_edge` semantics.
    fn test_graph(n: usize, edges: &[(usize, usize, DepPriority)]) -> Digraph {
        let mut g = Digraph {
            n,
            children: vec![Vec::new(); n],
            parents: vec![Vec::new(); n],
            order: (0..n).collect(),
            installed: vec![false; n],
            alive: vec![true; n],
        };
        for &(p, c, pr) in edges {
            g.add_edge(p, c, pr);
        }
        g
    }

    /// Every filter the loop can ask about: `None` plus each rung of
    /// both ladders.
    fn all_filters() -> Vec<Option<Ignore>> {
        let mut out = vec![None];
        out.extend(NORMAL.ignore.iter().copied());
        out.extend(SATISFIED.ignore.iter().copied());
        out
    }

    /// Assert frontier/direct agreement on a graph snapshot: every
    /// level's ready set equals the corresponding scan, and `is_leaf`
    /// agrees on every alive node.
    fn assert_equivalent(g: &Digraph, fr: &mut SerializeFrontier) {
        for ig in all_filters() {
            let level = fr.level_of(ig).expect("every loop filter has a level");
            let mut ready = fr.ready_nodes(level, &g.alive);
            ready.sort_unstable();
            let mut direct = g.leaf_nodes(ig);
            direct.sort_unstable();
            assert_eq!(ready, direct, "ready_nodes != leaf_nodes for {ig:?}");
        }
        for node in 0..g.n {
            if !g.alive[node] {
                continue;
            }
            for ig in all_filters() {
                let level = fr.level_of(ig).unwrap();
                assert_eq!(
                    fr.is_leaf(node, level),
                    g.is_leaf(node, ig),
                    "is_leaf disagrees on node {node} for {ig:?}"
                );
            }
        }
    }

    #[test]
    fn frontier_levels_cover_every_ladder_rung() {
        let g = test_graph(2, &[(0, 1, prio(0))]);
        let fr = SerializeFrontier::build(&g);
        // Level 0 is the None filter; every rung of both ladders
        // resolves to some level. The exact count is NOT pinned: the
        // toolchain may fold textually identical filters (the two
        // `p.optional` rungs) to one address, sharing a level -- which
        // is behavior-preserving, since identical bodies filter
        // identically (see build_levels).
        assert!(fr.levels[0].is_none(), "level 0 is the None filter");
        assert_eq!(fr.level_of(None), Some(0));
        for ig in all_filters() {
            assert!(fr.level_of(ig).is_some(), "no level for {ig:?}");
        }
        for (l, f) in fr.levels.iter().enumerate() {
            if l == 0 {
                continue;
            }
            assert!(f.is_some(), "level {l} must hold a filter");
        }
    }

    #[test]
    fn frontier_matches_direct_scans_through_removals() {
        // Diamond with mixed priorities: 0 -> {1, 2} -> 3, plus an
        // optional edge and a multi-priority edge.
        let g = test_graph(
            4,
            &[
                (0, 1, prio(2)),  // runtime
                (0, 2, prio(32)), // optional
                (1, 3, prio(4)),  // runtime_post
                (2, 3, prio(1)),  // buildtime
                (0, 3, prio(2)),  // second priority on a direct edge
            ],
        );
        let mut g = g;
        // Fold a second priority into the (0,3) edge, like add_edge does.
        g.add_edge(0, 3, prio(16));
        let mut fr = SerializeFrontier::build(&g);
        assert_equivalent(&g, &mut fr);
        // Drain in an order that frees parents mid-sequence, mirroring
        // the selection loop (remove + retain each step).
        for node in [3, 1, 2, 0] {
            g.alive[node] = false;
            fr.remove(node, &g.children[node].clone(), &g.parents[node].clone());
            g.order.retain(|&i| g.alive[i]);
            assert_equivalent(&g, &mut fr);
        }
        assert!(g.order.is_empty());
    }

    /// Deterministic xorshift64* -- no rand dependency for a unit test.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    #[test]
    fn frontier_matches_direct_scans_on_random_graphs() {
        // 50 deterministic graphs, 1..12 nodes, random edges (usually
        // acyclic, sometimes not -- cycles only make leaf sets smaller,
        // never less comparable) with random 1-2-priority lists, drained
        // in random order with retain, comparing after every mutation.
        let mut rng = Rng(0x12345678);
        for _ in 0..50 {
            let n = 1 + rng.below(12);
            let mut edges = Vec::new();
            let nedges = rng.below(n * 2 + 1);
            for _ in 0..nedges {
                let (mut p, mut c) = (rng.below(n), rng.below(n));
                if p == c {
                    continue;
                }
                if p > c {
                    std::mem::swap(&mut p, &mut c);
                }
                let mut prios = vec![prio(rng.next() & 127)];
                if rng.below(4) == 0 {
                    prios.push(prio(rng.next() & 127));
                }
                for pr in prios {
                    edges.push((p, c, pr));
                }
            }
            let mut g = test_graph(n, &edges);
            let mut fr = SerializeFrontier::build(&g);
            assert_equivalent(&g, &mut fr);
            let mut perm: Vec<usize> = (0..n).collect();
            for i in (1..n).rev() {
                let j = rng.below(i + 1);
                perm.swap(i, j);
            }
            for node in perm {
                g.alive[node] = false;
                fr.remove(node, &g.children[node].clone(), &g.parents[node].clone());
                g.order.retain(|&i| g.alive[i]);
                assert_equivalent(&g, &mut fr);
            }
        }
    }

    #[test]
    fn disabled_path_matches_direct_scans() {
        // The PORTAGE_SERIALIZE_FRONTIER_DISABLE fallback routes every
        // query to the plain scans: leaves_via/is_leaf_via with None
        // agree with the direct calls on an arbitrary graph.
        let mut rng = Rng(0xabcdef);
        let n = 8;
        let mut edges = Vec::new();
        for _ in 0..14 {
            edges.push((rng.below(n), rng.below(n), prio(rng.next() & 127)));
        }
        let edges: Vec<_> = edges.into_iter().filter(|(p, c, _)| p != c).collect();
        let g = test_graph(n, &edges);
        for ig in all_filters() {
            assert_eq!(leaves_via(None, &g, ig), g.leaf_nodes(ig));
            for node in 0..n {
                assert_eq!(is_leaf_via(None, &g, node, ig), g.is_leaf(node, ig));
            }
        }
    }

    #[test]
    fn frontier_add_edge_only_grows_survival() {
        // Parity with real's add_edge contract: adding a priority only
        // makes an edge survive more levels, never fewer.
        let mut g = test_graph(2, &[(0, 1, prio(32))]);
        let mut fr = SerializeFrontier::build(&g);
        let before: Vec<u32> = fr.surv[0].to_vec();
        // Sync a grown priority list (a second priority on the edge that
        // survives strictly more levels).
        g.add_edge(0, 1, prio(2));
        fr.add_edge(
            1,
            Some(0),
            &g.children[0].iter().find(|(c, _)| *c == 1).unwrap().1,
        );
        for (l, (&b, &a)) in before.iter().zip(fr.surv[0].iter()).enumerate() {
            assert!(a >= b, "counts must not shrink at level {l}");
        }
        assert_equivalent(&g, &mut fr);
        // Re-adding the identical mask is a no-op.
        let snapshot: Vec<u32> = fr.surv[0].to_vec();
        fr.add_edge(
            1,
            Some(0),
            &g.children[0].iter().find(|(c, _)| *c == 1).unwrap().1,
        );
        assert_eq!(fr.surv[0], snapshot);
        // A brand-new node gets an appended index and starts isolated.
        fr.add_edge(2, None, &[]);
        assert!(fr.surv.len() > 2);
    }

    /// Cycle node sets, order-insensitive (rotations of one ring are
    /// distinct records, like real).
    fn cycle_sets(cycles: &[Vec<usize>]) -> Vec<Vec<usize>> {
        let mut out: Vec<Vec<usize>> = cycles
            .iter()
            .map(|c| {
                let mut s = c.clone();
                s.sort_unstable();
                s
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn elementary_cycles_counts_rotations_like_real() {
        // Square 0->1->2->3->0: one ring per node (rotations), four
        // records -- the shape that trips real's `> 3` large-cycle
        // advisory.
        let square = test_graph(
            4,
            &[
                (0, 1, prio(1)),
                (1, 2, prio(1)),
                (2, 3, prio(1)),
                (3, 0, prio(1)),
            ],
        );
        let cycles = elementary_cycles(&square, None);
        assert_eq!(cycles.len(), 4);
        for c in &cycles {
            assert_eq!(c.len(), 4);
        }
        // Triangle: three rotations.
        let tri = test_graph(3, &[(0, 1, prio(1)), (1, 2, prio(1)), (2, 0, prio(1))]);
        assert_eq!(elementary_cycles(&tri, None).len(), 3);
        // Acyclic diamond: none.
        let dag = test_graph(
            4,
            &[
                (0, 1, prio(1)),
                (0, 2, prio(1)),
                (1, 3, prio(1)),
                (2, 3, prio(1)),
            ],
        );
        assert!(elementary_cycles(&dag, None).is_empty());
        // Self-loop: the singleton record, like real's
        // shortest_path(x, x) == [x].
        let slf = test_graph(1, &[(0, 0, prio(1))]);
        assert_eq!(elementary_cycles(&slf, None), vec![vec![0]]);
    }

    #[test]
    fn elementary_cycles_honors_the_survival_filter() {
        // Same square, but the 1->2 edge is optional-only: under the
        // satisfied medium_soft rung it drops out, breaking the ring.
        let g = test_graph(
            4,
            &[
                (0, 1, prio(1)),
                (1, 2, prio(32)),
                (2, 3, prio(1)),
                (3, 0, prio(1)),
            ],
        );
        assert_eq!(elementary_cycles(&g, None).len(), 4);
        assert!(
            elementary_cycles(&g, satisfied_medium_soft_rung()).is_empty(),
            "a filter-dropped edge breaks every ring"
        );
    }

    #[test]
    fn elementary_cycles_keeps_every_tied_shortest_path() {
        // 0->1, 0->2, 1->3, 2->3, 3->0: from 0 both children yield
        // length-3 paths ([1,3,0] and [2,3,0]) -- both recorded. The
        // other nodes contribute one each ([3,0,1], [3,0,2], and [0,1,3]
        // from 3, whose BFS reaches 3 via 1 first in sorted order).
        let g = test_graph(
            4,
            &[
                (0, 1, prio(1)),
                (0, 2, prio(1)),
                (1, 3, prio(1)),
                (2, 3, prio(1)),
                (3, 0, prio(1)),
            ],
        );
        let cycles = elementary_cycles(&g, None);
        let from_zero: Vec<_> = cycles.iter().filter(|c| c.last() == Some(&0)).collect();
        assert_eq!(from_zero.len(), 2, "both tied paths recorded: {cycles:?}");
        assert_eq!(cycles.len(), 5);
        assert_eq!(cycle_sets(&cycles).len(), 2);
    }

    #[test]
    fn reduced_merge_order_drains_a_set_in_leaf_order() {
        // Square with a downstream outsider (4, depending on 0), all in
        // the drain set like real's stuck remainder: leaf-drain order,
        // outsider with the rest.
        let g = test_graph(
            5,
            &[
                (0, 1, prio(1)),
                (1, 2, prio(1)),
                (2, 3, prio(1)),
                (3, 0, prio(1)),
                (4, 0, prio(1)),
            ],
        );
        let drain: HashSet<usize> = [0, 1, 2, 3, 4].into_iter().collect();
        let order = reduced_merge_order(&g, &drain);
        assert_eq!(order.len(), 5);
        assert_eq!(
            order.iter().copied().collect::<HashSet<_>>(),
            drain,
            "the whole drain set, outsider included"
        );
        // Members-only drain: exactly the members.
        let members: HashSet<usize> = [0, 1, 2, 3].into_iter().collect();
        let order = reduced_merge_order(&g, &members);
        assert_eq!(order.len(), 4);
        assert_eq!(order.iter().copied().collect::<HashSet<_>>(), members);
        // Chain: tail-first.
        let chain = test_graph(4, &[(0, 1, prio(1)), (1, 2, prio(1)), (0, 3, prio(1))]);
        let members: HashSet<usize> = [0, 1, 2, 3].into_iter().collect();
        assert_eq!(reduced_merge_order(&chain, &members), vec![2, 3, 1, 0]);
    }

    /// A minimal `New` `GraphEntry` for `build_digraph` tests --
    /// mirrors `synthetic_installed_entry`'s full field list.
    fn new_entry(category: &str, package: &str, version: &str, deps: Vec<DepEdge>) -> GraphEntry {
        GraphEntry {
            category: category.to_string(),
            package: package.to_string(),
            outcome: PretendOutcome::New {
                version: version.to_string(),
            },
            blockers: Vec::new(),
            slot: Some(version.to_string()),
            sub_slot: Some(version.to_string()),
            repo_name: None,
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: Vec::new(),
            source: CandidateSource::Ebuild,
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps,
        }
    }

    #[test]
    fn self_referential_disjunctive_bdepend_skips_the_self_branch() {
        // #53: `dev-lang/mogo`'s own BDEPEND is
        // `|| ( >=mogo-1.24 >=mogoboot-1.24 )`. The resolver picks the
        // non-circular `mogoboot` branch (L0 finding D, `b5b256f`) --
        // `alt_suppressed` must agree, not let the self branch win via
        // its trivial "matches the owner's own not-yet-merged graph
        // node" match, which used to add a permanent `mogo -> mogo`
        // buildtime self-edge and demote the real edge to `mogoboot`
        // down to the weaker `required_by`-fallback `runtime` priority.
        let mogo_deps = vec![
            DepEdge {
                atom: ">=dev-lang/mogo-1.24".to_string(),
                evaluated: ">=dev-lang/mogo-1.24".to_string().clone(),
                category: "dev-lang".to_string(),
                package: "mogo".to_string(),
                priority: DepPriority {
                    buildtime: true,
                    ..DepPriority::default()
                },
                disjunctive: true,
                alt: Some((0, 0)),
                key: 4,
            },
            DepEdge {
                atom: ">=dev-lang/mogoboot-1.24".to_string(),
                evaluated: ">=dev-lang/mogoboot-1.24".to_string(),
                category: "dev-lang".to_string(),
                package: "mogoboot".to_string(),
                priority: DepPriority {
                    buildtime: true,
                    ..DepPriority::default()
                },
                disjunctive: true,
                alt: Some((0, 1)),
                key: 4,
            },
        ];
        let entries = vec![
            new_entry("dev-lang", "mogo", "1.26", mogo_deps),
            new_entry("dev-lang", "mogoboot", "1.24", Vec::new()),
        ];
        let g = build_digraph(
            &entries,
            &["dev-lang/mogo".to_string()],
            Path::new("/nonexistent-root-for-unit-test"),
        );
        assert!(
            g.children[0].iter().all(|&(c, _)| c != 0),
            "mogo must not depend on itself: {:?}",
            g.children[0]
        );
        let to_boot = g.children[0].iter().find(|&&(c, _)| c == 1);
        assert!(
            to_boot.is_some(),
            "mogo must depend on mogoboot: {:?}",
            g.children[0]
        );
        let (_, prios) = to_boot.unwrap();
        assert!(
            prios.iter().any(|p| p.buildtime),
            "the mogo -> mogoboot edge must be the real buildtime one, \
             not the weaker required_by-fallback edge: {prios:?}"
        );
    }

    #[test]
    fn resolved_dep_targets_ranks_by_vercmp_not_by_string_order() {
        // #82: `print_tree` carried a private copy of this narrowing
        // whose tie-break was `jv.cmp(bv)`, a **string** compare, under
        // which `1.9` outranks `1.10`. Real resolves a slot-unqualified
        // atom through `_select_pkg_highest_available`, i.e. the
        // vercmp-highest of the matching graph nodes -- the container
        // oracle for the shape is `TEST/findings/l0.md` "#82"
        // (`dev-libs/treeslotuser`).
        let plain = |cat: &str, pkg: &str| DepEdge {
            atom: format!("{cat}/{pkg}"),
            evaluated: format!("{cat}/{pkg}").clone(),
            category: cat.to_string(),
            package: pkg.to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: false,
            alt: None,
            key: 3,
        };
        let mut low = new_entry("dev-libs", "slotted", "1.9", Vec::new());
        low.slot = Some("0".into());
        low.sub_slot = Some("0".into());
        let mut high = new_entry("dev-libs", "slotted", "1.10", Vec::new());
        high.slot = Some("1".into());
        high.sub_slot = Some("1".into());
        let user = new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain("dev-libs", "slotted")],
        );
        // `low` first in the array, so "first on ties" would pick it if
        // the ranking were not comparing versions at all.
        let entries = vec![low, high, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let targets = resolved_dep_targets(&entries, root);
        assert_eq!(
            targets[2][0],
            Some(1),
            "the unqualified atom resolves to 1.10, not to the \
             string-highest 1.9"
        );
        // `build_digraph` shares the ranking, so its edge agrees.
        let g = build_digraph(&entries, &["dev-libs/user".to_string()], root);
        let children: Vec<usize> = g.children[2].iter().map(|&(c, _)| c).collect();
        assert_eq!(
            children,
            vec![1],
            "the scheduling graph picks the same instance: {children:?}"
        );
    }

    #[test]
    fn tree_simulation_solves_a_stalled_replacement_row() {
        // Backlog #81 S1: the C0 p2b shape in miniature. Owner O merges
        // (1.0 -> 1.1) carrying a satisfied Replacement row for the
        // installed 1.0 that R=blocked-2.0 replaces in-slot. Both nodes
        // are roots, so tree mode's one-at-a-time selection stalls on
        // round one with the blocker pending, schedules the uninstall,
        // and solves the row when the uninstall is selected -- while the
        // flat greedy pop would drain both without stalling. (Real's
        // verdict for the full shape is in `TEST/findings/l0.md` "#75
        // C0"; the flat-hidden half is `replacement_wait_index`'s call.)
        let plain = |atom: &str, cat: &str, pkg: &str| DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string().clone(),
            category: cat.to_string(),
            package: pkg.to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: false,
            alt: None,
            key: 3,
        };
        let mut owner = new_entry("dev-libs", "bparent", "1.1", Vec::new());
        owner.slot = Some("0".into());
        owner.sub_slot = Some("0".into());
        owner.blockers = vec![crate::BlockerConflict {
            atom_str: "!<dev-libs/blocked-2.0".to_string(),
            strong: false,
            matched_category: "dev-libs".to_string(),
            matched_package: "blocked".to_string(),
            matched_version: "1.0".to_string(),
            unsolvable: false,
            satisfied_by: Some(crate::BlockerSatisfiedBy::Replacement {
                cp: ("dev-libs".to_string(), "blocked".to_string()),
                slot: "0".to_string(),
            }),
            tree_scheduled_uninstall: false,
        }];
        let mut replacement = new_entry(
            "dev-libs",
            "blocked",
            "2.0",
            vec![plain("dev-libs/bparent", "dev-libs", "bparent")],
        );
        replacement.slot = Some("0".into());
        replacement.sub_slot = Some("0".into());
        // A third, unrelated root merge: without it the replacement
        // would be the lone leaf of round two and real's len==1 shortcut
        // would take it immediately (no stall, no row -- same as real).
        // With two roots left, tree mode's one-at-a-time scan skips both
        // and the stuck branch fires while the blocker is pending.
        let other = new_entry("dev-libs", "other", "1.0", Vec::new());
        let entries = vec![owner, replacement, other];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let config = portage_profile::Config::default();
        let solved = tree_solved_replacements(
            &entries,
            &["dev-libs/blocked".to_string()],
            &config,
            root,
            true,
            &[],
            true,
        );
        assert_eq!(
            solved,
            vec![(0, 0)],
            "the stalled Replacement row is solved in tree mode: {solved:?}"
        );
    }

    #[test]
    fn unqualified_atom_collapses_onto_the_slot_qualified_sibling_target() {
        // #85: real's `_minimize_children` (`depgraph.py:4751-4856`).
        // `user` pulls slot `:0` (selects 1.9) and the bare cp (selects
        // 1.10, the vercmp-highest). Real eliminates the redundant 1.10
        // selection -- the `:0` atom matches only 1.9 while the bare atom
        // matches both -- so the bare atom resolves to 1.9 and the
        // scheduler graph holds no `user → 1.10` edge (real's own
        // `--debug` digraph dump for `dev-libs/treeslotuser` shows only
        // the 1.9 and parent edges). Without the elimination the extra
        // edge doubles 1.10's parent count and `_merge_order_bias` flips
        // the pair to `1.10, 1.9`.
        let plain = |atom: &str| DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string().clone(),
            category: "dev-libs".to_string(),
            package: "slotted".to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: false,
            alt: None,
            key: 3,
        };
        let mut low = new_entry("dev-libs", "slotted", "1.9", Vec::new());
        low.slot = Some("0".into());
        low.sub_slot = Some("0".into());
        let mut high = new_entry("dev-libs", "slotted", "1.10", Vec::new());
        high.slot = Some("1".into());
        high.sub_slot = Some("1".into());
        let user = new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain("dev-libs/slotted:0"), plain("dev-libs/slotted")],
        );
        let entries = vec![low, high, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let targets = resolved_dep_targets(&entries, root);
        assert_eq!(
            targets[2],
            vec![Some(0), Some(0)],
            "both atoms resolve to 1.9: the bare atom's 1.10 selection is redundant"
        );
        let g = build_digraph(&entries, &["dev-libs/user".to_string()], root);
        let children: Vec<usize> = g.children[2].iter().map(|&(c, _)| c).collect();
        assert_eq!(
            children,
            vec![0],
            "one edge, to 1.9 -- no extra parent for 1.10: {children:?}"
        );
    }

    #[test]
    fn nvc_entries_do_not_participate_in_minimize_elimination() {
        // #85 follow-up (`opartlya` disclosure order): a
        // `NoVisibleCandidate` entry is never a *selected* package --
        // real's `_select_package` returns None for its atom, so it
        // never enters `_minimize_children`'s elimination. Here the
        // bare atom matches both the NVC entry and the installed 1.0;
        // without the NVC exclusion the ascending-version elimination
        // drops the NVC entry (its empty version sorts first) and the
        // edge moves to the installed entry, flipping the merge order
        // of the disclosure rows. With it, the pre-#85 ranking stands
        // and the NVC entry keeps the edge.
        let plain = |atom: &str| DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string().clone(),
            category: "dev-libs".to_string(),
            package: "partly".to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: false,
            alt: None,
            key: 3,
        };
        let mut nvc = new_entry("dev-libs", "partly", "1.0", Vec::new());
        nvc.outcome = PretendOutcome::NoVisibleCandidate;
        nvc.slot = Some("0".into());
        nvc.sub_slot = Some("0".into());
        let mut installed = new_entry("dev-libs", "partly", "1.0", Vec::new());
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        installed.slot = Some("0".into());
        installed.sub_slot = Some("0".into());
        let user = new_entry("dev-libs", "user", "1.0", vec![plain("dev-libs/partly")]);
        // NVC first, like the resolver records it. `resolved_dep_targets`
        // is the tree view (`merge_bound_only`), which never edges to an
        // installed node -- assert on `build_digraph`, the scheduler
        // view, where the edge lives.
        let entries = vec![nvc, installed, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let g = build_digraph(&entries, &["dev-libs/user".to_string()], root);
        let children: Vec<usize> = g.children[2].iter().map(|&(c, _)| c).collect();
        assert_eq!(
            children,
            vec![0],
            "the NVC entry keeps the edge: {children:?}"
        );
    }

    #[test]
    fn resolved_dep_targets_reports_an_installed_target_as_no_tree_edge() {
        // #82: the single `merge_bound_only` difference between the two
        // callers. `build_digraph` edges to an installed (nomerge) node
        // the way real's digraph does; `print_tree` must not, because
        // portuale renders no line for one and the edge would open a
        // hole in the tree.
        let dep = DepEdge {
            atom: "dev-libs/onlyinstalled".to_string(),
            evaluated: "dev-libs/onlyinstalled".to_string().clone(),
            category: "dev-libs".to_string(),
            package: "onlyinstalled".to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: false,
            alt: None,
            key: 3,
        };
        let mut installed = new_entry("dev-libs", "onlyinstalled", "1.0", Vec::new());
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let entries = vec![new_entry("dev-libs", "user", "1.0", vec![dep]), installed];
        let root = Path::new("/nonexistent-root-for-unit-test");
        assert_eq!(resolved_dep_targets(&entries, root)[0][0], None);
        let g = build_digraph(&entries, &["dev-libs/user".to_string()], root);
        let children: Vec<usize> = g.children[0].iter().map(|&(c, _)| c).collect();
        assert_eq!(children, vec![1], "the scheduling graph keeps the edge");
    }

    #[test]
    fn kept_alt_branches_agrees_with_build_digraph_on_a_multi_branch_group() {
        // #76 B1: real `dep_zapdeps`' all-in-graph bin outranks
        // all-installed, so the kept branch is the merge-bound one; the
        // public helper must report exactly that (B0 q2), and
        // `build_digraph` -- which shares `suppressed_alt_edges` -- must
        // record an edge only to it.
        let owner_deps = vec![
            DepEdge {
                atom: "~dev-libs/newdep-2".to_string(),
                evaluated: "~dev-libs/newdep-2".to_string().clone(),
                category: "dev-libs".to_string(),
                package: "newdep".to_string(),
                priority: DepPriority {
                    runtime: true,
                    ..DepPriority::default()
                },
                disjunctive: true,
                alt: Some((0, 0)),
                key: 3,
            },
            DepEdge {
                atom: "dev-libs/olddep".to_string(),
                evaluated: "dev-libs/olddep".to_string().clone(),
                category: "dev-libs".to_string(),
                package: "olddep".to_string(),
                priority: DepPriority {
                    runtime: true,
                    ..DepPriority::default()
                },
                disjunctive: true,
                alt: Some((0, 1)),
                key: 3,
            },
        ];
        let mut olddep = new_entry("dev-libs", "olddep", "1.0", Vec::new());
        olddep.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let entries = vec![
            new_entry("app-misc", "owner", "1.0", owner_deps),
            new_entry("dev-libs", "newdep", "2", Vec::new()),
            olddep,
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let kept = kept_alt_branches(&entries, root);
        assert_eq!(
            kept[0],
            HashSet::from([0usize]),
            "the in-graph branch is kept over the installed one"
        );
        let g = build_digraph(&entries, &["app-misc/owner".to_string()], root);
        let children: Vec<usize> = g.children[0].iter().map(|&(c, _)| c).collect();
        assert!(
            children.contains(&1),
            "an edge to the kept branch must exist: {children:?}"
        );
        assert!(
            !children.contains(&2),
            "no edge to the suppressed branch: {children:?}"
        );
        // The #53 self-branch exclusion the helper shares.
        let mogo_deps = vec![
            DepEdge {
                atom: ">=dev-lang/mogo-1.24".to_string(),
                evaluated: ">=dev-lang/mogo-1.24".to_string().clone(),
                category: "dev-lang".to_string(),
                package: "mogo".to_string(),
                priority: DepPriority {
                    buildtime: true,
                    ..DepPriority::default()
                },
                disjunctive: true,
                alt: Some((0, 0)),
                key: 4,
            },
            DepEdge {
                atom: ">=dev-lang/mogoboot-1.24".to_string(),
                evaluated: ">=dev-lang/mogoboot-1.24".to_string().clone(),
                category: "dev-lang".to_string(),
                package: "mogoboot".to_string(),
                priority: DepPriority {
                    buildtime: true,
                    ..DepPriority::default()
                },
                disjunctive: true,
                alt: Some((0, 1)),
                key: 4,
            },
        ];
        let entries = vec![
            new_entry("dev-lang", "mogo", "1.26", mogo_deps),
            new_entry("dev-lang", "mogoboot", "1.24", Vec::new()),
        ];
        let kept = kept_alt_branches(&entries, root);
        assert_eq!(
            kept[0],
            HashSet::from([1usize]),
            "the circular self branch is not kept (#53)"
        );
    }
    // -----------------------------------------------------------------
    // #146: dep-key / priority mapping
    // -----------------------------------------------------------------

    fn dep_metadata(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn tokens(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn plain_edge(atom: &str, category: &str, package: &str, priority: DepPriority) -> DepEdge {
        DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string().clone(),
            category: category.to_string(),
            package: package.to_string(),
            priority,
            disjunctive: false,
            alt: None,
            key: 3,
        }
    }

    #[test]
    fn key_priority_pins_each_dep_key_ladder() {
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        assert_eq!(key_priority("RDEPEND", false), runtime);
        assert_eq!(key_priority("RDEPEND", true), runtime);
        assert_eq!(key_priority("IDEPEND", false), runtime);
        assert_eq!(key_priority("IDEPEND", true), runtime);
        assert_eq!(
            key_priority("PDEPEND", false),
            DepPriority {
                runtime_post: true,
                ..DepPriority::default()
            }
        );
        let buildtime = DepPriority {
            buildtime: true,
            ..DepPriority::default()
        };
        assert_eq!(key_priority("DEPEND", false), buildtime);
        assert_eq!(key_priority("BDEPEND", false), buildtime);
        // `built` only flips the build-time keys' `optional`.
        assert_eq!(
            key_priority("DEPEND", true),
            DepPriority {
                buildtime: true,
                optional: true,
                ..DepPriority::default()
            }
        );
        assert_eq!(
            key_priority("BDEPEND", true),
            DepPriority {
                buildtime: true,
                optional: true,
                ..DepPriority::default()
            }
        );
    }

    #[test]
    fn dep_edges_from_metadata_pins_every_key_and_priority() {
        let metadata = dep_metadata(&[
            ("RDEPEND", "dev-libs/rdep"),
            ("IDEPEND", "dev-libs/idep"),
            ("PDEPEND", "dev-libs/pdep"),
            ("DEPEND", "dev-libs/dep"),
            ("BDEPEND", "dev-libs/bdep"),
        ]);
        let keys = ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"];
        let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &keys, false);
        assert_eq!(edges.len(), 5);
        // Real's own deps-tuple key order.
        let by_key: HashMap<u8, &DepEdge> = edges.iter().map(|e| (e.key, e)).collect();
        assert_eq!(by_key.len(), 5, "one edge per key: {edges:#?}");
        assert_eq!(by_key[&0].package, "rdep");
        assert_eq!(by_key[&1].package, "idep");
        assert_eq!(by_key[&2].package, "pdep");
        assert_eq!(by_key[&3].package, "dep");
        assert_eq!(by_key[&4].package, "bdep");
        assert_eq!(
            by_key[&0].priority,
            DepPriority {
                runtime: true,
                ..DepPriority::default()
            }
        );
        assert_eq!(
            by_key[&1].priority,
            DepPriority {
                runtime: true,
                ..DepPriority::default()
            }
        );
        assert_eq!(
            by_key[&2].priority,
            DepPriority {
                runtime_post: true,
                ..DepPriority::default()
            }
        );
        assert_eq!(
            by_key[&3].priority,
            DepPriority {
                buildtime: true,
                ..DepPriority::default()
            }
        );
        assert_eq!(
            by_key[&4].priority,
            DepPriority {
                buildtime: true,
                ..DepPriority::default()
            }
        );
        // `built` marks only the build-time keys optional.
        let built = dep_edges_from_metadata(&metadata, &HashSet::new(), &keys, true);
        assert!(
            built
                .iter()
                .filter(|e| e.key >= 3)
                .all(|e| e.priority.optional)
        );
        assert!(
            built
                .iter()
                .filter(|e| e.key < 3)
                .all(|e| !e.priority.optional)
        );
        // The same atom named by two keys keeps both priorities, and a
        // duplicate within one key is deduped.
        let metadata = dep_metadata(&[
            ("RDEPEND", "dev-libs/a dev-libs/a"),
            ("DEPEND", "dev-libs/a"),
        ]);
        let edges =
            dep_edges_from_metadata(&metadata, &HashSet::new(), &["RDEPEND", "DEPEND"], false);
        assert_eq!(edges.len(), 2, "two keys keep both priorities: {edges:#?}");
        assert!(edges.iter().any(|e| e.priority.runtime));
        assert!(edges.iter().any(|e| e.priority.buildtime));
    }

    #[test]
    fn dep_edges_from_metadata_pins_blockers_slot_ops_and_disjunctions() {
        let metadata = dep_metadata(&[(
            "RDEPEND",
            "!dev-libs/blocked dev-libs/plain dev-libs/slotop:= || ( dev-libs/alt-a dev-libs/alt-b )",
        )]);
        let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &["RDEPEND"], false);
        let atoms: HashSet<&str> = edges.iter().map(|e| e.atom.as_str()).collect();
        assert!(
            !atoms.contains("!dev-libs/blocked"),
            "a blocker is never a merge-order edge: {atoms:?}"
        );
        assert_eq!(
            edges.len(),
            4,
            "plain + slot-op + two alternatives: {edges:#?}"
        );
        let plain = edges.iter().find(|e| e.atom == "dev-libs/plain").unwrap();
        assert!(
            plain.priority.runtime && !plain.priority.runtime_slot_op,
            "a plain atom is not slot-operator: {:?}",
            plain.priority
        );
        let slotop = edges
            .iter()
            .find(|e| e.atom == "dev-libs/slotop:=")
            .unwrap();
        assert!(
            slotop.priority.runtime && slotop.priority.runtime_slot_op,
            "`:=` promotes the runtime key: {:?}",
            slotop.priority
        );
        let alt_a = edges.iter().find(|e| e.atom == "dev-libs/alt-a").unwrap();
        let alt_b = edges.iter().find(|e| e.atom == "dev-libs/alt-b").unwrap();
        assert_eq!(alt_a.alt, Some((0, 0)));
        assert_eq!(alt_b.alt, Some((0, 1)));
        assert!(alt_a.disjunctive && alt_b.disjunctive);
        // A build-time `:=` promotes `buildtime_slot_op`, not runtime.
        let metadata = dep_metadata(&[("DEPEND", "dev-libs/dep:=")]);
        let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &["DEPEND"], false);
        assert_eq!(edges.len(), 1);
        assert!(
            edges[0].priority.buildtime && edges[0].priority.buildtime_slot_op,
            "{:?}",
            edges[0].priority
        );
        assert!(!edges[0].priority.runtime && !edges[0].priority.runtime_slot_op);
    }

    #[test]
    fn split_disjunctive_pins_group_and_branch_bookkeeping() {
        // Inline atoms only.
        assert_eq!(
            split_disjunctive(&tokens(&["dev-libs/a", "dev-libs/b"])),
            (tokens(&["dev-libs/a", "dev-libs/b"]), vec![])
        );
        // One `||` group, bare alternatives: each atom is its own branch.
        assert_eq!(
            split_disjunctive(&tokens(&["||", "(", "dev-libs/a", "dev-libs/b", ")"])),
            (
                vec![],
                vec![
                    ("dev-libs/a".to_string(), 0, 0),
                    ("dev-libs/b".to_string(), 0, 1),
                ]
            )
        );
        // A nested multi-atom branch keeps one branch number for every
        // atom inside it.
        assert_eq!(
            split_disjunctive(&tokens(&[
                "||",
                "(",
                "(",
                "dev-libs/a",
                "dev-libs/b",
                ")",
                "dev-libs/c",
                ")"
            ])),
            (
                vec![],
                vec![
                    ("dev-libs/a".to_string(), 0, 0),
                    ("dev-libs/b".to_string(), 0, 0),
                    ("dev-libs/c".to_string(), 0, 1),
                ]
            )
        );
        // A second `||` group counts separately.
        assert_eq!(
            split_disjunctive(&tokens(&[
                "||",
                "(",
                "dev-libs/a",
                ")",
                "||",
                "(",
                "dev-libs/b",
                ")"
            ])),
            (
                vec![],
                vec![
                    ("dev-libs/a".to_string(), 0, 0),
                    ("dev-libs/b".to_string(), 1, 0),
                ]
            )
        );
        // A `virtual/*` atom outside a group is deferred with the
        // sentinel group/branch; anything else stays inline.
        assert_eq!(
            split_disjunctive(&tokens(&["virtual/foo", "dev-libs/a"])),
            (
                tokens(&["dev-libs/a"]),
                vec![("virtual/foo".to_string(), u32::MAX, u32::MAX)]
            )
        );
        // After the group closes, atoms are inline again; a `||` with no
        // following group is not a deferral.
        assert_eq!(
            split_disjunctive(&tokens(&["||", "(", "dev-libs/a", ")", "dev-libs/b"])),
            (
                tokens(&["dev-libs/b"]),
                vec![("dev-libs/a".to_string(), 0, 0)]
            )
        );
        assert_eq!(
            split_disjunctive(&tokens(&["||", "dev-libs/a"])),
            (tokens(&["dev-libs/a"]), vec![])
        );
    }

    #[test]
    fn ignore_predicates_pin_every_ladder_rung() {
        // The expected value of every `ignore_priority` predicate for
        // every combination of `DepPriority` flags. Mirrors the ladder's
        // own documented rule; any deleted `!`, flipped connective, or
        // constant-return mutant disagrees on some word.
        fn expect_n_optional(p: &DepPriority) -> bool {
            p.optional
        }
        fn expect_n_runtime_post(p: &DepPriority) -> bool {
            p.optional || p.runtime_post
        }
        fn expect_n_runtime(p: &DepPriority) -> bool {
            !p.runtime_slot_op && (p.optional || !p.buildtime)
        }
        fn expect_s_satisfied_runtime_post(p: &DepPriority) -> bool {
            if p.optional {
                return true;
            }
            if !p.satisfied {
                return false;
            }
            if p.buildtime || p.runtime {
                return false;
            }
            p.runtime_post
        }
        fn expect_s_runtime_post(p: &DepPriority) -> bool {
            if p.optional {
                return true;
            }
            if p.buildtime || p.runtime {
                return false;
            }
            p.runtime_post
        }
        fn expect_s_satisfied_runtime(p: &DepPriority) -> bool {
            if p.optional {
                return true;
            }
            if p.buildtime {
                return false;
            }
            if !p.runtime {
                return true;
            }
            p.satisfied
        }
        fn expect_s_satisfied_buildtime(p: &DepPriority) -> bool {
            if p.optional {
                return true;
            }
            if p.buildtime_slot_op {
                return false;
            }
            p.satisfied
        }
        fn expect_s_satisfied_buildtime_slot_op(p: &DepPriority) -> bool {
            if p.optional {
                return true;
            }
            if p.satisfied {
                return true;
            }
            !p.buildtime && !p.runtime
        }
        fn expect_s_runtime(p: &DepPriority) -> bool {
            (!p.runtime_slot_op || p.satisfied) && (p.satisfied || p.optional || !p.buildtime)
        }
        for word in 0..128u64 {
            let p = prio(word);
            assert_eq!(
                n_ignore_optional(&p),
                expect_n_optional(&p),
                "n_ignore_optional {word}"
            );
            assert_eq!(
                n_ignore_runtime_post(&p),
                expect_n_runtime_post(&p),
                "n_ignore_runtime_post {word}"
            );
            assert_eq!(
                n_ignore_runtime(&p),
                expect_n_runtime(&p),
                "n_ignore_runtime {word}"
            );
            assert_eq!(
                s_ignore_optional(&p),
                expect_n_optional(&p),
                "s_ignore_optional {word}"
            );
            assert_eq!(
                s_ignore_satisfied_runtime_post(&p),
                expect_s_satisfied_runtime_post(&p),
                "s_ignore_satisfied_runtime_post {word}"
            );
            assert_eq!(
                s_ignore_runtime_post(&p),
                expect_s_runtime_post(&p),
                "s_ignore_runtime_post {word}"
            );
            assert_eq!(
                s_ignore_satisfied_runtime(&p),
                expect_s_satisfied_runtime(&p),
                "s_ignore_satisfied_runtime {word}"
            );
            assert_eq!(
                s_ignore_satisfied_buildtime(&p),
                expect_s_satisfied_buildtime(&p),
                "s_ignore_satisfied_buildtime {word}"
            );
            assert_eq!(
                s_ignore_satisfied_buildtime_slot_op(&p),
                expect_s_satisfied_buildtime_slot_op(&p),
                "s_ignore_satisfied_buildtime_slot_op {word}"
            );
            assert_eq!(
                s_ignore_runtime(&p),
                expect_s_runtime(&p),
                "s_ignore_runtime {word}"
            );
        }
    }

    #[test]
    fn ignore_name_pins_every_traced_filter() {
        assert_eq!(ignore_name(None), "none");
        assert_eq!(ignore_name(Some(n_ignore_optional)), "ignore_optional");
        assert_eq!(
            ignore_name(Some(n_ignore_runtime_post)),
            "ignore_runtime_post"
        );
        assert_eq!(ignore_name(Some(n_ignore_runtime)), "ignore_runtime");
        assert_eq!(ignore_name(Some(s_ignore_optional)), "ignore_optional");
        assert_eq!(
            ignore_name(Some(s_ignore_satisfied_runtime_post)),
            "ignore_satisfied_runtime_post"
        );
        assert_eq!(
            ignore_name(Some(s_ignore_runtime_post)),
            "ignore_runtime_post"
        );
        assert_eq!(
            ignore_name(Some(s_ignore_satisfied_runtime)),
            "ignore_satisfied_runtime"
        );
        assert_eq!(
            ignore_name(Some(s_ignore_satisfied_buildtime)),
            "ignore_satisfied_buildtime"
        );
        assert_eq!(
            ignore_name(Some(s_ignore_satisfied_buildtime_slot_op)),
            "ignore_satisfied_buildtime_slot_op"
        );
        assert_eq!(ignore_name(Some(s_ignore_runtime)), "ignore_runtime");
    }

    #[test]
    fn priority_ranges_pin_their_named_rungs() {
        // Every ladder index resolves to the predicate of its documented
        // name (behavioral comparison, so a textually-identical pair
        // folding to one address still agrees).
        for word in 0..128u64 {
            let p = prio(word);
            assert_eq!(
                NORMAL.ig(1).unwrap()(&p),
                n_ignore_optional(&p),
                "NORMAL.ig(1) {word}"
            );
            assert_eq!(
                NORMAL.ig(2).unwrap()(&p),
                n_ignore_runtime_post(&p),
                "NORMAL.ig(2) {word}"
            );
            assert_eq!(
                NORMAL.ig(3).unwrap()(&p),
                n_ignore_runtime(&p),
                "NORMAL.ig(3) {word}"
            );
            assert_eq!(
                SATISFIED.ig(1).unwrap()(&p),
                s_ignore_optional(&p),
                "SATISFIED.ig(1)"
            );
            assert_eq!(
                SATISFIED.ig(2).unwrap()(&p),
                s_ignore_satisfied_runtime_post(&p),
                "SATISFIED.ig(2) {word}"
            );
            assert_eq!(
                SATISFIED.ig(3).unwrap()(&p),
                s_ignore_runtime_post(&p),
                "SATISFIED.ig(3) {word}"
            );
            assert_eq!(
                SATISFIED.ig(4).unwrap()(&p),
                s_ignore_satisfied_runtime(&p),
                "SATISFIED.ig(4) {word}"
            );
            assert_eq!(
                SATISFIED.ig(5).unwrap()(&p),
                s_ignore_satisfied_buildtime(&p),
                "SATISFIED.ig(5) {word}"
            );
            assert_eq!(
                SATISFIED.ig(6).unwrap()(&p),
                s_ignore_satisfied_buildtime_slot_op(&p),
                "SATISFIED.ig(6) {word}"
            );
            assert_eq!(
                SATISFIED.ig(7).unwrap()(&p),
                s_ignore_runtime(&p),
                "SATISFIED.ig(7)"
            );
            assert_eq!(
                NORMAL.ig_medium().unwrap()(&p),
                n_ignore_runtime(&p),
                "medium"
            );
            assert_eq!(
                NORMAL.ig_medium_soft().unwrap()(&p),
                n_ignore_runtime_post(&p),
                "medium_soft"
            );
            assert_eq!(
                SATISFIED.ig_medium().unwrap()(&p),
                s_ignore_runtime(&p),
                "s-medium"
            );
            assert_eq!(
                SATISFIED.ig_medium_soft().unwrap()(&p),
                s_ignore_satisfied_buildtime_slot_op(&p),
                "s-medium_soft"
            );
        }
        assert_eq!(
            (NORMAL.medium, NORMAL.medium_soft, NORMAL.medium_post),
            (3, 2, 2)
        );
        assert_eq!(
            (
                SATISFIED.medium,
                SATISFIED.medium_soft,
                SATISFIED.medium_post
            ),
            (7, 6, 3)
        );
        assert!(NORMAL.ig(0).is_none(), "the NORMAL ladder starts at NONE");
    }

    #[test]
    fn rank_best_prefers_merge_bound_then_vercmp() {
        let entries = vec![
            new_entry("dev-libs", "a", "1.0", Vec::new()),
            new_entry("dev-libs", "a", "2.0", Vec::new()),
            new_entry("dev-libs", "a", "1.5", Vec::new()),
        ];
        // A merge-bound 1.0 beats an installed 2.0.
        let installed = vec![false, true, true];
        assert_eq!(
            DigraphPrelude::rank_best(&entries, &installed, [0usize, 1].into_iter()),
            Some(0)
        );
        // Among installed candidates, vercmp picks the highest, not the
        // first or the string-highest.
        let installed = vec![true, true, true];
        assert_eq!(
            DigraphPrelude::rank_best(&entries, &installed, [0usize, 1, 2].into_iter()),
            Some(1)
        );
        assert_eq!(
            DigraphPrelude::rank_best(&entries, &installed, [1usize, 0].into_iter()),
            Some(1)
        );
        // A version tie keeps the first candidate.
        let tied = vec![
            new_entry("dev-libs", "b", "1.0", Vec::new()),
            new_entry("dev-libs", "b", "1.0", Vec::new()),
        ];
        assert_eq!(
            DigraphPrelude::rank_best(&tied, &[true, true], [0usize, 1].into_iter()),
            Some(0)
        );
        // One candidate, and the empty set.
        assert_eq!(
            DigraphPrelude::rank_best(&entries, &installed, [2usize].into_iter()),
            Some(2)
        );
        assert_eq!(
            DigraphPrelude::rank_best(&entries, &installed, std::iter::empty()),
            None
        );
    }

    #[test]
    fn select_dep_target_narrows_and_ranks_like_real() {
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut installed = new_entry("dev-libs", "slotted", "1.0", Vec::new());
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        installed.slot = Some("0".into());
        installed.sub_slot = Some("0".into());
        let mut merged = new_entry("dev-libs", "slotted", "2.0", Vec::new());
        merged.slot = Some("0".into());
        merged.sub_slot = Some("0".into());
        let root = Path::new("/nonexistent-root-for-unit-test");
        let none = HashSet::new();
        // A version-qualified atom matches only the merge-bound 2.0, so
        // the single-match ranking path picks it.
        let entries = vec![
            installed.clone(),
            merged.clone(),
            new_entry(
                "dev-libs",
                "user",
                "1.0",
                vec![plain_edge(
                    ">=dev-libs/slotted-2",
                    "dev-libs",
                    "slotted",
                    runtime,
                )],
            ),
        ];
        let pre = digraph_prelude(&entries, root);
        assert_eq!(
            pre.select_dep_target(&entries, 2, &entries[2].deps, 0, &none, false),
            Some(1)
        );
        // A bare atom matches both, but `merge_bound_only` drops the
        // installed candidate and still picks the merge-bound one.
        let entries = vec![
            installed.clone(),
            merged,
            new_entry(
                "dev-libs",
                "user",
                "1.0",
                vec![plain_edge(
                    "dev-libs/slotted",
                    "dev-libs",
                    "slotted",
                    runtime,
                )],
            ),
        ];
        let pre = digraph_prelude(&entries, root);
        assert_eq!(
            pre.select_dep_target(&entries, 2, &entries[2].deps, 0, &none, true),
            Some(1),
            "both candidates match; merge_bound_only still prefers the merged one"
        );
        // The only match is installed and `merge_bound_only` drops it.
        let entries = vec![
            installed,
            new_entry(
                "dev-libs",
                "user",
                "1.0",
                vec![plain_edge(
                    "dev-libs/slotted",
                    "dev-libs",
                    "slotted",
                    runtime,
                )],
            ),
        ];
        let pre = digraph_prelude(&entries, root);
        assert_eq!(
            pre.select_dep_target(&entries, 1, &entries[1].deps, 0, &none, true),
            None,
            "the only match is installed and merge_bound_only drops it"
        );
    }

    #[test]
    fn build_digraph_drops_self_edges_except_unsatisfied_buildtime() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        // A runtime self-edge is dropped.
        let entries = vec![new_entry(
            "dev-libs",
            "selfish",
            "1.0",
            vec![plain_edge(
                "dev-libs/selfish",
                "dev-libs",
                "selfish",
                runtime,
            )],
        )];
        let g = build_digraph(&entries, &["dev-libs/selfish".to_string()], root);
        assert!(
            g.children[0].is_empty(),
            "runtime self-edge dropped: {:?}",
            g.children[0]
        );
        // An unsatisfied build-time self-edge is kept.
        let entries = vec![new_entry(
            "dev-libs",
            "selfish",
            "1.0",
            vec![plain_edge(
                "dev-libs/selfish",
                "dev-libs",
                "selfish",
                DepPriority {
                    buildtime: true,
                    ..DepPriority::default()
                },
            )],
        )];
        let g = build_digraph(&entries, &["dev-libs/selfish".to_string()], root);
        assert_eq!(
            g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            vec![0],
            "unsatisfied buildtime self-edge kept: {:?}",
            g.children[0]
        );
    }

    #[test]
    fn build_digraph_skips_a_blocker_and_a_nonmatching_top_atom() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let a = new_entry("dev-libs", "a", "1.0", Vec::new());
        let b = new_entry("dev-libs", "b", "1.0", Vec::new());
        let entries = vec![a, b];
        // A blocker top-level atom is skipped; the second atom seeds.
        let g = build_digraph(
            &entries,
            &["!dev-libs/a".to_string(), "dev-libs/b".to_string()],
            root,
        );
        assert_eq!(
            g.order,
            vec![1, 0],
            "b seeds, a is appended unwalked: {:?}",
            g.order
        );
        // A version-qualified top atom seeds only its matching instance.
        let a1 = new_entry("dev-libs", "a", "1.0", Vec::new());
        let mut a2 = new_entry("dev-libs", "a", "2.0", Vec::new());
        a2.slot = Some("2".into());
        a2.sub_slot = Some("2".into());
        let entries = vec![a1, a2];
        let g = build_digraph(&entries, &["<dev-libs/a-2".to_string()], root);
        assert_eq!(
            g.order,
            vec![0, 1],
            "1.0 is seeded, 2.0 appended: {:?}",
            g.order
        );
    }

    #[test]
    fn build_digraph_walks_inline_before_deferred_disjunctive_edges() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut deferred = plain_edge("dev-libs/late", "dev-libs", "late", runtime);
        deferred.disjunctive = true;
        deferred.alt = Some((0, 0));
        let owner = new_entry(
            "dev-libs",
            "owner",
            "1.0",
            vec![
                plain_edge("dev-libs/early", "dev-libs", "early", runtime),
                deferred,
            ],
        );
        let entries = vec![
            owner,
            new_entry("dev-libs", "early", "1.0", Vec::new()),
            new_entry("dev-libs", "late", "1.0", Vec::new()),
        ];
        let g = build_digraph(&entries, &["dev-libs/owner".to_string()], root);
        assert_eq!(
            g.order,
            vec![0, 1, 2],
            "inline edge discovered before the deferred disjunctive bundle: {:?}",
            g.order
        );
    }

    #[test]
    fn build_digraph_uninstall_pairs_the_owner_before_the_removal() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let owner = new_entry(
            "dev-libs",
            "owner",
            "1.0",
            vec![plain_edge("dev-libs/leaf", "dev-libs", "leaf", runtime)],
        );
        let leaf = new_entry("dev-libs", "leaf", "1.0", Vec::new());
        let mut removal = new_entry("dev-libs", "old", "1.0", Vec::new());
        removal.outcome = PretendOutcome::Uninstall {
            version: "1.0".into(),
        };
        removal.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
        let entries = vec![owner, leaf, removal];
        let g = build_digraph(&entries, &["dev-libs/owner".to_string()], root);
        assert_eq!(
            g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            vec![1],
            "the forward edge is the owner's only child: {:?}",
            g.children[0]
        );
        let reverse = g.children[2]
            .iter()
            .find(|(c, _)| *c == 0)
            .expect("the removal must carry the owner as its child");
        assert_eq!(reverse.1.len(), 1);
        assert!(
            reverse.1[0].buildtime && reverse.1[0].runtime && !reverse.1[0].satisfied,
            "the reverse edge is the hard buildtime+runtime one: {:?}",
            reverse.1
        );
    }

    #[test]
    fn build_digraph_required_by_fallback_adds_only_the_missing_owner_edge() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        // The forward walk missed the owner, so the fallback supplies it.
        let mut dep = new_entry("dev-libs", "dep", "1.0", Vec::new());
        dep.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
        let owner = new_entry("dev-libs", "owner", "1.0", Vec::new());
        let entries = vec![owner.clone(), dep.clone()];
        let g = build_digraph(&entries, &["dev-libs/owner".to_string()], root);
        let fallback = g.children[0]
            .iter()
            .find(|(c, _)| *c == 1)
            .expect("fallback edge owner -> dep");
        assert!(
            fallback.1[0].runtime && !fallback.1[0].satisfied,
            "fallback priority: {:?}",
            fallback.1
        );
        // With an unrelated child already on the owner, the fallback
        // still appears (the skip is keyed to the removal's own index).
        let leaf = new_entry("dev-libs", "leaf", "1.0", Vec::new());
        let entries = vec![owner, dep, leaf];
        let g = build_digraph(
            &entries,
            &["dev-libs/owner".to_string(), "dev-libs/leaf".to_string()],
            root,
        );
        assert!(
            g.children[0].iter().any(|(c, _)| *c == 1),
            "fallback edge still added beside an unrelated child: {:?}",
            g.children[0]
        );
    }

    #[test]
    fn build_digraph_fallback_skips_a_cp_the_owner_already_edged() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        // The owner's forward edge resolves the slot-qualified atom to
        // slot 1; slot 2's own required_by names the owner, but the
        // cp-level fallback must not add a second edge to it.
        let owner = new_entry(
            "dev-libs",
            "owner",
            "1.0",
            vec![plain_edge("dev-libs/x:1", "dev-libs", "x", runtime)],
        );
        let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
        x1.slot = Some("1".into());
        x1.sub_slot = Some("1".into());
        let mut x2 = new_entry("dev-libs", "x", "2.0", Vec::new());
        x2.slot = Some("2".into());
        x2.sub_slot = Some("2".into());
        x2.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
        let entries = vec![owner, x1, x2];
        let g = build_digraph(&entries, &["dev-libs/owner".to_string()], root);
        assert_eq!(
            g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            vec![1],
            "no fallback edge to the other slot of the same cp: {:?}",
            g.children[0]
        );
        // A same-category different-package child does not count as the
        // same cp: the fallback must still be added.
        let owner = new_entry(
            "dev-libs",
            "owner",
            "1.0",
            vec![plain_edge("dev-libs/y", "dev-libs", "y", runtime)],
        );
        let y = new_entry("dev-libs", "y", "1.0", Vec::new());
        let mut x = new_entry("dev-libs", "x", "1.0", Vec::new());
        x.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
        let entries = vec![owner, y, x];
        let g = build_digraph(
            &entries,
            &["dev-libs/owner".to_string(), "dev-libs/y".to_string()],
            root,
        );
        assert!(
            g.children[0].iter().any(|(c, _)| *c == 2),
            "an unrelated same-category child is not the same cp: {:?}",
            g.children[0]
        );
    }

    #[test]
    fn build_digraph_skips_a_self_named_required_by() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let mut a = new_entry("dev-libs", "a", "1.0", Vec::new());
        a.required_by = vec![("dev-libs".to_string(), "a".to_string())];
        let entries = vec![a];
        let g = build_digraph(&entries, &["dev-libs/a".to_string()], root);
        assert!(
            g.children[0].is_empty(),
            "a self fallback edge is skipped: {:?}",
            g.children[0]
        );
    }
    // -----------------------------------------------------------------
    // #146 follow-up: the survivors of the first after-run
    // -----------------------------------------------------------------

    #[test]
    fn split_disjunctive_keeps_a_group_when_a_second_group_marker_is_pending() {
        // A defensive-shape pin: `||` immediately followed by another
        // group inside an open group must not restart the group
        // bookkeeping (a structured reduce flattens nested `||`, so this
        // pins the guard against a future walker change).
        assert_eq!(
            split_disjunctive(&tokens(&["||", "(", "||", "(", "dev-libs/a", ")", ")"])),
            (vec![], vec![("dev-libs/a".to_string(), 0, 0)])
        );
    }

    #[test]
    fn select_dep_target_eliminates_with_suppressed_sibling_sets() {
        // A suppressed `||` sibling must not join the minimize universe:
        // with the sibling's set {x1} counted, the shared-by-all test
        // keeps x1 and drops x2, flipping the pick away from the
        // vercmp-highest merge-bound target.
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
        x1.slot = Some("0".into());
        x1.sub_slot = Some("0".into());
        let mut x2 = new_entry("dev-libs", "x", "2.0", Vec::new());
        x2.slot = Some("1".into());
        x2.sub_slot = Some("1".into());
        let bare = DepEdge {
            atom: "dev-libs/x".to_string(),
            evaluated: "dev-libs/x".to_string(),
            category: "dev-libs".to_string(),
            package: "x".to_string(),
            priority: runtime,
            disjunctive: true,
            alt: Some((0, 0)),
            key: 3,
        };
        let qualified = DepEdge {
            atom: "dev-libs/x:0".to_string(),
            evaluated: "dev-libs/x:0".to_string(),
            category: "dev-libs".to_string(),
            package: "x".to_string(),
            priority: runtime,
            disjunctive: true,
            alt: Some((0, 1)),
            key: 3,
        };
        let user = new_entry("dev-libs", "user", "1.0", vec![bare, qualified]);
        let entries = vec![x1, x2, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let pre = digraph_prelude(&entries, root);
        let suppressed = suppressed_alt_edges(&entries, &pre);
        assert_eq!(
            suppressed[2],
            HashSet::from([1usize]),
            "the qualified branch is the suppressed one"
        );
        assert_eq!(
            pre.select_dep_target(&entries, 2, &entries[2].deps, 0, &suppressed[2], false),
            Some(1),
            "the suppressed sibling's set must not enter the elimination"
        );
    }

    #[test]
    fn select_dep_target_elimination_keeps_the_last_on_a_version_tie() {
        // Two same-version candidates: elimination drops the first and
        // keeps the last; the single-match shortcut must keep the
        // vercmp tie on the first.
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
        x1.slot = Some("0".into());
        x1.sub_slot = Some("0".into());
        let mut x2 = new_entry("dev-libs", "x", "1.0", Vec::new());
        x2.slot = Some("1".into());
        x2.sub_slot = Some("1".into());
        let user = new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
        );
        let entries = vec![x1, x2, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let pre = digraph_prelude(&entries, root);
        let none = HashSet::new();
        assert_eq!(
            pre.select_dep_target(&entries, 2, &entries[2].deps, 0, &none, false),
            Some(1),
            "two candidates take the elimination path, which keeps the last"
        );
    }

    #[test]
    fn select_dep_target_elimination_orders_installed_before_merge_bound() {
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        // Installed 2.0 first: it is eliminated first, the merge-bound
        // 1.0 survives -- a version compare would keep the installed one.
        let mut installed = new_entry("dev-libs", "x", "2.0", Vec::new());
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "2.0".into(),
        };
        let merged = new_entry("dev-libs", "x", "1.0", Vec::new());
        let user = new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
        );
        let entries = vec![installed, merged, user.clone()];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let pre = digraph_prelude(&entries, root);
        let none = HashSet::new();
        assert_eq!(
            pre.select_dep_target(&entries, 2, &entries[2].deps, 0, &none, false),
            Some(1),
            "the installed candidate is eliminated first"
        );
        // The mirrored order: merge-bound 1.0 first in the entry array,
        // installed 2.0 second. The `(false, true)` arm must still sort
        // the installed one first.
        let mut installed = new_entry("dev-libs", "x", "2.0", Vec::new());
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "2.0".into(),
        };
        let merged = new_entry("dev-libs", "x", "1.0", Vec::new());
        let entries = vec![merged, installed, user];
        let pre = digraph_prelude(&entries, root);
        assert_eq!(
            pre.select_dep_target(&entries, 2, &entries[2].deps, 0, &none, false),
            Some(0),
            "the merge-bound candidate survives elimination"
        );
    }

    #[test]
    fn select_dep_target_keeps_nvc_out_of_the_elimination() {
        // An NVC candidate neither eliminates nor is eliminated: it
        // stays alive and wins the ranking (its empty version ties, and
        // it is first), where the elimination would otherwise remove it
        // and leave an installed candidate.
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut nvc = new_entry("dev-libs", "x", "1.0", Vec::new());
        nvc.outcome = PretendOutcome::NoVisibleCandidate;
        let mut a = new_entry("dev-libs", "x", "1.0", Vec::new());
        a.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let mut b = new_entry("dev-libs", "x", "1.0", Vec::new());
        b.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let user = new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
        );
        let entries = vec![nvc, a, b, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let pre = digraph_prelude(&entries, root);
        let none = HashSet::new();
        assert_eq!(
            pre.select_dep_target(&entries, 3, &entries[3].deps, 0, &none, false),
            Some(0),
            "the NVC candidate stays in the ranking"
        );
    }

    #[test]
    fn rank_best_prefers_a_merge_bound_candidate_over_a_newer_installed_one() {
        let entries = vec![
            new_entry("dev-libs", "a", "2.0", Vec::new()),
            new_entry("dev-libs", "a", "1.0", Vec::new()),
        ];
        let installed = vec![true, false];
        // Installed first in the iterator, merge-bound second: the
        // `(true, false)` arm picks the merge-bound one even though the
        // installed candidate's version is higher.
        assert_eq!(
            DigraphPrelude::rank_best(&entries, &installed, [0usize, 1].into_iter()),
            Some(1)
        );
    }

    #[test]
    fn build_digraph_expands_disjunctive_bundles_by_their_own_key() {
        // Two deferred bundles from different dep keys must each expand
        // only their own key's edges: RDEPEND's bundle is queued first,
        // DEPEND's second, and the LIFO pop expands DEPEND's before
        // RDEPEND's.
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut rdep = plain_edge("dev-libs/b", "dev-libs", "b", runtime);
        rdep.disjunctive = true;
        rdep.alt = Some((0, 0));
        rdep.key = 0;
        let mut dep = plain_edge("dev-libs/c", "dev-libs", "c", runtime);
        dep.disjunctive = true;
        dep.alt = Some((0, 0));
        dep.key = 3;
        let owner = new_entry(
            "dev-libs",
            "owner",
            "1.0",
            vec![
                plain_edge("dev-libs/a", "dev-libs", "a", runtime),
                rdep,
                dep,
            ],
        );
        let entries = vec![
            owner,
            new_entry("dev-libs", "a", "1.0", Vec::new()),
            new_entry("dev-libs", "b", "1.0", Vec::new()),
            new_entry("dev-libs", "c", "1.0", Vec::new()),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let g = build_digraph(&entries, &["dev-libs/owner".to_string()], root);
        assert_eq!(
            g.order,
            vec![0, 1, 3, 2],
            "inline a, then DEPEND's bundle (c), then RDEPEND's (b)"
        );
    }

    #[test]
    fn build_digraph_required_by_fallback_marks_an_installed_target_satisfied() {
        let mut dep = new_entry("dev-libs", "dep", "1.0", Vec::new());
        dep.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        dep.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
        let owner = new_entry("dev-libs", "owner", "1.0", Vec::new());
        let entries = vec![owner, dep];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let g = build_digraph(&entries, &["dev-libs/owner".to_string()], root);
        let fallback = g.children[0]
            .iter()
            .find(|(c, _)| *c == 1)
            .expect("fallback edge owner -> dep");
        assert!(
            fallback.1[0].runtime && fallback.1[0].satisfied,
            "an installed target's fallback edge is satisfied: {:?}",
            fallback.1
        );
    }

    #[test]
    fn select_dep_target_runs_elimination_for_three_candidates() {
        // Three matching slots and two kept sibling atoms: elimination
        // removes the two candidates every sibling set also matches,
        // leaving x1; the rank-only path would pick the vercmp-highest
        // x3.
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
        x1.slot = Some("0".into());
        x1.sub_slot = Some("0".into());
        let mut x2 = new_entry("dev-libs", "x", "2.0", Vec::new());
        x2.slot = Some("1".into());
        x2.sub_slot = Some("1".into());
        let mut x3 = new_entry("dev-libs", "x", "3.0", Vec::new());
        x3.slot = Some("2".into());
        x3.sub_slot = Some("2".into());
        // Two separate `||` groups (different keys), so both branches
        // are kept: the bare atom matches all three slots, the `:0`
        // atom only x1.
        let bare = DepEdge {
            atom: "dev-libs/x".to_string(),
            evaluated: "dev-libs/x".to_string(),
            category: "dev-libs".to_string(),
            package: "x".to_string(),
            priority: runtime,
            disjunctive: true,
            alt: Some((0, 0)),
            key: 0,
        };
        let qualified = DepEdge {
            atom: "dev-libs/x:0".to_string(),
            evaluated: "dev-libs/x:0".to_string(),
            category: "dev-libs".to_string(),
            package: "x".to_string(),
            priority: runtime,
            disjunctive: true,
            alt: Some((0, 0)),
            key: 3,
        };
        let user = new_entry("dev-libs", "user", "1.0", vec![bare, qualified]);
        let entries = vec![x1, x2, x3, user];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let pre = digraph_prelude(&entries, root);
        let none = HashSet::new();
        assert_eq!(
            pre.select_dep_target(&entries, 3, &entries[3].deps, 0, &none, false),
            Some(0),
            "the elimination leaves only the candidate no sibling set fully shares"
        );
    }
    // -----------------------------------------------------------------
    // #145: installed / vdb-backed inputs
    // -----------------------------------------------------------------

    /// A scratch vdb: `var/db/pkg/<cat>/<pf>/` with the given files.
    /// Mirrors `lib.rs`'s own `tmp_vdb` helper; the unique root avoids
    /// `all_installed_packages`' per-root cache and lets each test build
    /// the exact installed shape it needs (the committed fixture vdb has
    /// no injected-libc record).
    type VdbFile<'a> = (&'a str, &'a str);
    type VdbEntry<'a> = (&'a str, &'a str, &'a [VdbFile<'a>]);

    fn mo_tmp_vdb(tag: &str, entries: &[VdbEntry<'_>]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "portuale-mo-vdb-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        for (cat, pf, files) in entries {
            let dir = root.join("var/db/pkg").join(cat).join(pf);
            std::fs::create_dir_all(&dir).unwrap();
            for (name, value) in *files {
                std::fs::write(dir.join(name), value.as_bytes()).unwrap();
            }
        }
        root
    }

    fn fixtures_vdb_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn installed_by_cp(rows: &[(&str, &str, &[&str])]) -> HashMap<(String, String), Vec<String>> {
        let mut m = HashMap::new();
        for (cat, pkg, cands) in rows {
            m.insert(
                (cat.to_string(), pkg.to_string()),
                cands.iter().map(|s| s.to_string()).collect(),
            );
        }
        m
    }

    fn runtime_edge(atom: &str, cat: &str, pkg: &str) -> DepEdge {
        plain_edge(
            atom,
            cat,
            pkg,
            DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
        )
    }

    /// A runtime edge tagged as real's `RDEPEND` key (`key == 0`), which
    /// `seed_toolchain_asap` reads providers from.
    fn rdepend_edge(atom: &str, cat: &str, pkg: &str) -> DepEdge {
        let mut edge = runtime_edge(atom, cat, pkg);
        edge.key = 0;
        edge
    }

    #[test]
    fn outcome_version_pins_every_outcome() {
        let mut e = new_entry("dev-libs", "x", "1.0", Vec::new());
        e.outcome = PretendOutcome::NoVisibleCandidate;
        assert_eq!(outcome_version(&e), None);
        e.outcome = PretendOutcome::New {
            version: "1.0".into(),
        };
        assert_eq!(outcome_version(&e), Some("1.0"));
        e.outcome = PretendOutcome::Upgrade {
            from: "0.9".into(),
            to: "1.2".into(),
        };
        assert_eq!(outcome_version(&e), Some("1.2"));
        e.outcome = PretendOutcome::Downgrade {
            from: "2.0".into(),
            to: "1.0".into(),
        };
        assert_eq!(outcome_version(&e), Some("1.0"));
        e.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        assert_eq!(outcome_version(&e), Some("1.0"));
        e.outcome = PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: vec!["flip".into()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        };
        assert_eq!(outcome_version(&e), Some("1.0"));
        e.outcome = PretendOutcome::Uninstall {
            version: "1.0".into(),
        };
        assert_eq!(outcome_version(&e), None);
    }

    #[test]
    fn edge_satisfied_with_pins_the_slot_operator_narrowing() {
        let map = installed_by_cp(&[(
            "dev-libs",
            "slotted",
            &["dev-libs/slotted-1.0:0/0", "dev-libs/slotted-2.0:1/1"],
        )]);
        let plain = runtime_edge("dev-libs/slotted", "dev-libs", "slotted");
        assert!(edge_satisfied_with(&map, &plain, None));
        // No candidates, no atom match, no match at all.
        assert!(!edge_satisfied_with(&installed_by_cp(&[]), &plain, None));
        assert!(!edge_satisfied_with(
            &map,
            &runtime_edge(">=dev-libs/slotted-3", "dev-libs", "slotted"),
            None
        ));
        assert!(!edge_satisfied_with(
            &map,
            &runtime_edge("dev-libs/absent", "dev-libs", "absent"),
            None
        ));
        // A slot-operator edge with no resolved child falls back to the
        // plain atom check.
        let slotop = plain_edge(
            "dev-libs/slotted",
            "dev-libs",
            "slotted",
            DepPriority {
                runtime: true,
                runtime_slot_op: true,
                ..DepPriority::default()
            },
        );
        assert!(edge_satisfied_with(&map, &slotop, None));
        // With a resolved child it must carry the child's own slot/sub.
        assert!(edge_satisfied_with(&map, &slotop, Some(("1", "1"))));
        assert!(edge_satisfied_with(&map, &slotop, Some(("0", "0"))));
        assert!(
            !edge_satisfied_with(&map, &slotop, Some(("0", "1"))),
            "a half-matching slot/sub-slot is not satisfied"
        );
        assert!(
            !edge_satisfied_with(&map, &slotop, Some(("2", "2"))),
            "no candidate carries the child's slot"
        );
        // The build-time slot-operator variant narrows identically.
        let bslotop = plain_edge(
            "dev-libs/slotted",
            "dev-libs",
            "slotted",
            DepPriority {
                buildtime: true,
                buildtime_slot_op: true,
                ..DepPriority::default()
            },
        );
        assert!(edge_satisfied_with(&map, &bslotop, Some(("0", "0"))));
        assert!(!edge_satisfied_with(&map, &bslotop, Some(("2", "2"))));
    }

    #[test]
    fn installed_candidates_by_cp_reads_the_fixture_vdb() {
        let root = fixtures_vdb_root();
        let map = installed_candidates_by_cp(&root);
        let mut got = map
            .get(&("dev-libs".to_string(), "dualslotpkg".to_string()))
            .cloned()
            .expect("the fixture vdb carries two dualslotpkg slots");
        got.sort();
        assert_eq!(
            got,
            vec![
                "dev-libs/dualslotpkg-1.0:1/1".to_string(),
                "dev-libs/dualslotpkg-2.0:2/2".to_string()
            ]
        );
        assert!(!map.contains_key(&("dev-libs".to_string(), "definitely-absent".to_string())));
    }

    #[test]
    fn dep_edge_satisfied_by_installed_reads_the_vdb() {
        let root = mo_tmp_vdb(
            "satisfied",
            &[(
                "dev-libs",
                "inst-1.0",
                &[("SLOT", "0/1"), ("RDEPEND", "dev-libs/other")],
            )],
        );
        assert!(dep_edge_satisfied_by_installed(
            &root,
            &runtime_edge("dev-libs/inst", "dev-libs", "inst"),
            None
        ));
        assert!(!dep_edge_satisfied_by_installed(
            &root,
            &runtime_edge(">=dev-libs/inst-2", "dev-libs", "inst"),
            None
        ));
        // A slot-operator edge follows the resolved child's own slot.
        let slotop = plain_edge(
            "dev-libs/inst",
            "dev-libs",
            "inst",
            DepPriority {
                runtime: true,
                runtime_slot_op: true,
                ..DepPriority::default()
            },
        );
        let mut child = new_entry("dev-libs", "inst", "2.0", Vec::new());
        child.slot = Some("1".into());
        child.sub_slot = Some("1".into());
        assert!(!dep_edge_satisfied_by_installed(
            &root,
            &slotop,
            Some(&child)
        ));
        child.slot = Some("0".into());
        child.sub_slot = Some("1".into());
        assert!(dep_edge_satisfied_by_installed(
            &root,
            &slotop,
            Some(&child)
        ));
    }

    #[test]
    fn add_installed_dependency_closure_seeds_and_expands_the_installed_tree() {
        let root = mo_tmp_vdb(
            "closure-seed",
            &[
                (
                    "dev-libs",
                    "one-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "dev-libs/two")],
                ),
                (
                    "dev-libs",
                    "two-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "dev-libs/three")],
                ),
                ("dev-libs", "three-1.0", &[("SLOT", "0")]),
                ("dev-libs", "three-2.0", &[("SLOT", "0")]),
            ],
        );
        // 1 is an installed entry with no deps yet (seed 2 fills it).
        let mut two = new_entry("dev-libs", "two", "1.0", Vec::new());
        two.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let top = new_entry(
            "app-misc",
            "top",
            "1.0",
            vec![
                runtime_edge("<dev-libs/three-2", "dev-libs", "three"),
                runtime_edge("dev-libs/one", "dev-libs", "one"),
            ],
        );
        let mut entries = vec![top, two];
        add_installed_dependency_closure(&mut entries, &root, &[], &[], false, true);
        // top, two, three-1.0 (the version-qualified pick), one, three-2.0
        // (the highest match of two's bare atom).
        assert_eq!(
            entries.len(),
            5,
            "{:#?}",
            entries
                .iter()
                .map(|e| (&e.package, outcome_version(e)))
                .collect::<Vec<_>>()
        );
        assert_eq!(entries[2].package, "three");
        assert_eq!(
            outcome_version(&entries[2]),
            Some("1.0"),
            "the qualified atom picks 1.0"
        );
        assert_eq!(entries[3].package, "one");
        assert_eq!(
            entries[3]
                .deps
                .iter()
                .map(|e| e.atom.as_str())
                .collect::<Vec<_>>(),
            vec!["dev-libs/two"],
            "the seeded node's own vdb deps are filled"
        );
        assert_eq!(entries[4].package, "three");
        assert_eq!(
            outcome_version(&entries[4]),
            Some("2.0"),
            "the bare atom picks the highest"
        );
        assert_eq!(
            entries[1]
                .deps
                .iter()
                .map(|e| e.atom.as_str())
                .collect::<Vec<_>>(),
            vec!["dev-libs/three"],
            "seed 2 fills an installed entry's deps"
        );
    }

    #[test]
    fn add_installed_dependency_closure_virtuals_only_expands_virtuals() {
        let root = mo_tmp_vdb(
            "closure-virtuals",
            &[
                (
                    "virtual",
                    "libc-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "sys-libs/glibc-mine")],
                ),
                (
                    "sys-libs",
                    "glibc-mine-2.38",
                    &[("SLOT", "0"), ("RDEPEND", "dev-libs/glibc-dep")],
                ),
                ("dev-libs", "glibc-dep-1.0", &[("SLOT", "0")]),
                (
                    "dev-libs",
                    "plain-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "dev-libs/plain-dep")],
                ),
                ("dev-libs", "plain-dep-1.0", &[("SLOT", "0")]),
                ("dev-libs", "systemseed-1.0", &[("SLOT", "0")]),
            ],
        );
        let top = new_entry(
            "app-misc",
            "top",
            "1.0",
            vec![
                runtime_edge("virtual/libc", "virtual", "libc"),
                runtime_edge("dev-libs/plain", "dev-libs", "plain"),
            ],
        );
        let mut entries = vec![top];
        add_installed_dependency_closure(
            &mut entries,
            &root,
            &[],
            &["dev-libs/systemseed".to_string()],
            true,
            true,
        );
        let packages: Vec<&str> = entries.iter().map(|e| e.package.as_str()).collect();
        assert_eq!(
            packages,
            vec!["top", "libc", "plain", "glibc-mine"],
            "{packages:?}"
        );
        assert_eq!(
            entries[1]
                .deps
                .iter()
                .map(|e| e.atom.as_str())
                .collect::<Vec<_>>(),
            vec!["sys-libs/glibc-mine"],
            "the virtual expands to its provider"
        );
        assert!(
            entries[2].deps.is_empty(),
            "a non-virtual installed node is a leaf under virtuals_only"
        );
        assert!(
            entries[3].deps.is_empty(),
            "the provider is added as a leaf, not queued: {:?}",
            entries[3].deps
        );
        assert!(
            !packages.contains(&"glibc-dep") && !packages.contains(&"plain-dep"),
            "non-virtual nodes are never expanded under virtuals_only: {packages:?}"
        );
        assert!(
            !packages.contains(&"systemseed"),
            "virtuals_only does not seed @system: {packages:?}"
        );
    }

    #[test]
    fn add_installed_dependency_closure_strips_the_injected_libc_dep() {
        let files: &[(&str, &str)] = &[
            ("SLOT", "0"),
            (
                "RDEPEND",
                ">=sys-libs/glibc-mine-2.38 dev-libs/keep \
                 >sys-libs/glibc-mine-2.38 \
                 >=sys-libs/glibc-mine-2.38:2.38 >=sys-libs/glibc-mine-2.38[foo] \
                 =sys-libs/glibc-mine-2.38 >=sys-libs/other-2.38",
            ),
        ];
        let root = mo_tmp_vdb(
            "closure-libc",
            &[
                (
                    "virtual",
                    "libc-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "sys-libs/glibc-mine")],
                ),
                ("dev-libs", "inj-1.0", files),
            ],
        );
        let mut entries = vec![new_entry(
            "app-misc",
            "top",
            "1.0",
            vec![runtime_edge("dev-libs/inj", "dev-libs", "inj")],
        )];
        add_installed_dependency_closure(&mut entries, &root, &[], &[], false, true);
        let inj = entries
            .iter()
            .find(|e| e.package == "inj")
            .expect("the installed dep is seeded");
        let atoms: HashSet<&str> = inj.deps.iter().map(|e| e.atom.as_str()).collect();
        assert!(
            !atoms.contains(">=sys-libs/glibc-mine-2.38"),
            "the injected bare `>=libc-provider-version` atom is stripped: {atoms:?}"
        );
        for kept in [
            "dev-libs/keep",
            ">sys-libs/glibc-mine-2.38",
            ">=sys-libs/glibc-mine-2.38:2.38",
            ">=sys-libs/glibc-mine-2.38[foo]",
            "=sys-libs/glibc-mine-2.38",
            ">=sys-libs/other-2.38",
        ] {
            assert!(
                atoms.contains(kept),
                "only the exact injected shape is stripped; {kept} must stay: {atoms:?}"
            );
        }
        // Without an installed libc provider there is no injection to
        // strip: the same atom stays.
        let root = mo_tmp_vdb("closure-libc-none", &[("dev-libs", "inj-1.0", files)]);
        let mut entries = vec![new_entry(
            "app-misc",
            "top",
            "1.0",
            vec![runtime_edge("dev-libs/inj", "dev-libs", "inj")],
        )];
        add_installed_dependency_closure(&mut entries, &root, &[], &[], false, true);
        let inj = entries.iter().find(|e| e.package == "inj").unwrap();
        assert!(
            inj.deps
                .iter()
                .any(|e| e.atom == ">=sys-libs/glibc-mine-2.38"),
            "no libc provider means nothing is stripped: {:?}",
            inj.deps.iter().map(|e| e.atom.as_str()).collect::<Vec<_>>()
        );
    }
    // -----------------------------------------------------------------
    // #144: the scheduler loop
    // -----------------------------------------------------------------

    #[test]
    fn digraph_child_nodes_and_has_parents_pin_the_alive_filter() {
        let mut g = test_graph(3, &[(0, 1, prio(2)), (0, 2, prio(32))]);
        assert_eq!(g.child_nodes(0, None), vec![1, 2]);
        assert!(g.has_parents(1));
        assert!(!g.has_parents(0));
        // The optional edge drops under `n_ignore_optional`.
        assert_eq!(g.child_nodes(0, Some(n_ignore_optional)), vec![1]);
        // A dead child disappears.
        g.alive[1] = false;
        assert_eq!(g.child_nodes(0, None), vec![2]);
        // A dead parent does not count.
        g.alive[0] = false;
        assert!(!g.has_parents(2));
    }

    #[test]
    fn digraph_add_edge_merges_priorities_without_duplicates() {
        let mut g = test_graph(2, &[]);
        g.add_edge(0, 1, prio(2));
        g.add_edge(0, 1, prio(2));
        assert_eq!(g.children[0].len(), 1);
        assert_eq!(g.children[0][0].1, vec![prio(2)]);
        g.add_edge(0, 1, prio(1));
        assert_eq!(g.children[0][0].1, vec![prio(2), prio(1)]);
        assert_eq!(g.parents[1], vec![0]);
    }

    #[test]
    fn deep_system_deps_follows_only_runtime_edges() {
        let config = portage_profile::Config {
            system_packages: vec!["dev-libs/sys".to_string()],
            ..portage_profile::Config::default()
        };
        let entries = vec![
            new_entry("dev-libs", "sys", "1.0", Vec::new()),
            new_entry("dev-libs", "rt", "1.0", Vec::new()),
            new_entry("dev-libs", "bt", "1.0", Vec::new()),
            new_entry("dev-libs", "post", "1.0", Vec::new()),
        ];
        let g = test_graph(
            4,
            &[
                (0, 1, prio(2)), // runtime
                (0, 2, prio(1)), // buildtime
                (1, 3, prio(4)), // runtime_post
            ],
        );
        assert_eq!(
            deep_system_deps(&g, &entries, &config),
            vec![true, true, false, true]
        );
    }

    #[test]
    fn seed_toolchain_asap_pins_the_provider_lookup() {
        // Two new providers, plus a duplicate atom for the same provider
        // and an installed one that must not be seeded.
        let entries = vec![
            {
                let mut e = new_entry(
                    "virtual",
                    "os-headers",
                    "1.0",
                    vec![
                        rdepend_edge("sys-kernel/headers", "sys-kernel", "headers"),
                        rdepend_edge(">=sys-kernel/headers-1", "sys-kernel", "headers"),
                        rdepend_edge("sys-kernel/oldheaders", "sys-kernel", "oldheaders"),
                    ],
                );
                e.outcome = PretendOutcome::AlreadyInstalled {
                    version: "1.0".into(),
                };
                e
            },
            new_entry("sys-kernel", "headers", "6.0", Vec::new()),
            {
                let mut e = new_entry(
                    "virtual",
                    "libc",
                    "1.0",
                    vec![
                        rdepend_edge("sys-kernel/headers", "sys-kernel", "headers"),
                        rdepend_edge("sys-libs/libc", "sys-libs", "libc"),
                    ],
                );
                e.outcome = PretendOutcome::AlreadyInstalled {
                    version: "1.0".into(),
                };
                e
            },
            new_entry("sys-libs", "other", "1.0", Vec::new()),
            new_entry("sys-libs", "libc", "2.0", Vec::new()),
            {
                let mut e = new_entry("sys-kernel", "oldheaders", "5.0", Vec::new());
                e.outcome = PretendOutcome::AlreadyInstalled {
                    version: "5.0".into(),
                };
                e
            },
        ];
        // os-headers first, then libc; the duplicate atom is deduped and
        // the installed provider is skipped.
        assert_eq!(seed_toolchain_asap(&entries), vec![1, 4]);
        // A reinstall provider is not seeded.
        let mut entries = entries;
        entries[1].outcome = PretendOutcome::Reinstall {
            version: "6.0".into(),
            changed_flags: vec!["flip".into()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        };
        assert_eq!(seed_toolchain_asap(&entries), vec![4]);
    }

    #[test]
    fn gather_deps_collects_the_filtered_closure_and_rejects_escapes() {
        let g = test_graph(4, &[(0, 1, prio(2)), (1, 2, prio(2)), (0, 3, prio(32))]);
        let mergeable: HashSet<usize> = [0, 1, 2, 3].into_iter().collect();
        assert_eq!(
            gather_deps(&g, 0, None, &mergeable),
            Some([0, 1, 2, 3].into_iter().collect())
        );
        // The optional child 3 escapes the mergeable set.
        let mergeable: HashSet<usize> = [0, 1, 2].into_iter().collect();
        assert_eq!(gather_deps(&g, 0, None, &mergeable), None);
        // The filter drops the escaping optional edge.
        assert_eq!(
            gather_deps(&g, 0, Some(n_ignore_optional), &mergeable),
            Some([0, 1, 2].into_iter().collect())
        );
        // A deeper escape is rejected too.
        let mergeable: HashSet<usize> = [0].into_iter().collect();
        assert_eq!(gather_deps(&g, 0, None, &mergeable), None, "1 escapes");
    }

    #[test]
    fn find_smallest_cycle_pins_the_ladder_and_smallest_pick() {
        // A three-ring (names "aa*") and a two-ring ("zz*"): the smaller
        // closure wins even though the bigger ring sorts first.
        let g = test_graph(
            5,
            &[
                (0, 1, prio(2)),
                (1, 2, prio(2)),
                (2, 0, prio(2)),
                (3, 4, prio(2)),
                (4, 3, prio(2)),
            ],
        );
        let entries = vec![
            new_entry("dev-libs", "aa1", "1.0", Vec::new()),
            new_entry("dev-libs", "aa2", "1.0", Vec::new()),
            new_entry("dev-libs", "aa3", "1.0", Vec::new()),
            new_entry("dev-libs", "zz1", "1.0", Vec::new()),
            new_entry("dev-libs", "zz2", "1.0", Vec::new()),
        ];
        let mut frontier = SerializeFrontier::build(&g);
        let (sub, ig) = find_smallest_cycle(
            &g,
            Some(&mut frontier),
            &entries,
            &NORMAL,
            &[],
            true,
            &HashSet::new(),
        )
        .expect("a mergeable cycle exists");
        assert_eq!(sub, HashSet::from([3usize, 4]));
        assert!(ig.is_some());
        assert_eq!(
            ig.unwrap()(&prio(4)),
            n_ignore_runtime_post(&prio(4)),
            "the lowest relaxed rung that produces a leaf"
        );
    }

    #[test]
    fn harvest_cycle_pins_leaf_order_and_installed_preference() {
        // 0 -> 1 runtime; 1 has no children, so it is the first leaf and
        // 0 follows.
        let g = test_graph(2, &[(0, 1, prio(2))]);
        let sub: HashSet<usize> = [0, 1].into_iter().collect();
        assert_eq!(harvest_cycle(&g, &sub), vec![1, 0]);
        // An installed leaf is preferred over a merge-bound one.
        let mut g = test_graph(2, &[]);
        g.installed[0] = true;
        let sub: HashSet<usize> = [0, 1].into_iter().collect();
        assert_eq!(
            harvest_cycle(&g, &sub),
            vec![0, 1],
            "the installed leaf is picked first"
        );
    }

    #[test]
    fn cycle_report_reports_the_ring_and_its_requirer_cone() {
        let mut b = new_entry(
            "dev-libs",
            "b",
            "1.0",
            vec![runtime_edge("dev-libs/a", "dev-libs", "a")],
        );
        b.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
        let entries = vec![
            new_entry(
                "dev-libs",
                "a",
                "1.0",
                vec![runtime_edge("dev-libs/b", "dev-libs", "b")],
            ),
            b,
            new_entry("dev-libs", "owner", "1.0", Vec::new()),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let (cycles, display) = cycle_report(&entries, &["dev-libs/owner".to_string()], root);
        assert!(
            !cycles.is_empty(),
            "the runtime ring is recorded: {cycles:?}"
        );
        assert_eq!(
            display.len(),
            3,
            "the owner's cone joins the drain: {display:?}"
        );
        assert_eq!(
            display.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([0, 1, 2])
        );
    }

    #[test]
    fn tree_schedule_stuck_pins_guards_and_node_bookkeeping() {
        let mut walked = new_entry("dev-libs", "walkedinst", "1.0", Vec::new());
        walked.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        walked.slot = Some("0".into());
        let mut otherinst = new_entry("dev-libs", "otherinst", "1.0", Vec::new());
        otherinst.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        otherinst.slot = Some("0".into());
        let entries = vec![
            new_entry("dev-libs", "bparent", "1.1", Vec::new()),
            new_entry("dev-libs", "other", "2.0", Vec::new()),
            walked,
            otherinst,
        ];
        let mut g = test_graph(2, &[]);
        let row = |repl: usize, cp: (&str, &str), strong: bool| PendingUninstall {
            repl,
            inst_cp: (cp.0.to_string(), cp.1.to_string()),
            inst_slot: "0".to_string(),
            inst_version: "1.0".to_string(),
            owner: 1,
            blocker: 0,
            strong,
            node: None,
        };
        let mut ts = TreeStuck {
            pending: vec![
                // Walked installed instance: skipped.
                row(2, ("dev-libs", "walkedinst"), false),
                // Strong row: hidden.
                row(0, ("dev-libs", "blocked"), true),
                // Eligible row: schedules.
                row(0, ("dev-libs", "blocked"), false),
            ],
            scheduled: HashSet::new(),
            solved: Vec::new(),
            progressed: false,
        };
        tree_schedule_stuck(&mut g, &entries, &mut ts);
        assert!(
            ts.pending[0].node.is_none(),
            "a walked installed instance is skipped"
        );
        assert!(ts.pending[1].node.is_none(), "a strong row stays hidden");
        let node = ts.pending[2].node.expect("the soft eligible row schedules");
        assert_eq!(g.n, 3);
        assert_eq!(g.parents[node], vec![0]);
        assert!(g.installed[node]);
        assert!(ts.scheduled.contains(&node));
        assert!(ts.progressed);
        // A second call does not reschedule the same row.
        ts.progressed = false;
        tree_schedule_stuck(&mut g, &entries, &mut ts);
        assert_eq!(g.n, 3);
        assert!(!ts.progressed);
    }

    #[test]
    fn tree_note_selection_pins_the_verdict_lifecycle() {
        let entries = vec![
            new_entry("dev-libs", "repl", "1.0", Vec::new()),
            new_entry("dev-libs", "other", "1.0", Vec::new()),
        ];
        let pending = |node: Option<usize>, scheduled: bool| {
            let mut ts = TreeStuck {
                pending: vec![PendingUninstall {
                    repl: 0,
                    inst_cp: ("dev-libs".to_string(), "inst".to_string()),
                    inst_slot: "0".to_string(),
                    inst_version: "1.0".to_string(),
                    owner: 1,
                    blocker: 0,
                    strong: false,
                    node,
                }],
                scheduled: HashSet::new(),
                solved: Vec::new(),
                progressed: false,
            };
            if scheduled {
                ts.scheduled.insert(node.unwrap());
            }
            ts
        };
        // Selecting the synthetic uninstall records the verdict once.
        let mut g = test_graph(2, &[]);
        let mut ts = pending(Some(1), true);
        tree_note_selection(&mut g, &entries, &mut ts, 1);
        assert_eq!(ts.solved, vec![(1, 0)]);
        assert!(g.alive[1], "the uninstall selection leaves the node alive");
        tree_note_selection(&mut g, &entries, &mut ts, 1);
        assert_eq!(ts.solved, vec![(1, 0)], "the verdict is not duplicated");
        // Selecting the replacement kills the synthetic node and solves.
        let mut g = test_graph(2, &[]);
        let mut ts = pending(Some(1), true);
        tree_note_selection(&mut g, &entries, &mut ts, 0);
        assert!(
            !g.alive[1],
            "the replacement's merge removes the uninstall node"
        );
        assert_eq!(ts.solved, vec![(1, 0)]);
        // An unscheduled pending node is not touched.
        let mut g = test_graph(2, &[]);
        let mut ts = pending(Some(1), false);
        tree_note_selection(&mut g, &entries, &mut ts, 1);
        assert!(ts.solved.is_empty(), "an unscheduled row records nothing");
    }

    #[test]
    fn select_nodes_pins_the_greedy_batch_and_root_leaf_order() {
        // 1 -> 2, plus the parentless leaf 0: at rung NONE the batch
        // pops both leaves in order; the one-at-a-time path would pick
        // the parented 2 first.
        let entries: Vec<GraphEntry> = (0..3)
            .map(|i| new_entry("dev-libs", &format!("p{i}"), "1.0", Vec::new()))
            .collect();
        let mut g = test_graph(3, &[(1, 2, prio(2))]);
        let root = Path::new("/nonexistent-root-for-unit-test");
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(order, vec![0, 2, 1], "greedy rung-NONE pops both leaves");
    }

    #[test]
    fn select_nodes_harvests_a_runtime_cycle() {
        // 0 <-> 1 runtime plus 2 -> 0 runtime: no NORMAL rung leaf
        // exists, so the cycle handler harvests the ring (preferring the
        // installed node) and the requirer drains afterwards.
        let entries: Vec<GraphEntry> = (0..3)
            .map(|i| new_entry("dev-libs", &format!("c{i}"), "1.0", Vec::new()))
            .collect();
        let mut g = test_graph(3, &[(0, 1, prio(2)), (1, 0, prio(2)), (2, 0, prio(2))]);
        g.installed[1] = true;
        let root = Path::new("/nonexistent-root-for-unit-test");
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(
            order,
            vec![1, 0, 2],
            "the ring harvest prefers the installed leaf, then the requirer"
        );
    }

    #[test]
    fn select_nodes_escalates_to_the_satisfied_range() {
        // A ring of satisfied buildtime edges: NORMAL's cycle search has
        // no mergeable leaf, so the `drop_satisfied` escalation to
        // `DepPrioritySatisfiedRange` is what breaks the ring.
        let entries: Vec<GraphEntry> = (0..2)
            .map(|i| new_entry("dev-libs", &format!("s{i}"), "1.0", Vec::new()))
            .collect();
        let mut g = test_graph(2, &[(0, 1, prio(65)), (1, 0, prio(65))]);
        g.installed[1] = true;
        let root = Path::new("/nonexistent-root-for-unit-test");
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(
            order,
            vec![1, 0],
            "the satisfied-range harvest prefers the installed leaf"
        );
    }

    #[test]
    fn serialize_merge_order_returns_a_permutation_in_dependency_order() {
        let root = Path::new("/nonexistent-root-for-unit-test");
        let config = portage_profile::Config::default();
        let entries = vec![
            new_entry(
                "dev-libs",
                "top",
                "1.0",
                vec![runtime_edge("dev-libs/dep", "dev-libs", "dep")],
            ),
            new_entry("dev-libs", "dep", "1.0", Vec::new()),
            new_entry("dev-libs", "isolated", "1.0", Vec::new()),
        ];
        let order = serialize_merge_order(
            &entries,
            &["dev-libs/top".to_string()],
            &config,
            root,
            true,
            &[],
            true,
        );
        assert_eq!(
            order.len(),
            3,
            "every entry is scheduled exactly once: {order:?}"
        );
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![0, 1, 2],
            "a permutation of the entries: {order:?}"
        );
        assert!(
            order.iter().position(|&i| i == 1) < order.iter().position(|&i| i == 0),
            "the dependency merges before its owner: {order:?}"
        );
    }

    #[test]
    fn schedule_graph_prunes_rootless_synthetic_nodes_and_gates_complete_mode() {
        let root = mo_tmp_vdb(
            "schedule",
            &[
                ("dev-libs", "sysseed-1.0", &[("SLOT", "0")]),
                (
                    "dev-libs",
                    "dep-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "dev-libs/grand")],
                ),
                ("dev-libs", "grand-1.0", &[("SLOT", "0")]),
            ],
        );
        let repos: &[RepoConfig] = &[];
        let config = portage_profile::Config {
            system_packages: vec!["dev-libs/sysseed".to_string()],
            ..portage_profile::Config::default()
        };
        // An all-New resolve is virtuals_only: no installed closure.
        let entries = vec![new_entry("dev-libs", "top", "1.0", Vec::new())];
        let (ext, g, real_n, _) = schedule_graph(
            &entries,
            &["dev-libs/top".to_string()],
            &config,
            &root,
            true,
            repos,
            true,
        );
        assert_eq!(
            ext.len(),
            1,
            "no complete-mode closure for an all-New resolve"
        );
        assert_eq!(real_n, 1);
        assert!(g.order.iter().all(|&i| i < real_n));
        // A Reinstall entry turns complete mode on: the @system seed is
        // added, walked, and then pruned as a rootless nomerge node.
        let mut top = new_entry("dev-libs", "top", "1.0", Vec::new());
        top.outcome = PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: vec!["flip".into()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        };
        let entries = vec![top];
        let config = portage_profile::Config {
            system_packages: vec!["dev-libs/sysseed".to_string()],
            ..portage_profile::Config::default()
        };
        let (ext, g, real_n, _) = schedule_graph(
            &entries,
            &["dev-libs/top".to_string()],
            &config,
            &root,
            true,
            repos,
            true,
        );
        assert_eq!(real_n, 1);
        let seed = ext
            .iter()
            .position(|e| e.package == "sysseed")
            .expect("the @system seed is added");
        assert!(seed >= real_n);
        assert!(!g.alive[seed], "a rootless synthetic node is pruned");
        assert!(!g.order.contains(&seed));
    }

    #[test]
    fn tree_solved_replacements_skips_a_same_version_row() {
        // A Replacement row whose merge-bound entry resolves at the same
        // version as the matched installed one is not a pending row.
        let mut owner = new_entry("dev-libs", "bparent", "1.1", Vec::new());
        owner.slot = Some("0".into());
        owner.sub_slot = Some("0".into());
        owner.blockers = vec![crate::BlockerConflict {
            atom_str: "!<dev-libs/blocked-2.0".to_string(),
            strong: false,
            matched_category: "dev-libs".to_string(),
            matched_package: "blocked".to_string(),
            matched_version: "1.0".to_string(),
            unsolvable: false,
            satisfied_by: Some(crate::BlockerSatisfiedBy::Replacement {
                cp: ("dev-libs".to_string(), "blocked".to_string()),
                slot: "0".to_string(),
            }),
            tree_scheduled_uninstall: false,
        }];
        let mut replacement = new_entry(
            "dev-libs",
            "blocked",
            "1.0",
            vec![runtime_edge("dev-libs/bparent", "dev-libs", "bparent")],
        );
        replacement.slot = Some("0".into());
        replacement.sub_slot = Some("0".into());
        let other = new_entry("dev-libs", "other", "1.0", Vec::new());
        let entries = vec![owner, replacement, other];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let config = portage_profile::Config::default();
        let solved = tree_solved_replacements(
            &entries,
            &["dev-libs/blocked".to_string()],
            &config,
            root,
            true,
            &[],
            true,
        );
        assert!(
            solved.is_empty(),
            "the same-version entry is not a replacement row: {solved:?}"
        );
    }
    // -----------------------------------------------------------------
    // #144 follow-up: the survivors of the first cluster run
    // -----------------------------------------------------------------

    #[test]
    fn suppressed_alt_edges_requires_every_atom_of_a_branch_to_match() {
        let runtime = DepPriority {
            runtime: true,
            ..DepPriority::default()
        };
        let alt_edge = |atom: &str, cat: &str, pkg: &str, branch: u32| {
            let mut e = plain_edge(atom, cat, pkg, runtime);
            e.disjunctive = true;
            e.alt = Some((0, branch));
            e.key = 3;
            e
        };
        // Branch 0 is only *partly* installed (one installed atom, one
        // merge-bound); branch 1 is fully installed. The all-installed
        // bin must pick branch 1.
        let mut i1 = new_entry("dev-libs", "i1", "1.0", Vec::new());
        i1.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let mut i2 = new_entry("dev-libs", "i2", "1.0", Vec::new());
        i2.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let mut i3 = new_entry("dev-libs", "i3", "1.0", Vec::new());
        i3.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let owner = new_entry(
            "app-misc",
            "owner",
            "1.0",
            vec![
                alt_edge("dev-libs/i1", "dev-libs", "i1", 0),
                alt_edge("dev-libs/m1", "dev-libs", "m1", 0),
                alt_edge("dev-libs/i2", "dev-libs", "i2", 1),
                alt_edge("dev-libs/i3", "dev-libs", "i3", 1),
            ],
        );
        let entries = vec![
            owner,
            i1,
            new_entry("dev-libs", "m1", "1.0", Vec::new()),
            i2,
            i3,
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        assert_eq!(
            kept_alt_branches(&entries, root)[0],
            HashSet::from([2usize, 3]),
            "only a branch whose every atom matches installed wins the all-installed bin"
        );
        // A branch whose second atom matches nothing at all is not an
        // `all_any` candidate; with no other branch resolving, nothing
        // is suppressed.
        let owner = new_entry(
            "app-misc",
            "owner",
            "1.0",
            vec![
                alt_edge("dev-libs/i1", "dev-libs", "i1", 0),
                alt_edge("dev-libs/absent", "dev-libs", "absent", 0),
                alt_edge("dev-libs/absent2", "dev-libs", "absent2", 1),
            ],
        );
        let mut i1 = new_entry("dev-libs", "i1", "1.0", Vec::new());
        i1.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let entries = vec![owner, i1];
        assert_eq!(
            kept_alt_branches(&entries, root)[0],
            HashSet::from([0usize, 1, 2]),
            "no branch fully resolves, so every branch is kept"
        );
    }

    #[test]
    fn elementary_cycles_records_only_the_shortest_path_per_node() {
        // Node 0's first child leads to a length-3 path back to 0, its
        // second child to a length-2 one: only the shortest is recorded.
        let g = test_graph(
            4,
            &[
                (0, 1, prio(2)),
                (1, 2, prio(2)),
                (2, 0, prio(2)),
                (0, 3, prio(2)),
                (3, 0, prio(2)),
            ],
        );
        let cycles = elementary_cycles(&g, None);
        let to_zero: Vec<Vec<usize>> = cycles
            .iter()
            .filter(|c| c.last() == Some(&0))
            .cloned()
            .collect();
        assert_eq!(
            to_zero,
            vec![vec![3usize, 0]],
            "the longer tied candidate is dropped once a shorter one appears"
        );
    }

    #[test]
    fn find_smallest_cycle_keeps_the_first_of_two_equal_rings() {
        // A 3-ring ("aa*") then two 2-rings ("mm*", "zz*"): the first
        // smallest closure wins; `<=` would replace it with the later
        // equal-size one.
        let g = test_graph(
            7,
            &[
                (0, 1, prio(2)),
                (1, 2, prio(2)),
                (2, 0, prio(2)),
                (3, 4, prio(2)),
                (4, 3, prio(2)),
                (5, 6, prio(2)),
                (6, 5, prio(2)),
            ],
        );
        let entries = vec![
            new_entry("dev-libs", "aa1", "1.0", Vec::new()),
            new_entry("dev-libs", "aa2", "1.0", Vec::new()),
            new_entry("dev-libs", "aa3", "1.0", Vec::new()),
            new_entry("dev-libs", "mm1", "1.0", Vec::new()),
            new_entry("dev-libs", "mm2", "1.0", Vec::new()),
            new_entry("dev-libs", "zz1", "1.0", Vec::new()),
            new_entry("dev-libs", "zz2", "1.0", Vec::new()),
        ];
        let mut frontier = SerializeFrontier::build(&g);
        let (sub, _) = find_smallest_cycle(
            &g,
            Some(&mut frontier),
            &entries,
            &NORMAL,
            &[],
            true,
            &HashSet::new(),
        )
        .expect("a mergeable cycle exists");
        assert_eq!(
            sub,
            HashSet::from([3usize, 4]),
            "the first of the equal-size rings is kept"
        );
    }

    #[test]
    fn select_nodes_promotes_an_unsatisfied_pdep_child_to_asap() {
        // virtual/libc is installed and seeds its provider P as asap; P
        // is selected at the relaxed rung that ignores its runtime_post
        // edge, and its still alive unsatisfied PDEPEND child C is
        // promoted ahead of the bias order (which would batch D first).
        let mut virt = new_entry(
            "virtual",
            "libc",
            "1.0",
            vec![
                rdepend_edge("sys-libs/libc-prov", "sys-libs", "libc-prov"),
                runtime_edge("dev-libs/other", "dev-libs", "other"),
            ],
        );
        virt.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let entries = vec![
            virt,
            new_entry(
                "sys-libs",
                "libc-prov",
                "1.0",
                vec![{
                    let mut e = runtime_edge("dev-libs/consumer", "dev-libs", "consumer");
                    e.priority = DepPriority {
                        runtime_post: true,
                        ..DepPriority::default()
                    };
                    e
                }],
            ),
            new_entry("dev-libs", "consumer", "1.0", Vec::new()),
            new_entry("dev-libs", "other", "1.0", Vec::new()),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let mut g = build_digraph(&entries, &["virtual/libc".to_string()], root);
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(
            order,
            vec![1, 2, 3, 0],
            "the promoted PDEPEND child merges before the unrelated dep"
        );
    }
    // -----------------------------------------------------------------
    // #144 fix pass: the reviewer's counterexamples
    // -----------------------------------------------------------------

    #[test]
    fn select_nodes_prefers_a_leaf_whose_parent_is_an_asap_node() {
        // The asap block cannot select (every asap node has a child that
        // survives all SATISFIED rungs), and the cycle search's asap-only
        // candidate set is empty, so `prefer_asap` flips and the rung
        // loop reaches the asap-parent preference: leaf 4 (parent 1 is
        // asap) wins over 2/3, while a mutated gate/predicate picks 2 or
        // 3 instead.
        let entries = vec![
            new_entry(
                "virtual",
                "libc",
                "1.0",
                vec![rdepend_edge("sys-libs/prov", "sys-libs", "prov")],
            ),
            new_entry(
                "sys-libs",
                "prov",
                "1.0",
                vec![
                    plain_edge(
                        "dev-libs/x",
                        "dev-libs",
                        "x",
                        DepPriority {
                            buildtime: true,
                            ..DepPriority::default()
                        },
                    ),
                    runtime_edge("dev-libs/leaf", "dev-libs", "leaf"),
                    {
                        let mut e = runtime_edge("dev-libs/npost", "dev-libs", "npost");
                        e.priority = DepPriority {
                            runtime_post: true,
                            ..DepPriority::default()
                        };
                        e
                    },
                ],
            ),
            new_entry("dev-libs", "bparented", "1.0", Vec::new()),
            new_entry("dev-libs", "npost", "1.0", Vec::new()),
            new_entry("dev-libs", "leaf", "1.0", Vec::new()),
            new_entry("dev-libs", "x", "1.0", Vec::new()),
            new_entry(
                "dev-libs",
                "cparent",
                "1.0",
                vec![runtime_edge("dev-libs/bparented", "dev-libs", "bparented")],
            ),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let mut g = test_graph(
            7,
            &[
                (0, 1, prio(1)),
                (1, 5, prio(1)),
                (1, 4, prio(2)),
                (1, 3, prio(4)),
                (6, 2, prio(2)),
            ],
        );
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(
            order,
            vec![4, 5, 1, 0, 3, 2, 6],
            "the asap-parent preference picks leaf 4 before the fallback would"
        );
    }

    #[test]
    fn select_nodes_promotes_only_an_unsatisfied_pdep_child() {
        // The provider is selected at rung 1 (its child's edge is
        // optional), but that child is already "satisfied" for the
        // promotion predicate (`optional` returns true), so it must NOT
        // be promoted; `||` in the predicate promotes it and reorders
        // the run.
        let mut virt = new_entry(
            "virtual",
            "libc",
            "1.0",
            vec![rdepend_edge("sys-libs/prov", "sys-libs", "prov")],
        );
        virt.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let entries = vec![
            virt,
            new_entry(
                "sys-libs",
                "prov",
                "1.0",
                vec![plain_edge(
                    "dev-libs/consumer",
                    "dev-libs",
                    "consumer",
                    DepPriority {
                        buildtime: true,
                        optional: true,
                        ..DepPriority::default()
                    },
                )],
            ),
            new_entry("dev-libs", "consumer", "1.0", Vec::new()),
            new_entry("dev-libs", "other", "1.0", Vec::new()),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let mut g = build_digraph(&entries, &["virtual/libc".to_string()], root);
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(
            order,
            vec![1, 0, 2, 3],
            "an already-satisfied optional PDEPEND child is not promoted"
        );
    }

    #[test]
    fn add_installed_dependency_closure_keeps_the_first_of_vercmp_equal_versions() {
        // `1.0` and `1.0-r0` compare equal (a missing revision is 0), so
        // `pick_installed`'s `best` scan must keep the first installed
        // version it saw rather than replace it on the equality.
        let root = mo_tmp_vdb(
            "closure-vercmp-eq",
            &[
                (
                    "dev-libs",
                    "eqver-1.0",
                    &[("SLOT", "0"), ("RDEPEND", "dev-libs/leaf")],
                ),
                ("dev-libs", "eqver-1.0-r0", &[("SLOT", "1")]),
                ("dev-libs", "leaf-1.0", &[("SLOT", "0")]),
            ],
        );
        let mut entries = vec![new_entry(
            "app-misc",
            "top",
            "1.0",
            vec![runtime_edge("dev-libs/eqver", "dev-libs", "eqver")],
        )];
        add_installed_dependency_closure(&mut entries, &root, &[], &[], false, true);
        let picked = entries
            .iter()
            .find(|e| e.package == "eqver")
            .expect("the installed dependency is seeded");
        assert_eq!(
            outcome_version(picked),
            Some("1.0"),
            "the first vercmp-equal installed version is kept"
        );
    }
    #[test]
    fn select_nodes_escalates_to_the_satisfied_range_before_the_roots() {
        // The reviewer's counterexample for `drop_satisfied && !ptr::eq`
        // -> `||`: the early SATISFIED escalation steals the selection
        // from the roots-last-resort fallback that runs in the same
        // iteration.
        let entries = vec![
            new_entry("virtual", "libc", "1.0", Vec::new()),
            new_entry("dev-libs", "p1", "1.0", Vec::new()),
            new_entry("dev-libs", "p2", "1.0", Vec::new()),
            new_entry("dev-libs", "p3", "1.0", Vec::new()),
            new_entry("dev-libs", "p4", "1.0", Vec::new()),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let mut g = test_graph(5, &[(0, 4, prio(1)), (3, 2, prio(64)), (4, 0, prio(8))]);
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(
            order,
            vec![2, 1, 3, 4, 0],
            "the roots fallback keeps its turn"
        );
    }
    #[test]
    fn select_nodes_pins_the_normal_range_cycle_pass() {
        // The reviewer's counterexample for deleting the `!` on
        // `!ptr::eq(range, &NORMAL)`: when the range is already SATISFIED
        // the mutant drops the NORMAL cycle pass instead of adding it.
        let mut virt = new_entry(
            "virtual",
            "libc",
            "1.0",
            vec![rdepend_edge("sys-libs/p1", "sys-libs", "p1")],
        );
        virt.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        };
        let entries = vec![
            virt,
            new_entry("sys-libs", "p1", "1.0", Vec::new()),
            new_entry("dev-libs", "p2", "1.0", Vec::new()),
            new_entry("dev-libs", "p3", "1.0", Vec::new()),
            new_entry("dev-libs", "p4", "1.0", Vec::new()),
            new_entry("dev-libs", "p5", "1.0", Vec::new()),
        ];
        let root = Path::new("/nonexistent-root-for-unit-test");
        let mut g = test_graph(
            6,
            &[
                (1, 2, prio(6)),
                (1, 4, prio(6)),
                (2, 0, prio(4)),
                (2, 1, prio(32)),
                (3, 5, prio(64)),
                (4, 0, prio(6)),
                (4, 2, prio(33)),
                (4, 5, prio(1)),
            ],
        );
        let order = select_nodes(&mut g, &entries, root, None);
        assert_eq!(order, vec![1, 0, 5, 2, 3, 4], "the NORMAL cycle pass runs");
    }
}
