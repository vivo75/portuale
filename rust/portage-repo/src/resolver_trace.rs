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
//! full message inventory: `docs/emerge-pretend-debug.md`.
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
pub(crate) fn is_nomerge(e: &GraphEntry) -> bool {
    matches!(
        e.outcome,
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
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
        for c in &cands {
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

/// `emerge --pretend --debug` stages 2 / 4 / 6: the per-package
/// resolution narration -- real `_add_pkg` (`Child:`/`Parent Dep:`),
/// `_add_pkg_deps` (`Parent:`/`Depstring:`/`Priority:`/`Candidates:`),
/// `dep_check` (`Virtual Parent:`/`Virtual Depstring:`) and the
/// `\nExiting... <pkg>\n` end marker (`depgraph.py:3568/4298/4747`,
/// `dep/dep_check.py:225`).
///
/// **Divergence from real** (see this module's header, stated once as a
/// comment line at the top of the block): real interleaves these as a
/// LIFO `_create_graph` walk; portuale emits them in one pass over the
/// final `entries` (BFS-push order -- roots before their dependencies),
/// on the successful backtracking pass only. The information is the
/// same; the interleaving is not, which is why the Stage 1 `digraph:`
/// dump (emitted from `.order`) is the authoritative diff surface.
pub(crate) fn dump_resolution_walk(entries: &[GraphEntry], root: &Path) {
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
    out(format_args!(
        "\n# resolution walk (portuale BFS order, not real's LIFO _create_graph order)\n"
    ));
    for e in entries {
        let node = node_label(e, root, is_nomerge(e));
        out(format_args!(
            "\nChild:         {node} USE=\"{}\"\n",
            use_str(e)
        ));
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
            out(format_args!(
                "Candidates: [{}]\n",
                atoms
                    .iter()
                    .map(|a| format!("'{a}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out(format_args!("\nExiting... {node}\n"));
    }
}
