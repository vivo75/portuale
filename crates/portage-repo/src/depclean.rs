//! depclean, prune and clean selection.

use crate::*;

/// Real `emerge --depclean`'s own removal list (`_calc_depclean` +
/// `create_cleanlist`, no `args_set`): every installed package NOT
/// reachable, over the *installed* dependency graph, from the required
/// roots.
///
/// The graph: node = installed package; edge A -> B when B satisfies one
/// of A's own vdb-recorded `RDEPEND`, `PDEPEND`, `DEPEND` or `BDEPEND`
/// atoms, flattened against A's own vdb-recorded `USE` (`flat_dep_atoms`
/// -- every branch of a `||` group is kept, the conservative choice for a
/// removal decision).
///
/// The build-time keys (`DEPEND`/`BDEPEND`) are followed because real
/// `_calc_depclean` builds its graph via the full `depgraph` in "remove"
/// mode, where `create_depgraph_params(myopts, "remove")` sets
/// `bdeps="auto"` (`create_depgraph_params.py:100-103`) and
/// `depgraph.py:4208-4213` only discards `DEPEND`/`BDEPEND` from a
/// removal walk when `--with-bdeps=n` is given explicitly. So a package
/// that is *only* a build-time dependency of a kept package is itself
/// kept -- `emerge --depclean` will not remove something the tree still
/// needs in order to rebuild what stays installed. Real removal walks
/// build deps against the root being cleaned (`depend_root = myroot`,
/// `depgraph.py:4218-4219`), i.e. the same vdb, so portuale reads them
/// from the same `<root>` vdb as the runtime keys.
///
/// **No `args`** (a full `emerge --depclean`): roots = the installed
/// packages `world_atoms` ∪ `system_atoms` match; cleanlist = every
/// installed package none of them reach.
///
/// **`args` given** (`emerge --depclean <atoms>`): real `_calc_depclean`
/// plus `_complete_graph` in "remove" mode drop the world "selected"
/// plain atoms entirely (the default `--deselect` behavior -- the named
/// packages get removed *and* deselected) and make every installed
/// package NOT matching an `args` atom a protected root. So here roots =
/// `system_atoms` ∪ `{=cpv | installed, unmatched by args}`, and the
/// cleanlist is just the `args`-matched packages none of those reach.
///
/// **`lib_protected_providers`**: the `--depclean-lib-check` feedback
/// path. Real `_calc_depclean` computes the tentative cleanlist, then
/// (`actions.py:1356-1573`) runs a soname-linkage scan over `NEEDED.ELF.2`
/// and, for any cleanlist package that solely provides a library still
/// needed by a *surviving* consumer, re-adds that provider (and its own
/// dependencies, via `_complete_graph`) to the graph so it is kept. The
/// caller (which owns the `NEEDED.ELF.2` linkage map -- see
/// `portuale`'s `needed_elf`) performs that scan on the result of a
/// first `lib_protected_providers = &[]` call, then calls again passing
/// the packages it found here. They are seeded as additional reachability
/// roots, so the returned cleanlist, `required_count`, `kept_parents` and
/// removal order are all recomputed consistently -- exactly as if an
/// ordinary dependency had pulled each protected provider in.
///
/// **Documented narrowings** (real `_calc_depclean` via the full
/// `depgraph` in "remove" mode does more):
/// slot-operator rebuild edges, `package.provided`, `--deselect=n` (keeps
/// the world atoms as roots in `args` mode), and `world_sets` `@`-refs as
/// roots are all still out. A lib-protected provider is kept but, unlike
/// real portage (which labels its reverse-dep entry with the link-level
/// consumer), contributes no `--verbose` `kept_parents` line of its own
/// unless an ordinary dependency also reaches it.
/// `depclean_cleanlist`'s result: the packages to remove (already in
/// real `_calc_depclean`'s own unmerge order -- see
/// `topological_removal_order`), the size of the required-set closure
/// (real `req_pkg_count`, the `Required packages:` stat), and `ordered`
/// -- true when there were dependency edges *between* cleanlist members,
/// so the caller must render them in `cleanlist` order rather than
/// regrouping by `cat/pn` (real `_unmerge_display`'s own `if not
/// ordered:` regroup branch, `unmerge.py:459-474`).
#[derive(Debug, Clone)]
pub struct DepcleanResult {
    pub cleanlist: Vec<InstalledPackage>,
    pub required_count: usize,
    pub ordered: bool,
    /// Real `create_cleanlist`'s own `elif "--verbose": show_parents(pkg)`
    /// (`actions.py:1324`/`1331`): for every *kept* installed package
    /// (no-args: all of them; args: only the `args`-matched ones), the
    /// reverse-dependency lines real `show_parents` would print. Each
    /// `(package, parent_lines)` -- `parent_lines` already rendered and
    /// sorted the way `show_parents` emits them (`<parent> requires
    /// <atom>, <atom>` per line, lines sorted ascending, atoms within a
    /// line sorted by atom package-name descending). A kept package
    /// whose only parent is the internal protected-set (real
    /// `protected_set_name` filter) contributes no entry. `package`s are
    /// in cpv order.
    pub kept_parents: Vec<(InstalledPackage, Vec<String>)>,
    /// Real `_calc_depclean`'s `unresolved_deps()` (`actions.py:1137-1248`):
    /// `(atom, parent_cpv)` for every *kept* installed package's hard
    /// runtime dependency (`RDEPEND`/`PDEPEND`) that no installed package
    /// satisfies. Non-empty means real depclean/prune prints the
    /// `* Dependencies could not be completely resolved ...` block and
    /// exits 1 without removing anything -- the caller
    /// (`run_depclean_pretend` / `run_prune_pretend`) does exactly that.
    /// Sorted, deduped. See `unresolved_runtime_deps` for the narrowings.
    pub unresolved: Vec<(String, String)>,
}

