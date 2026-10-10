//! The walk queue.

use crate::*;

/// `--deep`/`-D`: how far past an already-installed, already-satisfied package to
/// keep walking dependencies. The default (`NotRequested`) never walks them (the
/// pre-existing hardcoded behavior); a bare `--deep` is unlimited; `--deep=N`
/// bounds the depth (`--deep=0` is indistinguishable from absent, so `Bounded(0)`
/// is never constructed; negative values are rejected at CLI parsing).
// Portage: lib/_emerge/main.py --deep; create_depgraph_params.py myparams[deep];
// depgraph.py _too_deep/_add_pkg
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deep {
    #[default]
    NotRequested,
    Unlimited,
    Bounded(u32),
}

impl Deep {
    /// Whether an already-installed, already-satisfied package at `depth` (0 for a
    /// directly-requested top-level atom) has its further dependencies walked:
    /// never for `NotRequested`, always for `Unlimited`, while `depth < n` for
    /// `Bounded(n)`.
    // Portage: depgraph.py recurse = deep is True or not _too_deep(_depth_increment(depth, n=1))
    pub(crate) fn recurses_at(self, depth: u32) -> bool {
        match self {
            Deep::NotRequested => false,
            Deep::Unlimited => true,
            Deep::Bounded(n) => depth < n,
        }
    }
}

/// One BFS-queued dependency-walk item: the atom text, its depth (see `Deep`),
/// the `(category, package)` that pushed it if any (`None` for a top-level atom,
/// consulted for `GraphEntry::required_by`), and the atom's own *unevaluated* text
/// when `evaluate_atom_conditionals` rewrote it (consulted only by
/// `--autounmask-use`'s `opt?`/REQUIRED_USE-conditional suggestion to recover the
/// original conditional form).
// Portage: _show_unsatisfied_dep atom.unevaluated_atom
#[derive(Clone)]
pub(crate) struct QueueItem {
    pub(crate) atom: String,
    pub(crate) depth: u32,
    pub(crate) owner: Option<(String, String)>,
    /// The *expanding* entry's own root flag: the owner half of the
    /// root-qualified edge this item records when it resolves. `false` for a
    /// directly-requested top-level atom.
    pub(crate) owner_targets_running_root: bool,
    pub(crate) unevaluated: Option<String>,
    /// Which root this atom resolves against: `true` only when the running root
    /// differs from the target root (otherwise a strict no-op); top-level atoms
    /// are always target-rooted.
    // Portage: depgraph.py, _add_pkg_deps five-group deps queue
    pub(crate) targets_running_root: bool,
    /// The edge from `owner` to this atom is an unsatisfied **build-time** dep
    /// (`DEPEND`/`BDEPEND`) with no run-time alternative: unbreakable in the
    /// cycle fallback, and an all-such cycle is the fatal `circular dependencies`
    /// error. `false` for top-level atoms, the `--deep` walk, and anything with
    /// a run-time alternative.
    // Portage: DepPriority buildtime && !runtime; _serialize_tasks ignore_priority scan
    pub(crate) buildtime_hard: bool,
    /// The atom was chosen out of an `|| ( ... )` group (see `or_group_universe`).
    /// `false` for plain deps, top-level atoms, and the auto-replace seeds.
    pub(crate) from_disjunction: bool,
}

/// Backtracking: `(cat, pkg)` targeted by an atom -> every puller this pass, as
/// `(parent_cat, parent_pkg, parent_version, atom_text)`. A top-level atom is
/// recorded with empty parent fields.
pub(crate) type SlotPullers = HashMap<(String, String), Vec<(String, String, String, String)>>;

/// Tokens of a pre-reduce dependency list inside an `|| ( ... )` group (any
/// depth). Portage drains the plain `dep_stack` fully before popping one entry
/// off `_dep_disjunctive_stack`, so an `||`-chosen atom enters the graph strictly
/// after every plain dep, including ones declared *after* the group. `flag? ( ... )`
/// bodies are NOT `||` groups (their members stay plain); a token occurring both
/// inside and outside classifies as group-chosen.
// Portage: depgraph.py
pub(crate) fn or_group_universe(tokens: &[String]) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut stack: Vec<bool> = Vec::new();
    let mut pending_or = false;
    for tok in tokens {
        match tok.as_str() {
            "||" => pending_or = true,
            "(" => {
                stack.push(pending_or);
                pending_or = false;
            }
            ")" => {
                stack.pop();
            }
            _ => {
                if tok.ends_with('?') {
                    pending_or = false;
                } else {
                    if stack.iter().any(|b| *b) {
                        out.insert(tok.clone());
                    }
                    pending_or = false;
                }
            }
        }
    }
    out
}

