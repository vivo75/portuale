//! depclean, prune and clean selection.

use crate::*;

/// Portage `emerge --depclean`'s removal list (`_calc_depclean` +
/// `create_cleanlist`): every installed package NOT reachable, over the
/// installed dependency graph (vdb `RDEPEND`/`PDEPEND`/`DEPEND`/`BDEPEND`,
/// flattened against vdb `USE`, every `||` branch kept), from the required
/// roots. Roots, `args`-mode and lib-check details: see `DepcleanCleanlistArgs`.
///
/// **Narrowings** (Portage runs the full `depgraph` in "remove" mode):
/// slot-operator rebuild edges, `package.provided`, `--deselect=n`, and
/// `world_sets` `@`-refs are still out.
///
/// Result: packages to remove (already in unmerge order -- see
/// `topological_removal_order`), the required-set size (`required_count`),
/// and `ordered` -- true when edges exist *between* cleanlist members, so the
/// caller renders in `cleanlist` order rather than regrouping by `cat/pn`.
#[derive(Debug, Clone)]
pub struct DepcleanResult {
    pub cleanlist: Vec<InstalledPackage>,
    pub required_count: usize,
    pub ordered: bool,
    /// Portage `show_parents` lines for every *kept* package, rendered and
    /// sorted (`<parent> requires <atom>, <atom>` per line), cpv order.
    pub kept_parents: Vec<(InstalledPackage, Vec<String>)>,
    /// `(atom, parent_cpv)` for every *kept* package's hard runtime dependency
    /// no installed package satisfies: the caller prints the unresolvable
    /// block and exits 1 without removing anything. Sorted, deduped.
    pub unresolved: Vec<(String, String)>,
}

/// Portage `_calc_depclean`'s unmerge-order pass (`actions.py:1591-1731`):
/// topologically sorts the cleanlist digraph so each package unmerges *before*
/// what it depends on. Returns `(ordered, cleanlist)`: with no edges `ordered`
/// is `false` and the input order stands; otherwise the order is Portage's
/// repeated-root-node pop (all current roots at once, cpv-descending).
///
/// Each edge carries its `*DEPEND` key's `UnmergeDepPriority` (higher = harder;
/// `RDEPEND` bumps to -1 for a `slot_operator_built` atom, bug 916135). Cycles
/// break via the `ignore_priority_range` scan, popping just the cpv-max node.
/// One narrowing: `flat_dep_atoms` keeps every `||` branch (Portage resolves
/// to one), adding extra ordering edges -- the conservative direction.
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
    // `deps[i]` = indices `j` that `cleanlist[i]` depends on (`i` unmerges
    // before `j`); `edge_prio[(i, j)]` = the highest priority seen for it.
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
                // Portage `Atom.slot_operator_built`: `:=` with a sub-slot.
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
            // Portage `nodes.sort(reverse=True)`: cpv descending; emit all.
            ready.sort_by(|&a, &b| cpv_cmp(&cleanlist[b], &cleanlist[a]));
            for k in ready {
                remove(k, &mut done, &mut indeg, &mut result);
            }
            continue;
        }
        // Genuine cycle -- Portage `ignore_priority_range` scan. Pop ONE.
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

/// Portage `show_parents`' per-package rendering (`actions.py:1274-1291`): one
/// `"<parent> requires <atom>, <atom>"` line per parent (atoms sorted by atom
/// package-name descending), then the lines sorted. Shared by both cleanlists.
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
/// `cat/pkg` is listed and its constraint matches one of that cp's provided
/// CPVs (Portage `match_from_list(atom, pprovided)`). Same check the main
/// resolver's `pprovided_refs` logic performs.
/// Portage: depgraph.py:5497-5503, dep_check.py:1052
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

/// Portage `_calc_depclean`'s `unresolved_deps()` check (`actions.py:1137-1245`),
/// narrowed: a *kept* package's hard runtime dependency (`RDEPEND`/`PDEPEND` --
/// Portage `dep.priority > UnmergeDepPriority.SOFT`) that no *installed*
/// package satisfies. Non-empty halts with the unresolvable-dependencies block.
///
/// **Documented narrowings**: atoms inside `||` groups are not checked (the
/// reachability walk already keeps every alternative); libc-provider atoms are
/// never flagged (`strip_libc_deps` treats implicit libc deps as noise); the
/// "show the unevaluated atom" readability case is not reproduced.
/// Portage: actions.py:1196, `find_libc_deps`
pub(crate) fn unresolved_runtime_deps(
    root: &Path,
    kept: &[&InstalledPackage],
    installed: &[InstalledPackage],
    libc_cps: &HashSet<(String, String)>,
    package_provided: &[String],
) -> Vec<(String, String)> {
    // Sub-slot included: Portage `_match_slot` checks it whenever the atom
    // specifies one -- a built `foo:2/3=` dep needs slot 2 sub-slot 3.
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
/// Arguments of [`depclean_cleanlist`].
pub struct DepcleanCleanlistArgs<'a> {
    pub root: &'a Path,
    /// `(atom, set_label)` -- the `@world` closure's seeds and which set token
    /// Portage `show_parents` would name as their parent (`@selected` for a
    /// `world` line, `@<name>` for a nested `world_sets` set). Display only.
    pub world_seeds: &'a [(String, String)],
    pub system_atoms: &'a [String],
    pub args: &'a [String],
    /// Portage `action_depclean`'s `deselect` (default `true`): in `args` mode
    /// the `@selected`/`world` set is emptied (`actions.py:1037`), so a named
    /// package also in `world` is still removed. `--deselect=n` keeps `world`
    /// as a protection root instead. No effect without `args`.
    pub deselect: bool,
    /// `--depclean-lib-check` feedback: packages the caller's `NEEDED.ELF.2`
    /// soname scan found still needed at link level. Seeded as extra roots;
    /// empty on the first pass (and always with `--depclean-lib-check=n`).
    pub lib_protected_providers: &'a [InstalledPackage],
    /// `package.provided` (`config.package_provided`). See `pprovided_matches`.
    pub package_provided: &'a [String],
}

