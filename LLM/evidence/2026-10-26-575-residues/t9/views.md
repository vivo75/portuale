# Task 9 S0a: which view decides an installed consumer's `||` (settles t8 `oracle.md` §E)

Portage 3.0.83 (`3rdparty/portage` at tag `portage-3.0.83`; `lib/_emerge/depgraph.py` is
byte-identical to the host copy, checked with `cmp`). D = `lib/_emerge/depgraph.py`,
DC = `lib/portage/dep/dep_check.py`, PT = `lib/_emerge/resolver/package_tracker.py`.

## The rule

For each pass (one depgraph per backtrack try), an installed consumer X that becomes a
graph node has its `||` (and its `virtual/*` atoms) decided **once per dep key**, in the
`_create_graph` call that pops X's Package off `_dep_stack`:
- **`OrView::Normal`** (`_filtered_trees`) when X is added during the forward walk **and**
  `deep is True || X.depth + 1 <= deep`, where `deep` is `--deep` (`True` for a bare
  `--deep` or `--emptytree`, an int for `--deep=N`, 0 if absent) and `X.depth` is its
  smallest depth from the arguments (argument = 0, each dep edge +1).
- **`OrView::PostMerge`** (`_graph_trees`) otherwise: X was parked in `_ignored_deps`, or it
  was first reached in `_complete_graph` (from the `@world` set, depth `UNREACHABLE`, or
  as an installed package pulled in by a broken dep). This happens **only if complete mode
  is entered**. If it is not, X's `||` is never evaluated in that pass, so X adds no parent
  atoms at all.

So: no `--deep` gives PostMerge for every installed consumer, even a depth-0 `@world`
member. A bare `--deep` gives Normal for every installed consumer that is
forward-reachable from the arguments, and PostMerge for one that only `@world` reaches
(for example `-uD <atom>`, where the consumer is not under the atom). `--deep=N` gives
Normal iff `depth < N`. The candidate rule (`requested_depth`, D:3438-3453) has the right
predicate, but the line that decides is the recursion gate at D:3834-3842 (see below).

## Evidence: source

1. **When `||` is evaluated.** `_add_pkg_deps` does not evaluate a `||` at once. It queues
   the `||` and any `virtual/*` atom on `_dep_disjunctive_stack` (D:4397-4401 →
   `_queue_disjunctive_deps` D:4872-4906, virtual at D:4899).
   - `_create_graph` drains `_dep_stack`, then pops one disjunction, and loops until both
     are empty (D:3269-3284).
   - So a `||` is popped (`_pop_disjunction` D:4912 → `_add_pkg_dep_string` →
     `_wrapped_add_pkg_dep_string` D:4508 `self._select_atoms(...)`) in the **same**
     `_create_graph` call that processed its parent.
   - `self._select_atoms` is looked up at call time. It is `_select_atoms_highest_available`
     from depgraph init (D:862; trees=None → `_filtered_trees`, docstring D:6028-6029). It
     is switched to `_select_atoms_from_graph` (→ `_graph_trees`, D:6009-6016) only at
     D:8705, once complete mode starts.
   - Nothing switches it back: `_complete_mode` is assigned only at D:730 (init) and D:8704.
   - Each backtrack pass builds a fresh depgraph (D:857-863), so the view is chosen again
     in every pass.
2. **The gate that decides Normal or deferred** is in `_add_pkg`:
   - `recurse = deep is True or not self._too_deep(self._depth_increment(depth, n=1))` (D:3834);
   - `if pkg.installed and not recurse: dep_stack = self._dynamic_config._ignored_deps`
     (D:3838-3839);
   - `if not previously_added: dep_stack.append(pkg)` (D:3841-3842).

   Supporting pieces:
   - `_too_deep` (D:7387-7403) returns `True` for `UNREACHABLE`, `False` for
     `deep is True`, and `depth > deep` otherwise.
   - `deep` is `myparams["deep"]`, set from `--deep` (`create_depgraph_params.py:157-159`)
     and forced to `True` by `--emptytree` (:176-179).
   - A non-installed node always recurses. `--nodeps` drops `recurse`, and then no deps
     are evaluated at all (D:3836-3837, `create_depgraph_params.py:181-184`).
