//! `emerge --pretend --debug` resolver trace -- portuale's port of real
//! portage's `initialize_logger(logging.DEBUG)` resolution trace.
//!
//! Real `_emerge/depgraph.py` is full of `writemsg_level(..., level=
//! logging.DEBUG)` / plain `portage.writemsg(...)` calls that only fire
//! under `--debug`. Together they are the single most useful debugging
//! artifact real portage has, and the one this project has already
//! exploited once (the 2026-09-06 `_serialize_tasks` port was validated
//! by replaying real's dumped `digraph:`). Emitting the same shapes
//! makes the two implementations directly diffable. Design note and the
//! full message inventory: `docs/history/emerge-pretend-debug.md`.
//!
//! **Stream split, matching real exactly** (real `writemsg_level`,
//! `portage/util/__init__.py:119`: `level >= logging.WARNING` -> stderr,
//! else stdout):
//!
//! * `tr!` -> **stdout** -- the DEBUG-level per-package narration
//!   (`Arg:`/`Atom:`, `Parent:`/`Depstring:`/`Priority:`/`Candidates:`,
//!   `Child:`/`Parent Dep:`/`Exiting...`, the rebuild summaries).
//! * `tr_err!` -> **stderr** -- the `\ndigraph:\n\n` + `debug_print()`
//!   dump, the `runtime cycle digraph` dumps, and the per-atom
//!   `ebuild:`/`binary:`/`installed:` candidate list.
//!
//! **Deliberate divergences from real** (see the design note): plain-text
//! node labels (no ANSI colour -- portuale is deterministic);
//! portuale's post-prune merge closure as the node set, with top-level
//! atoms printed as pseudo-arg nodes, rather than real's full
//! `@world`/`@system` universe; and the `Child:`/`Parent Dep:` line
//! *interleaving* follows portuale's BFS queue, not real's LIFO stack
//! (the resulting graph -- the `digraph:` dump -- matches; the narration
//! order does not).

use std::fmt;
use std::path::Path;

use crate::merge_order::{DepPriority, entry_version};
use crate::{
    CandidateSource, GraphEntry, PretendOutcome, RepoConfig, installed_candidates,
    installed_pkg_repo, list_candidates, read_vdb_slot,
};

/// Emit `args` to stdout only when the trace is active. Real
/// `writemsg_level(..., level=logging.DEBUG)` (`level < WARNING` ->
/// stdout). The per-package narration.
pub(crate) fn out(args: fmt::Arguments) {
    if crate::resolver_debug() {
        print!("{args}");
    }
}

/// Emit `args` to stderr only when the trace is active. Real plain
/// `portage.writemsg(...)`. The `digraph:` / `runtime cycle digraph` /
/// candidate-list dumps.
pub(crate) fn err(args: fmt::Arguments) {
    if crate::resolver_debug() {
        eprint!("{args}");
    }
}

/// `tr!(...)` -> stdout narration, no-op unless `--pretend --debug` is
/// active. (`err()` is called directly for the stderr dumps.)
macro_rules! tr {
    ($($arg:tt)*) => { $crate::resolver_trace::out(format_args!($($arg)*)) };
}
pub(crate) use tr;

/// Real `_emerge/DepPriority.py::DepPriority.__str__` -- the single
/// highest classification on the priority. Used for a `digraph:` edge
/// label (`priorities[-1]`) and every `Priority:` line.
pub(crate) fn dep_priority_str(p: &DepPriority) -> &'static str {
    // Real has no `ignored` field modelled here (see `DepPriority`'s doc
    // comment); `optional` is the widest portuale expresses.
    if p.optional {
        "optional"
    } else if p.buildtime_slot_op {
        "buildtime_slot_op"
    } else if p.buildtime {
        "buildtime"
    } else if p.runtime_slot_op {
        "runtime_slot_op"
    } else if p.runtime {
        "runtime"
    } else if p.runtime_post {
        "runtime_post"
    } else {
        "soft"
    }
}

/// Real `DepPriority.__int__` -- the hardness measure `digraph.add`'s
/// `bisect.insort` orders a per-edge priority list by, so the label is
/// the `max`. `buildtime_slot_op` 0, `buildtime` -1, `runtime_slot_op`
/// -2, `runtime` -3, `runtime_post` -4, `optional` -5, none -6.
pub(crate) fn dep_priority_rank(p: &DepPriority) -> i8 {
    if p.optional {
        -5
    } else if p.buildtime_slot_op {
        0
    } else if p.buildtime {
        -1
    } else if p.runtime_slot_op {
        -2
    } else if p.runtime {
        -3
    } else if p.runtime_post {
        -4
    } else {
        -6
    }
}

