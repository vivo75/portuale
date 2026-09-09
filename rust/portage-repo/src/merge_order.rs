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

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::{
    CandidateSource, GraphEntry, PretendOutcome, VisibilityProvenance, all_installed_packages,
    read_vdb_flag_set, read_vdb_slot, read_vdb_string,
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
fn add_installed_dependency_closure(
    entries: &mut Vec<GraphEntry>,
    root: &Path,
    system_atoms: &[String],
    virtuals_only: bool,
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
    let by_cp: HashMap<(&str, &str), &crate::InstalledPackage> = installed
        .iter()
        .map(|p| ((p.category.as_str(), p.package.as_str()), p))
        .collect();

    // Real's own `_serialize_tasks` digraph does NOT strip a package's
    // recorded libc dependency (`strip_libc_deps` is `--changed-deps`
    // only) -- but with `--dynamic-deps` on (the default), it walks the
    // *current ebuild*'s deps, not the vdb's. The difference that matters
    // here is `portage.package.ebuild.doebuild._inject_libc_dep`: every
    // package portage installs gets a bare `>=<libc-provider>-<version>`
    // appended to its vdb `RDEPEND` (bug #753500), which the ebuild never
    // declared. `dev-libs/gmp` -- ebuild `RDEPEND=""` -- ends up with
    // `RDEPEND=">=sys-libs/glibc-2.43-r2"` in the vdb, and that phantom
    // `gmp -> glibc (runtime)` edge held `gmp`/`mpfr`/`mpc` behind
    // `glibc`'s deep subtree so portuale's NORMAL frontier ran dry ~11
    // merges before real's. Strip exactly that injected shape -- a bare
    // `>=` atom on a libc provider with no slot and no USE deps -- and
    // nothing else: a genuine `sys-libs/glibc[-crypt(-)]` (has USE deps,
    // e.g. `sys-libs/libxcrypt`) is kept, exactly as real keeps it.
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
        let mut md: HashMap<String, String> = HashMap::new();
        for k in ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            let s = read_vdb_string(root, cat, pkg, ver, k);
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
        .filter(|e| !is_injected_libc(&e.atom))
        .collect()
    };

    let mut present: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
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
    let expandable = |cat: &str| !virtuals_only || cat == "virtual";
    let add_node = |entries: &mut Vec<GraphEntry>,
                    present: &mut HashSet<(String, String)>,
                    queue: &mut std::collections::VecDeque<usize>,
                    key: (String, String)| {
        if !present.insert(key.clone()) {
            return;
        }
        let Some(p) = by_cp.get(&(key.0.as_str(), key.1.as_str())) else {
            return;
        };
        entries.push(synthetic_installed_entry(
            p.category.clone(),
            p.package.clone(),
            p.version.clone(),
            Vec::new(),
        ));
        if expandable(&key.0) {
            queue.push_back(entries.len() - 1);
        }
    };

    let seed_targets: Vec<(String, String)> = entries
        .iter()
        .flat_map(|e| e.deps.iter())
        .map(|d| (d.category.clone(), d.package.clone()))
        .collect();
    for key in seed_targets {
        add_node(entries, &mut present, &mut queue, key);
    }

    // Seed 1b (complete mode only): real `_complete_graph` seeds a
    // `SetArg` for `@world`/`@system` and adds every atom in them, then
    // walks the lot deep. Add each installed `@system` package as a node
    // and queue its own dep walk. `serialize_merge_order`'s prune drops
    // the ones no merge-bound package reaches -- but the deep-only
    // members (`dev-libs/gmp` under `glibc` -> `gcc`, `sys-libs/readline`
    // under `bash`, ...) stay, exactly as real's own post-prune graph
    // keeps them.
    if !virtuals_only {
        for atom_str in system_atoms {
            let Some(atom) = portage_dep::parse_atom(atom_str) else {
                continue;
            };
            add_node(
                entries,
                &mut present,
                &mut queue,
                (atom.category, atom.package),
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
                (e.category.clone(), e.package.clone()),
            );
        }
        entries[i].deps = edges;
    }
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

    let mut g = Digraph {
        n,
        children: vec![Vec::new(); n],
        parents: vec![Vec::new(); n],
        order: Vec::new(),
        installed,
        alive: vec![true; n],
    };

    // Real `mypriority.satisfied`: an installed package matching the
    // atom. For a `:=` atom real additionally narrows the match to the
    // resolved child's own slot/sub-slot, so a sub-slot bump doesn't
    // read as satisfied -- `entry_slot` supplies that.
    let installed_by_cp = installed_candidates_by_cp(root);
    let satisfied = |edge: &DepEdge, child: Option<usize>| -> bool {
        let Some(cands) = installed_by_cp.get(&(edge.category.clone(), edge.package.clone()))
        else {
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
            let Some(ci) = child else { return true };
            let (Some(slot), Some(sub_slot)) =
                (entries[ci].slot.as_deref(), entries[ci].sub_slot.as_deref())
            else {
                return true;
            };
            return matched.iter().any(|m| {
                portage_dep::parse_candidate(m).is_some_and(|c| {
                    c.slot.as_deref() == Some(slot) && c.sub_slot.as_deref() == Some(sub_slot)
                })
            });
        }
        true
    };

    // Real `_create_graph` resolves every dep atom to a single package
    // (`_select_pkg_highest_available`) before `_add_pkg` records the
    // edge. Portuale looked each atom's `cat/pkg` up in `cp_indices` and
    // connected it to *every* scheduled instance of that `cp` -- so a
    // slot-qualified atom (`app-text/docbook-sgml-dtd:3.0`) wrongly
    // gained an edge to a sibling slot also being merged, and the extra
    // parent skewed `_merge_order_bias`'s parent-count ordering. Narrow
    // every edge to the entries its atom actually matches (version /
    // slot / repo); keep the edge whenever the candidate string can't be
    // built or the atom won't parse, so this only ever removes a
    // provably-wrong edge.
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
    let edge_matches = |atom: &str, j: usize| -> bool {
        let Some(cand) = entry_candidate[j].as_deref() else {
            return true;
        };
        match portage_dep::match_from_list(atom, &[cand]) {
            Some(m) => !m.is_empty(),
            None => true,
        }
    };

    // Real `dep_zapdeps` (`dep_check.py`): a `|| ( … )` group resolves to
    // one alternative, not all. Portuale keeps every branch's atoms in
    // `GraphEntry::deps` and picks here, per `(key, group)`, following
    // real's `choice_bins` ordering: the first branch (written order)
    // *all* of whose atoms already match a merge-bound graph node
    // (`preferred_in_graph`, and real's line-793 promotion of the
    // all-in-graph choice ahead of an all-installed one in the same
    // bin), else the first all of whose atoms match an installed entry
    // (`preferred_installed`), else the first all of whose atoms match
    // anything at all. Every other branch's `deps` index is then
    // suppressed from the discovery walk and the edge loop. If *no*
    // branch fully resolves, nothing is suppressed (keep the
    // over-inclusive stopgap).
    let alt_suppressed: Vec<HashSet<usize>> = entries
        .iter()
        .map(|e| {
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
                    if let Some(idxs) =
                        cp_indices.get(&(edge.category.as_str(), edge.package.as_str()))
                    {
                        for &j in idxs {
                            if edge_matches(&edge.atom, j) {
                                any_m = true;
                                if g.installed[j] {
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
        .collect();

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
            let Some(idxs) = cp_indices.get(&(edge.category.as_str(), edge.package.as_str()))
            else {
                continue;
            };
            for &j in idxs {
                if !edge_matches(&edge.atom, j) {
                    continue;
                }
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
/// compares after `cp`.
pub(crate) fn entry_version(e: &GraphEntry) -> Option<&str> {
    match &e.outcome {
        PretendOutcome::New { version }
        | PretendOutcome::Reinstall { version, .. }
        | PretendOutcome::AlreadyInstalled { version } => Some(version),
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
    entries: &[GraphEntry],
    range: &PriorityRange,
    asap: &[usize],
    prefer_asap: bool,
) -> Option<(HashSet<usize>, Option<Ignore>)> {
    let mergeable: HashSet<usize> = g.leaf_nodes(range.ig_medium()).into_iter().collect();
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
/// Returns the alive nodes in scheduling order (installed "nomerge"
/// nodes included -- the caller drops them).
fn select_nodes(g: &mut Digraph, entries: &[GraphEntry], root: &Path) -> Vec<usize> {
    let mut retlist: Vec<usize> = Vec::new();
    let mut asap: Vec<usize> = seed_toolchain_asap(entries);
    let mut prefer_asap = true;
    let mut drop_satisfied = false;

    while g.order.iter().any(|&i| g.alive[i]) {
        let mut selected: Option<Vec<usize>> = None;
        let mut used_ig: Option<Ignore> = None;
        let asap_active = prefer_asap && !asap.is_empty();
        let range: &PriorityRange = if asap_active { &SATISFIED } else { &NORMAL };

        if asap_active {
            asap.retain(|&i| g.alive[i]);
            'asap: for i in 1..=range.medium_soft {
                let ig = range.ig(i);
                for (pos, &node) in asap.iter().enumerate() {
                    if g.is_leaf(node, ig) {
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
                let nodes = g.leaf_nodes(ig);
                if nodes.is_empty() {
                    continue;
                }
                // Real: "Greedily pop all of these nodes since no
                // relationship has been ignored." Real additionally
                // gates this on `not tree_mode`, because the batch
                // "destroys --tree output" -- portuale's `--tree`
                // re-derives its own nesting from `required_by` top-down
                // instead of consuming the serialized list, so that gate
                // has nothing to protect here and is deliberately not
                // ported (see `pretend.rs::print_tree`).
                if nodes.len() == 1 || (ig.is_none() && asap.is_empty()) {
                    selected = Some(nodes);
                } else {
                    // "For optimal merge order: only pop one node;
                    // removing a root node (node without a parent) will
                    // not produce a leaf node, so avoid it." Real first
                    // prefers a node whose parent is itself an asap node.
                    let mut picked = None;
                    if !asap.is_empty() {
                        picked = nodes.iter().copied().find(|&node| {
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
                        picked = nodes.iter().copied().find(|&node| g.has_parents(node));
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
                if let Some((sub, ig)) = find_smallest_cycle(g, entries, lr, &asap, prefer_asap) {
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
            let roots: Vec<usize> = g
                .order
                .iter()
                .copied()
                .filter(|&i| g.alive[i] && g.is_leaf(i, None))
                .collect();
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

        prefer_asap = true;
        drop_satisfied = false;
        for i in selected {
            if !g.alive[i] {
                continue;
            }
            g.alive[i] = false;
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
pub(crate) fn serialize_merge_order(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    config: &portage_profile::Config,
    root: &Path,
    implicit_system_deps: bool,
) -> Vec<usize> {
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
    add_installed_dependency_closure(&mut ext, root, &config.system_packages, !complete);
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
    let scheduled: Vec<usize> = select_nodes(&mut g, entries, root)
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