3. **The deferred consumers get evaluated in complete mode.** `_complete_graph` does the
   following:
   - it enters complete mode at D:8704-8713: `_select_atoms_from_graph`,
     `_select_pkg_from_graph`, `_traverse_ignored_deps`, and `deep` forced to `True`, so
     everything added from then on recurses;
   - it adds the `@world` set (`_required_set_names` D:358-361) with
     `depth=_UNREACHABLE_DEPTH` (D:8771-8789); with `--deep` it skips sets that are
     already arguments (D:8752-8758);
   - it moves `_ignored_deps` onto `_dep_stack` and runs `_create_graph` (D:8791-8798);
   - its broken-dep loop also `_add_pkg`s installed packages (D:8803-8837).
4. **When complete mode is entered.** Any of these is enough:
   - `"complete"` is in myparams: `--complete-graph`, `--rebuild-if-new-*`, or a removal
     action (`create_depgraph_params.py:122-124, 169-175`);
   - the auto-detection at D:8635-8691 (on by default): a merge node changes the version,
     slot/sub-slot or USE of an installed package of the same slot, or adds a new slot of
     an installed cp;
   - a slot conflict while backtracking is allowed (`_resolve_conflicts` D:9473-9480).

   Otherwise `_complete_graph` returns at D:8693-8694 before D:8704. `--nodeps` returns
   even earlier (D:8621-8622).
5. **D:3438-3453 is a consequence, not the selector.** It exempts unsatisfied deps of a
   complete-mode installed parent with `depth is UNREACHABLE or depth >= requested_depth`
   (`requested_depth` = the original `deep`, D:311). For an integer depth, "not recursed"
   at D:3834 means `depth + 1 > deep`, which is `depth >= requested_depth`. So it is the
   same set of parents.
6. **The downgrade guards apply in both views (confirmed).** `dep_zapdeps` reads
   `graph_db`, `graph`, `graph_interface` and `downgrade_probe` from `trees[myroot]`
   (DC:405-420). Both tree sets carry them:
   - `_graph_trees`: D:768-771;
   - `_filtered_trees`: D:796-805.

   `conflict_downgrade` (DC:480-490) and `installed_downgrade` (DC:531-541) check only for
   their presence. What differs is `mydbapi`/`vardb` (DC:414-420):
   - PostMerge: the PT fakedb for both, `porttree` = `vartree` = fakedb (D:766-767);
   - Normal: `_dep_check_composite_db` and the real vdb (D:775-777, D:798-800).

   So in PostMerge an arm that only a replaced installed version satisfies has no
   `avail_pkg`. Such an arm leaves the loop at DC:475-478, before either guard runs. The
   guards do fire in PostMerge for a still-graphed lower version, for example an installed
   instance kept by a slot conflict (PT:230-252).
7. **Removal actions** (`depclean`/`prune`): `_graph_trees["porttree"]` is the composite db
   (D:806-811). This is out of scope for `emerge -p`.

## Evidence: probes

Captures `t9/captures/s0a-*.txt` come from `t9/playground/probe-s0a.sh` and
`probe-s0a-both.sh`, which use `views.py`. `views.py` wraps `_add_pkg`,
`_select_atoms_highest_available` and `_complete_graph`, and logs `T9 ADD` (depth, `deep`,
whether X went to `dep_stack` or `ignored`), `T9 OR` (`complete_mode`, trees, call chain,
selection) and the pass number.

Cells: T1 = probe-t8c.sh `swap-noslotop`, orig = `orig-noslotop`, R4 = probe-t8.sh §6.
Common argv: `PYTHONHASHSEED=0 python3 views.py --ignore-default-opts --getbinpkg=n
--usepkg=n --color=n -pv …`, with `PORTAGE_CONFIGROOT=$W/<cell>.cfg ROOT=$W/<cell>`. Each
file starts with its full argv.