/// Real `_calc_depclean`'s own unmerge-order pass (`actions.py:1591-1731`):
/// build a digraph over the cleanlist where an edge `depender -> dep`
/// exists whenever one cleanlist member satisfies another's `DEPEND` /
/// `RDEPEND` / `BDEPEND` / `PDEPEND` / `IDEPEND` (flattened against the
/// depender's own vdb `USE`), then topologically sort it so each package
/// is unmerged *before* the packages it depends on -- real portage does
/// this "to avoid breaking things that may need to run during pkg_prerm
/// or pkg_postrm".
///
/// Returns `(ordered, cleanlist)`. When the digraph has no edges at all
/// (`len(graph.order) == len(graph.root_nodes())` in real portage),
/// `ordered` is `false` and the input order (the `cat`/`pn`/version sort
/// `depclean_cleanlist` already applied) is kept -- real
/// `_unmerge_display` then does its own `cat/pn` grouping. When there
/// are edges, `ordered` is `true` and the returned order is real
/// portage's own repeated-root-node pop: every current root (nothing
/// left depends on it) is emitted at once, sorted by cpv descending
/// (real `nodes.sort(reverse=True)`), then removed.
///
/// Each edge carries the `UnmergeDepPriority` of the `*DEPEND` key that
/// created it (real `actions.py:1608-1622` + `UnmergeDepPriority.__int__`,
/// higher = harder): `IDEPEND` 0, `RDEPEND` -2 (bumped to **-1** when the
/// atom is `slot_operator_built` -- `foo/bar:2/2=`, real
/// `runtime_slot_op`, bug 916135), `PDEPEND` -3, `DEPEND`/`BDEPEND` -4.
/// A `DEPEND`/`BDEPEND` slot-op edge stays -4 (real `__int__` never
/// bumps `buildtime_slot_op`). When one `(i, j)` edge is produced by
/// more than one key, the highest priority wins (real `digraph`'s own
/// sorted `priorities[-1]`).
///
/// The topological pop mirrors real `actions.py:1713-1727`: while any
/// package still has an un-removed depender, take the current true roots
/// (nothing left depends on them) and emit **all** of them at once,
/// cpv-descending. Only when there are none -- a genuine dependency
/// cycle -- fall back to real portage's `ignore_priority_range` scan
/// (`[-4, -3, -2, -1, 0]`): find the packages whose every remaining
/// incoming edge is `<= ignore_priority`, and pop just **one** of them
/// (the cpv-max), so the fewest ordering edges possible are dropped to
/// break the cycle. `ignore_priority == 0` always yields at least one
/// node, so the loop always terminates.
///
/// Still narrower than real `actions.py` in one spot: `flat_dep_atoms`
/// keeps every `||` branch (real `_select_atoms` resolves to one), so a
/// disjunctive dep can add a few extra ordering edges -- the
/// conservative direction. Real's `node_refcounts` pre-sort has no
/// observable effect (every batch is re-sorted by cpv anyway), so it
/// isn't reproduced.
pub(crate) fn topological_removal_order(
    root: &Path,
    cleanlist: Vec<InstalledPackage>,
) -> (bool, Vec<InstalledPackage>) {
    let n = cleanlist.len();
    if n < 2 {
        return (false, cleanlist);
    }
    let cand_strs: Vec<String> = cleanlist
        .iter()
        .map(|p| format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot))
        .collect();
    // `deps[i]` = indices `j` such that `cleanlist[i]` depends on
    // `cleanlist[j]`, i.e. `i` must be unmerged before `j`.
    // `edge_prio[(i, j)]` = the highest `UnmergeDepPriority` seen for it.
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut edge_prio: HashMap<(usize, usize), i32> = HashMap::new();
    for (i, p) in cleanlist.iter().enumerate() {
        let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
        for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
            let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
            if depstr.trim().is_empty() {
                continue;
            }
            let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            let base_prio = match dep_key {
                "IDEPEND" => 0,
                "RDEPEND" => -2,
                "PDEPEND" => -3,
                _ => -4, // DEPEND / BDEPEND
            };
            for atom_str in &atoms {
                let Some(atom) = portage_dep::parse_atom(atom_str) else {
                    continue;
                };
                // Real `Atom.slot_operator_built`: `:=` with a sub-slot.
                let slot_op_built = atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
                    && atom.sub_slot.is_some();
                let prio = if dep_key == "RDEPEND" && slot_op_built {
                    -1
                } else {
                    base_prio
                };
                for (j, q) in cleanlist.iter().enumerate() {
                    if i == j || q.category != atom.category || q.package != atom.package {
                        continue;
                    }
                    if portage_dep::match_from_list(atom_str, &[cand_strs[j].as_str()])
                        .is_some_and(|m| !m.is_empty())
                    {
                        if !deps[i].contains(&j) {
                            deps[i].push(j);
                        }
                        let e = edge_prio.entry((i, j)).or_insert(i32::MIN);
                        *e = (*e).max(prio);
                    }
                }
            }
        }
    }
    if deps.iter().all(|d| d.is_empty()) {
        return (false, cleanlist);
    }

    // `rev[j]` = the dependers `i` of `j` (its incoming edges).
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for (i, d) in deps.iter().enumerate() {
        for &j in d {
            indeg[j] += 1;
            rev[j].push(i);
        }
    }
    let cpv_cmp = |a: &InstalledPackage, b: &InstalledPackage| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.package.cmp(&b.package))
            .then_with(|| vercmp_ordering(&a.version, &b.version))
    };
    let mut done = vec![false; n];
    let mut result: Vec<InstalledPackage> = Vec::with_capacity(n);
    let remove =
        |k: usize, done: &mut [bool], indeg: &mut [usize], result: &mut Vec<InstalledPackage>| {
            done[k] = true;
            for &j in &deps[k] {
                if !done[j] {
                    indeg[j] -= 1;
                }
            }
            result.push(cleanlist[k].clone());
        };
    while result.len() < n {
        let mut ready: Vec<usize> = (0..n).filter(|&k| !done[k] && indeg[k] == 0).collect();
        if !ready.is_empty() {
            // Real `nodes.sort(reverse=True)`: cpv descending; emit all.
            ready.sort_by(|&a, &b| cpv_cmp(&cleanlist[b], &cleanlist[a]));
            for k in ready {
                remove(k, &mut done, &mut indeg, &mut result);
            }
            continue;
        }
        // Genuine cycle -- real `ignore_priority_range` scan. Pop ONE.
        let mut popped = false;
        for ignore in [-4i32, -3, -2, -1, 0] {
            let mut cand: Vec<usize> = (0..n)
                .filter(|&k| {
                    !done[k]
                        && rev[k]
                            .iter()
                            .filter(|&&i| !done[i])
                            .all(|&i| edge_prio[&(i, k)] <= ignore)
                })
                .collect();
            if cand.is_empty() {
                continue;
            }
            cand.sort_by(|&a, &b| cpv_cmp(&cleanlist[b], &cleanlist[a]));
            remove(cand[0], &mut done, &mut indeg, &mut result);
            popped = true;
            break;
        }
        debug_assert!(popped, "ignore_priority == 0 must always yield a node");
        if !popped {
            break;
        }
    }
    (true, result)
}

