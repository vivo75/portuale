# Scope backlog

**Not** a Python-vs-Rust parity backlog. Every slice ships on both sides in
one commit, verified byte-identical via the shared contract suite before it
counts as done (`agent-context.md`'s "portability of change, not of
source"). ~1160 cases pass across all suites (869 in the `emerge`-pretend
contract file); an inventory scan (CLI flag tables, function-level
architecture, `--json` fields, git history) still finds zero
Rust-vs-Python behavioural gaps.

This file inventories real portage behaviour **not yet ported to either
side** — deliberate, documented scope cuts and `agent-context.md`
architecture boundaries. It deliberately carries **no shipped-item
narrative**: **`what-this-proves.md` is the authoritative record of what
has shipped**, `git log` is the slice-by-slice history. Re-verify any
entry here against both before trusting it.

> **Compaction passes:** 2026-08-31 (moved the per-slice shipped
> narrative to `what-this-proves.md`), 2026-09-03 (purged the narrative
> that had re-accreted — the pre-purge snapshot is
> [`history/scope-backlog-2026-09-03.md`](history/scope-backlog-2026-09-03.md)).
> The structure: a compact "already done" summary (Part 1), the
> genuinely-remaining work (Part 2), the standing non-goals (Part 3), and
> an honest distance-to-parity assessment (Part 4).

---

## Part 1 — already shipped (one-paragraph summary)