/// The `DepPriority` with the greatest `dep_priority_rank` in `prios`
/// (real `priorities[-1]` after `bisect.insort`). `prios` is never
/// empty at a real digraph edge.
pub(crate) fn max_priority(prios: &[DepPriority]) -> DepPriority {
    prios
        .iter()
        .copied()
        .max_by_key(dep_priority_rank)
        .unwrap_or_default()
}

/// Real `_emerge.Package.__str__` (`_emerge/Package.py:568`), minus the
/// ANSI colour real wraps the cpv in:
/// `(cat/pkg-ver[-build_id]:slot/sub_slot::repo, <state>)` where
/// `<state>` is `installed` for a nomerge node and
/// `<type_name> scheduled for merge` for a merge-bound one.
pub(crate) fn node_label(e: &GraphEntry, root: &Path, installed: bool) -> String {
    let ver = entry_version(e).unwrap_or("");
    let (slot, sub_slot, repo) = if installed {
        let (s, ss) = read_vdb_slot(root, &e.category, &e.package, ver);
        (
            s,
            ss,
            installed_pkg_repo(root, &e.category, &e.package, ver),
        )
    } else {
        (
            e.slot.clone().unwrap_or_else(|| "0".to_string()),
            e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
            e.repo_name
                .clone()
                .unwrap_or_else(|| "__unknown__".to_string()),
        )
    };
    // Real `build_id_str` -- only a merge-bound multi-instance binary
    // carries one here (portuale doesn't track an installed package's
    // vdb `BUILD_ID` on `GraphEntry`; that only shows for the rare
    // `binpkg-multi-instance` vdb, a documented cut).
    let build_id_str = if installed {
        String::new()
    } else {
        e.build_id
            .as_deref()
            .map(|b| format!("-{b}"))
            .unwrap_or_default()
    };
    let state = if installed {
        "installed".to_string()
    } else {
        let ty = match e.source {
            CandidateSource::Binary => "binary",
            CandidateSource::Ebuild => "ebuild",
        };
        format!("{ty} scheduled for merge")
    };
    format!(
        "({}/{}-{ver}{build_id_str}:{slot}/{sub_slot}::{repo}, {state})",
        e.category, e.package
    )
}

/// Whether a `GraphEntry` is a nomerge (installed / no-visible-candidate)
/// node -- real `pkg.operation == "nomerge"`, the same test
/// `merge_order::build_digraph` makes for `Digraph::installed`.
///
/// #72 B3: a `Uninstall` removal reports `true` as well. Real's uninstall
/// node has its own `operation`, but every *merge-list* consumer of this
/// helper treats "not a merge" the same way (`is_nomerge` gates the
/// merge-order trace's merge set and the `_serialize_tasks` port's
/// selection), and portuale's removal must never be selected as a merge.
pub(crate) fn is_nomerge(e: &GraphEntry) -> bool {
    matches!(
        e.outcome,
        PretendOutcome::AlreadyInstalled { .. }
            | PretendOutcome::NoVisibleCandidate
            | PretendOutcome::Uninstall { .. }
    )
}