/// Real `show_parents`'s own per-package rendering (`actions.py:1274-1291`):
/// group a kept package's recorded `(parent descriptor, atom)` edges by
/// parent, render one `"<parent> requires <atom>, <atom>"` line each with
/// the atoms sorted by atom package-name descending
/// (`sorted(atoms, reverse=True, key=attrgetter("package"))`), then sort
/// the lines. Shared by `depclean_cleanlist` and `prune_cleanlist`.
pub(crate) fn render_show_parents(edges: &[(String, String)]) -> Vec<String> {
    let mut by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
    for (par, atom) in edges {
        let v = by_parent.entry(par.as_str()).or_default();
        if !v.contains(&atom.as_str()) {
            v.push(atom.as_str());
        }
    }
    let mut lines: Vec<String> = by_parent
        .into_iter()
        .map(|(par, mut atoms)| {
            atoms.sort_by(|a, b| {
                let pa = portage_dep::parse_atom(a)
                    .map(|x| x.package)
                    .unwrap_or_default();
                let pb = portage_dep::parse_atom(b)
                    .map(|x| x.package)
                    .unwrap_or_default();
                pb.cmp(&pa)
            });
            format!("{par} requires {}", atoms.join(", "))
        })
        .collect();
    lines.sort();
    lines
}