The core `emerge` / `ebuild` loop is **real and live** — it resolves,
builds, merges, and unmerges real Gentoo packages (verified end to end
against an actual tree inside `TEST/`'s container). Shipped, at a
capability-area level (see `what-this-proves.md` for the cited-source
detail of each):

- **`--pretend` dependency resolution** — the full atom / slot / sub-slot
  / USE-dep grammar; `||` groups; every `*DEPEND` key; the `--update` /
  `--deep` / `--newuse` / `--changed-*` / `--with-*` / `--exclude` /
  `--newrepo` / … selection family; every `package.*` file, repo-scoped
  across main **and** overlays; the whole `env.d → repo → features →
  pkginternal → defaults → conf → pkg → env` `USE_ORDER` chain;
  `USE_EXPAND`; `REQUIRED_USE`; keyword / license / PROPERTIES / RESTRICT
  masking; slot‑operator rebuild edges; blocker + slot‑conflict
  detection; a `'backtrack` retry loop that reconciles solvable slot
  conflicts, masks unsolvable ones (`runtime_pkg_mask`), and reports the
  rest with real portage's own notice; the full `resolver/output.py`
  bracket layout + ANSI colour + counters + `--tree` / `--columns`; the
  whole `--autounmask*` read-only family; bare command-line names
  (`emerge eix` → `app-portage/eix`); `--json` provenance trace.
- **Real ebuild phase execution** — the full `pkg_pretend → … → install`
  chain via an embedded `brush` driving unmodified `bin/*.sh`; real
  eclass `inherit()`; real `SRC_URI` fetch (Manifest digests, `mirror://`
  + custom/third-party mirrors, `RESTRICT=mirror`/`fetch`, resume).
- **Real filesystem mutation** — `ebuild <file>` merge / unmerge / qmerge
  / package / config / info / prerm / postrm; real `CONFIG_PROTECT`,
  `collision-protect` / `protect-owned`, preserve-libs (full `LinkageMap`
  computation, wired into merge **and** unmerge), `env_update()` /
  `ldconfig`, fifo/device `CONTENTS` nodes.
- **`emerge` itself, non-`--pretend`** — `emerge <atom>` source
  build+merge (New / Upgrade / Downgrade / Reinstall, in-place same-slot
  replace); `--getbinpkg` / `--getbinpkgonly` remote download+merge;
  `--buildpkgonly`; `FEATURES=buildpkg` / `--buildpkg`; `--keep-going`;
  `emerge -jN` parallel build scheduler + `--load-average` +
  build-log capture + `--quiet-build`; `--resume` / `--skipfirst`
  (mtimedb) incl. `--resume --pretend`; `--ask` / `CLEAN_DELAY`;
  world / world_sets recording (real `create_world_atom`); `@world` /
  `@system` / `@selected` / `@installed` / `@<custom>` sets; `--oneshot`;
  `emerge -C` / `--unmerge` / `--depclean` / `--prune` / `--config` /
  `--deselect` real removal; `elog` (`echo` / `save` / `save_summary`
  modules, merge **and** removal paths); `PORTAGE_NICENESS` /
  `PORTAGE_IONICE_COMMAND` / `PORTAGE_SCHEDULING_POLICY`.
- **Standalone actions** — `--search` / `-s` / `-S` (fuzzy + regex),
  `--list-sets`, `--check-news`, `--info` (incl. `--info <atom>`),
  `--clean`, `--rage-clean`, `--regen`; every recognized-but-unimplemented
  modifier flag from the 2026-09-02 batches.
- **Binary packages** — xpak + gpkg readers/writers, `$PKGDIR` scan,
  `--usepkg` family, all six compressors, `build-info`-into-vdb metadata
  + `:=` binding, gpkg internal `Manifest` digest check.
- **Sandbox / build isolation** — `sandbox` / `usersandbox` +
  `network` / `ipc` / `mount` / `pid`-sandbox all wrap the `src_*`
  phases; the build-phase env carries the resolved `USE` + compiler/make
  flags + `package.env`'s non-USE vars.
- **Misc** — `profiles/updates/` package moves, `color.map` /
  `PORTAGE_COLORMAP`, `--quiet` verbosity level 1, `emerge --help` /
  `portuale` applet listing, `emerge --shell bash|brush` (merge, removal
  hooks, and `--config`).

---

## Part 2 — genuinely still open

### A. Resolver

Every *forward-pass* resolver feature is shipped. What remains is
architectural — a single-pass BFS can't grow into these incrementally:

- **Backtracking.** `resolve_pretend_graph` is a `'backtrack` retry loop
  (real `_emerge/resolver/backtracking.py` shape): it reconciles a
  **solvable** slot conflict, masks a version to resolve an **unsolvable**
  one via real `runtime_pkg_mask` (trial-and-revert), exposes
  `--backtrack=COUNT`, and renders the real slot-collision (now with
  `collision_reasons` grouping, one-representative-per-reason selection,
  `--verbose-conflicts`, the `^` marker line, and the `(and N more …)` /
  `NOTE:` trailer) and circular-dependency notices. **Still open** (the
  deferred half):
  - "backtracking exhausted" diagnostics — *narrower than it looks:* the
    `--backtrack=30` advisory-hint gating already ships; real portage's
    remaining signal is the `Dependency resolution took X s (backtrack:
    N/M).` report line, whose timing is non-deterministic (a deliberate
    cut — portuale is a deterministic tool);
  - autounmask in-loop (plan: `docs/history/autounmask-in-loop-plan.md`) —
    **Slice 0** (`effective_use_flags(&Config)` + `Config::autounmask_use`
    tier) and **Slice 1** (the *backward* cascade: a `[flag]` dep on an
    already-resolved slot folds a `suggested_use_flip` into an
    `autounmask_use_config` accumulator and the driver re-runs the whole
    walk — real `_needed_use_config_changes` / `_feedback_config`),
    **Slice 2** (`_autounmask_levels` ordering: the `*_masked_only`
    fallbacks run `+license` → `+~arch` → `+masks`), **Slice 3**
    (`_autounmask_breakage`: a flag the accumulator ends up wanting both
    ways abandons autounmask wholesale — `myparams["autounmask"] = False`
    — and re-resolves one clean pass) and **Slice 4** (the whole-graph
    parent-flip re-resolve: `'parent_flip` folds the parent-USE flip into
    `autounmask_use_config` and the driver re-walks everything — removes
    the `'parent_flip` single-dep cut), **Slice 5** (`--autounmask-
    backtrack` gate — **off by default**, matching real: collect + display
    the change and re-render the flipped package's USE line, but no
    graph re-walk unless `=y` / `--autounmask-continue`) and **Slice 6**
    (keyword/mask backward cascade — `resolve_pretend`'s `*_masked_only`
    fallback gate is now "no visible candidate satisfies `atom_str` +
    the folded slot constraints", so a slot pulled to a keyword/mask-
    masked version re-resolves there) all **shipped 2026-09-03**, plus a
    follow-up making the fallback a real `_autounmask_levels` per-level
    version re-scan (`visible_with_relax` + cumulative levels; a candidate
    blocked by `~arch` **and** `LICENSE` resolves once both relaxations
    are in play, recording both changes) — **the plan is complete**;
  - the slot-operator-rebuild feedback — complete-graph reachability
    gate, the multi-level sub-slot cascade to a fixpoint, real's `r`
    marker + `str(Package)` "causing rebuilds:" rendering — **shipped
    2026-09-03**, container-verified against real portage 3.0.82.2 via
    `TEST/scripts/40-slotop-cascade.sh`;
    `docs/history/slot-op-rebuild-cascade-plan.md`;
  - `||`-preference feedback driving a retry — **both paths shipped
    2026-09-03**, container-verified: the slot-conflict
    `runtime_pkg_mask → dep_zapdeps` path
    (`TEST/scripts/42-or-backtrack.sh`) and the "missing dependency"
    path (`_feedback_missing_dep`, an unsatisfiable `||` subtree
    yielding to the next alternative — `TEST/scripts/43-or-missing-dep.sh`);
    `docs/history/or-preference-backtrack-plan.md`;
  - the slot-collision notice's remaining cuts: `pkg_use_display` for a
    package with non-default USE (the ` USE=""` slot renders, non-empty
    flag lists don't), the `use`/`soname` reason keys, operator/USE-token
    colorization, the `need_rebuild` "cannot be rebuilt" trailer;
  - the circular-dep cuts: the reduced cycle-only `--tree` re-display,
    full elementary-cycle enumeration / `large_cycle_count`
    (`_find_suggestions`' USE-flag heuristic **shipped 2026-09-03** —
    `circular_dep_solutions`, `docs/history/find-suggestions-plan.md`; the
    grandparent-atom conflict path has no fixture yet).

  *(The resolver-extraction item shipped 2026-09-03: the ~1700-line
  graph walk + backtracking loop is now `backtracking_resolve(req:
  &ResolveRequest)` behind a `trait Resolver` / `BacktrackingResolver` /
  `active_resolver()`; `resolve_pretend_graph` is a thin 44-arg
  marshaller. Self-contained and runtime-swappable — a different
  resolver architecture is one `impl Resolver` + an `active_resolver`
  branch away, no call-site changes.)*

  `agent-context.md` lists a real backtracking resolver as out of scope
  for v1; the shipped loop nonetheless takes it from "detects and reports
  conflicts" to "reconciles solvable conflicts, masks unsolvable ones,
  and reports the rest with real portage's own notice". The items above
  are the substantive remainder.

- **Merge-list order (`-p` non-tree), remaining sub-algorithms.** Real
  `_serialize_tasks` (depgraph.py:9457+) is a genuine bidirectional
  digraph scheduler: priority-ranged `leaf_nodes`, a `gather_deps`
  cycle-breaking helper, `asap_nodes`/libc-first special-casing,
  `_merge_order_bias` (system-deps-first, descending reference count), a
  `_FrontierDigraph` perf layer, blocker/uninstall interleaving.
  `topological_merge_order` / `_topological_merge_order` ported two of
  its sub-algorithms (2026-09-04): `real_discovery_order` (an
  explicit-stack DFS simulating real's own `.order` discovery position —
  `_create_graph`'s LIFO `dep_stack`, `_add_pkg`'s early `digraph.add` /
  late `dep_stack.append`, `_add_pkg_dep_string`'s RDEPEND/IDEPEND/
  PDEPEND/DEPEND/BDEPEND key order; top-level atoms seed in the *given*
  order, not real's own per-arg alphabetical `sorted()` — a documented
  cut, portuale's flattened atom list has no arg/pset boundary left to
  sort within) and `merge_order_bias`/`deep_system_deps`
  (`_find_deep_system_runtime_deps` + the system-first/descending-
  reference-count re-sort, correctly scoped this time to *merge-bound*
  entries only — see below). Verified against a live
  `gnome-base/gnome-control-center` merge (a ~26-entry real graph): a
  long contiguous run now matches real exactly.

  `_merge_order_bias`'s first attempt (earlier the same day) fixed one
  fixture (`@system` alone) but broke five others the instant a
  non-system explicit atom shared the command line with `@system`/
  `@world`/a custom set. Root cause, confirmed live (`emerge -p
  --noreplace <already-installed pkg>` prints nothing at all, not even a
  notice): real's `_serialize_tasks` prunes every "nomerge"
  (`AlreadyInstalled`) root node from `mygraph` *before*
  `_merge_order_bias` ever runs (`depgraph.py:9505-9519`) — trivial
  top-level targets never enter scheduling, so bias never compares them
  against anything. Portuale's own "package is already installed;
  nothing to do" notice (a portuale-only UX addition, no real precedent)
  was wrongly being bias-compared against real merge tasks. Fixed by
  restricting the bias re-sort to `merge_bound_cpv(entry).is_some()`
  entries only, weaving trivial entries back in afterward by simple
  insertion on their own unbiased discovery rank. Full writeup in
  `docs/what-this-proves.md`'s "Merge-list order" entries.

  **`gather_deps` researched, not ported.** Direct read of
  `_serialize_tasks`'s main loop: `gather_deps` (and
  `_gather_deps_closures`/`find_smallest_cycle`) is called *only* from
  the `if not selected_nodes:` branch — i.e. only once the ordinary
  priority-ranged `leaf_nodes()` scan finds nothing available at *any*
  priority level, a genuine unresolved runtime cycle. It is not a
  general "cluster RDEPEND-connected packages together" mechanism for
  the ordinary (acyclic) scheduling path, contradicting this doc's own
  earlier (pre-2026-09-04-research) guess. The `net-libs/rest`/
  `net-libs/gnome-online-accounts` gcc-case gap is therefore NOT
  `gather_deps`-shaped; live gnome-control-center has no actual
  dependency cycle, so real never even calls `gather_deps` while
  resolving it. Portuale's own existing cycle-breaking fallback (prefer
  an entry whose every unplaced dependency is a soft edge; otherwise
  emit the earliest-discovered entry) stays as the practical
  approximation -- a full `gather_deps`/`find_smallest_cycle` port
  (smallest-cycle selection across multiple priority levels) is deferred
  until a real fixture actually needs it.

  **Batched leaf selection shipped (2026-09-04, same day, third pass).**
  The leading hypothesis above was right: `topological_merge_order_impl`
  / `_topological_merge_order`'s main loop picked one entry at a time,
  recomputing availability after every placement, so a freshly-freed
  entry could jump ahead of an already-available sibling with a higher
  rank that real would have already committed to *that round*. Fixed to
  mirror real's own "Greedily pop all of these nodes since no
  relationship has been ignored" optimization
  (`depgraph.py:9764-9777`): each round now computes the *whole* current
  batch of genuine leaves (no unplaced dependency of any kind), sorts it
  by discovery/bias rank, and emits the entire batch before recomputing
  -- only an empty batch falls through to the unchanged one-at-a-time
  cycle-breaking fallback. Live gcc-merge exact-position matches against
  real went from 3/26 to 8/26 lines; `net-libs/rest`/`net-libs/gnome-
  online-accounts` both moved substantially earlier, though not yet to
  their exact real positions.

  **Priority-hierarchy hypothesis falsified, real cause found (2026-09-04,
  same day, fourth pass).** Instrumented the "strict batch is empty"
  fallback (the only place `DepPriorityNormalRange`'s `SOFT`/
  `MEDIUM_SOFT` ladder could ever matter) and ran the live gcc merge: it
  never fires, not once. Real's ladder only activates when a round's
  strict-leaf batch comes up empty — a cycle-like stall — and the gcc
  graph has no circular dependencies (confirmed: portuale's own
  `find_hard_cycles` reports none, matching real's own `--debug` output),
  so every round already finds a full batch at the strictest level. The
  priority hierarchy is provably irrelevant to this gap.

  The actual cause, found via real's own `--debug` digraph dump: real's
  full dependency graph for the gcc case has **~267 already-installed
  nodes** in its transitive closure (`emerge -p -d ...gnome-control-
  center 2>&1 | grep -c ", installed)"`) — vastly more than the ~26
  entries that end up needing action. Real's `.order` discovery position
  (what `real_discovery_order` simulates) is computed across that *entire*
  ~300-node graph, every already-satisfied dependency included. Portuale
  only ever creates a `GraphEntry` for packages it actually needs to
  track (a top-level target's own transitive deps down to whatever
  actually needs an operation) — it doesn't walk into an
  already-installed package's own further dependencies without `--deep`
  (`Deep::recurses_at`), a pre-existing, load-bearing simplification many
  other tests already depend on (real's own default resolution *does*
  build the full transitive graph regardless of `--deep`; portuale's
  `--deep` gate is a narrower, different thing: whether to check an
  already-satisfied dependency for a possible *upgrade*, not whether to
  discover it at all). `real_discovery_order`'s rank simulation therefore
  runs over a far smaller universe than real's own, spacing ranks out
  very differently — which reproduces exactly the "same batches, scrambled
  fine-grained order" pattern observed (structurally correct groupings,
  wrong relative order within them).

  Closing this needs portuale to walk (or at minimum rank) the *entire*
  already-installed transitive tree for discovery-order purposes,
  independent of `--deep` and independent of whether an entry is ever
  rendered — a real architectural change (every `AlreadyInstalled`
  outcome would need its own further recursion walked, purely to feed
  `dep_order`, with no display/resolution consequence), not a bounded
  fixture-sized fix, and carries real risk to the existing, well-tested
  BFS walk's own performance and semantics at scale. Deferred rather than
  attempted without a concrete need -- see `docs/what-this-proves.md`'s
  "Merge-list order" entries for the full empirical trail.

  **Still open:** the full-transitive-tree discovery-order walk above (now
  the confirmed, not merely hypothesized, remaining cause of the gcc gap);
  real's fuller priority hierarchy (`DepPrioritySatisfiedRange`/
  `DepPriorityNormalRange`'s `NONE`/`SOFT`/`MEDIUM_SOFT`/`MEDIUM_POST`
  ladder, vs. portuale's binary hard/soft edge distinction) — real, cited
  behavior for genuinely circular graphs, but confirmed *not* the cause of
  any currently-observed gap; `asap_nodes`/libc-first special-casing; the
  `_FrontierDigraph` perf layer (not needed at portuale's graph sizes);
  blocker/uninstall interleaving (portuale's `-p` pretend path has no
  uninstall/blocker resolution yet, a separate pre-existing gap); a full
  `gather_deps` port for the genuine-cycle case.

- **`--root-deps` / multi-root, remaining edges.** *Mostly a non-gap for
  this fork* — the ebuilds are all EAPI 7+, where `--root-deps=rdeps` is
  a complete no-op and `BDEPEND`/`IDEPEND` always resolve against the
  running root (which portuale does, `--root-deps` or not). The full
  multi-root graph (a `root` per dependency edge) stays a deliberate
  edge-by-edge approximation; a running-root entry's `PDEPEND` stays a
  target-`ROOT` concern (a permanent non-gap).

- **Slot-operator rebuild v1 cuts** — single-pass (no backtracking for a
  rebuild that itself shifts another sub-slot); the rebuilt consumer's
  own `:=` deps not re-bound in the pretend graph; no `--changed-slot`
  interaction; `IUSE_EFFECTIVE` in the built-dep domain.

- **`package.provided` depclean corner** — a provided CPV as a depclean
  root, and the `-pc` advisory's "will be removed by depclean even if in
  world" wording.

- **Bare command-line names, remaining shape** — real's non-`--quiet`
  `ambiguous_package_name` runs a full `search` before the `!!!` lines
  (portuale emits only the deterministic list). *(The versioned/slotted
  bare name — `emerge eix-1.2`, `emerge '>=eix-1.2'`, `emerge eix:0` —
  shipped 2026-09-03: real `dep_expand`'s `null/`-insertion +
  missing-`=` retry + `cpv_expand` + splice, as `dep_expand_token`.)*

### B. Scheduler / build orchestration

**Substantially complete.** Remaining:

- the merge step's `pkg_*` hooks still run uncaptured through brush
  (residual stderr noise under `-jN` / `--quiet-build`);
- one tokio runtime per `run_commands`; killing in-flight builds on a
  hard failure; `PORTAGE_LOGDIR` / `split-log`;
- `mtimedb["resume"]`: `resume_backup` rotation; the build-time-flag half
  of `myopts`; binary-entry replay;
- `--ask`: TTY gating, prompt colour, re-prompt on a bad answer;
- `elog`: `mail` / `mail_summary` (out of scope — a real SMTP client +
  MIME assembly), `syslog` / `custom` (unported);
- `PORTAGE_SCHEDULING_POLICY` reaches only this process (real
  `apply_priorities` also does the `multiprocessing` forkserver pid).

### C. Config resolution depth — **complete (2026-09-03)**

The whole `env.d → repo → features → pkginternal → defaults → conf → pkg
→ env` `USE_ORDER` chain is modelled, per-profile-level `defaults`
interleaving included; the build-phase env carries the resolved `USE` +
compiler/make flags + `package.env`'s non-USE vars.

Remaining are documented simplifications only, none observed to matter:
env-layer `USE_EXPAND` values are last-wins into `scalars`, not
genuinely incremental; no per-file `${VAR}` expand map for
`package.env` / `env.d` (real portage seeds one from the global config);
portuale's `FEATURES` is a last-wins scalar (modelled via
`feature_enabled`), not real incremental stacking; `env.d` is read
relative to `config_root`, not a distinct `eroot` (they coincide in
every tested and typical configuration).