| capture | extra argv | X, depth | ADD → | `||` view | selected | merge rows |
|---|---|---|---|---|---|---|
| s0a-t1-nodeep | `dev-libs/p569pull` | p569virt, 1 | ignored | PostMerge | `=p569prov-5.40*` | U p569prov-5.44, N p569pull-2 |
| s0a-t1-deep | `--deep dev-libs/p569pull` | p569virt, 1 | dep_stack | **Normal** | `~p569core-1.0` | + **N p569core-1.0** |
| s0a-t1-deep1 | `--deep=1 …` | p569virt, 1 | ignored | PostMerge | `=5.40*` | as nodeep |
| s0a-t1-deep2 | `--deep=2 …` | p569virt, 1 | dep_stack | Normal | `~p569core-1.0` | as deep |
| s0a-t1-cg | `--complete-graph …` | p569virt, 1 | ignored | PostMerge | `=5.40*` | as nodeep |
| s0a-t1-nocomplete | `--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n …` | p569virt, 1 | ignored | **never evaluated** | — | U 5.44, N pull-2 |
| s0a-orig-nodeep | `dev-libs/p569pull` | p569virt, 1 | ignored | PostMerge (passes 0, 1); pass 2: complete not entered, never evaluated | `=5.42*` | N p569pull-1, 4/20 |
| s0a-orig-deep | `--deep …` | p569virt, 1 | dep_stack | Normal | `~p569core-1.0` | U 5.44, N p569core, N pull-2, 0/20 |
| s0a-r4-deep | `--update --deep @world` (the R4 keeper argv) | mllablgl, 0 | dep_stack | Normal | `mllabltk:=` | rR mllablgl, 1/20 |
| s0a-r4-nodeep | `--update @world` | mllablgl, 0 | ignored | PostMerge | `<mlocaml-4.02` | mlocaml update skipped |
| s0a-r4-deep-atomarg | `--update --deep dev-libs/mlocaml` | mllablgl, UNREACHABLE (via @world) | dep_stack in complete mode | PostMerge | `<mlocaml-4.02` | update skipped |
| s0a-revdep-deep-inworld | `--deep =dev-libs/p569prov-5.44`, p569virt in world | p569virt, UNREACHABLE | dep_stack in complete mode | PostMerge | `=5.40*` | U 5.44 |
| s0a-revdep-deep-notworld | same, p569virt not in world | — | never added | **never evaluated** | — | U 5.44 |

How to read the table:
- T1 and orig with `--deep` show that the view, not the arm order, changes the outcome.
  Normal picks `~p569core-1.0`: `=5.42*` has 5.44 graphed in its slot (guards), `=5.40*`
  and `=5.38*` have no package, and p569core has a tree ebuild. Portage then merges
  p569core.
- R4 needs Normal (its argv has `--deep`). The same root without `--deep`, or with an atom
  argument, picks the other arm.

## Both views in one pass: yes, through `_solve_non_slot_operator_slot_conflicts`

The forward walk evaluates a Package at most once per pass:
- D:3841-3842 pushes it only when it is not `previously_added`;
- re-adding it from `@world` in complete mode does not re-push it (`s0a-r4-deep`: second
  ADD `none(previously_added)`, no second OR).

There is one re-walk, `_solve_non_slot_operator_slot_conflicts`:
- it removes the losing packages (D:2073-2080, `_remove_pkg`);
- it puts their Package parents plus the forced packages back on `_dep_stack`
  (D:2084-2095);
- it runs `_create_graph` again (D:2097-2100).

It runs from `_process_slot_conflicts` (D:2131) after `_complete_graph`, so that re-walk
uses PostMerge whenever complete mode was entered. An X walked in Normal can therefore be
evaluated a second time in PostMerge in the same pass:
- `s0a-both-deep`, argv `--update --deep @world` on the scratch cell `both`:
  - installed-only consumer `t9cons` with `RDEPEND=dev-libs/mlocaml || ( =p569prov-5.40* ~p569core-1.0 )`;
  - pin `a9pin` with `<mlocaml-4.02`;
  - world = mlocaml, t9cons, a9pin.