/// `package.provided` (`pkgsettings.pprovideddict`): true when `atom_str`'s
/// `cat/pkg` is listed and its constraint matches one of that cp's
/// provided CPVs (real `portage.match_from_list(atom, pprovided)`, e.g.
/// `depgraph.py:5497-5503` for a top-level target, `dep_check.py:1052`
/// area for a dependency -- the same check the main resolver's own
/// `pprovided_refs` logic performs).
pub(crate) fn pprovided_matches(atom_str: &str, package_provided: &[String]) -> bool {
    !package_provided.is_empty()
        && portage_dep::match_from_list(
            atom_str,
            &package_provided
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
        .is_some_and(|m| !m.is_empty())
}

/// Real `_calc_depclean`'s `unresolved_deps()` check (`actions.py:1137-1245`),
/// narrowed: a *kept* installed package's hard runtime dependency
/// (`RDEPEND`/`PDEPEND` -- real `dep.priority > UnmergeDepPriority.SOFT`,
/// i.e. `runtime`/`runtime_post`; `DEPEND`/`BDEPEND` are `buildtime` =
/// SOFT and never trip the halt) that no *installed* package satisfies.
/// Non-empty -> real depclean/prune prints the `* Dependencies could not
/// be completely resolved ...` block and exits 1 without removing
/// anything.
///
/// **Documented narrowings**: an atom inside a `||` group is not checked
/// -- the any-of resolution needed to decide whether the *whole* group is
/// unsatisfiable is out of scope, and the reachability walk already keeps
/// every `||` alternative so a partly-broken group never wrongly shrinks
/// the cleanlist. A libc-provider atom (real `find_libc_deps` -- what an
/// installed `virtual/libc` pulls in) is never flagged: real relies on
/// libc genuinely being installed, and `strip_libc_deps`'s whole premise
/// is that implicit libc deps are noise, so a config/fixture without an
/// installed libc must not spuriously halt. The real "show the
/// unevaluated atom when it differs and vardb matches it"
/// readability case (`actions.py:1196`) is not reproduced.
pub(crate) fn unresolved_runtime_deps(
    root: &Path,
    kept: &[&InstalledPackage],
    installed: &[InstalledPackage],
    libc_cps: &HashSet<(String, String)>,
    package_provided: &[String],
) -> Vec<(String, String)> {
    // Include the sub-slot (real `_match_slot` checks it whenever the
    // atom specifies one, slot operator or not -- a built `foo:2/3=` dep
    // is only satisfied by an installed `foo` at slot 2 sub-slot 3).
    let candidate_strs: Vec<String> = installed
        .iter()
        .map(|p| {
            let (_slot, sub_slot) = read_vdb_slot(root, &p.category, &p.package, &p.version);
            format!(
                "{}/{}-{}:{}/{}",
                p.category, p.package, p.version, p.slot, sub_slot
            )
        })
        .collect();
    let matches_any = |atom_str: &str, atom: &portage_dep::Atom| -> bool {
        for (p, cs) in installed.iter().zip(candidate_strs.iter()) {
            if p.category == atom.category
                && p.package == atom.package
                && portage_dep::match_from_list(atom_str, &[cs.as_str()])
                    .is_some_and(|m| !m.is_empty())
            {
                return true;
            }
        }
        false
    };

    let mut out: Vec<(String, String)> = Vec::new();
    for pkg in kept {
        let use_flags = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
        let parent_cpv = pkg.cpv();
        for dep_key in ["RDEPEND", "PDEPEND"] {
            let depstr = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, dep_key);
            if depstr.trim().is_empty() {
                continue;
            }
            let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
            let Ok(stream) = portage_use_reduce::use_reduce_structured(
                &tokens,
                &use_flags,
                portage_use_reduce::MatchMode::Normal,
            ) else {
                continue;
            };
            // Track `||`-group nesting: an atom under any `|| ( ... )` is
            // skipped (see the doc comment).
            let mut pending_anyof = false;
            let mut group_stack: Vec<bool> = Vec::new();
            for tok in stream {
                match tok.as_str() {
                    "||" => pending_anyof = true,
                    "(" => {
                        group_stack.push(pending_anyof);
                        pending_anyof = false;
                    }
                    ")" => {
                        group_stack.pop();
                    }
                    atom_str => {
                        if group_stack.iter().any(|&x| x) {
                            continue;
                        }
                        let Some(atom) = portage_dep::parse_atom(atom_str) else {
                            continue;
                        };
                        if atom.blocker != portage_dep::Blocker::None {
                            continue;
                        }
                        if libc_cps.contains(&(atom.category.clone(), atom.package.clone())) {
                            continue;
                        }
                        if pprovided_matches(atom_str, package_provided) {
                            continue;
                        }
                        if !matches_any(atom_str, &atom) {
                            let edge = (atom_str.to_string(), parent_cpv.clone());
                            if !out.contains(&edge) {
                                out.push(edge);
                            }
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn depclean_cleanlist(
    root: &Path,
    // `(atom, set_label)` -- the `@world` closure's seeds and which set
    // token real `show_parents` would name as their parent (`@selected`
    // for a `world` file line, `@<name>` for a `world_sets` nested set).
    // The label is only used for the `--verbose` reverse-dep display.
    world_seeds: &[(String, String)],
    system_atoms: &[String],
    args: &[String],
    // Real `action_depclean`'s `deselect = myopts.get("--deselect") !=
    // "n"` (default `true`): in `args` mode, `if deselect:` empties the
    // `@selected`/`world` set (`actions.py:1037`) so a named package
    // that is *also* in `world` still gets removed. `--depclean <atoms>
    // --deselect=n` keeps `world` as a protection root instead, so a
    // world member named as an arg is kept. No effect without `args`
    // (the `world` seeds are always used then).
    deselect: bool,
    // `--depclean-lib-check` feedback: installed packages the caller's
    // `NEEDED.ELF.2` soname scan found are still needed at link level by
    // a surviving consumer. Seeded as extra reachability roots. Empty on
    // the caller's first pass (and always when `--depclean-lib-check=n`).
    lib_protected_providers: &[InstalledPackage],
    // `package.provided` (`config.package_provided`, real
    // `pkgsettings.pprovideddict`). See `pprovided_matches`'s own doc
    // comment for the real citations and the exact real behaviour this
    // mirrors.
    package_provided: &[String],
) -> DepcleanResult {
    let installed = all_installed_packages(root);
    // Candidate strings for `match_from_list`, one per installed package,
    // kept alongside the package itself.
    let candidate_strs: Vec<(String, &InstalledPackage)> = installed
        .iter()
        .map(|p| {
            (
                format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot),
                p,
            )
        })
        .collect();
    let matches_atom = |atom_str: &str| -> Vec<&InstalledPackage> {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            return Vec::new();
        };
        candidate_strs
            .iter()
            .filter(|(_, p)| p.category == atom.category && p.package == atom.package)
            .filter(|(cs, _)| {
                portage_dep::match_from_list(atom_str, &[cs.as_str()])
                    .is_some_and(|m| !m.is_empty())
            })
            .map(|(_, p)| *p)
            .collect()
    };

    let key = |p: &InstalledPackage| (p.category.clone(), p.package.clone(), p.version.clone());
    let matched_by_args = |p: &InstalledPackage| -> bool {
        let cs = format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot);
        args.iter().any(|a| {
            portage_dep::parse_atom(a)
                .is_some_and(|pa| pa.category == p.category && pa.package == p.package)
                && portage_dep::match_from_list(a, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
        })
    };

    // Roots.
    let mut reachable: HashSet<(String, String, String)> = HashSet::new();
    let mut queue: Vec<InstalledPackage> = Vec::new();
    // Real `_dynamic_config._parent_atoms`: child key -> [(parent
    // descriptor, atom)], where the parent descriptor is a cpv (a
    // `Package` parent) or an `@set` label (a `SetArg` parent). Every
    // dep that resolves to an installed package records an edge, whether
    // or not it was the one that first pulled that package in.
    let mut parent_atoms: HashMap<(String, String, String), Vec<(String, String)>> = HashMap::new();
    for (atom_str, label) in system_atoms.iter().map(|a| (a.as_str(), "@system")).chain(
        // `args` mode drops the `@world` seeds (real `_complete_graph`
        // empties `selected_set`) -- UNLESS `--deselect=n` keeps them
        // (real `action_depclean`'s `if deselect:` guard); `@system`
        // seeds in every mode.
        world_seeds
            .iter()
            .filter(|_| args.is_empty() || !deselect)
            .map(|(a, l)| (a.as_str(), l.as_str())),
    ) {
        // `package.provided`: a `@world`/`@system` seed atom this covers
        // is intercepted before it ever becomes a graph root (real
        // `_resolve`'s per-arg loop, `depgraph.py:5497-5503` --
        // `pprovided and match_from_list(atom, pprovided): ... continue`,
        // *before* `_add_pkg`/`_select_package` ever run). It therefore
        // never becomes a depclean protection root here either, even if
        // an installed package of the same cpv happens to exist --
        // that's the real `-pc` advisory's "will be removed by depclean
        // even if in world" case. See `pprovided_matches`.
        if pprovided_matches(atom_str, package_provided) {
            continue;
        }
        for p in matches_atom(atom_str) {
            parent_atoms
                .entry(key(p))
                .or_default()
                .push((label.to_string(), atom_str.to_string()));
            if reachable.insert(key(p)) {
                queue.push(p.clone());
            }
        }
    }
    if !args.is_empty() {
        // `args` mode: every installed package the args *don't* match is
        // a protected root (real `protected_set`). Its parent is the
        // internal protected-set SetArg, which `show_parents` filters
        // out -- so no edge is recorded for the seed itself.
        for p in &installed {
            if !matched_by_args(p) && reachable.insert(key(p)) {
                queue.push(p.clone());
            }
        }
    }
    // `--depclean-lib-check` feedback: a provider the caller's soname
    // scan found is still needed at link level becomes a root, so it and
    // its own dependency closure drop out of the cleanlist (real
    // `_calc_depclean` re-adds it via `resolver._add_pkg` then
    // `_complete_graph`). No parent edge for the provider itself -- see
    // this function's own doc comment.
    for p in lib_protected_providers {
        if reachable.insert(key(p)) {
            queue.push(p.clone());
        }
    }
    while let Some(p) = queue.pop() {
        let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
        let parent_cpv = p.cpv();
        for dep_key in ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
            if depstr.trim().is_empty() {
                continue;
            }
            let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            for atom_str in atoms {
                // `package.provided`: a dependency atom this covers is
                // satisfied without ever adding a graph edge to a
                // literal installed provider (real `dep_check.py:1052`
                // area -- the same intercept the main resolver's own
                // `pprovided_refs` check applies). See `pprovided_matches`.
                if pprovided_matches(&atom_str, package_provided) {
                    continue;
                }
                for dep in matches_atom(&atom_str) {
                    parent_atoms
                        .entry(key(dep))
                        .or_default()
                        .push((parent_cpv.clone(), atom_str.clone()));
                    if reachable.insert(key(dep)) {
                        queue.push(dep.clone());
                    }
                }
            }
        }
    }

    let mut cleanlist: Vec<InstalledPackage> = installed
        .iter()
        .filter(|p| !reachable.contains(&key(p)))
        .filter(|p| args.is_empty() || matched_by_args(p))
        .cloned()
        .collect();
    cleanlist.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.package.cmp(&b.package))
            .then_with(|| vercmp_ordering(&a.version, &b.version))
    });

    // Real `show_parents` for every *kept* installed package, cpv-sorted
    // (no-args: all reachable; args: only `args`-matched reachable ones).
    let mut kept: Vec<&InstalledPackage> = installed
        .iter()
        .filter(|p| reachable.contains(&key(p)))
        .filter(|p| args.is_empty() || matched_by_args(p))
        .collect();
    kept.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.package.cmp(&b.package))
            .then_with(|| vercmp_ordering(&a.version, &b.version))
    });
    let mut kept_parents: Vec<(InstalledPackage, Vec<String>)> = Vec::new();
    for p in kept {
        let Some(edges) = parent_atoms.get(&key(p)) else {
            continue;
        };
        let lines = render_show_parents(edges);
        if !lines.is_empty() {
            kept_parents.push((p.clone(), lines));
        }
    }

    // Real `unresolved_deps()` -- checked over *every* kept installed
    // package (real `_complete_graph`'s full required set, not just the
    // `args`-matched ones).
    let all_kept: Vec<&InstalledPackage> = installed
        .iter()
        .filter(|p| reachable.contains(&key(p)))
        .collect();
    let unresolved = unresolved_runtime_deps(
        root,
        &all_kept,
        &installed,
        &libc_provider_cps(root),
        package_provided,
    );

    let required_count = reachable.len();
    let (ordered, cleanlist) = topological_removal_order(root, cleanlist);
    DepcleanResult {
        cleanlist,
        required_count,
        ordered,
        kept_parents,
        unresolved,
    }
}

