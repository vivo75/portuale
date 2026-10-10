//! Fold the entry's dependency keys and queue the atoms they name.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::sync::Arc;

/// The values [`enqueue_entry_deps`] reads from its caller.
pub(crate) struct EnqueueEntryDepsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) depth: u32,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) config: &'a Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) candidate_str: &'a str,
}

/// Fold the entry's dependency keys and queue the atoms they name.
pub(crate) fn enqueue_entry_deps(args: EnqueueEntryDepsArgs<'_>) {
    let EnqueueEntryDepsArgs {
        ctx,
        depth,
        metadata,
        candidate_source,
        state,
        entry_idx,
        use_flags,
        bp,
        config,
        union_constraints,
        key,
        version,
        candidate_str,
    } = args;
    // Real `depgraph.py:4195-4247` (`_add_pkg_dep_string`): for a
    // *built* package (`pkg.built` -- true for a binary, not for a
    // fresh ebuild) with `myparams["bdeps"]` not in `("y","auto")`
    // -- i.e. `--usepkg`/`--getbinpkg` given, which leaves `bdeps`
    // unset (`create_depgraph_params.py:100`) -- `DEPEND` and
    // `BDEPEND` are both emptied before the walk. `with_bdeps`
    // (built from that same rule in `pretend.rs`) carries the bit;
    // a binary's recorded `BDEPEND` (e.g. libgweather's `|| ( (
    // python:3.14 pygobject[...] ) ... )`) must not drive a merge.
    //
    // #143 S1: real `depgraph.py:4176-4183` (`_add_pkg_dep_string`)
    // blanks `RDEPEND`/`PDEPEND`/`IDEPEND` for a *non-built*
    // package under `--buildpkgonly` without `--deep` — runtime
    // blockers never enter the graph (upstream pg1 E/F). A binary
    // or installed candidate is `pkg.built` and keeps its keys.
    //
    // Backlog #194: real `depgraph.py:4186-4193` (`_add_pkg_deps`)
    // blanks an `--onlydeps` root's (`pkg.onlydeps` -- the
    // arg-selected package, i.e. this walk's `depth == 0`)
    // `RDEPEND` and `PDEPEND` when `--onlydeps-with-rdeps=n`, and
    // its `IDEPEND` too unless `--onlydeps-with-ideps` is `y` or
    // `True` (`man emerge.1`: rdeps enabled by default so only an
    // explicit `=n` blanks; ideps disabled by default so only an
    // explicit `=y`/bare flag keeps). Build-time keys
    // (`DEPEND`/`BDEPEND`) always stay (`man emerge.1`: "When this
    // is disabled only build time dependencies are included").
    // Without `--onlydeps` both flags are inert (no `pkg.onlydeps`
    // node exists to blank).
    let onlydeps_blanks_runtime = ctx.onlydeps && depth == 0 && !ctx.onlydeps_with_rdeps;
    let onlydeps_blanks_idepend = onlydeps_blanks_runtime && !ctx.onlydeps_with_ideps;
    // Real `bin/ebuild.sh`'s own `depend` phase (`if !
    // ___eapi_has_IDEPEND; then unset IDEPEND; fi`) + real
    // `portage/eapi.py:135` (`eapi_has_idepend`): below EAPI 8
    // `IDEPEND` never reaches saved metadata, so `portdb.aux_get`
    // serves `""` for it -- for ebuild and binary candidates alike
    // (real's `bintree` defaults a missing `EAPI` to `"0"`, same as
    // `porttree.py:644-648` does for ebuilds). #194's EAPI-7 oracle
    // cell needs this: `--onlydeps-with-rdeps=y` on an EAPI-7 root
    // merges `[B, C, D]`, never `E`.
    let eapi_has_idepend =
        md5_dict::eapi_has_idepend(metadata.get("EAPI").map(String::as_str).unwrap_or("0"));
    let keep_key = |key: &&str| {
        onlydeps_walk_dep_key(OnlydepsWalkDepKeyArgs {
            key,
            eapi_has_idepend,
            onlydeps_blanks_runtime,
            onlydeps_blanks_idepend,
        })
    };
    let base_keys: &[&str] = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    } else if ctx.buildpkgonly
        && candidate_source != CandidateSource::Binary
        && matches!(ctx.deep, Deep::NotRequested)
    {
        &["DEPEND", "BDEPEND"]
    } else {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    };
    // The filter only ever removes keys, so when nothing is blanked
    // the static slices above are used as-is (no per-package alloc
    // on the hot path).
    let filtered_keys: Vec<&str>;
    let dep_keys: &[&str] = if onlydeps_blanks_runtime || !eapi_has_idepend {
        filtered_keys = base_keys.iter().copied().filter(keep_key).collect();
        &filtered_keys
    } else {
        base_keys
    };
    let mut depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            depstr.push_str(d);
            depstr.push(' ');
        }
    }
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();

    // Real graph *discovery* order (`GraphEntry::deps`, see
    // its own doc comment): real `_add_pkg_dep_string` walks
    // `RDEPEND`, `IDEPEND`, `PDEPEND`, `DEPEND`, `BDEPEND` in that
    // exact order (`depgraph.py:4253-4289`'s own `deps` tuple) --
    // a different order than `dep_keys` above (which only cares
    // which keys are included, not their order). Each key is
    // flattened separately so cross-key token order can't leak in;
    // `use_reduce_flat` transparently drops `||`/`(`/`)` markers,
    // listing every branch's atoms in written order (the *other*
    // branches are simply never a real entry, so they contribute
    // no edge once the merge-order digraph looks them up).
    let base_order_keys: &[&str] = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps
    {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    } else if ctx.buildpkgonly
        && candidate_source != CandidateSource::Binary
        && matches!(ctx.deep, Deep::NotRequested)
    {
        // #143 S1: same runtime-key blanking as `dep_keys`
        // above — runtime edges must not reach the merge-order
        // digraph under `--buildpkgonly` either.
        &["DEPEND", "BDEPEND"]
    } else if state.entries[entry_idx].targets_running_root {
        // Track X Slice B fix (#242): a running-root build entry's
        // own `deps` preserve real's insertion order -- `BDEPEND`
        // before `RDEPEND` (`rdrtooldep` before `rdrlib`), like the
        // old sidecar `["DEPEND", "BDEPEND", "RDEPEND", "IDEPEND"]`
        // walk did -- instead of the target-root
        // `RDEPEND`-first `deps` tuple above. `DEPEND`-only
        // running entries (the `cyc0z-3`/`cyc0y` cross-root
        // `DEPEND` cycle, the #207 flip) carry a single key either
        // way, so the flip is untouched.
        &["DEPEND", "BDEPEND", "RDEPEND", "IDEPEND", "PDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    };
    // #194: the same blanked keys stay out of the merge-order
    // digraph (real never walks them, so they contribute no edge).
    let filtered_order_keys: Vec<&str>;
    let portage_order_keys: &[&str] = if onlydeps_blanks_runtime || !eapi_has_idepend {
        filtered_order_keys = base_order_keys.iter().copied().filter(keep_key).collect();
        &filtered_order_keys
    } else {
        base_order_keys
    };
    state.entries[entry_idx].deps = merge_order::dep_edges_from_metadata(
        metadata,
        use_flags,
        portage_order_keys,
        candidate_source == CandidateSource::Binary,
    );
    // Track X Slice B fix (#242): the running-root subgraph's own
    // cycle scope for the merge-order digraph too -- a running
    // `BDEPEND` edge back into an already-visited package (the
    // `rdrcycb -> rdrcyca` bootstrap closing edge) is dropped here,
    // like the old sidecar `seen` guard dropped its second
    // `required_by` edge (and like the `required_by` cut above drops
    // it for `find_hard_cycles`). A running `DEPEND` edge back is
    // kept (the `cyc0z-3`/`cyc0y` cross-root `DEPEND` cycle still
    // reports, the #207 flip), as is every first-visit edge (its
    // target is not visited yet -- `rdrtooldep`/`rdrlib` stay, so
    // the insertion-order pin above still orders them). Key `4`
    // is `BDEPEND` (`DepEdge::key`'s own `0=R,1=I,2=P,3=D,4=B`
    // numbering); visited is compared by `cat/pkg` (bare and
    // versioned atom texts alike collapse to their cp, matching
    // `required_by_map`'s own cp keying).
    if state.entries[entry_idx].targets_running_root {
        let mut visited_cps: HashSet<(String, String)> = HashSet::new();
        for v in state.visited_atoms.iter() {
            if let Some(a) = portage_dep::parse_atom(v) {
                visited_cps.insert((a.category, a.package));
            }
        }
        // Track X Slice C (#242): real `_add_pkg` drops a direct
        // self-edge "unless it is an unsatisfied build-time
        // dependency" (`depgraph.py:3765-3770`) -- and
        // `_serialize_tasks` dead-ends on exactly that kept edge
        // (`depgraph.py:10262`, the g216 running-root self cycle).
        // The owner's own cp is trivially "visited" (its own pulling
        // atom resolved to it), so without the exemption below this
        // cut deletes the true self-loop along with the
        // bootstrap-ring closing edges it exists for
        // (`rdrcycb -> rdrcyca`) -- and the pass-1 abort never fires.
        // A satisfied self-edge is still dropped downstream, by
        // `build_digraph`'s own self-edge rule and
        // `owner_self_loop`'s installed gate.
        let own_category = state.entries[entry_idx].category.clone();
        let own_package = state.entries[entry_idx].package.clone();
        state.entries[entry_idx].deps.retain(|d| {
            !(d.key == 4
                && !(d.category == own_category && d.package == own_package)
                && visited_cps.contains(&(d.category.clone(), d.package.clone())))
        });
    }

    // Per-edge build-time/run-time classification for the
    // merge-order sort + circular-dependency detection (real
    // `DepPriority`): re-flatten just the build-time keys and just
    // the run-time keys, so `enqueue_flat_deps` can tag an atom
    // present in the first and absent from the second as
    // `buildtime_hard`. A plain (non-disjunctive) `use_reduce_flat`
    // is enough here -- the classification only needs which key an
    // atom came from, and a `||` group's branch choice matches the
    // main flatten below in every case without one.
    let flatten_keys = |keys: &[&str]| -> HashSet<String> {
        let joined: String = keys
            .iter()
            .filter_map(|k| metadata.get(*k))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let toks: Vec<String> = joined.split_whitespace().map(String::from).collect();
        portage_use_reduce::use_reduce_flat(&toks, use_flags, portage_use_reduce::MatchMode::Normal)
            .map(|v| v.into_iter().filter(|t| t != "||").collect())
            .unwrap_or_default()
    };
    let buildtime_atoms = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps {
        // Build-time deps were dropped from the walk above -- no
        // queued atom can be classified from them.
        HashSet::new()
    } else {
        flatten_keys(&["DEPEND", "BDEPEND"])
    };
    // #194: a blanked runtime key classifies nothing -- real never
    // walks it, so an atom that only survives via a build-time key
    // is `buildtime_hard`, not runtime. A *walked* `IDEPEND` still
    // classifies (rdeps=n + ideps=y on EAPI 8), so the selection
    // mirrors `onlydeps_walk_dep_key`, not just the rdeps blank.
    // Without blanking this is the long-standing unconditional
    // three-key set.
    let runtime_keys: &[&str] = onlydeps_runtime_keys(OnlydepsRuntimeKeysArgs {
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    });
    let runtime_atoms = flatten_keys(runtime_keys);
    // Track X Slice B fix (#242): `BDEPEND` alone, for the running-
    // `BDEPEND`-soft rule in `enqueue_flat_deps` (a running `BDEPEND`
    // edge is soft so its bootstrap cycle terminates; a running
    // `DEPEND` edge stays hard so the `cyc0z-3` cross-root `DEPEND`
    // cycle still reports -- the #207 flip). Same plain flatten
    // family as `buildtime_atoms`/`runtime_atoms` above (groups never
    // span keys, so the `||` branch choice matches). Empty for a
    // binary without `with_bdeps`, mirroring `buildtime_atoms`.
    let bdepend_atoms = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps {
        HashSet::new()
    } else {
        flatten_keys(&["BDEPEND"])
    };
    // Track X Slice B (#242): this expansion's per-group roots --
    // real `_add_pkg_deps`' `depend_root` selection plus the
    // `BDEPEND`/`IDEPEND`-against-running-root groups. `None` on every
    // same-root run (the whole walk stays target-rooted, a strict
    // no-op); on a cross-root run each queued atom carries its group
    // root with it (`QueueItem::targets_running_root`), candidates
    // select against that root's installed db, and same-root entries
    // alone satisfy the in-graph preference below.
    let cross_running_root: Option<&Path> = ctx.cross_root();
    let running_tags: HashSet<String> = cross_running_root
        .map(|_| {
            dep_running_tags(
                metadata,
                use_flags,
                state.entries[entry_idx].targets_running_root,
            )
        })
        .unwrap_or_default();
    // Whether one `||` alternative (a single dep key, hence a single
    // group root -- groups never span keys) resolves running-rooted:
    // every member tagged running (mixed-key same text falls target,
    // matching the queue tag's target-wins rule). Gated on an active
    // cross root so same-root runs keep the pass-through below.
    let group_running = |atoms: &[String]| -> bool {
        cross_running_root.is_some() && atoms.iter().all(|a| running_tags.contains(a))
    };
    // Real `--root-deps` branch-selection feed-in (see
    // `root_deps_satisfied_atoms`'s own doc comment): a `||` group
    // with no branch tree-visible still needs a branch selected
    // here too, not just in `root_deps_satisfied_atoms`'s own
    // separate re-flatten -- otherwise the *other*, genuinely
    // unsatisfiable branch would remain in `flat_deps` and get
    // queued as an ordinary (and wrongly reported) dependency. Real
    // `--root-deps` only ever applies to `DEPEND`/`BDEPEND` -- this
    // closure can't tell which of the five merged dep keys a given
    // atom came from (portuale's own single-unified-graph
    // architecture merges them into one combined string before
    // flattening at all -- the same documented limitation
    // `LLM/agent-context.md`'s own `--root-deps` backlog entry
    // names for the bigger, still-unattempted "recursive second-root
    // graph" gap), so an `RDEPEND`/`PDEPEND`/`IDEPEND` `||` group
    // gets this same permissive check too -- harmless in practice
    // (those atoms almost always resolve via ordinary tree
    // visibility already; a running-root coincidence only ever
    // widens acceptance, never narrows it).
    // Real `dep_zapdeps` re-choosing a `||` alternative once
    // backtracking has masked the preferred one: an atom whose
    // only satisfying candidates are excluded by the negatives
    // this loop accumulated for its `cat/pkg` (`slot_constraints`,
    // = `runtime_pkg_mask`) counts as unsatisfiable, so the next
    // alternative wins on the retry -- `disjunction_preference`
    // takes `slot_constraints` itself (not a lookup closure over
    // it; see that function's own doc comment for why).
    // Backlog #216: real `dep_zapdeps` has no self-exclusion on the
    // first pass -- e.g. `dev-lang/go`'s BDEPEND
    // `|| ( >=dev-lang/go-<min> dev-lang/go-bootstrap-<min> )` with
    // in-graph `go` ranks the first branch `preferred_in_graph`
    // (bin 0), so the cycle forms and the `circular_dependency`
    // map re-resolves it (`BacktrackParams::circular_dependency`);
    // only a self atom matching nothing in-graph (and nothing
    // installed) refuses. See `disjunction_preference`'s own
    // circular-self-dep check.
    let self_cp = (
        state.entries[entry_idx].category.clone(),
        state.entries[entry_idx].package.clone(),
    );
    // Track X Slice C (#242): the node key for the resolver map --
    // the expanding entry's own root (real's node-identity keying).
    let self_node: RootCp = (
        state.entries[entry_idx].targets_running_root,
        state.entries[entry_idx].category.clone(),
        state.entries[entry_idx].package.clone(),
    );
    // The whole `dep_zapdeps` choice-bin classification lives in
    // the single shared `disjunction_preference` helper -- see its
    // own doc comment (2026-09-11 review F1/F2: this closure and
    // `enqueue_dependencies`'s used to be two independently
    // hand-maintained copies, and only this one got the fine
    // `unsat_use_*` bins).
    // Backlog #90 (S1): the queued-atom snapshot for the
    // drain-state approximation in `atoms_all_in_graph` -- real
    // resolves `||` only after the dep stack drains, so a
    // queued-but-unresolved dep counts as will-be-in-graph.
    let queued: Vec<QueueItem> = state.queue.iter().cloned().collect();
    // #249: real expands a new-style virtual's deps inline, inside the
    // dep_check of the package that pulled it, and its `dep_zapdeps`
    // chains `circular_dependency.get(parent)` and
    // `.get(virt_parent)` (`portage/dep/dep_check.py:673-678`, with
    // `virt_parent` set at `:236-252`). Portuale walks the virtual as
    // its own node, so `self_cp` is already real's `virt_parent` key;
    // the `parent` half is the virtual's puller(s), whose recorded
    // cycle children join the lookup here. (Real runs the expansion
    // once per pulling edge; the union over this pass's pullers is
    // the one-node approximation.)
    let circ_with_pullers: Option<HashMap<RootCp, Vec<CircularDepChild>>> =
        if self_node.1 == "virtual" && !bp.circular_dependency.is_empty() {
            let mut extra: Vec<CircularDepChild> = Vec::new();
            if let Some(pullers) = state.required_by_map.get(&self_cp) {
                let mut pullers: Vec<&(String, String)> = pullers.iter().collect();
                pullers.sort();
                for p in pullers {
                    // Track X Slice C (#242): pullers are cp-keyed
                    // (that map stays root-blind); the cycle map is
                    // node-keyed, so consult both roots -- same-root
                    // runs only ever hit the target half.
                    for flag in [false, true] {
                        if let Some(children) =
                            bp.circular_dependency
                                .get(&(flag, p.0.clone(), p.1.clone()))
                        {
                            extra.extend(children.iter().cloned());
                        }
                    }
                }
            }
            (!extra.is_empty()).then(|| {
                let mut m = bp.circular_dependency.clone();
                let bucket = m.entry(self_node.clone()).or_default();
                for c in extra {
                    if !bucket.contains(&c) {
                        bucket.push(c);
                    }
                }
                m
            })
        } else {
            None
        };
    let circular_view = circ_with_pullers
        .as_ref()
        .unwrap_or(&bp.circular_dependency);
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens: &tokens,
            uselist: use_flags,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
            ),
            tie_break: &mut |alts: &[Vec<String>]| {
                // Same root scoping for the tied-alternative promotion
                // (its installed/in-graph checks are the group's own).
                let grp_running = cross_running_root.is_some()
                    && alts
                        .iter()
                        .flat_map(|v| v.iter())
                        .next()
                        .is_some_and(|a| running_tags.contains(a));
                let grp_root: &Path = if grp_running {
                    cross_running_root.expect("running-root group without a cross root")
                } else {
                    ctx.root
                };
                let scoped_entries: Option<Vec<GraphEntry>> = cross_running_root.map(|_| {
                    state
                        .entries
                        .iter()
                        .filter(|e| e.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let entries_view: &[GraphEntry] =
                    scoped_entries.as_deref().unwrap_or(&state.entries);
                let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
                    queued
                        .iter()
                        .filter(|q| q.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
                promote_tied_alternative(PromoteTiedAlternativeArgs {
                    repos: &ctx.repos,
                    config,
                    root: grp_root,
                    entries: entries_view,
                    constraints: union_constraints,
                    alts,
                    queued: queued_view,
                })
            },
        },
        &mut |atoms: &[String]| {
            // Track X Slice B (#242): branch selection sees the
            // group's own root -- its installed db, and only the
            // same-root graph (a target-rooted argument never
            // satisfies a running-root dep's in-graph preference, and
            // vice versa). Pass-through unless cross-root and running.
            let grp_running = group_running(atoms);
            let grp_root: &Path = if grp_running {
                cross_running_root.expect("running-root group without a cross root")
            } else {
                ctx.root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_running_root.map(|_| {
                state
                    .entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(&state.entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
                queued
                    .iter()
                    .filter(|q| q.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
            disjunction_preference(DisjunctionPreferenceArgs {
                repos: &ctx.repos,
                config,
                root: grp_root,
                entries: entries_view,
                self_cp: &self_node,
                constraints: union_constraints, // Backlog #221: this pass's recorded cycle edges --
                // empty on the first attempt, populated on the
                // restart after a stranded walk (see
                // `collect_feedback`); #249 adds a virtual's pullers.
                circular_dependency: circular_view,
                root_deps_running_root: ctx.root_deps_running_root,
                atoms,
                queued: queued_view,
                update: ctx.update,
                gate_entries: &state.entries,
                gate_queued: &queued,
            })
        },
    ) else {
        return;
    };
    // Track X Slice B (#242): the existence-check split below only
    // runs for flag-on coinciding-roots builds (its long-pinned
    // behavior there). On a cross-root run the unified per-group walk
    // above already resolved every build atom against its own root, so
    // filtering here would double-resolve (and disconnect) them.
    let root_deps_split: Option<&Path> = ctx
        .root_deps_running_root
        .filter(|_| cross_running_root.is_none());
    // `--root-deps`: real `ESYSROOT`-vs-`ROOT` distinction (see
    // `root_deps_satisfied_atoms`'s own doc comment for the full
    // grounding and its documented scope cut) -- a strict no-op when
    // `root_deps_split` is `None`, matching every pre-existing
    // call site/test.
    let root_deps_satisfied: HashSet<String> = root_deps_split
        .map(|root| {
            root_deps_satisfied_atoms(RootDepsSatisfiedAtomsArgs {
                metadata,
                use_flags,
                repos: &ctx.repos,
                config,
                running_root: root,
                dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                    metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            })
        })
        .unwrap_or_default();
    // Real "recursively pull in and build new packages against the
    // running root" (see `resolve_root_deps_build_entries`'s own doc
    // comment for the full grounding): the *other* half of the same
    // real `DEPEND`/`BDEPEND` set `root_deps_satisfied` above already
    // covers -- every atom in it isn't satisfied by the running root
    // either, so (unlike before the first `--root-deps` build-entry
    // slice) it must *not* fall through into the ordinary `flat_deps`
    // queue below and get wrongly resolved against `ROOT` instead
    // (real `DEPEND`/`BDEPEND` never targets `ROOT`/`ESYSROOT` at all
    // under portuale's own established `--root-deps` simplification
    // -- see `root_deps_satisfied_atoms`'s own doc comment). Each one
    // instead gets resolved against the running root directly, the
    // same way any other atom would be, added as its own
    // `targets_running_root` entry, and recursed into. Kept as a
    // `Vec` (not a `HashSet`) so the resulting entry order is
    // deterministic. A strict no-op when `root_deps_split` is
    // `None`, matching every pre-existing call site/test.
    let root_deps_unsatisfied: Vec<String> = root_deps_split
        .map(|root| {
            unsatisfied_root_deps_atoms(UnsatisfiedRootDepsAtomsArgs {
                metadata,
                use_flags,
                repos: &ctx.repos,
                config,
                running_root: root,
                dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                    metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            })
        })
        .unwrap_or_default();
    // #133: the two sets above are evaluated-keyed (real evaluates at
    // every producer), while `flat_deps` here is still raw --
    // `enqueue_flat_deps` evaluates later, at queue time -- so the
    // membership test compares evaluated forms. The surviving tokens
    // stay raw on purpose: `buildtime_atoms`/`runtime_atoms` below
    // are raw-keyed flattens, and `enqueue_flat_deps` runs its own
    // post-pass over whatever survives.
    let flat_deps: Vec<String> = flat_deps
        .into_iter()
        .filter(|tok| {
            let evaluated = portage_dep::evaluate_atom_conditionals(tok, use_flags)
                .unwrap_or_else(|| tok.clone());
            !root_deps_satisfied.contains(&evaluated)
        })
        .filter(|tok| {
            let evaluated = portage_dep::evaluate_atom_conditionals(tok, use_flags)
                .unwrap_or_else(|| tok.clone());
            !root_deps_unsatisfied.contains(&evaluated)
        })
        .collect();
    if let Some(running_root) = root_deps_split {
        for atom_str in &root_deps_unsatisfied {
            state.entries.extend(resolve_root_deps_build_entries(
                ResolveRootDepsBuildEntriesArgs {
                    repos: &ctx.repos,
                    running_root,
                    atom_str,
                    config,
                    owner: key.clone(),
                    seen: &mut state.root_deps_build_seen,
                    local_binpkg: &ctx.local_binpkg,
                },
            ));
        }
    }
    // Backtracking (slices 3/4): record this package as a puller of
    // every non-blocker `cat/pkg` its dependency string names,
    // keeping the atom text for slice 4's `pulled in by` lines,
    // before `flat_deps` is consumed below.
    for tok in &flat_deps {
        if let Some(dep_atom) = portage_dep::parse_atom(tok)
            && dep_atom.blocker == portage_dep::Blocker::None
        {
            state
                .slot_pullers
                .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                .or_default()
                .push((
                    key.0.clone(),
                    key.1.clone(),
                    version.to_string(),
                    tok.clone(),
                ));
        }
    }
    enqueue_flat_deps(EnqueueFlatDepsArgs {
        flat_deps,
        key,
        version,
        depth,
        parent_use: use_flags,
        queue: &mut state.queue,
        pending_blockers: &mut state.pending_blockers,
        buildtime_atoms: &buildtime_atoms,
        runtime_atoms: &runtime_atoms,
        bdepend_atoms: &bdepend_atoms,
        or_universe: &or_group_universe(&tokens),
        running_atoms: &running_tags,
        owner_targets_running_root: state.entries[entry_idx].targets_running_root,
    });

    // --with-test-deps: additive on top of the normal deps just
    // queued above, never a replacement for them -- see
    // resolve_pretend_graph's own doc comment for the full gating
    // (depth == 0, "test" a valid, not-already-enabled, not-masked
    // IUSE flag) and why this needs use_reduce_flat_subset rather
    // than another plain use_reduce_flat pass.
    if ctx.with_test_deps && depth == 0 && !use_flags.contains("test") {
        let iuse_flags: HashSet<String> = metadata
            .get("IUSE")
            .map(String::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        let test_masked = config.use_mask.contains("test")
            || specificity_ordered_flags(
                &config.package_use_mask,
                candidate_str,
                &key.0,
                &key.1,
                HashSet::new(),
            )
            .contains("test");
        if iuse_flags.contains("test") && !test_masked {
            let mut test_uselist = use_flags.clone();
            test_uselist.insert("test".to_string());
            let subset: HashSet<String> = ["test".to_string()].into_iter().collect();
            if let Ok(test_deps) = portage_use_reduce::use_reduce_flat_subset(
                &tokens,
                &test_uselist,
                portage_use_reduce::MatchMode::Normal,
                &subset,
            ) {
                enqueue_flat_deps(EnqueueFlatDepsArgs {
                    flat_deps: test_deps,
                    key,
                    version,
                    depth,
                    parent_use: &test_uselist,
                    queue: &mut state.queue,
                    pending_blockers: &mut state.pending_blockers, // A `test?` dep is `DepPriority.optional` --
                    // never a hard cycle contributor.
                    buildtime_atoms: &HashSet::new(),
                    runtime_atoms: &HashSet::new(), // Track X Slice B fix (#242): no `BDEPEND`
                    // origin for a `test?` dep (never hard anyway).
                    bdepend_atoms: &HashSet::new(), // `test?`-gated deps are plain atoms, never a
                    // `||` choice (the subset filter drops the
                    // group structure) -- empty universe.
                    or_universe: &HashSet::new(), // Track X Slice B (#242): a `test?` dep stays
                    // target-rooted.
                    running_atoms: &HashSet::new(),
                    owner_targets_running_root: state.entries[entry_idx].targets_running_root,
                });
            }
        }
    }
}