### D. Sandbox / build isolation — **substantially complete**

The whole `FEATURES` isolation set wraps the six real `src_*` phases
(`unshare` + `sandbox`). Remaining (deliberate cuts):

- `RESTRICT=network-sandbox` / `PROPERTIES=live` / `test_network`
  exemptions (no USE-reduced `RESTRICT`/`PROPERTIES` in the phase env);
- the `AI_ADDRCONFIG` loopback addresses; SELinux sandbox;
- `userpriv` / `fakeroot` (single-user dev/test context — see the
  `os.lchown` note below);
- various **non-isolation** `FEATURES`, forced to `""` in the phase env:
  `ccache`, `distcc`, `splitdebug`, `installsources`, `nostrip`/`strip`,
  `compressdebug`, `test` gating beyond `src_test` running, the
  `preserve-libs` live-`scanelf` orphan branch. A scoped real-`FEATURES`
  passthrough is a separate slice;
- build flags / resolved `USE` are still `""`/absent for a standalone
  `ebuild <file> <phase>` (no graph) and for `emerge --resume`
  (`resume_entry` carries none); the `Packages` *index* `USE` field for
  an `emerge -b` binpkg isn't back-filled from build-info.

### E. Binary packages / fetch

- **Remote binhost** — live `layout.conf` negotiation,
  `RESTRICT=primaryuri` interleave, `Packages.bz2` / `.lz4`, binpkg
  `SHA1` (no sha1 crate).
