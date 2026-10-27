# Task 9 S0b — the cgp keepers and the bed's perl heals, traced with Portage's own resolver (2026-10-27)

Portage 3.0.83. `3rdparty/portage` is at tag `portage-3.0.83`, and `depgraph.py`, `backtracking.py`,
`dep_check.py` and `package_tracker.py` are byte-identical (`cmp`) to the host's
`/usr/lib/python3.14/site-packages` and to the bed image's 3.0.83-r1. D, B, DC and PT as in
`t8/oracle.md`; `PD` = `lib/portage/dep/__init__.py`. Paths below are relative to `t9/`.

Probes (in `playground/`). None of them changes behaviour; they only wrap a function and call through to it:
- `probe-s0b-cgp.sh` builds a scratch copy of `pmtest/fixtures` per cell. World gets the consumer added
  (the contract's `_world_extra_env`), and repos.conf is rewritten to absolute paths for Portage. It runs
  Portage `--debug` and portuale `PORTUALE_BT_TRACE=1` with the same argv and `PYTHONHASHSEED=0`.
- `bt-trace.py` logs every `_slot_confict_backtrack` call with its **full** choice list, the
  `_slot_conflict_backtrack_abi` results, `need_config_change`, each `Backtracker.get/_add/feedback`
  and `get_best_run`. Lines start with `S0B`.
- `which-trees-or.py` adapts `t8/playground/which-trees.py`. It logs every installed parent's `||`
  (filtered by `S0B_PARENT_RE`) with each arm's vdb match, the view (`_graph_trees` or `_filtered_trees`)
  and the selected atoms. Lines start with `T9OR`. It also loads `bt-trace.py`.
- `virt-ids.py` dumps the per-virtual atom lists from `_select_atoms_highest_available` with `id()`.
- `scan-or.py` statically scans the vdb for `||` groups.
- `bed-cell.sh` replays an L0 cell inside the bed.
- `no-intern.py` is a counterfactual, explained in §3. It is **not** Portage's behaviour.

The bed probes ran in my own container, `claude-s0b-bed`: image `localhost/test-portuale:latest`
`53a68e8e5939`, `--network=none`, the env of `layers/l0/in-container.sh` (`PORTAGE_CONFIGROOT=/ ROOT=/
LC_ALL=C.UTF-8 TZ=UTC PYTHONHASHSEED=0`, umask 022, world `tmux eix gentoolkit blake3`) and
`emerge -pv --color=n <atom>`. Plain Portage and portuale gave the same output as the
`l0-20261026T222948Z` run, so the replay is faithful (`diff`, apart from the timing; see `captures/bed/xmlparser-*.txt`).

## 1. The two cgp keepers (`captures/cgp/`)

Argv as in the contract (`test_emerge_pretend_contract.py:14906-15010`): `--pretend dev-libs/cgp0x`
with world += `dev-libs/cgp0q`, and `--pretend '>=dev-libs/cgp1x-2'` / `'<dev-libs/cgp1x-1'` with
world += `dev-libs/cgp1a`. Captures were taken with `--ignore-default-opts --getbinpkg=n --usepkg=n
--color=n` added, and again with the exact contract argv: same rc, rows and counts.

| cell | Portage | portuale (same argv) |
|---|---|---|
| USE `dev-libs/cgp0x` | rc 1, **0/20**, `R cgp0x-2.8.0`, conflict, hint `It might be possible to solve this slot collision by applying ... dev-libs/cgp0x-2.8.0 (Change USE: -icu)` | rc 1, 0/20, same row and conflict, but the generic `package.mask` hint |
| VER `>=dev-libs/cgp1x-2` | rc 1, **3/20**, `U cgp1x-2 [1]`, residual conflict | rc 1, **0/20**, same row and residual |
| VER `<dev-libs/cgp1x-1` | rc 1, **3/20**, `UD cgp1x-0.1 [1]`, residual conflict | rc 1, **0/20**, same row and residual |
| the 6 opt-out cells (`--ignore-world`, `--complete-graph-if-new-{use,ver}=n [--rebuild-if-new-slot=n]`) | rc 0, 0/20 | rc 0, 0/20, same rows |

**Is the first hop `_slot_confict_backtrack`?** Yes, in all three cells. `_process_slot_conflict`
(D:2149) is called and `_slot_conflict_backtrack_abi` returns False for every conflict package
(D:2205-2213). The parents are AtomArgs, or the atoms are not built slot-operator atoms (D:2311-2315).
Then comes `_slot_confict_backtrack` (D:2214-2218 → D:2220). Its `all_parents` are the argument atom
and the installed consumer's atoms. These are the parent atoms of every slot node (D:2160-2165).

**USE cell: no backtrack at all.** `need_config_change()` returns True. The slot conflict handler
finds a change (`{cgp0x-2.8.0: {'icu': False}}`, D:11849-11865), so the loop breaks at D:12407-12411
before any feedback: `captures/cgp/bt-use.txt`. The keeper outcome is the config-change break, not a
dead-end backtrack.

**VER `>=cgp1x-2`, try by try** (`captures/cgp/bt-ver-up.txt`, `portage-ver-up.trim.txt`):
- try 0: conflict on `cgp1x:0`. `conflict_pkgs` = [`cgp1x-2`, `cgp1x-1` (installed)], sorted with
  `reverse=True` (D:2235). There are two choices, and each has 1 conflict atom, so the length sort
  (D:2273) keeps their order:
  - choice[0] masks {`cgp1x-2`}; its atom is `cgp1a-1 → <cgp1x-2`;
  - choice[1] masks {`cgp1x-1` ebuild, `cgp1x-0.1`, `cgp1x-1` **installed**}. The two ebuilds are
    similar packages (D:2248-2264). Its atom is the argument `>=cgp1x-2`.

  Both nodes are added (B:185-195). `get` pops the last one, so choice[1] is explored first.
- try 1: mask {1 (ebuild), 0.1, 1 (installed)}. The installed instance stays in the graph, so the
  same conflict comes back (`!!! backtracking loop detected: cgp1x-1 installed`, D:2172-2180). The new
  choices are [mask 2] → node {1, 0.1, 1-inst, 2}, ACCEPTED; and [mask 1-inst] → a node equal to the
  current one, **REJECTED** by the dedupe check (B:126).
- try 2: mask {1, 0.1, 1-inst, 2}. The argument has no satisfier, so there is no restart (dead end).
- try 3: mask {2}. The argument is `masked by: backtracking: slot conflict`: dead end.
- `backtracking aborted after 3 tries`. `get_best_run` returns `{}` (B:287-298), and the final pass with
  `allow_backtracking=False` (D:12420-12439) prints the residual conflict.

**VER `<cgp1x-1`** (`captures/cgp/bt-ver-down.txt`) mirrors it:
- try 0: choice[0] masks {2, 1 (ebuild), 1-inst}, with the argument atom; choice[1] masks {0.1}, with
  `cgp1a → >=cgp1x-1`. choice[1] is explored first;
- try 1: mask {0.1}. Dead end on the argument;
- try 2: mask {2, 1, 1-inst}. The installed instance stays in the graph, so the conflict comes back.
  [mask 1-inst] is REJECTED as a duplicate, and [mask 0.1] gives node {2, 1, 1-inst, 0.1};
- try 3: that node. Dead end;
- aborted after 3 tries, then `get_best_run` returns `{}`, and the residual is printed.

**portuale.** `PORTUALE_BT_TRACE=1` shows two `Settle` passes, no `BT conflict` line and no mask
(`captures/cgp/portuale-*.txt`). The residual comes from `build_residual_slot_conflicts`.
`dropped_pin_mask_restart` stops at G1: the merge side's only parent is the argument
(`reverse_dep_pins.rs:106-116`). The rows, rc and residual are the same as Portage's; only the count differs.

## 2. `dev-perl/XML-Parser` in the bed (`captures/bed/xmlparser-*`)

Portage: rc 0, 3/20, 31 `N` rows. Its skip block has **69** rows; portuale's has **42**
(`xmlparser-portage.txt`, `xmlparser-portuale.txt`).

**Try by try** (`xmlparser-bt.txt`):
- try 0: one conflict on `dev-lang/perl:0`. `_slot_conflict_backtrack_abi` returns False for both
  `perl-5.42.2` (installed) and `perl-5.44.0`. Both are conflict packages, so the inner loop has no
  `other_pkg` to probe (D:2317-2319). 232 `all_parents`. Two choices:
  - choice[0] masks {5.42.2 ebuild, 5.42.0-r1, 5.40.2, 5.42.2 installed}, with 14 conflict atoms, all
    `=dev-lang/perl-5.44*` (9 from installed virtuals, 5 from ebuild virtuals);
  - choice[1] masks {5.40.2, 5.44.0}, with **69** conflict atoms: 48 built `dev-lang/perl:0/5.42=`
    from installed parents plus 21 `=dev-lang/perl-5.42*` from installed virtuals. It sorts last
    (D:2273), so it is explored first.
- try 1: missing dependency `virtual/perl-Compress-Raw-Bzip2-2.218.0 → =dev-lang/perl-5.44*`
  (B:197-206);
- try 2: missing dependency `virtual/perl-IO-Compress-2.223.0-r1`;
- try 3: success.

The skip block is `runtime_pkg_mask[5.44.0]["slot conflict"]` (D:1544-1579 → D:1665-1720): exactly
those 69 atoms. Choice[0] is never explored. portuale's trace has the same three hops: a one-choice
`SlotConflict` mask of `!=5.44.0` (no similar `5.40.2`), then the same two missing deps; L0 count 3/20.

**Is the built `:S/SS=` atom a conflict parent?** Yes. All 48 installed parents' `dev-lang/perl:0/5.42=`
are in `all_parents` (D:2160-2165) and fail `atom.match(5.44.0)` (D:2189-2196). The ABI path does not
take them (above), so they reach `_slot_confict_backtrack` as conflict atoms of the 5.44 choice and are
printed as skip rows.

portuale misses 27 of them:
- 26 from real (non-virtual) consumers: `app-text/po4a`, 24 `dev-perl/*` and `perl-core/File-Temp`;
- `virtual/perl-File-Temp`, whose `||` picks `~perl-core/File-Temp`, so portuale has no dropped pin for it.

That is the review's finding 2 (`rev_dep.rs:35-41` cuts the built atom down to the bare name).

**Which `||` arm each installed perl virtual selects** (`xmlparser-perl-or.txt`, `T9OR` lines):
- **9 installed virtuals list the installed arm second.** Their vdb RDEPEND is
  `|| ( =dev-lang/perl-5.44* =dev-lang/perl-5.42* … )`, the same in the ebuild: CPAN, Carp,
  Data-Dumper, Exporter, File-Path, JSON-PP, Text-ParseWords, XSLoader and podlators.
  - In try 0 Portage picks arm [0], `=5.44*`, in **both** views: the normal walk (`_filtered_trees`,
    complete_mode False) and complete mode (`_graph_trees`). 5.44.0 is available, and it is a graph node.
  - In tries 1-3, 5.44 is runtime-masked, so arm [0] is unavailable (D:7910 via
    `_dep_check_composite_db`) and they pick [1], `=5.42*`.
- **11 virtuals list `=5.42*` first** and pick it in every try and view.
- `virtual/perl-Encode` picks `~perl-core/Encode` in complete mode (#567). `virtual/perl-Test-Harness`
  picks `~perl-core/Test-Harness` in one normal-walk evaluation.

**The finding: the 9 virtuals still carry `=dev-lang/perl-5.42*` as a parent atom in try 0**, and it
is one of the 21 `=5.42*` skip rows. The cause is not their own `||` choice:
1. In the indirect-virtual path (D:4640-4700; debug `Candidates: virtual/perl-File-Path-2.180.0-r5:
   ['=dev-lang/perl-5.44*', '=dev-lang/perl-5.42*', …]`, `xmlparser-debug-virtual-expansion-L17186-17270.txt`),
   a parent such as `dev-perl/List-MoreUtils-XS` depends on several virtuals in one dep string.
2. The virtual's atoms are taken from `atom_graph`. That graph holds **every** atom of the virtual's dep
   string, unchosen `||` arms included (DC:132/141/192).
3. They are filtered by `id(atom) in chosen_atom_ids` (D:6122-6133, D:6164-6167), and that set holds the
   atoms selected anywhere in the parent's dep_check.
4. Since 3.0.83, Atoms are **interned**: `_c_atom_from_c` → `_intern_atom` (PD:103-130, PD:1598-1608),
   with a constructor cache (PD:1897-1949), commit 9d109000e. So `=dev-lang/perl-5.42*` is **one object**
   for File-Spec, MakeMaker, File-Path, Carp and the others (`xmlparser-vids.txt`: the same `@id` in
   every list).
5. A sibling virtual that chose `=5.42*` therefore attributes its `=5.42*` to File-Path as well.

Host probe: `python3 -I -c "from portage.dep import use_reduce, Atom; a=use_reduce('|| ( =dev-lang/perl-5.42* ~perl-core/File-Spec-3.940.0 ) dev-lang/perl:=', opconvert=True, token_class=Atom, eapi='8'); b=use_reduce('|| ( =dev-lang/perl-5.44* =dev-lang/perl-5.42* ~perl-core/Carp-1.540.0 )', opconvert=True, token_class=Atom, eapi='8'); print(a[0][1] is b[0][2])"` prints `True`.

**Counterfactual** (`no-intern.py`: interning switched off, `xmlparser-nointern-counterfactual.txt`;
this is not Portage's behaviour): rc **1**, 13/20, `U dev-lang/perl-5.44.0`, residual slot conflict,
and **0** `=5.42*` rows for the 9 virtuals. So the bed's rc-0 heal depends on this identity effect.
This looks unintended: the interning commit says "Output is unchanged for @world, --emptytree and
--depclean", while `chosen_atom_ids` relies on distinct objects. Candidate for `LLM/portage-quirks.md`.
I did not run 3.0.82.2 to confirm that it is new.

## 3. The other L0 target cells and the static scan (`captures/bed/*.wt.txt`, `scan-or-*.txt`)

**Static scan** (`scan-or.py`, all 320 installed packages, vdb and ebuild deps, installed USE):
- In RDEPEND/PDEPEND/IDEPEND, the **only** `||` groups whose first vdb-matching arm is not arm [0] are
  the 9 perl virtuals above (18 groups = 9 × {vdb, ebuild}).
- In DEPEND/BDEPEND, 49 groups `|| ( >=autoconf-2.73:2.73 >=autoconf-2.72-r1:2.72 )` and the matching
  automake pair. In XML-Parser all 19 of their evaluations are in complete mode (`_graph_trees`) and
  pick the installed arm [1], because 2.73/1.19 are not graph nodes. Both the installed-arm rule and a
  post-merge `dep_zapdeps` pick [1] there.
- Every other installed `||` reached in XML-Parser has its installed arm first.

**Per cell** (Portage through `which-trees-or.py`, `S0B_PARENT_RE=^virtual/perl-`). Every cell has the
same try-0 shape: one `_slot_confict_backtrack` on `dev-lang/perl:0`, ABI False, choice[1] = {5.40.2,
5.44.0} explored first. In every cell the 9 5.44-first virtuals pick `=5.44*` in try 0 in both views.

| cell | Portage count / skip rows | portuale L0 count / rows | Portage hops after try 0 |
|---|---|---|---|
| dev-lang_perl | 1/20, 69 | 1/20, **2** | success |
| dev-perl_XML-Parser | 3/20, 69 | 3/20, 42 | 2 × missing dep (Bzip2, IO-Compress) |
| virtual_perl-ExtUtils-MakeMaker | 2/20, 67 | 2/20, 40 | missing dep `virtual/perl-ExtUtils-MakeMaker-7.780.0` |
| dev-db_mariadb | 2/20, 69 | 2/20, 42 | missing dep `virtual/perl-Storable-3.410.0` |
| dev-lang_ruby, dev-ruby_rake | 1/20, 69 | 1/20, 42 | success |
| kde-apps_kdecore-meta | 3/20, 69 | 3/20, 42 | same as XML-Parser |
| media-gfx_gimp, gnome-base_gnome-shell | 0/20, rc 1 | 0/20, rc 1 | `need_config_change` True (handler changes empty, so another arm of D:11838-11885), no feedback |

- In every rc-0 cell, Portage's skip block has the 9 `=5.42*` rows of the 5.44-first virtuals, and so
  does portuale's (except in dev-lang_perl).
- In gimp and gnome-shell, Portage's rc-1 conflict display shows `=dev-lang/perl-5.44* required by
  (virtual/perl-Exporter… installed)` as a parent of 5.44. portuale shows `virtual/perl-Time-Local`
  (ebuild).
- The display picks are set-order sensitive: with the wrappers loaded, Portage printed XSLoader instead
  of Exporter, and the skip-row order changed in ruby and rake. Row **sets** stayed identical in the
  rc-0 cells (`sort | diff` empty).

## Escapes and edge cases

- The slot conflict handler's suggestion stops backtracking before any feedback (D:11849-11865 →
  D:12407-12411; cgp USE). `grep` finds no port of the handler's `It might be possible to solve this slot
  collision` suggestion in `crates/`.
- An installed conflict instance stays in the graph after it is masked, so the same conflict comes back
  ("loop detected"), and a feedback node equal to the current one is dropped (B:126). This is what makes
  both cgp VER counts 3, not 2 or 4.
- The ABI path cannot fire when every slot node is a conflict package (D:2317-2319). That is the case in
  every bed cell here, so the built atoms go straight into the choice list.
- Skip-row order is Python set iteration (D:1665-1720 iterates `parent_atoms` unsorted). It is
  deterministic only per `PYTHONHASHSEED`, and loading a wrapper changes it.
- `--debug` itself does not change the result: the `--debug` and plain runs gave the same rows and counts
  (cgp, XML-Parser).
- The host runs (cgp) also read the host's `/` vdb (read-only): Portage printed `The following installed
  packages are masked: dev-python/entrypoints-0.4` from the host. Resolution was not affected.

## Not established

- Why `dev-lang_perl`'s portuale skip block has only 2 rows (Portage 69). Not traced.
- portuale's counts under 10a for the cgp VER cells. Portage's 3/20 needs the masked **installed**
  instance to stay in the graph and re-conflict. If portuale's `MaskChoice.installed` hides it
  (`backtrack_params.rs:13-16`, per the plan; I did not read it), the count will differ. This is the
  same question as S0d.
- Whether 3.0.82.2 (no interning) gives the counterfactual's rc 1 on the bed. A guess from the
  counterfactual only. My counterfactual switches off both caches and may not equal 3.0.82.2.
- Which `need_config_change` arm fires in gimp and gnome-shell (`changes=[]`, so it is the autounmask or
  REQUIRED_USE arm). Not separated.
- How `compare/resolve-compare.py` grades skip-row order. Not read.

## Predicted moves

**10a** (consumer pin becomes a slot conflict through `_slot_confict_backtrack`, built atoms as conflict parents):
- **Bed, skip rows.** In XML-Parser, MakeMaker, mariadb, ruby, rake and kdecore-meta, portuale gains the
  27 missing `dev-lang/perl:0/5.42=` rows (26 real consumers + `virtual/perl-File-Temp`), so the row set
  becomes Portage's (69; 67 for MakeMaker). That would clear `perl-544-stabilization-built-pin-residual`
  there, unless row order is graded.
- **Bed, counts.** They should stay as they are: choice[1] (69 atoms) still sorts last and succeeds, as
  today's single mask does.
- **Bed, dev-lang_perl** (2 rows): unknown.
- **Bed, gimp and gnome-shell**: no change, as long as P3 keeps the config-change break.
- **Contract, cgp VER keeper cells**: 0/20 → **3/20**, with rows, rc and residual unchanged. The keeper
  test asserts neither the count nor anything else that changes. There is no corpus entry for the keeper
  tests (no `complete_graph_*_with_world_consumer` key in `corpus/contract.json.xz`).
- **Contract, cgp USE keeper**: Portage stays at **0/20** through the slot-conflict-handler break. 10a
  must reproduce that break (D:11849-11865) at the P2 site. Without it, removing G1 makes portuale back
  off over the cgp0x record, so its count leaves Portage's.
- **Contract, cgp opt-out CASES rows** (6): no change.

**10b** (`||` choice through `dep_zapdeps` over the right view):
- **Bed, 9 `=5.42*` rows per cell.** For the 9 5.44-first virtuals, Portage's own choice is `=5.44*` in
  both views, so a plain 10b **removes** their 9 `=dev-lang/perl-5.42*` skip rows. Portage prints them
  only because of the interned-atom attribution (§2). In the 6 rc-0 cells (`dev-lang_perl` too, which
  has 1 today), portuale would move **away** from Portage, by -9 rows each.
  - To stay with Portage, 10b must also port the attribution: when an installed virtual is reached
    through indirect virtual expansion, every atom of its reduced dep string that is equal by value to
    an atom selected anywhere in the same parent's dep_check becomes a parent atom of that virtual
    (D:6122-6133, D:6164-6167 with PD:130).
  - Or it must keep the installed-arm rule for that path.
- **Bed, `=5.44*` parent atoms.** 10b would also add the 9 virtuals' `=dev-lang/perl-5.44*` as parent
  atoms of 5.44 (choice[0] atoms: 5 → 14 in portuale's terms, still fewer than 69, so the order and
  counts do not change). It could change which parent the gimp/gnome-shell rc-1 display picks; that pick
  is set-order dependent in Portage, so it cannot be predicted.
- **Bed, autoconf/automake BDEPEND groups**: no change (both rules pick [1]).
- **Contract pins**: none of the cgp pins has a `||`, so 10b does not touch them.
