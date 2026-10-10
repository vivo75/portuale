//! The walk queue.

use crate::*;

/// `--deep`/`-D` (real `lib/_emerge/main.py`'s own `"--deep": valid_integers`
/// declaration, `create_depgraph_params.py`'s `myparams["deep"]`, and
/// `depgraph.py`'s own `_too_deep`/`_add_pkg` combination): how far past
/// an already-installed, already-satisfied package to keep walking
/// dependencies. Real portage's own default (`deep` absent from
/// `myparams` entirely, since `create_depgraph_params.py` only sets it
/// when `--deep`'s own value is present and non-zero) means an
/// AlreadyInstalled package's own further dependencies are *never*
/// walked, at any depth -- exactly portuale's own pre-existing,
/// hardcoded behavior (see `resolve_pretend_graph`'s own doc comment,
/// "A package's dependencies are only walked if..."), which is why
/// `NotRequested` needed no new code of its own to stay correct. A bare
/// `--deep` stores real Python `True` (unlimited depth); `--deep=N`
/// stores the integer `N`, rejected by real `parser.error()` if negative
/// -- `--deep=0` parses fine but is indistinguishable from not passing
/// `--deep` at all, since `create_depgraph_params.py`'s own `!= 0` check
/// excludes it from `myparams` either way (`Bounded(0)` is never
/// constructed here for the same reason -- see the CLI parsing in
/// `portuale/src/pretend.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deep {
    #[default]
    NotRequested,
    Unlimited,
    Bounded(u32),
}

impl Deep {
    /// Whether an already-installed, already-satisfied package sitting at
    /// `depth` (0 for a directly-requested top-level atom) should have
    /// its own further dependencies walked. Mirrors real depgraph.py's
    /// `recurse = deep is True or not self._too_deep(self._depth_increment(depth, n=1))`:
    /// `NotRequested` (`deep=0`) is never satisfied, regardless of depth;
    /// `Unlimited` (`deep is True`) always is; `Bounded(n)` is satisfied
    /// while `depth < n`, so a dependency discovered this way lands at
    /// `depth + 1 <= n`.
    pub(crate) fn recurses_at(self, depth: u32) -> bool {
        match self {
            Deep::NotRequested => false,
            Deep::Unlimited => true,
            Deep::Bounded(n) => depth < n,
        }
    }
}