/// Which atoms of one owner's flattened dependencies resolve against the running
/// root rather than the target root, as a per-token predicate over a walk that
/// flattens every key into one string (key provenance is recovered from per-key
/// flattens here).
///
/// For a target-rooted owner `RDEPEND`/`PDEPEND` stay target-rooted and
/// `DEPEND`/`BDEPEND`/`IDEPEND` go to the running root; for a running-rooted
/// owner only a `PDEPEND`-only atom stays target-rooted. Overlaps resolve by
/// group order (`RDEPEND` first, so runtime wins over build; `IDEPEND` precedes
/// `PDEPEND`). Tokens are keyed exactly like `enqueue_flat_deps`' `buildtime_atoms`
/// / `runtime_atoms` sets. Only invoked when the roots differ; otherwise the empty
/// set is the whole answer.
// Portage: depgraph.py, _add_pkg_deps five-group deps queue
pub(crate) fn dep_running_tags(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    owner_targets_running_root: bool,
) -> HashSet<String> {
    let flatten_key = |key: &str| -> HashSet<String> {
        let joined: String = metadata
            .get(key)
            .map(String::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let toks: Vec<String> = joined.split_whitespace().map(String::from).collect();
        portage_use_reduce::use_reduce_flat(&toks, use_flags, portage_use_reduce::MatchMode::Normal)
            .map(|v| v.into_iter().filter(|t| t != "||").collect())
            .unwrap_or_default()
    };
    let (in_r, in_p, in_d, in_b, in_i) = (
        flatten_key("RDEPEND"),
        flatten_key("PDEPEND"),
        flatten_key("DEPEND"),
        flatten_key("BDEPEND"),
        flatten_key("IDEPEND"),
    );
    let all: HashSet<String> = in_r
        .union(&in_p)
        .chain(in_d.union(&in_b))
        .chain(in_i.iter())
        .cloned()
        .collect();
    all.into_iter()
        .filter(|tok| {
            if !owner_targets_running_root {
                (in_d.contains(tok) || in_b.contains(tok) || in_i.contains(tok))
                    && !in_r.contains(tok)
                    && (!in_p.contains(tok) || in_i.contains(tok))
            } else {
                !(in_p.contains(tok)
                    && !in_r.contains(tok)
                    && !in_i.contains(tok)
                    && !in_d.contains(tok)
                    && !in_b.contains(tok))
            }
        })
        .collect()
}

/// Arguments of [`enqueue_flat_deps`].
pub(crate) struct EnqueueFlatDepsArgs<'a> {
    pub(crate) flat_deps: Vec<String>,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) depth: u32,
    pub(crate) parent_use: &'a HashSet<String>,
    pub(crate) queue: &'a mut VecDeque<QueueItem>,
    pub(crate) pending_blockers: &'a mut Vec<PendingBlocker>,
    /// The same string re-flattened over just the build-time keys and just the
    /// run-time keys: an atom in the first but not the second is a
    /// `QueueItem::buildtime_hard` edge. `&[]` (all soft) at the `--with-test-deps`
    /// call site.
    pub(crate) buildtime_atoms: &'a HashSet<String>,
    pub(crate) runtime_atoms: &'a HashSet<String>,
    /// `BDEPEND` alone (subset of `buildtime_atoms`): a running-root `BDEPEND`
    /// edge is soft, a running `DEPEND` edge stays hard. Empty at the
    /// `--with-test-deps` call sites.
    // Portage: DepPriority
    pub(crate) bdepend_atoms: &'a HashSet<String>,
    pub(crate) or_universe: &'a HashSet<String>,
    /// `dep_running_tags` over the same string: atoms in this set resolve against
    /// the running root. Empty at the `--with-test-deps` call sites and wherever
    /// the roots coincide (strict no-op).
    pub(crate) running_atoms: &'a HashSet<String>,
    /// The expanding entry's own root flag. `false` for the `--with-test-deps`
    /// call sites and wherever the owner is target-rooted.
    pub(crate) owner_targets_running_root: bool,
}