- **gpkg** — `.sig` verification + signing (`FEATURES=binpkg-signing` —
  cut, portuale has no crypto), bare `.xpak` multi-instance,
  mtime-staleness index revalidation, `BUILD_ID`
  in the basename. *(The `-pretend` `-N` `BUILD_ID` display suffix and
  binpkg-multi-instance selection — every build into the pool, per-
  instance `--binpkg-respect-use` + atom-`[use]` filtering,
  `dedup_binary_instances` keeps the newest `BUILD_TIME` survivor —
  shipped 2026-09-04, plus `--binpkg-changed-deps` (auto whenever not
  `--usepkgonly` -- `binary_deps_changed`), `--rebuilt-binaries` for a
  *remote* binhost binary, and `_equiv_ebuild_visible` (a binary needs a
  visible ebuild at its own exact version once some ebuild has matched
  the atom, `--useoldpkg-atoms`-exempted). Still open: `identical_binary`
  (guards `_equiv_ebuild_visible` against rejecting an *installed*
  package -- moot for portuale, whose installed-package path never runs
  that check to begin with), BUILD_TIME-vs-installed *reinstall* trigger
  outside `--rebuilt-binaries`, `useoldpkg` multi-instance, the explicit
  `--binpkg-changed-deps=y|n` / `--use-ebuild-visibility` overrides.)*