/// One BFS-queued dependency-walk item: the atom text, its own depth
/// (see `Deep`), the `(category, package)` that pushed it, if any (`None`
/// for a directly-requested top-level atom) -- consulted by
/// `resolve_pretend_graph`'s own `required_by_map`, for `GraphEntry::
/// required_by` -- and the atom's own *unevaluated* text, if
/// `evaluate_atom_conditionals` actually rewrote it (`None` when nothing
/// changed, or when this atom was never conditional-evaluated at all --
/// see `enqueue_flat_deps`'s own doc comment). Consulted only by
/// `--autounmask-use`'s own `opt?`/REQUIRED_USE-conditional suggestion
/// mechanism (real `_show_unsatisfied_dep`'s own `atom.unevaluated_atom`
/// -- see `suggested_parent_use_candidate`'s own doc comment) to recover
/// the original conditional form after evaluation has already replaced
/// it in the queued atom text itself.
#[derive(Clone)]
pub(crate) struct QueueItem {
    pub(crate) atom: String,
    pub(crate) depth: u32,
    pub(crate) owner: Option<(String, String)>,
    /// Track X Slice C (#242): the *expanding* entry's own root flag --
    /// the owner half of the root-qualified edge this item records when
    /// it resolves. The atom's own resolving root rides in
    /// `targets_running_root` below (the child half); the two differ
    /// exactly for a cross-root group (a target-rooted owner's
    /// `BDEPEND` atom, or a running-rooted owner's `RDEPEND` atom).
    /// `false` for a directly-requested top-level atom.
    pub(crate) owner_targets_running_root: bool,
    pub(crate) unevaluated: Option<String>,
    /// Track X Slice B (#242): real `_add_pkg_deps`' five-group `deps`
    /// queue (`depgraph.py:4255-4291`) resolves each group against its own
    /// root -- `RDEPEND`/`PDEPEND` against the target root, `DEPEND`/
    /// `BDEPEND`/`IDEPEND` against the running root -- so this atom must
    /// be selected against the running root's tree/installed db, and its
    /// entry carries that root (`GraphEntry::targets_running_root`).
    /// `true` only when the running root differs from the target root
    /// (otherwise every atom is target-rooted, a strict no-op); top-level
    /// atoms are always target-rooted (real installs the argument under
    /// the target `ROOT`).
    pub(crate) targets_running_root: bool,
    /// The edge from `owner` to this atom's `cat/pkg` is an unsatisfied
    /// **build-time** dep (`DEPEND`/`BDEPEND`) with no run-time
    /// alternative -- real `DepPriority` `buildtime && !runtime`. Such an
    /// edge is unbreakable in `topological_merge_order`'s cycle
    /// fallback, and a cycle made entirely of them is the fatal
    /// `* Error: circular dependencies:` real portage reports
    /// (`_serialize_tasks`'s `ignore_priority` scan can drop an
    /// unsatisfied run-time edge but never an unsatisfied build-time
    /// one). `false` for a top-level atom, the `--deep` walk (only ever
    /// reaches already-satisfied deps), and anything with a run-time
    /// alternative.
    pub(crate) buildtime_hard: bool,
    /// The atom was chosen out of an `|| ( ... )` group (the `disj`
    /// partition of `enqueue_flat_deps` / `enqueue_dependencies`,
    /// computed via `or_group_universe`). Real resolves the disjunction
    /// layer's group-level classification (`dep_check.py` soft 715-724:
    /// `all_available` vs `all_installed`/`some_installed` vs `other`)
    /// before any per-atom selection, and the chosen alternative's atoms
    /// each still need a visible tree candidate of their own -- an
    /// installed-only atom in the chosen alternative stays a reported
    /// miss (pinned by
    /// `test_or_group_other_installed_some_bin_beats_plain_other`: both
    /// `opartlya` and `opartlyb` are reported even though `opartlya`
    /// alone is installed). `false` for plain deps, top-level atoms,
    /// and the auto-replace seeds.
    pub(crate) from_disjunction: bool,
}

/// Backtracking: `(cat, pkg)` targeted by an atom -> every puller this
/// pass, as `(parent_cat, parent_pkg, parent_version, atom_text)`. A
/// top-level atom is recorded with empty parent fields. Slice 4 uses the
/// atom text to render the `pulled in by` lines; C3's
/// `slot_conflict_mask_choices` uses the `(parent, version, atom)`
/// triples to build each choice's `conflict_atoms`. See
/// `resolve_pretend_graph`'s `slot_pullers`.
pub(crate) type SlotPullers = HashMap<(String, String), Vec<(String, String, String, String)>>;

/// Tokens of a pre-reduce dependency list that sit inside an `|| ( ...
/// )` group (any depth). Backlog #90 (S1): real drains the plain
/// `dep_stack` fully before popping one entry off
/// `_dep_disjunctive_stack` (`depgraph.py:3257-3268`), so a `||`-chosen
/// atom enters the graph strictly after every plain dep -- including
/// ones declared *after* the group. `enqueue_flat_deps` needs that
/// split because the flattened list no longer says which tokens a
/// `||` produced. `flag? ( ... )` bodies are NOT `||` groups (their
/// members stay plain); a token occurring both inside and outside a
/// group classifies as group-chosen (pathological overlap, documented
/// here rather than handled).
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

