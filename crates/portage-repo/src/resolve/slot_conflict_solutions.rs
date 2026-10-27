//! The slot-conflict handler's search for USE changes that would solve a
//! slot conflict.

use portage_dep::{Atom, UseDep, UseDepDefault, UseDepOp};
use std::collections::HashSet;

/// The search checks at most this many flag settings per configuration.
const CHECK_CONFIGURATION_MAX: usize = 1024;
/// With more slot conflicts than this only the all-ebuild configuration is tried.
const FULL_SEARCH_CONFLICT_LIMIT: usize = 4;

/// Identity of one package node in the search: `cat/pkg-version` plus
/// whether it is the installed instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PkgKey {
    pub(crate) cpv: String,
    pub(crate) installed: bool,
}

/// One package as the search sees it: a conflicting instance or a parent.
#[derive(Debug, Clone)]
pub(crate) struct HandlerPkg {
    pub(crate) key: PkgKey,
    /// `cat/pkg-ver:slot/sub::repo`, the string atoms are matched against.
    pub(crate) match_str: String,
    /// Declared IUSE with the `+`/`-` markers stripped.
    pub(crate) iuse: HashSet<String>,
    /// The enabled USE flags of this node.
    pub(crate) use_enabled: HashSet<String>,
    pub(crate) required_use: String,
}

/// Who pulled a conflicting instance in.
#[derive(Debug, Clone)]
pub(crate) enum HandlerParentKind {
    /// A command-line atom or a set: it has no USE of its own.
    Argument,
    Package(Box<HandlerPkg>),
}

/// One `(parent, atom)` pair of a slot conflict.
#[derive(Debug, Clone)]
pub(crate) struct HandlerParent {
    pub(crate) kind: HandlerParentKind,
    /// The atom as stored in the graph (conditionals evaluated).
    pub(crate) atom: String,
    /// The atom as written in the parent's dependency string.
    pub(crate) unevaluated: String,
}

/// One slot conflict: its instances and every parent that pulled any of them.
#[derive(Debug, Clone)]
pub(crate) struct HandlerConflict {
    pub(crate) pkgs: Vec<HandlerPkg>,
    pub(crate) parents: Vec<HandlerParent>,
}

/// The USE changes of one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseChange {
    /// `cat/pkg-version`, as the explanation prints it.
    pub cpv: String,
    /// `(flag, enable)` in the order the search recorded them.
    pub flags: Vec<(String, bool)>,
}