/// `USE="flag -flag …"` -- `use_flags_display` (bare-name-sorted
/// `(flag, enabled)`, same on both sides) rendered the way real
/// `pkg_use_display` opens its string.
fn use_str(e: &GraphEntry) -> String {
    e.use_flags_display
        .iter()
        .map(|(f, on)| if *on { f.clone() } else { format!("-{f}") })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `emerge --pretend --debug` Stage 3: real
/// `_wrapped_select_pkg_highest_available_imp`'s candidate list
/// (`depgraph.py:8347`) -- `f"{type_name + ':':>10} {cpv}::{repo}\n"`
/// for every candidate that matched `atom`, in real's `dbs` order
/// (ebuild, then binary, then installed), to stderr.
///
/// **Divergence from real:** portuale lists ebuild and installed
/// candidates only. Binary (`$PKGDIR` / binhost) candidates would need
/// a `BinaryIndex` threaded here that this call site doesn't hold; for a
/// fixture with no binaries the list is identical to real's.
pub(crate) fn dump_atom_candidates(
    repos: &[RepoConfig],
    root: &Path,
    atom: &str,
    category: &str,
    package: &str,
) {
    if !crate::resolver_debug() {
        return;
    }
    let line = |ty: &str, cpv: &str, repo: &str| {
        err(format_args!("{:>10} {cpv}::{repo}\n", format!("{ty}:")));
    };
    // ebuild candidates -- filter by the atom.
    if let Ok(cands) = list_candidates(repos, category, package) {
        let strs: Vec<String> = cands
            .iter()
            .map(|c| {
                format!(
                    "{category}/{package}-{}:{}/{}::{}",
                    c.version, c.slot, c.sub_slot, c.repo_name
                )
            })
            .collect();
        let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
        let matched = portage_dep::match_from_list(atom, &refs).unwrap_or_default();
        for c in cands.iter() {
            let s = format!(
                "{category}/{package}-{}:{}/{}::{}",
                c.version, c.slot, c.sub_slot, c.repo_name
            );
            if matched.iter().any(|m| *m == s) {
                line(
                    "ebuild",
                    &format!("{category}/{package}-{}", c.version),
                    &c.repo_name,
                );
            }
        }
    }
    // installed candidates -- filter by the atom.
    let inst = installed_candidates(root, category, package);
    let strs: Vec<String> = inst
        .iter()
        .map(|(v, s, ss)| format!("{category}/{package}-{v}:{s}/{ss}"))
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom, &refs).unwrap_or_default();
    for (v, s, ss) in &inst {
        let key = format!("{category}/{package}-{v}:{s}/{ss}");
        if matched.iter().any(|m| *m == key) {
            line(
                "installed",
                &format!("{category}/{package}-{v}"),
                &installed_pkg_repo(root, category, package, v),
            );
        }
    }
}

/// #59 S1: one `Parent Dep:` narration row -- real `_add_pkg`'s own
/// per-package parent-atom list (`_add_parent_atom`,
/// `depgraph.py:3583-3604`). Collected during the walk, where the child
/// instance an atom actually resolved to is known -- including an
/// `AlreadyInstalled` instance that a later merge-bound visit of the
/// same cp shadows in `entries` (the #57 `paired-1.0` case).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParentAtom {
    pub child_category: String,
    pub child_package: String,
    pub child_version: String,
    pub child_installed: bool,
    /// `None` for a top-level argument / internal set seed.
    pub parent: Option<(String, String)>,
    pub atom: String,
    /// Real's `unevaluated_atom` -- present only when evaluating the
    /// parent's conditional use-deps rewrote the atom.
    pub unevaluated: Option<String>,
}