/// Real `emerge --prune`'s removal list (`_calc_depclean` with
/// `action="prune"` -- `actions.py:1059-1110` plus `create_cleanlist`'s
/// own prune branch, `:1334-1340`).
///
/// `--prune` removes *superseded* installed versions: for every cp with
/// more than one version installed, the non-highest versions, kept only
/// if something still needs that exact old version. Real portage seeds
/// its `protected_set` with every installed cp as a bare `cp` atom --
/// which resolves to just the *highest* installed version -- then the
/// per-package loop additionally, explicitly protects the highest
/// version of every cp and every non-highest version an argument atom
/// does not match. With no `args`, `args_set` auto-fills with every
/// multi-version cp, so the removal candidates are exactly the
/// non-highest versions of multi-version cps. `emerge --prune <atom>`
/// uses those atoms as `args_set` instead: only non-highest versions
/// they match become candidates.
///
/// Equivalently: seed the reachability closure from every installed
/// package *except* the ones that are both non-highest-in-their-cp and
/// matched by `args_set`; those excepted packages are the candidates,
/// and the cleanlist is the ones the closure doesn't reach. Same
/// `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND` closure and
/// `topological_removal_order` as `depclean_cleanlist`.
///
/// `--depclean-lib-check` is wired the same way as `depclean_cleanlist`
/// (via `lib_protected_providers`).
///
/// **Deliberately out** (matching `depclean_cleanlist`'s own cuts):
/// `--prune --nodeps` (the obscure `_unmerge_display` prune branch that
/// skips the closure entirely), the `--deselect` world-file rewrite
/// (`--pretend` never writes it), and slot-operator rebuild edges.
pub fn prune_cleanlist(
    root: &Path,
    args: &[String],
    // `--depclean-lib-check` feedback -- see `depclean_cleanlist`.
    lib_protected_providers: &[InstalledPackage],
    // `package.provided` -- see `depclean_cleanlist`'s own param. `--prune`
    // has no atom-seeded roots (its protected set is bare installed
    // packages, not atoms), so this only reaches the dependency-walk and
    // `unresolved_deps()` intercepts, same as `depclean_cleanlist`.
    package_provided: &[String],
) -> DepcleanResult {
    let installed = all_installed_packages(root);
    let key = |p: &InstalledPackage| (p.category.clone(), p.package.clone(), p.version.clone());

    // Highest installed version per cp (real `pkgs_for_cp[-1]`).
    let mut highest: HashMap<(String, String), String> = HashMap::new();
    for p in &installed {
        let cp = (p.category.clone(), p.package.clone());
        match highest.get(&cp) {
            Some(v) if vercmp_ordering(&p.version, v) != Ordering::Greater => {}
            _ => {
                highest.insert(cp, p.version.clone());
            }
        }
    }
    let is_highest = |p: &InstalledPackage| {
        highest
            .get(&(p.category.clone(), p.package.clone()))
            .is_some_and(|v| v == &p.version)
    };

    // `args_set`: the given atoms, or -- with none -- every cp with more
    // than one installed version (real `actions.py:1071-1075`).
    let multi_version: HashSet<(String, String)> = {
        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for p in &installed {
            *counts
                .entry((p.category.clone(), p.package.clone()))
                .or_default() += 1;
        }
        counts
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(cp, _)| cp)
            .collect()
    };
    let matched_by_args = |p: &InstalledPackage| -> bool {
        if args.is_empty() {
            return multi_version.contains(&(p.category.clone(), p.package.clone()));
        }
        let cs = format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot);
        args.iter().any(|a| {
            portage_dep::parse_atom(a)
                .is_some_and(|pa| pa.category == p.category && pa.package == p.package)
                && portage_dep::match_from_list(a, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
        })
    };

    // A candidate for pruning = a non-highest version its args_set
    // matches; everything else seeds the closure.
    let is_candidate = |p: &InstalledPackage| !is_highest(p) && matched_by_args(p);

    let candidate_strs: Vec<(String, &InstalledPackage)> = installed
        .iter()
        .map(|p| {
            (
                format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot),
                p,
            )
        })
        .collect();
    let matches_atom = |atom_str: &str| -> Vec<&InstalledPackage> {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            return Vec::new();
        };
        candidate_strs
            .iter()
            .filter(|(_, p)| p.category == atom.category && p.package == atom.package)
            .filter(|(cs, _)| {
                portage_dep::match_from_list(atom_str, &[cs.as_str()])
                    .is_some_and(|m| !m.is_empty())
            })
            .map(|(_, p)| *p)
            .collect()
    };

    let mut reachable: HashSet<(String, String, String)> = HashSet::new();
    let mut queue: Vec<InstalledPackage> = Vec::new();
    // Real `_parent_atoms` -- only the dep-walk edges (a `Package`
    // parent). The prune seeds are all pulled in by the internal
    // protected-set / bare-`cp` roots, which `show_parents` filters out,
    // so no edge is recorded for the seed itself.
    let mut parent_atoms: HashMap<(String, String, String), Vec<(String, String)>> = HashMap::new();
    for p in &installed {
        if !is_candidate(p) && reachable.insert(key(p)) {
            queue.push(p.clone());
        }
    }
    // `--depclean-lib-check` feedback -- see `depclean_cleanlist`.
    for p in lib_protected_providers {
        if reachable.insert(key(p)) {
            queue.push(p.clone());
        }
    }
    while let Some(p) = queue.pop() {
        let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
        let parent_cpv = p.cpv();
        for dep_key in ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
            if depstr.trim().is_empty() {
                continue;
            }
            let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            for atom_str in atoms {
                // `package.provided` -- see `depclean_cleanlist`'s own
                // dependency-walk intercept.
                if pprovided_matches(&atom_str, package_provided) {
                    continue;
                }
                for dep in matches_atom(&atom_str) {
                    parent_atoms
                        .entry(key(dep))
                        .or_default()
                        .push((parent_cpv.clone(), atom_str.clone()));
                    if reachable.insert(key(dep)) {
                        queue.push(dep.clone());
                    }
                }
            }
        }
    }

    let mut cleanlist: Vec<InstalledPackage> = installed
        .iter()
        .filter(|p| is_candidate(p) && !reachable.contains(&key(p)))
        .cloned()
        .collect();
    cleanlist.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.package.cmp(&b.package))
            .then_with(|| vercmp_ordering(&a.version, &b.version))
    });

    // Real `create_cleanlist`'s prune branch: `elif "--verbose":
    // show_parents(pkg)` for every `args_set`-matched *kept* version --
    // i.e. every reachable installed package `matched_by_args` covers,
    // that has a non-protected-set parent edge. cpv-sorted.
    let mut kept: Vec<&InstalledPackage> = installed
        .iter()
        .filter(|p| reachable.contains(&key(p)) && matched_by_args(p))
        .collect();
    kept.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.package.cmp(&b.package))
            .then_with(|| vercmp_ordering(&a.version, &b.version))
    });
    let mut kept_parents: Vec<(InstalledPackage, Vec<String>)> = Vec::new();
    for p in kept {
        let Some(edges) = parent_atoms.get(&key(p)) else {
            continue;
        };
        let lines = render_show_parents(edges);
        if !lines.is_empty() {
            kept_parents.push((p.clone(), lines));
        }
    }

    let all_kept: Vec<&InstalledPackage> = installed
        .iter()
        .filter(|p| reachable.contains(&key(p)))
        .collect();
    let unresolved = unresolved_runtime_deps(
        root,
        &all_kept,
        &installed,
        &libc_provider_cps(root),
        package_provided,
    );

    let required_count = reachable.len();
    let (ordered, cleanlist) = topological_removal_order(root, cleanlist);
    DepcleanResult {
        cleanlist,
        required_count,
        ordered,
        kept_parents,
        unresolved,
    }
}