/// What the handler found for one dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotConflictSolutions {
    /// Number of slot conflicts the handler looked at.
    pub conflict_count: usize,
    /// Number of solutions before they were reduced to minimal changes.
    pub solution_count: usize,
    /// The minimal changes; each is a list of per-package changes.
    pub changes: Vec<Vec<UseChange>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Involved {
    Empty,
    Cond,
    Enabled,
    Disabled,
    Contradiction,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Forced {
    Empty,
    Enabled,
    Disabled,
    Contradiction,
}

/// Insertion-ordered `flag -> value` map.
type Ordered<T> = Vec<(String, T)>;

fn ordered_get<T: Copy>(map: &Ordered<T>, flag: &str) -> Option<T> {
    map.iter().find(|(f, _)| f == flag).map(|(_, v)| *v)
}

fn ordered_set<T>(map: &mut Ordered<T>, flag: &str, value: T) {
    match map.iter_mut().find(|(f, _)| f == flag) {
        Some(slot) => slot.1 = value,
        None => map.push((flag.to_string(), value)),
    }
}

/// A solution: the packages whose USE changes and what each flag becomes.
type Solution = Vec<(HandlerPkg, Ordered<Forced>)>;

/// The USE-dep flags a violated atom still requires.
struct Violated {
    enabled: HashSet<String>,
    disabled: HashSet<String>,
    /// Every flag of the original atom's USE block.
    required: Vec<String>,
}

/// `Atom.violated_conditionals`: what is left of `use_deps` after the
/// satisfied tokens are dropped. `None` when nothing is left (Portage's
/// atom then has no `.use`).
// Portage: lib/portage/dep/__init__.py, _use_dep.violated_conditionals
fn violated_conditionals(
    use_deps: &[UseDep],
    other_use: &HashSet<String>,
    is_valid: &dyn Fn(&str) -> bool,
    parent_use: &HashSet<String>,
) -> Option<Violated> {
    let missing_enabled: HashSet<&str> = use_deps
        .iter()
        .filter(|u| u.default == Some(UseDepDefault::Enabled))
        .map(|u| u.flag.as_str())
        .collect();
    let missing_disabled: HashSet<&str> = use_deps
        .iter()
        .filter(|u| u.default == Some(UseDepDefault::Disabled))
        .map(|u| u.flag.as_str())
        .collect();
    let validate = |flag: &str| {
        is_valid(flag) || missing_enabled.contains(flag) || missing_disabled.contains(flag)
    };
    let mut enabled = HashSet::new();
    let mut disabled = HashSet::new();
    let mut kept = 0usize;
    for ud in use_deps {
        let flag = ud.flag.as_str();
        let in_other = other_use.contains(flag);
        let in_parent = parent_use.contains(flag);
        let valid = is_valid(flag);
        if !validate(flag) {
            kept += 1;
            match ud.op {
                UseDepOp::Enabled => {
                    enabled.insert(flag.to_string());
                }
                UseDepOp::Disabled => {
                    disabled.insert(flag.to_string());
                }
                _ => {}
            }
            continue;
        }
        match ud.op {
            UseDepOp::Enabled => {
                if !in_other && (valid || missing_disabled.contains(flag)) {
                    kept += 1;
                    enabled.insert(flag.to_string());
                }
            }
            UseDepOp::Disabled => {
                if in_other || (!valid && missing_enabled.contains(flag)) {
                    kept += 1;
                    disabled.insert(flag.to_string());
                }
            }
            UseDepOp::IfParentEnabled => {
                if in_parent && !in_other && (valid || missing_disabled.contains(flag)) {
                    kept += 1;
                }
            }
            UseDepOp::EqualParent => {
                let keep = if in_parent && !in_other {
                    valid || missing_disabled.contains(flag)
                } else if !in_parent {
                    in_other || (!valid && missing_enabled.contains(flag))
                } else {
                    false
                };
                kept += usize::from(keep);
            }
            UseDepOp::OppositeParent => {
                let keep = if !in_parent && !in_other {
                    valid || missing_disabled.contains(flag)
                } else if in_parent {
                    in_other || (!valid && missing_enabled.contains(flag))
                } else {
                    false
                };
                kept += usize::from(keep);
            }
            UseDepOp::IfParentDisabled => {
                if !in_parent && (in_other || (!valid && missing_enabled.contains(flag))) {
                    kept += 1;
                }
            }
        }
    }
    (kept > 0).then(|| Violated {
        enabled,
        disabled,
        required: use_deps.iter().map(|u| u.flag.clone()).collect(),
    })
}

/// One parent entry with its atoms parsed once.
struct ParentView<'a> {
    parent: &'a HandlerParent,
    atom: Option<Atom>,
    unevaluated: Option<Atom>,
}

struct Search<'a> {
    conflicts: &'a [HandlerConflict],
    views: Vec<Vec<ParentView<'a>>>,
    nodes: HashSet<PkgKey>,
}

/// Whether `atom_str` matches `pkg` with `use_set` as its USE.
// Portage: lib/portage/_sets/base.py, InternalPackageSet.findAtomForPackage
fn atom_matches(atom_str: &str, pkg: &HandlerPkg, use_set: &HashSet<String>) -> bool {
    if !portage_dep::match_from_list(atom_str, &[pkg.match_str.as_str()])
        .is_some_and(|m| !m.is_empty())
    {
        return false;
    }
    match crate::parse_dep_atom(atom_str, "slot_conflict_solutions").and_then(|a| a.use_deps) {
        Some(deps) => portage_dep::use_deps_satisfied(&deps, &pkg.iuse, use_set),
        None => true,
    }
}

