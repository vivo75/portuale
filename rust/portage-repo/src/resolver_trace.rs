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
pub(crate) fn dump_resolution_walk(
    entries: &[GraphEntry],
    root: &Path,
    parent_atoms: &[ParentAtom],
) {
    if !crate::resolver_debug() {
        return;
    }
    const KEYS: [&str; 5] = ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"];
    let label_of = |cat: &str, pkg: &str| -> String {
        entries
            .iter()
            .find(|e| e.category == cat && e.package == pkg)
            .map(|e| node_label(e, root, is_nomerge(e)))
            .unwrap_or_else(|| format!("({cat}/{pkg})"))
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
        "\n# resolution walk (portuale BFS order, not real's LIFO _create_graph order)\n"
    ));
    let mut consumed: std::collections::BTreeSet<(String, String, String, bool)> =
        std::collections::BTreeSet::new();
    for e in entries {
        let node = node_label(e, root, is_nomerge(e));
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
        out(format_args!("\nExiting... {node}\n"));
    }
    // #59 S1: installed children the walk created but `entries` no
    // longer holds (a same-slot merge shadows them). Real adds both
    // instances; this block reproduces the one portuale's `entries`
    // lost. `paired-1.0` under `<dev-libs/paired-2.0` is the canonical
    // case.
    for (key, list) in &rows {
        if consumed.contains(key) {
            continue;
        }
        let (cat, pkg, ver, installed) = key;
        let (slot, sub_slot) = crate::read_vdb_slot(root, cat, pkg, ver);
        let repo = installed_pkg_repo(root, cat, pkg, ver);
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