/// One cp's `emerge --prune --nodeps` split: `best_version` is
/// `protected` (kept), `other_versions` (ascending) are all `selected`
/// for removal.
#[derive(Debug, Clone)]
pub struct PruneNodepsCp {
    pub category: String,
    pub package: String,
    pub best_version: String,
    pub other_versions: Vec<String>,
}

/// Real `emerge --prune --nodeps`'s own selection (`unmerge.py:245-272`,
/// the `_unmerge_display` prune branch that `--nodeps` routes to
/// *instead of* `_calc_depclean` -- `actions.py:2684`). No dependency
/// check at all: for every cp with more than one version matched,
/// protect the "best" version and select every other for removal.
///
/// "Best" is real portage's own: the highest version, except that on a
/// same-slot collision (two versions in one slot -- a broken vdb) the
/// one with the highest `COUNTER` (most recently installed) wins. This
/// portuale's single-slot fixtures never hit that collision, so `best` is
/// just the highest version -- the `COUNTER` tiebreak is a documented
/// narrowing, the same category as the other `-pC` slot refinements.
///
/// **`args` empty** (`emerge --prune --nodeps`): every installed cp
/// (real `vardb.cp_all()`); a single-version cp is skipped (real `if
/// len(mymatch) == 1: continue`).
///
/// **`args` given**: each atom's `vardb.match` set; the best/rest split
/// is over *that* set (so `--prune --nodeps '<cat/pkg-3'` keeps
/// `pkg-2` even if `pkg-3` is also installed). A cp matched by several
/// atoms appears once -- the `selected` sets unioned, `best` taken over
/// the union (real `_unmerge_display`'s own `if not ordered:` regroup).
pub fn prune_nodeps_selection(root: &Path, args: &[String]) -> Vec<PruneNodepsCp> {
    let installed = all_installed_packages(root);
    let matched: Vec<&InstalledPackage> = if args.is_empty() {
        installed.iter().collect()
    } else {
        installed
            .iter()
            .filter(|p| {
                let cs = format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot);
                args.iter().any(|a| {
                    portage_dep::parse_atom(a)
                        .is_some_and(|pa| pa.category == p.category && pa.package == p.package)
                        && portage_dep::match_from_list(a, &[cs.as_str()])
                            .is_some_and(|m| !m.is_empty())
                })
            })
            .collect()
    };

    let mut by_cp: HashMap<(String, String), Vec<String>> = HashMap::new();
    for p in matched {
        by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push(p.version.clone());
    }
    let mut out: Vec<PruneNodepsCp> = Vec::new();
    for ((category, package), mut versions) in by_cp {
        versions.sort_by(|a, b| vercmp_ordering(a, b));
        versions.dedup();
        if versions.len() < 2 {
            continue;
        }
        let best_version = versions.pop().unwrap();
        out.push(PruneNodepsCp {
            category,
            package,
            best_version,
            other_versions: versions,
        });
    }
    out.sort_by(|a, b| a.category.cmp(&b.category).then(a.package.cmp(&b.package)));
    out
}