fn apply_forced(base: &HashSet<String>, changes: Option<&Ordered<Forced>>) -> HashSet<String> {
    let mut out = base.clone();
    for (flag, state) in changes.into_iter().flatten() {
        match state {
            Forced::Enabled => {
                out.insert(flag.clone());
            }
            Forced::Disabled => {
                out.remove(flag);
            }
            Forced::Empty | Forced::Contradiction => {}
        }
    }
    out
}

fn forced_for<'s>(solution: &'s Solution, key: &PkgKey) -> Option<&'s Ordered<Forced>> {
    solution.iter().find(|(p, _)| &p.key == key).map(|(_, c)| c)
}

/// `_force_flag_for_package`.
// Portage: lib/_emerge/resolver/slot_collision.py, _force_flag_for_package
fn force_flag(solution: &mut Solution, pkg: &HandlerPkg, flag: &str, enable: bool) {
    let at = match solution.iter().position(|(p, _)| p.key == pkg.key) {
        Some(i) => i,
        None => {
            solution.push((pkg.clone(), Vec::new()));
            solution.len() - 1
        }
    };
    let changes = &mut solution[at].1;
    let prior = ordered_get(changes, flag).unwrap_or(Forced::Empty);
    let next = if enable {
        if prior == Forced::Disabled {
            Forced::Contradiction
        } else {
            Forced::Enabled
        }
    } else if prior == Forced::Enabled {
        Forced::Contradiction
    } else if pkg.use_enabled.contains(flag) {
        Forced::Disabled
    } else {
        prior
    };
    ordered_set(changes, flag, next);
}

/// Which conditional form a use dep has in the unevaluated atom.
fn conditional_form(deps: &[UseDep], flag: &str) -> Option<UseDepOp> {
    deps.iter()
        .find(|u| u.flag == flag)
        .and_then(|u| match u.op {
            UseDepOp::Enabled | UseDepOp::Disabled => None,
            ref op => Some(op.clone()),
        })
}

impl<'a> Search<'a> {
    fn new(conflicts: &'a [HandlerConflict]) -> Self {
        let views = conflicts
            .iter()
            .map(|c| {
                c.parents
                    .iter()
                    .map(|p| ParentView {
                        parent: p,
                        atom: crate::parse_dep_atom(&p.atom, "slot_conflict_solutions"),
                        unevaluated: crate::parse_dep_atom(
                            &p.unevaluated,
                            "slot_conflict_solutions",
                        ),
                    })
                    .collect()
            })
            .collect();
        let nodes = conflicts
            .iter()
            .flat_map(|c| c.pkgs.iter().map(|p| p.key.clone()))
            .collect();
        Self {
            conflicts,
            views,
            nodes,
        }
    }