pub fn depclean_cleanlist(args: DepcleanCleanlistArgs<'_>) -> DepcleanResult {
    let DepcleanCleanlistArgs {
        root,
        world_seeds,
        system_atoms,
        args,
        deselect,
        lib_protected_providers,
        package_provided,
    } = args;
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
    // Portage `_dynamic_config._parent_atoms`: child key -> [(parent, atom)],
    // recording every resolving dep edge, not just the first puller.
    let mut parent_atoms: HashMap<(String, String, String), Vec<(String, String)>> = HashMap::new();
    for (atom_str, label) in system_atoms.iter().map(|a| (a.as_str(), "@system")).chain(
        // `args` mode drops the `@world` seeds (Portage empties `selected_set`)
        // unless `--deselect=n`; `@system` seeds in every mode.
        world_seeds
            .iter()
            .filter(|_| args.is_empty() || !deselect)
            .map(|(a, l)| (a.as_str(), l.as_str())),
    ) {
        // `package.provided`: a seed atom this covers never becomes a graph root
        // (Portage intercepts before `_add_pkg`; the `-pc` "removed even if
        // in world" case). See `pprovided_matches`.
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
        // `args` mode: every installed package the args *don't* match is a
        // protected root (Portage `protected_set`); no edge is recorded for
        // the seed itself, since `show_parents` filters that parent out.
        for p in &installed {
            if !matched_by_args(p) && reachable.insert(key(p)) {
                queue.push(p.clone());
            }
        }
    }
    // `--depclean-lib-check` feedback: a still-needed provider becomes a root,
    // dropping it and its closure out of the cleanlist (Portage re-adds via
    // `_add_pkg` + `_complete_graph`). No parent edge for the provider itself.
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
                // `package.provided`: satisfied without adding a graph edge to the
                // installed provider (Portage `dep_check.py:1052` intercept).
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

    // Portage `show_parents` for every *kept* package, cpv-sorted (no-args: all
    // reachable; args: only `args`-matched reachable ones).
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

    // Portage `unresolved_deps()`, over *every* kept package (the full required
    // set, not just the `args`-matched ones).
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

/// Portage `emerge --prune`'s removal list (`_calc_depclean` with
/// `action="prune"`): seed the closure from every installed package *except*
/// non-highest versions matched by `args_set` (given atoms, or every
/// multi-version cp); the cleanlist is what the closure doesn't reach. Same
/// closure and order as `depclean_cleanlist`, same lib-check wiring.
///
/// **Deliberately out**: `--prune --nodeps`, the `--deselect` world-file
/// rewrite, and slot-operator rebuild edges.
/// Portage: actions.py:1059-1110, unmerge.py `_unmerge_display` prune branch
pub fn prune_cleanlist(
    root: &Path,
    args: &[String],
    // `--depclean-lib-check` feedback and `package.provided`: see
    // `depclean_cleanlist`'s params. `--prune` has no atom-seeded roots, so
    // `package.provided` only reaches the walk and `unresolved_deps()`.
    lib_protected_providers: &[InstalledPackage],
    package_provided: &[String],
) -> DepcleanResult {
    let installed = all_installed_packages(root);
    let key = |p: &InstalledPackage| (p.category.clone(), p.package.clone(), p.version.clone());

    // Highest installed version per cp (Portage `pkgs_for_cp[-1]`).
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

    // `args_set`: the given atoms, or every multi-version cp with none
    // (Portage `actions.py:1071-1075`).
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

    // A candidate for pruning = a non-highest version its args_set matches;
    // everything else seeds the closure.
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
    // Portage `_parent_atoms`: only dep-walk edges here (the prune seeds come
    // from the internal protected-set roots, which `show_parents` filters out).
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
                // `package.provided`: same dependency-walk intercept as above.
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

    // Portage `create_cleanlist`'s prune branch: `show_parents(pkg)` for every
    // `args_set`-matched *kept* version with a non-protected-set parent edge.
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

/// Portage `emerge --prune --nodeps` (`unmerge.py:245-272`, routed *instead of*
/// `_calc_depclean`): no dependency check -- per matched cp, protect the "best"
/// version (highest; highest `COUNTER` on a same-slot collision, never hit in
/// fixtures), select the rest. With `args` the split is over each atom's match
/// set; without, over every installed cp (Portage `vardb.cp_all()`), skipping
/// single-version cps.
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

/// Portage `emerge --clean`'s selection (`unmerge.py:274-293`): like
/// `--prune --nodeps` but **per slot** -- for every `(cp, slot)` keep only the
/// highest version. Same `COUNTER`-tiebreak narrowing as
/// [`prune_nodeps_selection`], and no `sys-apps/portage` self-skip (Portage
/// `unmerge.py:368` explicitly skips it for `--clean`).
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
    // Collapse back to one entry per cp (Portage regroups by cp), unioning
    // the per-slot removal sets.
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

/// Portage `strip_slots` (`lib/portage/dep/_slot_operator.py:11`) for one atom:
/// rewrites a "built" slot-operator atom (`cat/pkg:2=`, the concrete slot
/// Portage records in the vdb) back to `cat/pkg:=`, so `deps_changed`'s
/// vdb-vs-ebuild comparison doesn't flag every `:=` dep as changed. Only
/// `slot_operator == "="` with an explicit slot; the slot expression is unique
/// in an atom string, so a single leftmost replacement is safe.
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