/// Real `emerge --clean`'s own selection (`unmerge.py:274-293`, the
/// `unmerge_action == "clean"` branch): like `--prune --nodeps` but
/// **per slot** -- for every `(cp, slot)`, keep only the highest
/// version, select every older one for removal. `args` given: the
/// best/rest split is over the union of the matched sets, grouped by
/// `(cp, slot)`; `args` empty (`emerge --clean` alone): every installed
/// `(cp, slot)` (real `vardb.cp_all()` -> `cp_list`).
///
/// Same `COUNTER`-tiebreak narrowing as [`prune_nodeps_selection`] (the
/// portuale's single-instance-per-slot fixtures never hit it), and no
/// `sys-apps/portage` self-skip (real `unmerge.py:368`: the self-skip is
/// explicitly *not* applied for `--clean`).
pub fn clean_selection(root: &Path, args: &[String]) -> Vec<PruneNodepsCp> {
    let installed = all_installed_packages(root);
    let matched: Vec<&InstalledPackage> = if args.is_empty() {
        installed.iter().collect()
    } else {
        installed
            .iter()
            .filter(|p| {
                let cs = format!("{}/{}-{}:{}", p.category, p.package, p.version, p.slot);
                args.iter().any(|a| {
                    portage_dep::parse_atom(a)
                        .is_some_and(|pa| pa.category == p.category && pa.package == p.package)
                        && portage_dep::match_from_list(a, &[cs.as_str()])
                            .is_some_and(|m| !m.is_empty())
                })
            })
            .collect()
    };

    let mut by_cp_slot: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    for p in matched {
        by_cp_slot
            .entry((p.category.clone(), p.package.clone(), p.slot.clone()))
            .or_default()
            .push(p.version.clone());
    }
    // Collapse back to one entry per cp (real `_unmerge_display` regroups
    // the `pkgmap` by `mykey` = cp), unioning the per-slot removal sets.
    let mut by_cp: HashMap<(String, String), (String, Vec<String>)> = HashMap::new();
    for ((category, package, _slot), mut versions) in by_cp_slot {
        versions.sort_by(|a, b| vercmp_ordering(a, b));
        versions.dedup();
        if versions.len() < 2 {
            continue;
        }
        let best = versions.pop().unwrap();
        let entry = by_cp
            .entry((category, package))
            .or_insert_with(|| (best.clone(), Vec::new()));
        entry.1.extend(versions);
        if vercmp_ordering(&best, &entry.0) == std::cmp::Ordering::Greater {
            entry.0 = best;
        }
    }
    let mut out: Vec<PruneNodepsCp> = by_cp
        .into_iter()
        .map(|((category, package), (best_version, mut other))| {
            other.sort_by(|a, b| vercmp_ordering(a, b));
            other.dedup();
            PruneNodepsCp {
                category,
                package,
                best_version,
                other_versions: other,
            }
        })
        .collect();
    out.sort_by(|a, b| a.category.cmp(&b.category).then(a.package.cmp(&b.package)));
    out
}