    /// `_check_configuration`: the solutions for one configuration, `None`
    /// when the configuration is rejected.
    // Portage: lib/_emerge/resolver/slot_collision.py, _check_configuration
    fn check_configuration(&self, config: &[&HandlerPkg]) -> Option<Vec<Solution>> {
        // An installed package with pending USE changes cannot be part of a
        // valid configuration: the ebuild would be pulled in again.
        for pkg in config.iter().filter(|p| p.key.installed) {
            for conflict in self.conflicts {
                if !conflict.pkgs.iter().any(|p| p.key == pkg.key) {
                    continue;
                }
                for other in conflict.pkgs.iter().filter(|o| o.key != pkg.key) {
                    if pkg.iuse != other.iuse || pkg.use_enabled != other.use_enabled {
                        return None;
                    }
                }
            }
        }
        let config_keys: HashSet<&PkgKey> = config.iter().map(|p| &p.key).collect();
        let mut all_involved: Vec<Ordered<Involved>> = Vec::new();
        for (idx, pkg) in config.iter().enumerate() {
            let mut involved: Ordered<Involved> = Vec::new();
            for view in &self.views[idx] {
                let parent_pkg = match &view.parent.kind {
                    HandlerParentKind::Package(p) => Some(p.as_ref()),
                    HandlerParentKind::Argument => None,
                };
                if let Some(pp) = parent_pkg
                    && self.nodes.contains(&pp.key)
                    && !config_keys.contains(&pp.key)
                {
                    continue;
                }
                if atom_matches(&view.parent.atom, pkg, &pkg.use_enabled) {
                    continue;
                }
                let without_use = portage_dep::without_use(&view.parent.atom);
                if !atom_matches(without_use, pkg, &pkg.use_enabled) {
                    return None;
                }
                let required: Vec<&str> = view
                    .unevaluated
                    .as_ref()
                    .and_then(|a| a.use_deps.as_ref())
                    .map(|d| d.iter().map(|u| u.flag.as_str()).collect())
                    .unwrap_or_default();
                if required.iter().any(|f| !pkg.iuse.contains(*f)) {
                    return None;
                }
                let is_valid = |f: &str| pkg.iuse.contains(f);
                let violated = match parent_pkg {
                    Some(pp) if !pp.key.installed => {
                        let deps = view
                            .unevaluated
                            .as_ref()
                            .and_then(|a| a.use_deps.as_deref())
                            .unwrap_or(&[]);
                        violated_conditionals(deps, &pkg.use_enabled, &is_valid, &pp.use_enabled)
                    }
                    _ => {
                        let deps = view
                            .atom
                            .as_ref()
                            .and_then(|a| a.use_deps.as_deref())
                            .unwrap_or(&[]);
                        violated_conditionals(deps, &pkg.use_enabled, &is_valid, &HashSet::new())
                    }
                };
                let Some(violated) = violated else {
                    // Autounmask can change the parent's USE after the
                    // evaluated atom was stored; the atom then matches.
                    continue;
                };
                if pkg.key.installed
                    && (!violated.enabled.is_empty() || !violated.disabled.is_empty())
                {
                    return None;
                }
                for flag in &violated.required {
                    let state = ordered_get(&involved, flag).unwrap_or(Involved::Empty);
                    let next = if violated.enabled.contains(flag) {
                        match state {
                            Involved::Empty | Involved::Cond | Involved::Enabled => {
                                Involved::Enabled
                            }
                            _ => Involved::Contradiction,
                        }
                    } else if violated.disabled.contains(flag) {
                        match state {
                            Involved::Empty | Involved::Cond | Involved::Disabled => {
                                Involved::Disabled
                            }
                            _ => Involved::Contradiction,
                        }
                    } else if state == Involved::Empty {
                        Involved::Cond
                    } else {
                        state
                    };
                    ordered_set(&mut involved, flag, next);
                }
            }
            if pkg.key.installed {
                // The installed package's USE stays as it is.
                for (flag, state) in involved.iter_mut() {
                    let on = pkg.use_enabled.contains(flag.as_str());
                    *state = match *state {
                        Involved::Enabled if !on => Involved::Contradiction,
                        Involved::Disabled if on => Involved::Contradiction,
                        Involved::Cond if on => Involved::Enabled,
                        Involved::Cond => Involved::Disabled,
                        other => other,
                    };
                }
            }
            if involved.iter().any(|(_, s)| *s == Involved::Contradiction) {
                return None;
            }
            all_involved.push(involved);
        }
        let cond_slots: Vec<(usize, usize)> = all_involved
            .iter()
            .enumerate()
            .flat_map(|(i, m)| {
                m.iter()
                    .enumerate()
                    .filter(|(_, (_, s))| !matches!(s, Involved::Enabled | Involved::Disabled))
                    .map(move |(j, _)| (i, j))
            })
            .collect();
        let mut solutions = Vec::new();
        let candidates = 1usize
            .checked_shl(u32::try_from(cond_slots.len()).unwrap_or(u32::MAX))
            .unwrap_or(usize::MAX);
        for counter in 0..candidates.min(CHECK_CONFIGURATION_MAX) {
            // The last conditional flag varies fastest.
            let mut flags: Vec<Ordered<bool>> = all_involved
                .iter()
                .map(|m| {
                    m.iter()
                        .map(|(f, s)| (f.clone(), *s == Involved::Enabled))
                        .collect()
                })
                .collect();
            for (n, (i, j)) in cond_slots.iter().enumerate() {
                let bit = cond_slots.len() - 1 - n;
                flags[*i][*j].1 = counter.checked_shr(bit as u32).unwrap_or(0) & 1 == 1;
            }
            if let Some(solution) = self.check_solution(config, &flags) {
                solutions.push(solution);
            }
        }
        Some(solutions)
    }

