# Portage quirks and suspected bugs

Portuale mimics Portage (3.0.82.2) closely, because Portage is the oracle. But
knowing where Portage itself looks wrong has value: for users who hit the same
surprise, for upstream reports, and for deciding later whether to copy a quirk
or fix it. Every place where a Portage behaviour looks unintended gets an entry
here, whether or not portuale copies it.

**Important: which applet follows Portage.** `portuale emerge` is bound to
Portage's behaviour strictly: it copies every quirk below, because its output
must match Portage's. `portuale mrg` (the Mrg interface) is free to differ
where Portage is suspected to be buggy: an entry here is where that
divergence would be decided. Until a `mrg` divergence is implemented, the entry
says "copied by `emerge`" and nothing more.

Each entry says: what Portage does (with source citation and how it was
observed), the **first Portage version where it was seen**, why it looks wrong,
whether `portuale emerge` / `portuale mrg` copy it, and a status (`suspect` =
looks wrong, not yet confirmed with upstream; `confirmed` = an upstream bug
exists; `by design` = intended but surprising). The first-seen version is the
oldest Portage the behaviour was observed on; when only the current one was
checked, say so (this is not a claim that older versions are free of it).

Add new entries at the end. Do not rewrite old ones; correct them in place only
when a claim turns out false.

## Q1. `emerge -uD` silently skips an update when an installed node is already in the graph (suspect)