- **`splitdebug` / `packdebug` / RPM**, PKGDIR-index
  locking, `FEATURES=buildpkg-live`, real `EbuildBinpkg` failure
  semantics under `--keep-going`.
- **Fetch** — real candidate ordering / shuffling.

### F. Whole `emerge` actions

The action and modifier-flag surface is broadly complete. Remaining:

- `--info`: the host-state half (version header, uname/mem, tool-version
  probes, `info_pkgs`, timestamps) is a documented cut. For `--info
  <atom>`: the installed block reads the individual vdb `build-info`
  files, not `environment.bz2`, and skips the `( )` force/mask wrapping;
  the `(non-installed binary)` case, the `pkg_info()` phase run itself,
  and ANSI colour on the `USE=` line are all cut;
- `--regen`: sequential (no `--jobs` threading), no stale-entry pruning,
  `_eclasses_` from this repo only (no masters chain);
- `--check-news`: no `.unread` / `.skip` *write-back*,
  `Display-If-Installed` only;
- `--metadata` is an architectural no-op (portuale reads
  `metadata/md5-cache` directly, models no `depcachedir`);
- `--sync` is a permanent non-goal (points at `emaint sync`); GLSA /
  `@security` is not in scope.

### G. Shell backend

- minimize + report the brush `declare -f` heredoc bug upstream (it
  corrupts a function with a redirected here-doc, which is why the
  default backend is `bash`, not the embedded `brush`);