    /// `_check_solution`: the USE changes implied by one setting of the
    /// involved flags, if they introduce no new conflict.
    // Portage: lib/_emerge/resolver/slot_collision.py, _check_solution
    fn check_solution(&self, config: &[&HandlerPkg], flags: &[Ordered<bool>]) -> Option<Solution> {
        let mut required: Solution = Vec::new();
        for (idx, pkg) in config.iter().enumerate() {
            if !pkg.key.installed {
                for (flag, enable) in &flags[idx] {
                    if pkg.iuse.contains(flag.as_str()) {
                        force_flag(&mut required, pkg, flag, *enable);
                    }
                }
            }
            for view in &self.views[idx] {
                let Some(deps) = view
                    .unevaluated
                    .as_ref()
                    .and_then(|a| a.use_deps.as_deref())
                    .filter(|d| !d.is_empty())
                else {
                    continue;
                };
                let HandlerParentKind::Package(ppkg) = &view.parent.kind else {
                    continue;
                };
                for (flag, enable) in &flags[idx] {
                    let Some(form) = conditional_form(deps, flag) else {
                        continue;
                    };
                    let force_to = match (form, *enable) {
                        (UseDepOp::IfParentEnabled, false) => Some(false),
                        (UseDepOp::IfParentDisabled, true) => Some(false),
                        (UseDepOp::EqualParent, e) => Some(e),
                        (UseDepOp::OppositeParent, true) => Some(false),
                        (UseDepOp::OppositeParent, false) => Some(true),
                        _ => None,
                    };
                    if let Some(to) = force_to {
                        force_flag(&mut required, ppkg, flag, to);
                    }
                }
            }
        }
        let valid = required.iter().all(|(_, c)| {
            c.iter()
                .all(|(_, s)| matches!(s, Forced::Enabled | Forced::Disabled))
        });
        if !valid {
            return None;
        }
        for (idx, pkg) in config.iter().enumerate() {
            let new_use = apply_forced(&pkg.use_enabled, forced_for(&required, &pkg.key));
            for view in &self.views[idx] {
                let HandlerParentKind::Package(ppkg) = &view.parent.kind else {
                    continue;
                };
                let ppkg_new_use =
                    apply_forced(&ppkg.use_enabled, forced_for(&required, &ppkg.key));
                let new_atom = portage_dep::evaluate_atom_conditionals(
                    &view.parent.unevaluated,
                    &ppkg_new_use,
                )?;
                if !atom_matches(&new_atom, pkg, &new_use) {
                    return None;
                }
            }
        }
        for (pkg, changes) in &required {
            if pkg.required_use.trim().is_empty() {
                continue;
            }
            let new_use = apply_forced(&pkg.use_enabled, Some(changes));
            if !portage_required_use::check_required_use(&pkg.required_use, &new_use, &pkg.iuse)
                .unwrap_or(true)
            {
                return None;
            }
        }
        (!required.is_empty()).then_some(required)
    }
}