- **Observed** (2026-10-22, host `@world`, backlog #484; re-confirmed on the
  repaired binhost cache, see #487):
  `sys-firmware/edk2-bin` 202408 is installed; 202608 is visible and newer. The
  dep `|| ( sys-firmware/edk2 sys-firmware/edk2-bin )` of an installed parent
  resolves to the installed 202408, and no update is offered, and no "skipped
  due to a dependency conflict" notice is printed.
- **Why**: `_wrapped_select_pkg_highest_available_imp` first looks for an
  existing node in the graph (`find_existing_node`, `depgraph.py` ~8190). The
  package tracker's `match(..., installed=False)` still lists installed
  packages that earlier deps added as graph nodes, so an installed node pinned
  earlier by a different parent (here the `qemu` binary's baked
  `~sys-firmware/edk2-bin-202408`) satisfies the later bare atom, and the newer
  version is never considered.
- **Why it looks wrong**: the result depends on the order in which deps are
  walked. Had the bare atom been walked first, Portage would have picked 202608,
  hit a slot conflict with `qemu`'s pin, and printed the missed-update notice.
  The user sees an update vanish silently in one order and a loud warning in the
  other.
- **First seen**: Portage 3.0.82.2 (the only version checked; the
  `find_existing_node` pass is old, so earlier versions very likely behave the
  same).
- **Portuale**: `portuale emerge` copies it (`graph_installed_reuse` in
  `pass_helpers.rs`), because the merge list must match. `portuale mrg`:
  **decided 2026-10-22: diverge** -- always pick the newest visible version,
  independent of walk order (then a slot conflict and the skipped-update
  notice, if any, follow normally). Implementation filed as #489, not done
  yet. Fixture: pmtest `f484b-*`.

## Q2. The missed-update notice depends on binhost build timestamps (suspect)

- **Observed** (2026-10-22, host `@world`, backlog #484; re-confirmed on the
  repaired binhost cache, see #487): `app-misc/mime-types`
  has an installed instance (build 2, `USE=nginx`) and a binhost instance
  (build 1, `USE=-nginx`, newer `BUILD_TIME`). `--newuse` makes the first walk
  pick the binhost build; `www-servers/nginx` needs `[nginx]`; the slot conflict
  removes the binary and keeps the installed one. Portage then prints
  "WARNING: One or more updates/rebuilds have been skipped due to a dependency
  conflict" for `mime-types`.
- **Why**: `_get_missed_updates` skips the notice only if the kept package is
  greater than the removed one, and for two built packages of the same version
  `Package.__gt__` compares `BUILD_TIME` (`Package.py` 850-858). The binhost
  build is newer, so the kept installed package counts as "older".
- **Why it looks wrong**: the "skipped update" is the same version and the same
  cpv; it is a rebuild with different USE, and the reported conflict is just
  the USE the user already runs. Had the installed build been newer by
  timestamp, there would be no notice. A timestamp is an accident of when a
  binhost was filled.
- **First seen**: Portage 3.0.82.2 (the only version checked; the `BUILD_TIME`
  tie-break in `Package.__gt__` belongs to the binpkg-multi-instance work, so
  earlier versions without it do not have this behaviour).
- **Portuale**: `portuale emerge` copies it (`installed_twin_is_newer_build` in
  `direct_solve.rs`; the notice names `binary` and the build id). `portuale
  mrg`: **decided 2026-10-22: copy** (same as `emerge`, no divergence).
  Fixture: pmtest `f484-*` (both timestamp orders).

## Q3. The "causing rebuilds" list is in hash order, so two runs can print it differently (suspect)

- **Observed** (2026-10-22, host `@world`, backlog #486 close-out): in
  `The following packages are causing rebuilds:` Portage lists the providers
  and, under each, its rebuilt consumers in an order that is not the merge-list
  order and not alphabetical (for example `libbpf` before `boost` and `xorg-server`
  before `libplasma`; under `boost` the rows run hugin, mdds, mariadb, libcmis, ...).
- **Why**: `_show_abi_rebuild_info` (`depgraph.py`) walks sets and dicts keyed by
  `Package` objects, whose hash comes from the cpv string, so the order most likely
  follows `PYTHONHASHSEED` (random per process; not re-run with a pinned seed).
- **Why it looks wrong**: the same resolution prints a differently ordered block
  on every run; a diff or a log comparison sees noise.
- **First seen**: Portage 3.0.82.2 (the only version checked).
- **Portuale**: `portuale emerge` prints the group in provider and consumer
  alphabetical order, because there is no stable Portage order to copy; compare
  these blocks as sets. `portuale mrg`: same (sorted).

## Q4. The flags of a `Change USE:` cycle suggestion are printed in hash order (suspect)

- **Observed** (2026-10-23, backlog #506, `LLM/evidence/2026-10-23-506/`): for
  `dev-libs/cddseva` (`doc? ( B ) nls? ( B )`, `--autounmask-use=n`) Portage
  3.0.83-r1 printed `(Change USE: -nls -doc)` in the recorded run and
  `-doc -nls` under `PYTHONHASHSEED=0..6`, `-nls -doc` under `PYTHONHASHSEED=7`;
  the twelve-flag `PORTAGE_CIRCULAR_MAX_USE_FLAGS=16` suggestion is a random
  permutation as well.
- **Why**: `circular_dependency_handler._find_suggestions`
  (`lib/_emerge/resolver/circular_dependency.py`) joins the flags of each
  solution, a `frozenset` of `(flag, state)` pairs, without sorting, and string
  hashing is seeded per process.
- **Why it looks wrong**: the same resolution prints a different line from run to
  run; the per-edge `USE=` list added in 3.0.83 (`_edge_description`) is sorted.
- **First seen**: Portage 3.0.83-r1 (the only version checked; the join is older).
- **Portuale**: `portuale emerge` prints the flags sorted; compare the line as a
  set (`compare.sh` in the evidence directory does). `portuale mrg`: same.

## Q5. The `-arch keyword` mask reason can never be printed (suspect)

- **Observed** (2026-10-23, backlog #506): a package with `KEYWORDS="-amd64"` on
  an `amd64` system is reported `(masked by: missing keyword)`, and the circular
  report lists it as a masked alternative that `package.accept_keywords` can
  unmask (`dev-lang/cddgok-bootstrap-1`, shape 42 of
  `LLM/evidence/2026-10-23-506/`).
- **Why**: `_getmaskingstatus` (`lib/portage/package/ebuild/getmaskingstatus.py`)
  has a branch for `gp == "-" + myarch` that would report `-amd64 keyword`
  without an unmask hint, but it walks the keywords from
  `KeywordsManager.getKeywords`, which stacks them incrementally
  (`stack_lists(..., incremental=True)`) and so drops every `-arch` token first.
- **Why it looks wrong**: the dead branch says the author meant `-arch` to be a
  distinct, not config-liftable mask; the user is told a keyword is missing when
  the ebuild explicitly marks the arch broken.
- **First seen**: Portage 3.0.83-r1 (the only version checked).
- **Portuale**: `portuale emerge` copies it (`masked_deps.rs`,
  `candidate_masking_reasons`). `portuale mrg`: same until decided.

## Q6. `--circular-deps-report=json` prints hash-ordered `use_changes` objects and every cycle once per member (suspect)

- **Observed** (2026-10-23, backlog #507, `LLM/evidence/2026-10-23-507/`): for
  `dev-libs/cddseva` the `solutions[0].use_changes[0]` object came out as
  `{"nls": false, "doc": false}` in one run and `{"doc": false, "nls": false}`
  in the next (six runs without `PYTHONHASHSEED`: 2 and 4); the twelve-flag
  `PORTAGE_CIRCULAR_MAX_USE_FLAGS=16` object is a random permutation. Separately,
  a two-package ring is listed twice in `cycles`, once starting at each member
  (`dev-libs/usecyclea` -> `[b, a]` then `[a, b]`; a four-ring four times), and
  `shortest_cycle` is the first of them.
- **Why**: `_show_circular_deps_json` (`lib/_emerge/depgraph.py`) builds each
  `use_changes` entry with `dict(solution)`, where `solution` is a `frozenset`
  of `(flag, state)` pairs (same cause as Q4), and takes `handler.cycles`
  straight from `digraph.get_cycles`, which yields one rotation per node.
  `parent_solutions[pkg]` is a `set` of those frozensets, so the order of the
  `use_changes` list of one package is hash order too.
- **Why it looks wrong**: the report is meant for tinderboxes and other
  consumers ("machine readable form"), and two runs of the same resolution give
  different bytes; the rest of the document is sorted on purpose (`affecting_use`,
  `test_dep_parents`, `search_truncated`, `solutions` by cpv). The rotations are
  the same cycle reported again, so a consumer counting `len(cycles)` counts
  members, not cycles.
- **First seen**: Portage 3.0.83-r1 (the option is new in 3.0.83).
- **Portuale**: `portuale emerge` prints `use_changes` objects with their flags
  sorted and the solutions of one package in sorted order (one of Portage's
  possible outputs), and copies the rotations exactly. `portuale mrg`: same until
  decided. Compare the key order of `use_changes` as a set (`compare.sh` in the
  evidence directory sorts the keys).

## Q7. `--depclean ARG` names a cycle as "only required by packages that it requires itself" even when a world-listed member is still needed by @world (suspect)

- **Observed** (2026-10-23, backlog #508, `LLM/evidence/2026-10-23-508/`, shape
  `06-pair-world-parent`): the world file lists `dev-libs/dccycw` and
  `dev-libs/dccyca`, the cycle is `dccyca <-> dccycb`, and `emerge -p --depclean
  dev-libs/dccyca` prints `dev-libs/dccyca-1.0 is only required by packages that
  it requires itself. Pass all of them at once in order to remove them:
  emerge --depclean =dev-libs/dccyca-1.0 =dev-libs/dccycb-1.0`. The suggested
  command, passed with both atoms, selects both packages for removal (`Number to
  remove: 2`), `dccyca` included even though `@world` names it.
- **Why**: `show_cycle_suggestion` (`lib/_emerge/actions.py`, added by
  `96c320967`) skips only the protected set when it looks for parents outside the
  cycle. In `args` mode `_calc_depclean` empties `selected_set` (`deselect`
  defaults to on), so the `world` set is never recorded as a parent of the
  cycle members (`_parent_atoms` of `dccyca` holds just `dccycb`). The
  `@selected` membership therefore never blocks the notice, and the wording
  "only required by packages that it requires itself" is untrue for that package.
- **Why it looks wrong**: the notice exists to explain why a package survives
  `--depclean ARG`; for a package the world file still names, it is not the
  reason. The command it prints removes a world-listed package. This is the
  documented argument semantics (named packages are removed and deselected), so
  the removal itself is consistent; the wording is what looks off. The commit's
  own comment ("Only protected because --depclean was given arguments") shows the
  author considered the parent set, and did not mention world.
- **First seen**: Portage 3.0.83-r1 (`96c320967`, bug 346351).
- **Portuale**: copies it. `pretend/removal/depclean.rs` and
  `portage-repo/src/depclean.rs` (`cycle_suggestions`) print the notice for a
  world-listed member, because portuale drops `@selected` seeds in `args` mode
  like Portage. Pinned by `test_508_world_listed_member_still_gets_the_notice`.