/// Queues every atom in `flat_deps` onto `queue` at `depth + 1`, owned by
/// `key`/`version`, splitting blockers into `pending_blockers` instead. Shared
/// by the normal-deps queueing and the `--with-test-deps` follow-up so the two
/// can't drift apart.
///
/// `parent_use` (the owner's effective USE, the same set passed as
/// `use_reduce_flat`'s `uselist`) evaluates each token's conditional use-deps
/// before queueing, blockers included. A token failing re-evaluation is queued
/// unevaluated rather than dropped.
// Portage: use_reduce per-token integration point; PMS 8.3.4
pub(crate) fn enqueue_flat_deps(args: EnqueueFlatDepsArgs<'_>) {
    let EnqueueFlatDepsArgs {
        flat_deps,
        key,
        version,
        depth,
        parent_use,
        queue,
        pending_blockers,
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
        or_universe,
        running_atoms,
        owner_targets_running_root,
    } = args;
    // Plains in reverse declaration order first, then the `||`-chosen tokens
    // (same stack discipline one level out): Portage drains every plain dep
    // before touching a `||` group, so a blanket `.rev()` would schedule a
    // `||` choice ahead of a later-declared plain dep.
    // Portage: depgraph.py _dep_disjunctive_stack
    let (mut disj, mut plains): (Vec<String>, Vec<String>) = flat_deps
        .into_iter()
        .partition(|t| t != "||" && or_universe.contains(t));
    plains.reverse();
    disj.reverse();
    for (tok, from_disjunction) in plains
        .into_iter()
        .map(|t| (t, false))
        .chain(disj.into_iter().map(|t| (t, true)))
    {
        if tok == "||" {
            continue;
        }
        // A running-root `BDEPEND` edge is soft (its bootstrap cycle terminates
        // instead of reporting) while a running `DEPEND` edge stays hard. Bound on
        // the raw token, like `buildtime_hard`/`targets_running` below.
        let targets_running_first = running_atoms.contains(&tok);
        // The running-`BDEPEND`-soft exception does not apply to a self-naming
        // atom (Portage never relaxes a build-time edge in `_serialize_tasks`,
        // so a self loop dead-ends and aborts either way).
        // Portage: depgraph.py
        let self_naming = portage_dep::parse_atom(&tok).is_some_and(|at| {
            at.blocker == portage_dep::Blocker::None && at.category == key.0 && at.package == key.1
        });
        let buildtime_hard = buildtime_atoms.contains(&tok)
            && !runtime_atoms.contains(&tok)
            && !(targets_running_first && bdepend_atoms.contains(&tok) && !self_naming);
        // The per-group root travels with the atom, bound on the raw token before
        // evaluation moves it, like `buildtime_hard`.
        let targets_running = targets_running_first;
        // Only a genuine rewrite populates `unevaluated` (compare text, not
        // `Option`): `evaluate_atom_conditionals` returns `Some` even when
        // nothing changed.
        let evaluated = portage_dep::evaluate_atom_conditionals(&tok, parent_use)
            .unwrap_or_else(|| tok.clone());
        let unevaluated = if evaluated != tok { Some(tok) } else { None };
        let tok = evaluated;
        if let Some(dep_atom) = portage_dep::parse_atom(&tok)
            && dep_atom.blocker != portage_dep::Blocker::None
        {
            // The token survived the build-time-keys flatten.
            // Portage: blocker.priority.buildtime
            let buildtime = buildtime_atoms.contains(&tok);
            pending_blockers.push(PendingBlocker {
                atom_str: tok,
                strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                buildtime,
                target_category: dep_atom.category,
                target_package: dep_atom.package,
                owner_key: key.clone(),
                owner_version: version.to_string(),
                owner_merging: true,
                // A merged candidate's own dependency string, not the vdb-scan producer.
                owner_installed: false,
            });
            continue;
        }
        queue.push_back(QueueItem {
            atom: tok,
            depth: depth + 1,
            owner: Some(key.clone()),
            owner_targets_running_root,
            unevaluated,
            targets_running_root: targets_running,
            buildtime_hard,
            from_disjunction,
        });
    }
}