/// `USE="flag -flag …"` for an installed-only child (`read_vdb_iuse` /
/// `USE`), the vdb equivalent of `use_str` in bare-name-sorted order.
fn installed_use_str(root: &Path, cat: &str, pkg: &str, ver: &str) -> String {
    let iuse = crate::read_vdb_flag_set(root, cat, pkg, ver, "IUSE");
    let enabled = crate::read_vdb_flag_set(root, cat, pkg, ver, "USE");
    let mut names: Vec<&String> = iuse.iter().collect();
    names.sort();
    names
        .iter()
        .map(|f| {
            if enabled.contains(*f) {
                (*f).clone()
            } else {
                format!("-{f}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `Child:` row's `USE="…"` column: an installed keep renders
/// its vdb-recorded USE via [`installed_use_str`] (real's
/// `installed_use_str`), every other row renders the entry's own
/// `use_flags_display` via [`use_str`]. #134: the primary printer used
/// `use_str` unconditionally, so every installed `Child:` row printed
/// `USE=""` (the resolver never builds display fields for a keep).
pub(crate) fn child_use_str(e: &GraphEntry, root: &Path) -> String {
    if is_nomerge(e) {
        let ver = entry_version(e).unwrap_or("");
        installed_use_str(root, &e.category, &e.package, ver)
    } else {
        use_str(e)
    }
}

/// Backlog #277: emission order for the bounded inner-ring re-narration
/// (see [`dump_resolution_walk`]).
///
/// Real's `--debug` trace is a live LIFO narration: every `_add_pkg`
/// call prints its `Child:`/`Parent Dep:` pair, including re-visits of
/// an already-graphed node (the innermost chain of each slot-conflict
/// node on the oldslot shape). Portuale's dump is post-hoc (one stanza
/// per final entry), so a re-visit prints nothing at its use site --
/// only the first-visit (outer) chain appears. This planner recovers
/// the missing inner chains without re-running the walk: for every
/// recorded `(parent, child, atom)` edge whose child stanza was already
/// emitted (a back edge in entries order, i.e. a revisit or cycle step),
/// it schedules the compact two-line re-narration real prints there.
///
/// Returns `(parent_entry_idx, parent_atom_idx)` pairs in emission
/// order (parents in entries order, each parent's edges in walk
/// order). Pure and total: no recursion (re-visits never re-expand in
/// real either -- they print only the two-line chain), so termination
/// needs no visited set; `budget` caps the volume instead (at most
/// `budget` edges per dump call -- the caller passes `entries.len()`,
/// mirroring the existing guard style). Single-root local by
/// construction: both inputs come from one pass's walk, which never
/// crosses a root (Track X resolves each root in its own pass).
pub(crate) fn renarration_order(
    entry_cps: &[(String, String)],
    entry_keys: &[(String, String, String, bool)],
    parent_atoms: &[ParentAtom],
    budget: usize,
) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut remaining = budget;
    if remaining == 0 {
        return out;
    }
    for (pi, pa) in parent_atoms.iter().enumerate() {
        let Some((pc, pp)) = pa.parent.as_ref() else {
            continue;
        };
        // Host stanza: the first entry with this (category, package).
        // The walk records the parent without a version, so a cp with
        // two entries (a slot conflict's pair) hosts under the first --
        // a deterministic approximation, documented at the call site.
        let Some(i) = entry_cps.iter().position(|(c, p)| c == pc && p == pp) else {
            continue;
        };
        // Re-narrate only back edges: the child stanza must already be
        // emitted (at or before the host in entries order, self
        // included). Forward edges are covered by the child's own
        // later stanza, so DAG walks schedule nothing.
        let child_key = (
            pa.child_category.clone(),
            pa.child_package.clone(),
            pa.child_version.clone(),
            pa.child_installed,
        );
        if !entry_keys[..=i].contains(&child_key) {
            continue;
        }
        out.push((i, pi));
        remaining -= 1;
        if remaining == 0 {
            break;
        }
    }
    // Emission order: hosts in entries order, each host's edges in walk
    // order (stable: the per-host walk sequence is preserved). The
    // caller re-sorts each host's edges into dependency-text order
    // (see [`sort_by_dep_position`]) so the ring reads like real's
    // expansion order.
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Re-sort one host's scheduled re-narration edges (`parent_atoms`
/// indices, in walk order) into the host's dependency-text order --
/// real re-narrates in expansion order, while portuale's BFS visit
/// order is an accident of queue dynamics. `deps` carries the host
/// entry's `(atom, evaluated)` pairs in ebuild-text order; an edge
/// matches on either form (queue texts are evaluated, dep records
/// keep both). Edges matching no dep keep walk order at the end
/// (stable). Pure.
pub(crate) fn sort_by_dep_position(
    deps: &[(String, String)],
    order: Vec<usize>,
    atom_of: impl Fn(usize) -> String,
) -> Vec<usize> {
    let mut indexed: Vec<(usize, usize)> = order
        .into_iter()
        .map(|pi| {
            let atom = atom_of(pi);
            let pos = deps
                .iter()
                .position(|(raw, evaluated)| *raw == atom || *evaluated == atom)
                .unwrap_or(usize::MAX);
            (pos, pi)
        })
        .collect();
    indexed.sort_by_key(|(pos, _)| *pos);
    indexed.into_iter().map(|(_, pi)| pi).collect()
}

/// `emerge --pretend --debug` stages 2 / 4 / 6: the per-package
/// resolution narration -- real `_add_pkg` (`Child:`/`Parent Dep:`),
/// `_add_pkg_deps` (`Parent:`/`Depstring:`/`Priority:`/`Candidates:`),
/// `dep_check` (`Virtual Parent:`/`Virtual Depstring:`) and the
/// `\nExiting... <pkg>\n` end marker (`depgraph.py:3568/4298/4747`,
/// `dep/dep_check.py:225`).
///
/// `parent_atoms` (#59 S1) carries the walk's own per-instance
/// `Parent Dep:` data. Each entry's rows are looked up by its resolved
/// instance; an installed child the walk created but `entries` no longer
/// holds (shadowed by a same-slot merge) gets its own `Child:` block
/// after the entries loop. Real interleaves that block at the atom's
/// LIFO position; appending keeps portuale's documented BFS-order
/// divergence while emitting the same content.
///
/// **Divergence from real** (see this module's header, stated once as a
/// comment line at the top of the block): real interleaves these as a
/// LIFO `_create_graph` walk; portuale emits them in one pass over the
/// final `entries` (BFS-push order -- roots before their dependencies),
/// on the successful backtracking pass only. The information is the
/// same; the interleaving is not, which is why the Stage 1 `digraph:`
/// dump (emitted from `.order`) is the authoritative diff surface.
///
/// Backlog #277 adds the bounded inner-ring re-narration (see
/// [`renarration_order`]): after an entry's own dependency groups, the
/// dump re-prints the compact `Child:`/`Parent Dep:` pair for every
/// already-emitted dependency child -- the chains real's live walk
/// re-narrates on revisit (each slot-conflict node's innermost chain).
/// At most `entries.len()` such pairs per dump, so the cost stays
/// bounded per abort; DAG walks schedule none (no back edges), so their
/// traces are byte-identical to before.
pub(crate) fn dump_resolution_walk(
    entries: &[GraphEntry],
    root: &Path,
    parent_atoms: &[ParentAtom],
    repos: &[RepoConfig],
) {
    if !crate::resolver_debug() {
        return;
    }
    const KEYS: [&str; 5] = ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"];
    // #277: a withheld (never merged) entry carries no slot/sub-slot or
    // repo on its `GraphEntry`, which would render as `:0/0::__unknown__`.
    // Its true identity is the ebuild the walk withheld, so resolve the
    // display triple from the candidate list (trace-only, `--debug`
    // only; the resolver record itself is untouched).
    let enriched = |e: &GraphEntry| -> GraphEntry {
        if e.repo_name.is_some() {
            return e.clone();
        }
        let ver = entry_version(e).unwrap_or("").to_string();
        if ver.is_empty() {
            return e.clone();
        }
        if let Ok(cands) = list_candidates(repos, &e.category, &e.package)
            && let Some(c) = cands.iter().find(|c| c.version == ver)
        {
            let mut filled = e.clone();
            if filled.slot.is_none() {
                filled.slot = Some(c.slot.clone());
            }
            if filled.sub_slot.is_none() {
                filled.sub_slot = Some(c.sub_slot.clone());
            }
            filled.repo_name = Some(c.repo_name.clone());
            return filled;
        }
        e.clone()
    };
    let label_of = |cat: &str, pkg: &str| -> String {
        entries
            .iter()
            .find(|e| e.category == cat && e.package == pkg)
            .map(|e| {
                let filled = enriched(e);
                node_label(&filled, root, is_nomerge(e))
            })
            .unwrap_or_else(|| format!("({cat}/{pkg})"))
    };
    let label_of_key = |key: &(String, String, String, bool)| -> (String, String) {
        let (cat, pkg, ver, installed) = key;
        let found = entries.iter().find(|e| {
            e.category == *cat
                && e.package == *pkg
                && entry_version(e).unwrap_or("") == ver
                && is_nomerge(e) == *installed
        });
        let entry = found.or_else(|| {
            entries
                .iter()
                .find(|e| e.category == *cat && e.package == *pkg)
        });
        match entry {
            Some(e) => {
                let filled = enriched(e);
                (
                    node_label(&filled, root, is_nomerge(e)),
                    child_use_str(e, root),
                )
            }
            None => (format!("({cat}/{pkg})"), String::new()),
        }
    };
    // #59 S1: per-instance rows, keyed by `(cat, pkg, version,
    // installed)`; a BTreeMap keeps the block order deterministic while
    // each key's rows stay in walk order. Dedup by
    // `(child, parent, atom, unevaluated)`.
    let mut rows: std::collections::BTreeMap<(String, String, String, bool), Vec<&ParentAtom>> =
        std::collections::BTreeMap::new();
    for pa in parent_atoms {
        let list = rows
            .entry((
                pa.child_category.clone(),
                pa.child_package.clone(),
                pa.child_version.clone(),
                pa.child_installed,
            ))
            .or_default();
        if !list.contains(&pa) {
            list.push(pa);
        }
    }
    let render_rows = |child_cat: &str,
                       child_pkg: &str,
                       child_ver: &str,
                       child_installed: bool,
                       fallback: &dyn Fn()| {
        match rows.get(&(
            child_cat.to_string(),
            child_pkg.to_string(),
            child_ver.to_string(),
            child_installed,
        )) {
            Some(list) if !list.is_empty() => {
                for pa in list {
                    match &pa.parent {
                        None => out(format_args!("Parent Dep:    {}\n", pa.atom)),
                        Some((pc, pp)) => {
                            let unevaluated = pa
                                .unevaluated
                                .as_deref()
                                .map(|u| format!(" ({u})"))
                                .unwrap_or_default();
                            out(format_args!(
                                "Parent Dep:    {}{unevaluated} required by {}\n",
                                pa.atom,
                                label_of(pc, pp)
                            ));
                        }
                    }
                }
            }
            _ => fallback(),
        }
    };
    out(format_args!(
        "\n# resolution walk (portuale BFS order, not Portage's LIFO _create_graph order)\n"
    ));
    let mut consumed: std::collections::BTreeSet<(String, String, String, bool)> =
        std::collections::BTreeSet::new();
    // #277: schedule the bounded inner-ring re-narration before emitting
    // anything (at most `entries.len()` edges per dump -- bounded per
    // abort; DAG walks schedule none).
    let entry_cps: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    let entry_keys: Vec<(String, String, String, bool)> = entries
        .iter()
        .map(|e| {
            (
                e.category.clone(),
                e.package.clone(),
                entry_version(e).unwrap_or("").to_string(),
                is_nomerge(e),
            )
        })
        .collect();
    let plan = renarration_order(&entry_cps, &entry_keys, parent_atoms, entries.len());
    let mut renarrate_at: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    for (host, pi) in plan {
        renarrate_at[host].push(pi);
    }
    // #277: read each host's ring in dependency-text order (real's
    // expansion order), not BFS visit order.
    for (host, list) in renarrate_at.iter_mut().enumerate() {
        let deps: Vec<(String, String)> = entries[host]
            .deps
            .iter()
            .map(|d| (d.atom.clone(), d.evaluated.clone()))
            .collect();
        let sorted = sort_by_dep_position(&deps, std::mem::take(list), |pi| {
            parent_atoms[pi].atom.clone()
        });
        *list = sorted;
    }
    for (idx, e) in entries.iter().enumerate() {
        let filled = enriched(e);
        let node = node_label(&filled, root, is_nomerge(e));
        out(format_args!(
            "\nChild:         {node} USE=\"{}\"\n",
            child_use_str(e, root)
        ));
        let ver = entry_version(e).unwrap_or("").to_string();
        let installed = is_nomerge(e);
        consumed.insert((
            e.category.clone(),
            e.package.clone(),
            ver.clone(),
            installed,
        ));
        render_rows(&e.category, &e.package, &ver, installed, &|| {
            if e.required_by.is_empty() {
                out(format_args!(
                    "Parent Dep:    {}/{} (Argument)\n",
                    e.category, e.package
                ));
            } else {
                for (pc, pp) in &e.required_by {
                    out(format_args!(
                        "Parent Dep:    {}/{} required by {}\n",
                        e.category,
                        e.package,
                        label_of(pc, pp)
                    ));
                }
            }
        });
        // Stage 6: a new-style virtual's own RDEPEND recursion, which
        // real's `dep_check` traces as a distinct step. Portuale walks a
        // `virtual/*` entry like any other, so synthesise the pair.
        if e.category == "virtual" && !is_nomerge(e) {
            let rdep: Vec<&str> = e
                .deps
                .iter()
                .filter(|d| d.key == 0)
                .map(|d| d.atom.as_str())
                .collect();
            out(format_args!(
                "Virtual Parent:      {node}\nVirtual Depstring:   {}\n",
                rdep.join(" ")
            ));
        }
        for (ki, kname) in KEYS.iter().enumerate() {
            let atoms: Vec<&str> = e
                .deps
                .iter()
                .filter(|d| d.key as usize == ki)
                .map(|d| d.atom.as_str())
                .collect();
            if atoms.is_empty() {
                continue;
            }
            let prio = e
                .deps
                .iter()
                .find(|d| d.key as usize == ki)
                .map(|d| d.priority)
                .unwrap_or_default();
            out(format_args!(
                "\nParent:    {node}\nDepstring: {} ({kname})\nPriority:  {}\n",
                atoms.join(" "),
                dep_priority_str(&prio)
            ));
            // #138: real's `--debug` prints `Depstring:` raw and
            // `Candidates:` evaluated (`logs/l111-s0-20260921/real-rest-
            // debug.log`) -- the evaluated form rides `DepEdge` beside
            // the (already reduced-raw) token, so the walk itself is
            // untouched. Real's true-raw first stanza has no
            // counterpart here (see `DepEdge::evaluated`'s doc).
            let evaluated: Vec<&str> = e
                .deps
                .iter()
                .filter(|d| d.key as usize == ki)
                .map(|d| d.evaluated.as_str())
                .collect();
            out(format_args!(
                "Candidates: [{}]\n",
                evaluated
                    .iter()
                    .map(|a| format!("'{a}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // #277: the bounded inner-ring re-narration -- the compact
        // `Child:`/`Parent Dep:` pair real's live walk prints when it
        // re-visits an already-graphed node (each slot-conflict node's
        // innermost chain). Only back edges were scheduled (forward
        // edges are covered by the child's own later stanza), in walk
        // order, so DAG traces emit nothing new here.
        for pi in &renarrate_at[idx] {
            let pa = &parent_atoms[*pi];
            let child_key = (
                pa.child_category.clone(),
                pa.child_package.clone(),
                pa.child_version.clone(),
                pa.child_installed,
            );
            let (child_label, child_use) = label_of_key(&child_key);
            out(format_args!(
                "\nChild:         {child_label} USE=\"{child_use}\"\n"
            ));
            let unevaluated = pa
                .unevaluated
                .as_deref()
                .map(|u| format!(" ({u})"))
                .unwrap_or_default();
            out(format_args!(
                "Parent Dep:    {}{unevaluated} required by {node}\n",
                pa.atom,
            ));
        }
        out(format_args!("\nExiting... {node}\n"));
    }
    // #59 S1: installed children the walk created but `entries` no
    // longer holds (a same-slot merge shadows them). Real adds both
    // instances; this block reproduces the one portuale's `entries`
    // lost. `paired-1.0` under `<dev-libs/paired-2.0` is the canonical
    // case.
    //
    // #277: the same gap covers a *withheld* ebuild child (never
    // merged, so no entry at all -- the oldslot shape's `prov-2.0`,
    // dropped by the direct solve while its `Parent Dep:` rows
    // survive). There is no vdb record to read for one, so resolve
    // the display triple from the candidate list instead of printing
    // the `:0/0::__unknown__` placeholder fallback.
    for (key, list) in &rows {
        if consumed.contains(key) {
            continue;
        }
        let (cat, pkg, ver, installed) = key;
        let (slot, sub_slot, repo) = if *installed {
            let (s, ss) = crate::read_vdb_slot(root, cat, pkg, ver);
            (s, ss, installed_pkg_repo(root, cat, pkg, ver))
        } else if let Ok(cands) = list_candidates(repos, cat, pkg)
            && let Some(c) = cands.iter().find(|c| c.version == *ver)
        {
            (c.slot.clone(), c.sub_slot.clone(), c.repo_name.clone())
        } else {
            ("0".to_string(), "0".to_string(), "__unknown__".to_string())
        };
        let state = if *installed {
            "installed".to_string()
        } else {
            "ebuild scheduled for merge".to_string()
        };
        out(format_args!(
            "\nChild:         ({cat}/{pkg}-{ver}:{slot}/{sub_slot}::{repo}, {state}) USE=\"{}\"\n",
            installed_use_str(root, cat, pkg, ver)
        ));
        for pa in list {
            match &pa.parent {
                None => out(format_args!("Parent Dep:    {}\n", pa.atom)),
                Some((pc, pp)) => {
                    let unevaluated = pa
                        .unevaluated
                        .as_deref()
                        .map(|u| format!(" ({u})"))
                        .unwrap_or_default();
                    out(format_args!(
                        "Parent Dep:    {}{unevaluated} required by {}\n",
                        pa.atom,
                        label_of(pc, pp)
                    ));
                }
            }
        }
        out(format_args!("\nExiting... ({cat}/{pkg}-{ver})\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        child: (&str, &str, &str, bool),
        parent: Option<(&str, &str)>,
        atom: &str,
    ) -> ParentAtom {
        ParentAtom {
            child_category: child.0.to_string(),
            child_package: child.1.to_string(),
            child_version: child.2.to_string(),
            child_installed: child.3,
            parent: parent.map(|(c, p)| (c.to_string(), p.to_string())),
            atom: atom.to_string(),
            unevaluated: None,
        }
    }

    fn key(cat: &str, pkg: &str, ver: &str, installed: bool) -> (String, String, String, bool) {
        (cat.to_string(), pkg.to_string(), ver.to_string(), installed)
    }

    fn cp(cat: &str, pkg: &str) -> (String, String) {
        (cat.to_string(), pkg.to_string())
    }

    #[test]
    fn renarration_dag_schedules_nothing() {
        // Linear a -> b -> c in entries order: every edge is forward,
        // covered by the child's own later stanza.
        let cps = vec![cp("x", "a"), cp("x", "b"), cp("x", "c")];
        let keys = vec![
            key("x", "a", "1", false),
            key("x", "b", "1", false),
            key("x", "c", "1", false),
        ];
        let rows = vec![
            row(("x", "b", "1", false), Some(("x", "a")), "x/b"),
            row(("x", "c", "1", false), Some(("x", "b")), "x/c"),
        ];
        assert!(renarration_order(&cps, &keys, &rows, 10).is_empty());
    }

    #[test]
    fn renarration_back_edges_schedule_in_walk_order() {
        // Oldslot-like ring: cons(0) -> prov(1) -> abi(2) -> {cons, prov}.
        let cps = vec![cp("d", "cons"), cp("d", "prov"), cp("d", "abi")];
        let keys = vec![
            key("d", "cons", "1", false),
            key("d", "prov", "1", false),
            key("d", "abi", "1", false),
        ];
        let rows = vec![
            row(("d", "prov", "1", false), Some(("d", "cons")), "d/prov:0="),
            row(("d", "abi", "1", false), Some(("d", "prov")), "d/abi"),
            row(("d", "cons", "1", false), Some(("d", "abi")), "d/cons"),
            row(("d", "prov", "1", false), Some(("d", "abi")), "=d/prov-1"),
        ];
        // Forward edges (rows 0, 1) schedule nothing; the two back edges
        // under abi (rows 2, 3) schedule in walk order.
        assert_eq!(
            renarration_order(&cps, &keys, &rows, 10),
            vec![(2, 2), (2, 3)]
        );
    }

    #[test]
    fn renarration_budget_bounds_volume() {
        let cps = vec![cp("d", "cons"), cp("d", "prov"), cp("d", "abi")];
        let keys = vec![
            key("d", "cons", "1", false),
            key("d", "prov", "1", false),
            key("d", "abi", "1", false),
        ];
        let rows = vec![
            row(("d", "cons", "1", false), Some(("d", "abi")), "d/cons"),
            row(("d", "prov", "1", false), Some(("d", "abi")), "=d/prov-1"),
        ];
        assert_eq!(renarration_order(&cps, &keys, &rows, 1), vec![(2, 0)]);
        assert!(renarration_order(&cps, &keys, &rows, 0).is_empty());
    }

    #[test]
    fn renarration_cycle_terminates_without_recursion() {
        // Two-node ring a <-> b: one back edge, no expansion, no loop.
        let cps = vec![cp("x", "a"), cp("x", "b")];
        let keys = vec![key("x", "a", "1", false), key("x", "b", "1", false)];
        let rows = vec![
            row(("x", "b", "1", false), Some(("x", "a")), "x/b"),
            row(("x", "a", "1", false), Some(("x", "b")), "x/a"),
        ];
        assert_eq!(renarration_order(&cps, &keys, &rows, 10), vec![(1, 1)]);
    }

    #[test]
    fn renarration_skips_argument_and_unknown_parents() {
        let cps = vec![cp("x", "a")];
        let keys = vec![key("x", "a", "1", false)];
        let rows = vec![
            row(("x", "a", "1", false), None, "x/a"),
            row(("x", "a", "1", false), Some(("x", "ghost")), "x/a"),
        ];
        assert!(renarration_order(&cps, &keys, &rows, 10).is_empty());
    }

    #[test]
    fn renarration_sorts_by_dep_position() {
        // Walk order [prov, cons], ebuild text order [cons, prov].
        let deps = vec![
            ("d/cons".to_string(), "d/cons".to_string()),
            ("=d/prov-1".to_string(), "=d/prov-1".to_string()),
        ];
        let atoms = ["=d/prov-1".to_string(), "d/cons".to_string()];
        assert_eq!(
            sort_by_dep_position(&deps, vec![0, 1], |pi| atoms[pi].clone()),
            vec![1, 0]
        );
        // Unmatched edges keep walk order at the end.
        let atoms = ["=d/prov-1".to_string(), "d/ghost".to_string()];
        assert_eq!(
            sort_by_dep_position(&deps, vec![0, 1], |pi| atoms[pi].clone()),
            vec![0, 1]
        );
    }

    #[test]
    fn renarration_hosts_under_first_cp_match() {
        // A slot conflict's pair shares one cp: the row hosts under the
        // first entry (deterministic approximation, documented).
        let cps = vec![cp("d", "prov"), cp("d", "prov")];
        let keys = vec![key("d", "prov", "1", false), key("d", "prov", "2", false)];
        let rows = vec![row(
            ("d", "prov", "2", false),
            Some(("d", "prov")),
            "d/prov",
        )];
        // Child prov-2 (idx 1) is not in keys[..=0]: a self-cp forward
        // edge to a later instance schedules nothing.
        assert!(renarration_order(&cps, &keys, &rows, 10).is_empty());
    }
}