/// Real `strip_slots` (`lib/portage/dep/_slot_operator.py:11`), for one
/// atom string: rewrites a "built" slot-operator atom (`cat/pkg:2=` -- the
/// concrete slot portage records in the vdb when a `cat/pkg:=` dependency
/// is merged) back to `cat/pkg:=`, so `deps_changed`'s own vdb-vs-ebuild
/// comparison doesn't flag every `:=` dependency as changed purely because
/// the vdb side carries the resolved slot and the current ebuild doesn't.
/// Only touches atoms whose `slot_operator` is `=` *and* that carry an
/// explicit slot -- a plain `:=` (no slot), a `:2` / `:2/3` (no operator),
/// or a `:2*` is left untouched, matching real `strip_slots`'s own
/// `x.slot_operator == "=" and x.slot is not None` guard. The slot
/// expression is unique in an atom string (`::repo` uses `::`, use-deps
/// live inside `[...]`), so a single leftmost replacement is safe.
pub(crate) fn strip_slot_operator_one(tok: &str) -> String {
    let Some(atom) = portage_dep::parse_atom(tok) else {
        return tok.to_string();
    };
    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
        return tok.to_string();
    }
    let Some(slot) = atom.slot.as_deref() else {
        return tok.to_string();
    };
    let slot_text = match atom.sub_slot.as_deref() {
        Some(sub) => format!(":{slot}/{sub}="),
        None => format!(":{slot}="),
    };
    tok.replacen(&slot_text, ":=", 1)
}