/// `_get_change`: the per-package flag flips a solution needs.
fn solution_change(solution: &Solution) -> Vec<UseChange> {
    let mut out = Vec::new();
    for (pkg, flags) in solution {
        let mut changed = Vec::new();
        for (flag, state) in flags {
            if !pkg.iuse.contains(flag.as_str()) {
                continue;
            }
            let on = pkg.use_enabled.contains(flag.as_str());
            match state {
                Forced::Enabled if !on => changed.push((flag.clone(), true)),
                Forced::Disabled if on => changed.push((flag.clone(), false)),
                _ => {}
            }
        }
        if !changed.is_empty() {
            out.push(UseChange {
                cpv: pkg.key.cpv.clone(),
                flags: changed,
            });
        }
    }
    out
}

/// `_is_subset`: every flag change of `small` is in `big`, with the same value.
fn is_subset(small: &[UseChange], big: &[UseChange]) -> bool {
    small.iter().all(|s| {
        big.iter().find(|b| b.cpv == s.cpv).is_some_and(|b| {
            s.flags
                .iter()
                .all(|(f, v)| b.flags.iter().any(|(bf, bv)| bf == f && bv == v))
        })
    })
}

/// `_add_change`: keep only minimal changes.
pub(crate) fn add_change(changes: &mut Vec<Vec<UseChange>>, new: Vec<UseChange>) {
    let mut ignore = false;
    let mut obsolete = Vec::new();
    for (i, existing) in changes.iter().enumerate() {
        if is_subset(existing, &new) {
            ignore = true;
            break;
        } else if is_subset(&new, existing) {
            obsolete.push(i);
        }
    }
    if ignore {
        return;
    }
    for i in obsolete.into_iter().rev() {
        changes.remove(i);
    }
    changes.push(new);
}

/// Odometer step over the per-conflict choices: the last conflict varies
/// fastest. `false` once every configuration was visited.
// Portage: lib/_emerge/resolver/slot_collision.py, _configuration_generator._next
pub(crate) fn next_configuration(ids: &mut [usize], ordered: &[Vec<&HandlerPkg>]) -> bool {
    for pos in (0..ids.len()).rev() {
        if ids[pos] + 1 < ordered[pos].len() {
            ids[pos] += 1;
            ids[pos + 1..].iter_mut().for_each(|later| *later = 0);
            return true;
        }
    }
    false
}

/// Runs the handler's configuration search over `conflicts`.
// Portage: lib/_emerge/resolver/slot_collision.py, slot_conflict_handler.__init__
pub(crate) fn solve_slot_conflicts(conflicts: &[HandlerConflict]) -> SlotConflictSolutions {
    let search = Search::new(conflicts);
    // Installed instances come last in each conflict, the rest keep their order.
    let ordered: Vec<Vec<&HandlerPkg>> = conflicts
        .iter()
        .map(|c| {
            c.pkgs
                .iter()
                .filter(|p| !p.key.installed)
                .chain(c.pkgs.iter().filter(|p| p.key.installed))
                .collect()
        })
        .collect();
    let mut solutions: Vec<Solution> = Vec::new();
    if !ordered.is_empty() && ordered.iter().all(|o| !o.is_empty()) {
        let mut ids = vec![0usize; ordered.len()];
        let mut first = true;
        loop {
            let config: Vec<&HandlerPkg> = ids
                .iter()
                .enumerate()
                .map(|(i, &n)| ordered[i][n])
                .collect();
            if let Some(found) = search.check_configuration(&config)
                && !found.is_empty()
            {
                solutions.extend(found);
                if first {
                    break;
                }
            }
            first = false;
            if ordered.len() > FULL_SEARCH_CONFLICT_LIMIT {
                break;
            }
            if !next_configuration(&mut ids, &ordered) {
                break;
            }
        }
    }
    let mut changes = Vec::new();
    for solution in &solutions {
        add_change(&mut changes, solution_change(solution));
    }
    SlotConflictSolutions {
        conflict_count: conflicts.len(),
        solution_count: solutions.len(),
        changes,
    }
}