/// Track X Slice B (#242): which atoms of one owner's flattened
/// dependencies resolve against the running root rather than the target
/// root -- real `_add_pkg_deps`' five-group `deps` queue
/// (`depgraph.py:4255-4291`) as a per-token predicate, for a walk that
/// flattens every key into one string before queueing (so key provenance
/// is recovered from per-key flattens here).
///
/// Real's mapping, for an owner in the target root: `RDEPEND`/`PDEPEND`
/// stay target-rooted; `DEPEND`/`BDEPEND`/`IDEPEND` go to the running
/// root (native builds: `ESYSROOT` collapses to the running root, and the
/// pre-EAPI-7 `DEPEND`-against-running-root branch lands in the same
/// place, so no EAPI gate is needed -- both real branches map here).
/// For an owner already in the running root: `RDEPEND`/`IDEPEND`/`DEPEND`/
/// `BDEPEND` stay running-rooted and only a `PDEPEND`-only atom stays a
/// target-`ROOT` concern (scope-backlog's permanent non-gap, not
/// reopened). Overlaps resolve by real's group order (`RDEPEND` first, so
/// a runtime key always wins over a build key; `IDEPEND` precedes
/// `PDEPEND`, so it wins that overlap).
///
/// The returned tokens are keyed exactly like the `buildtime_atoms` /
/// `runtime_atoms` sets `enqueue_flat_deps` classifies with (same
/// per-key `use_reduce_flat` over the same `use_flags`), so the same
/// raw-text `contains` comparisons apply -- including their shared
/// assumption that a merged-flatten token matches its per-key flatten
/// textually. Callers only invoke this when the running root differs
/// from the target root; otherwise every atom is target-rooted and the
/// empty set is the whole answer (strict no-op).
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
    /// The same dependency string re-flattened over just the build-time
    /// keys (`DEPEND`/`BDEPEND`) and just the run-time keys
    /// (`RDEPEND`/`PDEPEND`/`IDEPEND`) -- an atom present in the first and
    /// absent from the second is a `QueueItem::buildtime_hard` edge. `&[]`
    /// (all edges soft) at the `--with-test-deps` call site: a `test?`
    /// dep is `DepPriority.optional`, never a hard cycle contributor.
    pub(crate) buildtime_atoms: &'a HashSet<String>,
    pub(crate) runtime_atoms: &'a HashSet<String>,
    /// Track X Slice B fix (#242): `BDEPEND` alone (subset of
    /// `buildtime_atoms` above) -- a running-root `BDEPEND` edge is
    /// `DepPriority` soft (its own bootstrap cycle scope, terminating
    /// like the old sidecar `seen` guard instead of reporting), while a
    /// running `DEPEND` edge stays hard (the cross-root `DEPEND` cycle
    /// around `cyc0z-3` still reports, the #207 flip). Empty at the
    /// `--with-test-deps` call sites and in the unit-test pin below.
    pub(crate) bdepend_atoms: &'a HashSet<String>,
    pub(crate) or_universe: &'a HashSet<String>,
    /// Track X Slice B (#242): `dep_running_tags` over the same dependency
    /// string -- an atom in this set resolves against the running root
    /// (its `QueueItem::targets_running_root`). Empty at the
    /// `--with-test-deps` call sites (a `test?` dep stays target-rooted)
    /// and everywhere the roots coincide (strict no-op).
    pub(crate) running_atoms: &'a HashSet<String>,
    /// Track X Slice C (#242): the expanding entry's own root flag (its
    /// `GraphEntry::targets_running_root`) -- the owner half of the
    /// root-qualified edge each queued atom records. `false` for the
    /// `--with-test-deps` call sites (a `test?` dep stays target-rooted)
    /// and everywhere else the owner is target-rooted.
    pub(crate) owner_targets_running_root: bool,
}