- Result: `T9 OR pass=0 … complete_mode=False trees=_filtered_trees … selected=['~dev-libs/p569core-1.0']`,
  then `T9 OR pass=0 … complete_mode=True trees=_graph_trees via=…<_solve_non_slot_operator_slot_conflicts … selected=['~dev-libs/p569core-1.0']`.
- `s0a-both-nodeep`, `s0a-r4-nodeep` and `s0a-r4-deep-atomarg` show PostMerge twice in one
  pass along the same route.

**Which atoms become parent atoms:** both selections'.
- `_add_parent_atom` only adds to a set (D:4151-4156).
- `_remove_pkg` drops only the entries whose parent or child is the removed package
  (D:4096-4106).
- So after a re-walk, `_parent_atoms` holds the union of the first selection's atoms
  (unless they pointed at a removed package) and the second selection's.
- `_slot_operator_deps` (D:4158) is appended to the same way and pruned the same way
  (D:4108-4117).

In the probes the second selection always equals the first. The first pick's packages
are graph nodes by then, and all-in-graph choices are promoted (DC:637-649). Not every
re-walk is complete mode: with backtracking off (`--backtrack=0`, or the final
`get_best_run` pass), a slot conflict does not force complete mode (D:9475-9476). The
re-walk is then Normal again, unless the auto-detection at D:8635-8691 turned complete
mode on.

Not a parent-atom source: `_select_atoms_probe` (D:2975-2985) re-evaluates with whatever
`_select_atoms` is current. Its callers:
- `_compute_abi_rebuild_info` D:1005 (display: `s0a-r4-deep` pass 1, `caller=_select_atoms_probe<…<_compute_abi_rebuild_info`);
- `_slot_operator_update_probe` D:2744, D:2765 (from `_process_slot_conflicts` →
  `_slot_operator_trigger_reinstalls` D:2147, or from `_complete_graph` D:8829).

Their results feed decisions only. The probe site (S0f) uses PostMerge when it runs after
complete mode started, and Normal otherwise. `_expand_virt_from_graph` (D:6183) and
`_validate_blockers` (D:9070) also use `_graph_trees`, but only for virtual expansion and
blocker atoms.

## Edge cases

- **Consumer reachable only through `@world`**: PostMerge, even with `--deep`
  (s0a-r4-deep-atomarg, s0a-revdep-deep-inworld).
- **Consumer not reachable from the arguments or `@world`**: never evaluated
  (s0a-revdep-deep-notworld). Portage has no vdb-wide reverse-dependency scan here. With
  `--ignore-world`, `_required_set_names` is empty (D:358-359).
- **Installed consumer replaced by a merge node**: not a graph node, so its `||` is never
  evaluated (t8 `oracle.md` C1b).
- **`--deep=N` and `previously_added`** (code reading, not probed): X first parked at
  depth ≥ N and later reached at a smaller depth gets the smaller `pkg.depth` (D:3806-3816).
  It is not re-pushed (D:3841), so it stays PostMerge.
- **Dep keys are evaluated separately.** Each of RDEPEND, IDEPEND, PDEPEND, DEPEND and
  BDEPEND queues its own disjunction (D:4310-4406). Build-time ones are popped first
  (D:4266-4268). Built-package DEPEND/BDEPEND can be dropped (D:4209-4262).

## Not established

- A pass where the Normal and the PostMerge re-walk choose **different** arms, leaving
  both atom sets in `_parent_atoms`, was not seen. It follows from D:4151-4156 and
  D:4096-4106, but whether any real cell reaches it is unknown.
- The D:3438-3453 = D:3834 equivalence was not probed for installed packages first
  reached in complete mode with an integer depth below `--deep=N`, for example through
  ignored build-time deps (D:4620-4628). It probably never matters for `emerge -p` rows.
- Probe hazard: these roots also load the running root `/` (the host vdb is read for the
  BDEPEND root, and `_complete_graph` adds `@world` for every root, D:8722-8728).
  Nothing was written: the `/var/cache/edb` mtimes are unchanged. The t8 probes did the
  same.
