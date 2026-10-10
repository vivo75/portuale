//! Dependency priorities, the edges they label and the ignore rungs.

use crate::merge_order::*;

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
pub(crate) fn split_disjunctive(tokens: &[String]) -> (Vec<String>, Vec<(String, u32, u32)>) {
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
pub(crate) fn key_priority(key: &str, built: bool) -> DepPriority {
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

/// One `ignore_priority` filter: a rung of `NORMAL` (`N*`) or
/// `SATISFIED` (`S*`). An enum, not a `fn` pointer, so a filter has one
/// identity whatever the codegen (function addresses are not unique
/// across codegen units).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ignore {
    NOptional,
    NRuntimePost,
    NRuntime,
    SOptional,
    SSatisfiedRuntimePost,
    SRuntimePost,
    SSatisfiedRuntime,
    SSatisfiedBuildtime,
    SSatisfiedBuildtimeSlotOp,
    SRuntime,
}

impl Ignore {
    /// Whether the filter drops an edge with priority `p`.
    pub(crate) fn ignores(self, p: &DepPriority) -> bool {
        match self {
            Self::NOptional => n_ignore_optional(p),
            Self::NRuntimePost => n_ignore_runtime_post(p),
            Self::NRuntime => n_ignore_runtime(p),
            Self::SOptional => s_ignore_optional(p),
            Self::SSatisfiedRuntimePost => s_ignore_satisfied_runtime_post(p),
            Self::SRuntimePost => s_ignore_runtime_post(p),
            Self::SSatisfiedRuntime => s_ignore_satisfied_runtime(p),
            Self::SSatisfiedBuildtime => s_ignore_satisfied_buildtime(p),
            Self::SSatisfiedBuildtimeSlotOp => s_ignore_satisfied_buildtime_slot_op(p),
            Self::SRuntime => s_ignore_runtime(p),
        }
    }

    /// Portage's `ignore_priority.__name__`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::NOptional => "ignore_optional",
            Self::NRuntimePost => "ignore_runtime_post",
            Self::NRuntime => "ignore_runtime",
            Self::SOptional => "ignore_optional",
            Self::SSatisfiedRuntimePost => "ignore_satisfied_runtime_post",
            Self::SRuntimePost => "ignore_runtime_post",
            Self::SSatisfiedRuntime => "ignore_satisfied_runtime",
            Self::SSatisfiedBuildtime => "ignore_satisfied_buildtime",
            Self::SSatisfiedBuildtimeSlotOp => "ignore_satisfied_buildtime_slot_op",
            Self::SRuntime => "ignore_runtime",
        }
    }
}

// `_emerge/DepPriorityNormalRange.py`.
pub(crate) fn n_ignore_optional(p: &DepPriority) -> bool {
    p.optional
}

pub(crate) fn n_ignore_runtime_post(p: &DepPriority) -> bool {
    p.optional || p.runtime_post
}

pub(crate) fn n_ignore_runtime(p: &DepPriority) -> bool {
    !p.runtime_slot_op && (p.optional || !p.buildtime)
}

// `_emerge/DepPrioritySatisfiedRange.py`.
pub(crate) fn s_ignore_optional(p: &DepPriority) -> bool {
    p.optional
}

pub(crate) fn s_ignore_satisfied_runtime_post(p: &DepPriority) -> bool {
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

pub(crate) fn s_ignore_runtime_post(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime || p.runtime {
        return false;
    }
    p.runtime_post
}

pub(crate) fn s_ignore_satisfied_runtime(p: &DepPriority) -> bool {
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

pub(crate) fn s_ignore_satisfied_buildtime(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime_slot_op {
        return false;
    }
    p.satisfied
}

pub(crate) fn s_ignore_satisfied_buildtime_slot_op(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.satisfied {
        return true;
    }
    !p.buildtime && !p.runtime
}

pub(crate) fn s_ignore_runtime(p: &DepPriority) -> bool {
    (!p.runtime_slot_op || p.satisfied) && (p.satisfied || p.optional || !p.buildtime)
}

/// Real `ignore_priority.__name__` for the `_serialize_tasks` trace
/// (`PORTUALE_MO_SEL`), so a portuale `MO_SEL` line lines up field-for-
/// field with the `RT_SEL` line `TEST/scripts/mo-trace/real-trace.py`
/// injects into real's loop. `None` is real's `ignore_priority = None`.
pub(crate) fn ignore_name(ig: Option<Ignore>) -> &'static str {
    ig.map_or("none", Ignore::name)
}

/// Real's two `DepPriority*Range` classes: an `ignore_priority` ladder
/// indexed `NONE=0 .. MEDIUM`, plus the named rungs `_serialize_tasks`
/// reaches for directly.
pub(crate) struct PriorityRange {
    pub(crate) ignore: &'static [Option<Ignore>],
    pub(crate) medium: usize,
    pub(crate) medium_soft: usize,
    pub(crate) medium_post: usize,
}

pub(crate) static NORMAL: PriorityRange = PriorityRange {
    ignore: &[
        None,
        Some(Ignore::NOptional),
        Some(Ignore::NRuntimePost),
        Some(Ignore::NRuntime),
    ],
    medium: 3,
    medium_soft: 2,
    medium_post: 2,
};

pub(crate) static SATISFIED: PriorityRange = PriorityRange {
    ignore: &[
        None,
        Some(Ignore::SOptional),
        Some(Ignore::SSatisfiedRuntimePost),
        Some(Ignore::SRuntimePost),
        Some(Ignore::SSatisfiedRuntime),
        Some(Ignore::SSatisfiedBuildtime),
        Some(Ignore::SSatisfiedBuildtimeSlotOp),
        Some(Ignore::SRuntime),
    ],
    medium: 7,
    medium_soft: 6,
    medium_post: 3,
};

impl PriorityRange {
    pub(crate) fn ig(&self, i: usize) -> Option<Ignore> {
        self.ignore[i]
    }
    pub(crate) fn ig_medium(&self) -> Option<Ignore> {
        self.ignore[self.medium]
    }
    pub(crate) fn ig_medium_soft(&self) -> Option<Ignore> {
        self.ignore[self.medium_soft]
    }
}

impl std::fmt::Display for DepPriority {
    /// Real `_emerge/DepPriority.py::DepPriority.__str__` -- the single
    /// highest classification. `Priority:` lines and `digraph:` edge
    /// labels both use it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::resolver_trace::dep_priority_str(self))
    }
}