/// Queues every atom in `flat_deps` (a `use_reduce_flat`/
/// `use_reduce_flat_subset` result) onto `queue` at `depth + 1`, owned by
/// `key`/`version`, splitting off a blocker atom into `pending_blockers`
/// instead -- shared by `resolve_pretend_graph`'s own normal-deps queueing
/// and its `--with-test-deps` follow-up below, so the two can't drift
/// apart on blocker handling or `depth`/`owner` bookkeeping.
///
/// `parent_use` (the owning package's own already-computed effective USE
/// -- the exact same set passed as `use_reduce_flat`'s own `uselist`
/// argument for this same dependency string) evaluates each token's own
/// PMS 8.3.4 conditional use-deps (`flag?`/`!flag?`/`flag=`/`!flag=`)
/// before it's ever queued or classified as a blocker -- real
/// `use_reduce`'s own per-token integration point (see
/// `portage_dep::evaluate_atom_conditionals`'s own doc comment), applied
/// uniformly to every token the same way real portage does, blockers
/// included (a blocker atom can syntactically carry use-deps too, e.g.
/// `!foo/bar[baz=]`). A token that fails to re-evaluate (shouldn't
/// happen for anything `use_reduce_flat` itself already accepted, but
/// treated the same "can't tell, so pass it through as-is" way an
/// unparseable-at-all token already silently falls through the
/// `parse_atom` check below) is queued unevaluated rather than dropped.
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
    // Backlog #90 (S1): real pops each deplist last-declared-first
    // (stack discipline), BUT drains every plain dep before touching a
    // `||` group (`_dep_disjunctive_stack` only pops on an empty
    // `dep_stack`). So: plains in reverse declaration order first, then
    // the `||`-chosen tokens (reversed among themselves, the same
    // stack discipline one level out). A blanket `.rev()` would schedule
    // a `||` choice ahead of a later-declared plain dep -- the
    // orbtblocked shape (`|| (...) =1.0` must file 1.0 first, staged
    // oracle). `flat_deps` itself already serializes plains before
    // `||`-chosen (`resolve_disjunctions`); the partition only has to
    // re-split them.
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
        // Track X Slice B fix (#242): a running-root `BDEPEND` edge is
        // soft -- its bootstrap cycle (`rdrcyca <-> rdrcycb`) terminates
        // instead of reporting, like the old sidecar `seen` guard did --
        // while a running `DEPEND` edge stays hard (the `cyc0z-3`/`cyc0y`
        // cross-root `DEPEND` cycle still reports, the #207 flip, which
        // this must not touch). Bound on the raw token like
        // `buildtime_hard`/`targets_running` below (same flatten family,
        // same raw-text assumption; `||` groups never span keys).
        let targets_running_first = running_atoms.contains(&tok);
        // Track X Slice C (#242): the running-`BDEPEND`-soft exception
        // above does not apply to a self-naming atom (its `cat/pkg` is
        // the owner's own): real never relaxes a build-time edge in
        // `_serialize_tasks`, so a running-root self loop dead-ends and
        // aborts exactly like a target-root one (`depgraph.py:10262`),
        // while a non-self bootstrap ring keeps Slice B's terminating
        // scope. Bound on the raw token like everything else here.
        let self_naming = portage_dep::parse_atom(&tok).is_some_and(|at| {
            at.blocker == portage_dep::Blocker::None && at.category == key.0 && at.package == key.1
        });
        let buildtime_hard = buildtime_atoms.contains(&tok)
            && !runtime_atoms.contains(&tok)
            && !(targets_running_first && bdepend_atoms.contains(&tok) && !self_naming);
        // Track X Slice B (#242): the per-group root travels with the
        // atom -- bound on the raw token here, before evaluation moves
        // it, exactly like `buildtime_hard` (same flatten family, same
        // raw-text form assumption).
        let targets_running = targets_running_first;
        // `evaluate_atom_conditionals` returns `Some(...)` even when
        // nothing changed (the common case: no conditional use-deps at
        // all) -- only `None` on a genuinely unparseable atom. So
        // "was this atom actually rewritten" has to compare the
        // before/after text, not just match on `Option` -- a real
        // rewrite is exactly what `unevaluated` (the real
        // "unevaluated_atom" `_show_unsatisfied_dep` consults) should
        // ever be populated for.
        let evaluated = portage_dep::evaluate_atom_conditionals(&tok, parent_use)
            .unwrap_or_else(|| tok.clone());
        let unevaluated = if evaluated != tok { Some(tok) } else { None };
        let tok = evaluated;
        if let Some(dep_atom) = portage_dep::parse_atom(&tok)
            && dep_atom.blocker != portage_dep::Blocker::None
        {
            // #143 S1: real's `blocker.priority.buildtime` — the
            // token survived the build-time-keys flatten, evaluated
            // against the same raw text `buildtime_hard` uses below
            // (bound before `tok` moves into `atom_str`).
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
                // A merged candidate's own dependency string -- not the
                // vdb-scan producer (`collect_unwalked_installed_blockers`).
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