- periodic re-pin to keep up with upstream `reubeno/brush` `main` (see
  `brush-pin.md`'s checklist).

### H. Misc / cosmetic

- `os.lchown` / privilege-preserving ownership is not reproduced (needs
  root, which portuale's single-user context never has — it would only
  no-op); directory merge traversal is sorted for test determinism, not
  real `os.listdir()` order (`CONTENTS` line order carries no semantics
  portage relies on). Both are deliberate — see `ebuild_merge.rs`'s
  module doc comment.

---

## Part 3 — explicit non-goals / architecture boundaries

Standing decisions, not oversights.

- **`--autounmask-write`** and any config-*writing* autounmask mode
  (conflicts with "never writes config" — the read-only suggest/resolve
  half is shipped). Note `emerge --deselect` / `-C` / `--depclean` *do*
  write `world` / `world_sets` / the vdb now — those are user state the
  package manager owns, not `/etc/portage` config.
- **A real backtracking resolver** (see Part 2.A — listed there because it
  is the substantive gap, here because `agent-context.md` scoped it out for v1).
- **PyO3 / in-process FFI embedding** — would foreclose the
  two-sibling-implementations end state.
- **EAPI 0/1/2/3/4/6** — dead in this repo; the `portage-*` crates have no
  EAPI parametrization at all within the 5+ floor.
- **`bsd_chflags`** — `None` on non-BSD; portuale is Linux-only/musl-static.
- **RPM binary packages, repo syncing (`emerge --sync`), news items,
  GLSA/`@security`, GPG signing/verification, Prefix/cross-`ROOT` beyond
  the `ESYSROOT` distinction** — not in scope.
- **`equery` / `portageq` / `etc-update` / `dispatch-conf`** — separate
  tools, separate binaries.
- **Switching CLI option parsing to `clap`** — evaluated 2026-09-02,
  rejected. The parser (`pretend.rs`'s parse loop + `emerge_options.rs`
  tables) faithfully reproduces `emerge`'s `argparse` quirks that `clap`
  has no idiom for: optional values consumed only when they look like an
  integer (`--deep[=N]`, `--jobs[=N]`, `--backtrack[=N]`), `true_y_or_n`
  (bare / `=y` / `=n` / space `y`/`n`) vs `y_or_n` (required),
  `action:"append"` atom lists where each occurrence is itself
  space-split, `-pX requires an argument and can't be bundled`, and the
  exact real error strings. It also carries the
  recognized-but-unimplemented machinery (a real emerge option reports
  "not yet implemented in portuale", not "unknown") and is kept
  structurally parallel to the Python reference so the two parsers can't
  drift. `clap` would fight every one of these; ~1500 lines across two
  languages under ~1100 contract tests, near-zero payoff.

---

## Part 4 — how far is this from a "perfect clone that installs and uninstalls"?

**Short answer: portuale already installs and uninstalls packages for
real** — `emerge <atom>` (source and binary), `emerge -C`, `--depclean`,
`--prune`, `--config`, `--deselect` all perform real filesystem mutation,
with real ebuild-phase execution, real vdb bookkeeping, real
`CONFIG_PROTECT` / `collision-protect` / preserve-libs / `env_update`, and
it has built + merged + unmerged real Gentoo packages end to end. For the
**happy path of operating on one package (or a small dependency closure)
at a time**, it is close.

The distance to a drop-in replacement is now dominated by **one** large
item, with a short incremental tail:

1. **The backtracking resolver (Part 2.A) — the architectural core is
   now in place.** The shipped `'backtrack` loop reconciles solvable
   slot conflicts, masks unsolvable ones, renders the real notices,
   tries USE/keyword autounmask levels *inside* the loop (2026-09-03),
   drives the slot-operator-rebuild sub-slot cascade to a fixpoint
   (2026-09-03, container-verified), and drives **both** real
   `runtime_pkg_mask` feedback paths — `_feedback_slot_conflict` and
   `_feedback_missing_dep` — into `||` alternative re-selection
   (2026-09-03, container-verified —
   `docs/history/or-preference-backtrack-plan.md`). What is left here is
   depth/fidelity work on the pieces already built (richer
   `_slot_conflict_backtrack` mask-target analysis, `dep_zapdeps`'
   full preference bins, deeper multi-constraint interplay), not a
   missing mechanism.

2. **The rest of Part 2** — the scheduler tail (2.B), the deliberate
   sandbox and `FEATURES` cuts (2.D), the remote-binhost / gpkg-signing
   gaps (2.E), the `--info` host-state half and `--regen` threading
   (2.F), the brush `declare -f` upstream fix (2.G). Each is one focused
   slice, the rhythm portuale already runs at.

Config-resolution depth (2.C) is complete; the action/flag surface (2.F)
and sandbox isolation (2.D) are substantially complete.
