//! depclean, prune and clean selection.

use crate::*;

/// Portage `_calc_depclean`'s unmerge-order pass (Portage actions):
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

/// Portage `show_parents`' per-package rendering (Portage actions): one
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
/// Portage: depgraph, dep_check
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

/// Portage `_calc_depclean`'s `unresolved_deps()` check (Portage actions),
/// narrowed: a *kept* package's hard runtime dependency (`RDEPEND`/`PDEPEND` --
/// Portage `dep.priority > UnmergeDepPriority.SOFT`) that no *installed*
/// package satisfies. Non-empty halts with the unresolvable-dependencies block.
///
/// **Documented narrowings**: atoms inside `||` groups are not checked (the
/// reachability walk already keeps every alternative); libc-provider atoms are
/// never flagged (`strip_libc_deps` treats implicit libc deps as noise); the
/// "show the unevaluated atom" readability case is not reproduced.
/// Portage: actions, `find_libc_deps`
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
    /// the `@selected`/`world` set is emptied (Portage actions), so a named
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

/// Portage `strip_slots` (Portage dep) for one atom:
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
