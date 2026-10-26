# Portage quirks and suspected bugs

Portuale mimics Portage (3.0.83) closely, because Portage is the oracle. But
knowing where Portage itself looks wrong has value: for users who hit the same
surprise, for upstream reports, and for deciding later whether to copy a quirk
or fix it. Every place where a Portage behaviour looks unintended gets an entry
here, whether or not portuale copies it.

**Version wording.** `portage.VERSION` and `emerge --version` read `3.0.83`;
`3.0.83-r1` is the installed ebuild (`sys-apps/portage-3.0.83-r1`, one
downstream patch, bug 984255). Entries written before 2026-10-24 used the
ebuild revision as the version; they now say `3.0.83` (backlog #530).

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
  3.0.83 printed `(Change USE: -nls -doc)` in the recorded run and
  `-doc -nls` under `PYTHONHASHSEED=0..6`, `-nls -doc` under `PYTHONHASHSEED=7`;
  the twelve-flag `PORTAGE_CIRCULAR_MAX_USE_FLAGS=16` suggestion is a random
  permutation as well.
- **Why**: `circular_dependency_handler._find_suggestions`
  (`lib/_emerge/resolver/circular_dependency.py`) joins the flags of each
  solution, a `frozenset` of `(flag, state)` pairs, without sorting, and string
  hashing is seeded per process.
- **Why it looks wrong**: the same resolution prints a different line from run to
  run; the per-edge `USE=` list added in 3.0.83 (`_edge_description`) is sorted.
- **First seen**: Portage 3.0.83 (the only version checked; the join is older).
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
- **First seen**: Portage 3.0.83 (the only version checked).
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
- **First seen**: Portage 3.0.83 (the option is new in 3.0.83).
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
- **First seen**: Portage 3.0.83 (`96c320967`, bug 346351).
- **Portuale**: copies it. `pretend/removal/depclean.rs` and
  `portage-repo/src/depclean.rs` (`cycle_suggestions`) print the notice for a
  world-listed member, because portuale drops `@selected` seeds in `args` mode
  like Portage. Pinned by `test_508_world_listed_member_still_gets_the_notice`.

## Q8. `use_reduce` without `token_class` accepts a use dep that `Atom` rejects, and its two error texts differ (suspect)

- **Observed** (2026-10-23, backlog #510, `LLM/evidence/2026-10-23-510/`,
  `probe.py` run with `python3 -I` on the host and on the 3.0.82.2 tag, first
  built by hand and rebuilt with the tag's meson `_parser` target for #529):
  for `cat/pkg[a(+),-a]` in EAPI 8 and in EAPI-less mode,
  `Atom("cat/pkg[a(+),-a]")` raises `Invalid use dep: '-a'`, while
  `use_reduce("cat/pkg[a(+),-a]", eapi="8")` (no `token_class`) returns
  `['cat/pkg[a(+),-a]']`. With `token_class=Atom` **and no `is_valid_flag`**
  the reducer raises `InvalidDependString: invalid use dep token: '-a'` on
  **both** 3.0.82.2 and 3.0.83 (the C fast path, `src/dep_parser.c`
  `py_classify_use_deps`); that text did not change in 3.0.83.
- **The two texts**: `Package._validate_deps` passes `is_valid_flag`, and the
  C fast path requires `is_valid_flag is None`
  (`lib/portage/dep/__init__.py:1079-1086`), so `emerge` takes the Python
  path and prints the wrapper `Invalid atom (%s), token %s` (:974). The
  capture is `LLM/evidence/2026-10-24-531/`: `emerge -p` masks the package
  with `invalid: RDEPEND: Invalid atom (Invalid use dep: '-a'), token 1`,
  identical on the host and on portuale.
- **Why**: `use_reduce` only builds `Atom`s when `token_class` is given
  (`lib/portage/dep/__init__.py`, the fast-path gate `token_class is Atom`, and
  the regex path that calls `token_class`). Without it the tokens are plain
  strings, so the missing-USE-default rule of `_use_dep` is never run. The
  reducer's `Atom` path and the constructor agree on acceptance; only the
  string path is looser.
- **Why it looks wrong**: a dependency string the reducer hands back as valid
  would be rejected by the constructor it is supposed to mirror; callers that
  reduce a string to flags and never build `Atom`s (`use_reduce` default) see
  an atom Portage itself calls invalid.
- **Not reproduced**: the plan (`LLM/02.442-port-portage-3.0.83.opus.md`,
  S10b) said 3.0.82.2 made the two entry points disagree on `games-arcade/2048`,
  `dev-util/timidity--` and `cat/pkg[a(+),-a]`. On the 3.0.82.2 tag, built with
  its own C parser (hand-built and meson-built), all three agree: the C-parser
  fixes it needs (`6a64fb2f4`, `4f76d9ee5`, `a2206fc32`, equal in 3.0.83 to
  `450282d84`, `8bf578fd7`, `bdce305cd`) are already in that tag. The
  disagreement existed only before those commits, not in a released version.
  Erratum: `LLM/02.442-port-portage-3.0.83.errata.md`.
- **First seen**: 3.0.82.2 (the string path and both error texts).
- **Portuale**: copies the string path. `portage-use-reduce`'s `use_reduce_flat`
  takes the plain token list and only the `_validated` variant runs an atom hook
  (`crates/portage-use-reduce/src/lib.rs`).
- **Decision (backlog #526): mimic.** `portuale emerge` and `portuale mrg` both
  reduce dep strings through `portage-repo`'s `validate_dep_string`, which
  always supplies the Atom hook and the valid-flag closure
  (`crates/portage-repo/src/dep_validate.rs`), so the loose string path is
  never the validation path in either applet. It stays loose for the
  flag-only reductions (collisions, licence/fetch restriction, slot
  expansion), where Portage is just as loose and an atom's use-dep defaults
  cannot change the answer.

## Q9. A deprecated profile `virtuals` file is reported through `warnings.warn`, so the interpreter's install path and a source line of Portage's module are printed (suspect)

- **Observed** (2026-10-24, backlog #513, `LLM/evidence/2026-10-24-513/captures/02-profile-nonempty.portage.err`, host 3.0.83): a non-empty profile `virtuals` file prints, on stderr, `/usr/lib/python3.14/site-packages/portage/package/ebuild/_config/VirtualsManager.py:54: UserWarning: <profile>/virtuals is deprecated, support for PROVIDE virtuals was removed in portage-2.3.25, use GLEP 37 virtual packages instead` and then the line `  warnings.warn(`. It is printed once per run for each such file (chain order, parents first, `/etc/portage/profile` last) and for every action that loads the config (`-p`, `-s`, `--info`, `--version`, merges). The user-level `/etc/portage/virtuals` is different: a plain `writemsg` block with no Python frame, printed only by the validating actions (`config.py:1474-1479`).
- **Why it looks wrong**: the frame names the interpreter's install prefix and a line of Portage's own module, which is an implementation detail of the Python warnings machinery and differs by machine. Portage's other deprecation of this kind has the same shape: `KeywordsManager.py:88-92` (`package.keywords`) is also `warnings.warn(... UserWarning)`, so the frame is not specific to `virtuals`. The two user-level notices (`--info` gets the profile frame but not the `/etc/portage/virtuals` block) are also inconsistent with each other.
- **Portuale**: copies the message and the `  warnings.warn(` line, with the location written as `portage/package/ebuild/_config/VirtualsManager.py:54` (no interpreter prefix; the pins stay hermetic). The user block is copied in the validating actions only. Portuale's `package.keywords` notice (`portage-profile/src/resolve/package_masks.rs`, `atom_lines`) carries the same frame shape at `KeywordsManager.py:89` (backlog #547).
- **First seen**: 3.0.83 (commit `3a3a119d8`, `64e2735c3`).

## Q10. A hard link member breaks the compressed-stream extraction that follows it (suspect)

- **Observed** (2026-10-24, backlog #514, `/tmp/probe514e.py`,
  `/tmp/probe514f.py`, L5 cell `dotdot-hardlink` control output): Portage
  3.0.83 `gpkg.tar_safe_extract.extractall` over a zstd stream (`tarfile`
  `r|` over the decompressor pipe) fails with `StreamError: seeking
  backwards is not allowed` when a data member follows a hard link member.
  Minimized: `image/{r,r/f,l -> r,h -> image/l,x}` extracts rc 0 plain or
  seekable, and rc 0 with no data member after the hard link, but fails
  exactly when the hard link extraction advances the stream past the next
  member's blocks (position 3072 to 4608 in the probe). Through
  `gpkg.decompress` the run reports `!!! Error extracting '<archive>',
  seeking backwards is not allowed`, rc 1, nothing installed.
- **Why it looks wrong**: the archive is valid (plain `tar` and GNU `tar`
  extract it; the uncompressed Portage path extracts it, oracle matrix row
  `c-dotdot-same-depth`), and genuine `image.tar.zst` binpkgs with a
  hardlink-before-data layout would fail the same way. The checks all pass;
  only the extraction half trips, inside CPython's `tarfile` stream
  bookkeeping around link extraction (the 3.14 `makelink_with_filter`
  `realpath` + fallback dance, `tarfile.py` `makelink`, `ExFileObject`
  seek).
- **First seen**: 3.0.83 (container image `localhost/test-portuale`,
  CPython 3.14.8, GNU tar 1.35, host kernel refusing hard-link-to-symlink
  creation, so only Python-written `t=1` members were exercised).
- **Portuale**: does not copy the failure mode (GNU `tar` truncates the
  member under the deferred `..` symlink instead, rc 2), but the composed
  outcome matches: rc 1, nothing installed, empty image dir. The L5 cell
  pins the pair (1, 1) with this quirk cited.

## Q11. The `usage:` block's option order is hash-randomized per process (suspect)

- **What it does**: every argparse-native error (`invalid choice`,
  `unrecognized arguments`) is preceded by the full 78-line `usage:`
  block, whose option order differs from run to run: three identical
  host invocations (`emerge -p --circular-deps-report=none x`) with
  `PYTHONHASHSEED=0/1/42` produce three different md5s, differing from
  the first wrapped line on. Source: argparse builds the usage parts
  from a `frozenset`-ordered option container (the 347-USG cut).
- **Why it looks wrong**: `--help` text is stable, but the error-path
  usage is not -- the same command prints different help in different
  processes.
- **First seen**: 3.0.83 (host Portage, `LLM/evidence/2026-10-23-507/
  captures/53-none.portage.err` vs re-runs).
- **Portuale**: prints the oracle capture's order verbatim on every
  `invalid choice` error (backlog #520), like the sorted `Change USE:`
  flags of Q4 -- deterministic, matching Portage's line set but only
  occasionally its order.

## Q12. `FEATURES=cgroup` resource summaries need kernel cgroup accounting (by design)

- **What it does**: under `FEATURES=cgroup`, Portage's `Scheduler`
  constructs a `CgroupManager` (`Scheduler.py:244-252`) and, after each
  build and each install, logs
  `=== Resource usage for {build,install} of {cpv}: {rendered}`
  (`Scheduler.py:421-444`, upstream `55beac58c`); in background mode
  (`-j`) with `--verbose` the line is also echoed to stdout
  (`a6fca880a`). When the cgroup filesystem is not writable (unprivileged
  run: `Permission denied`, no leaf created), the manager is never kept
  and `_cgroup_finish` returns silently -- no line anywhere, rc 0.
  Oracle: `LLM/evidence/2026-10-23-512/cgroup.sh` (shape 14 as root with
  both lines on stdout; `14-cgroup-degraded` as vivo silent).
- **Why it is not a bug**: the text reports measured kernel counters
  (CPU time, max memory, I/O via cgroupfs); without the accounting there
  is nothing true to print, and Portage's own degraded path prints
  nothing rather than zeros.
- **First seen**: 3.0.83 (only version checked).
- **Portuale**: `portuale emerge` prints nothing under `FEATURES=cgroup`
  (backlog #543, Tier 3): no `CgroupManager` is ported, no counters are
  fabricated -- the output matches Portage's degraded (unprivileged) run
  exactly, and stays silent as root where Portage prints the two lines.
  Porting the manager would mean reading cgroupfs for every build; until
  then the gap is a documented wontfix, not a format to mimic.

## Q13. A `categories` line with two names lists neither (grabfile rejoins lines)

- **What it does**: `grabfile` (`util/__init__.py:171-200`) splits each line on whitespace, cuts `#` comments, then rejoins the survivors with single spaces into ONE token per line. A `profiles/categories` line reading `dev-libs dev-python` becomes the single token `"dev-libs dev-python"`, which never matches `dbapi._category_re` (`^\w[-.+\w]*$`) and is dropped — so neither category is listed. One-category-per-line is not just convention; it is load-bearing.
- **Why it looks wrong**: a whitespace-separated list file that silently drops multi-token lines instead of splitting them contradicts every other stacked-list file's reading. portuale's `read_categories_file` splits on whitespace (both names list), which is the lenient reading.
- **First seen**: Portage 3.0.83 (source `3rdparty/portage`, Task 12 S0).
- **Portuale**: copies the file locations, stacking and regex, but not the line-join quirk. A fixture with two categories on one line would diverge (Portage lists neither, portuale both); no shipped file does this.

## Q14. The unlisted-category `!!!` names `etc/portage/categories` for a union set

- **What it does**: `porttree.py:1216-1225` prints `!!! '<cp>' has a category that is not listed in <PORTAGE_CONFIGROOT>etc/portage/categories` even though `settings.categories` is the incremental union of every repo's `profiles/categories` plus that file (`config.py:948-960`). A category missing only from the user file but present in a repo file is listed (no notice); the message still names the user file.
- **Why it looks wrong**: the message points at one member file of a union, sending the reader to edit the wrong file when the omission is in a repo's `profiles/categories`.
- **First seen**: Portage 3.0.83 (source `3rdparty/portage`, Task 12 S0).
- **Portuale**: prints the line verbatim (same path rendering), quirks and all.

## Q15. `int(SIZE)` accepts underscores and newlines (`_get_digests`) (suspect)

- **What it does**: `bintree._get_digests` (`dbapi/bintree.py:2864-2871`) runs the `Packages` `SIZE` through plain `int()`, which accepts surrounding whitespace (including `\n`), a leading `+`/`-`, and single underscores between digits: `int("46_18") == 4618`, `int("4618\n") == 4618` (probed in `LLM/evidence/2026-10-25-review/t4/`).
- **Why it looks wrong**: `SIZE` is written by Portage itself as `str(st.st_size)` (decimal, no adornments), so the leniency only ever fires on hand-edited or hostile indexes; an underscore digit-separator in a size field is never intended input.
- **First seen**: Portage 3.0.83 (source `3rdparty/portage`, Task 4 S0).
- **Portuale**: `parse_index_size` copies the whitespace/sign/digits behaviour but not the underscore quirk (such a `SIZE` is treated as missing: local verifies nothing, remote is refused). Fails closed either way.

## Q16. A global-updates SLOT move on a gpkg warns "Package basename mismatched" (suspect)

- **What it does**: `binarydbapi.aux_update` (`dbapi/bintree.py:270-285`) builds `cpv_str = str(cpv)` (the full `dev-libs/slotpkg-1.0`) and passes it to `gpkg(settings, cpv_str, path)` as the basename. The container's own prefix is `slotpkg-1.0`, so `_get_inner_tarinfo` (`gpkg.py:2237-2241`) sees that the prefix does not start with the basename and writes `Package basename mismatched, using slotpkg-1.0` to stderr. It then carries on and rewrites the package correctly.
- **Why it looks wrong**: the basename Portage passes is one it chose itself, not user input. Every gpkg touched by a binary update prints the warning, and the warning points at no real problem.
- **First seen**: Portage 3.0.83 (source `3rdparty/portage`, plan 02.575 Task 5 capture, `LLM/evidence/2026-10-25-review/t5/stderr.txt`).
- **Portuale**: not copied. The line goes to stderr only and the result is the same. Task 9 (binpkg moves) does not reproduce it.

## Q17. `grab_updates` says "sorted" but returns `scandir` order

- **What it does**: the docstring of `grab_updates` (`lib/portage/update.py:130-162`) promises "a sorted list of tuples", but the function appends files in `os.scandir` order and never sorts them. `_do_global_updates` (`_global_updates.py:89-100`) applies them in that order, so the quarter files (`1Q-2026`, `4Q-2025`, …) are applied in whatever order the filesystem lists them, not by date.
- **Why it looks wrong**: chained moves (A→B in one quarter, B→C in a later one) only resolve if the files are applied oldest first. The docstring shows that this order was intended.
- **First seen**: Portage 3.0.83 (source `3rdparty/portage`, plan 02.575 Task 6 review).
- **Portuale**: not copied. `grab_updates_for_repo` sorts by (year, quarter), which is what the docstring promises. Only a chain that spans several files can tell the two orders apart, and no test fixture has one.
