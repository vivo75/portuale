# Cluster B diagnosis (2026-11-06): #604 #606 #607 #654

Portage 3.0.83. Captures: `/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/3c60fa85-0c1a-42e7-b42c-5a4d843557bb/scratchpad/clusterB/` (scripts `cell.sh pe.sh pu.sh inv.py`; Portage q19 debug at `scratchpad/q19-portage-debug.short.txt`).

## #604 — Q19 leak, interning key
Atom intern key `portage/dep/__init__.py:1577-1595` (string, eapi, repo, build_id, use, unevaluated form, orig_atom, blocker). Parent deps parsed with `eapi=pkg.eapi` even if installed (`depgraph.py:4367-4374`); an installed virtual's atoms use `eapi=None`, an ebuild virtual's its own EAPI (`dep_check.py:866-871`). Only virtual atoms + `||` groups share one dep_check call (`depgraph.py:4872-4904`); plain non-virtual atoms never (`:6122-6131`, `:6164-6167`).
Consequence: installed virtuals leak only into each other; a parent's `||` pick leaks only into an ebuild virtual with the same EAPI.
Probes e604/e604b/e604c/e604d (`--backtrack=0 --verbose-conflicts dev-libs/zz604force dev-libs/zz604par`): portuale leaks parent plain atoms/`||` picks into installed virtuals (wrong, e604, e604c) and misses merge-bound virtuals (e604d, where Portage leaks).
Fix: `leak_one_parent` (`resolve/rev_dep_or.rs:385-386,402,450`): per disjunction-call comparison, key = text + EAPI context, drop parent's plain atoms, expand merge-bound virtual. Fixtures = 4 shapes (real md5 cache). Correct `LLM/portage-quirks.md` Q19 (~line 374) which overstates the leak. Risk medium. Closable.

## #606 — p590vpull 2/20 vs 6/20
Portage's `_slot_confict_backtrack` (`depgraph.py:2220-2279`) sorts by version desc then by conflict-atom count; Backtracker pops last (`resolver/backtracking.py:131-139`). Both choices have 2 atoms, so 5.42 popped first (runs 1-4), 5.44 branch is runs 5-6.
portuale's 5.42 choice has 1 atom (sort `backtrack_params.rs:255`) so 5.44 first. Missing atom: `=p590vprov-5.44*` required by installed `virtual/p590vva` (its own `||` pick).
Fix: record installed walked consumers'/virtuals' selected atoms as `slot_pullers`/conflict parents of the instance they select (`resolve/pass/enqueue_deps.rs:637-660`, `pass/prepare_atom.rs:172`, `slot_conflict.rs:526,569-615`). Pin `backtrack: 6/20` + `--backtrack=0 --verbose-conflicts`. Risk medium-high; 6/20 landing is a guess; #600 may shift by one.

## #607 — settled
Portage prints `<dev-libs/r25lib-2.0:0/1=` then `<dev-libs/r25lib-2.0:=` ("and 1 more"); never the stripped text; `--dynamic-deps=n` prints only the built atom (`FakeVartree.py:179-195`). portuale renders stripped first (`slot_conflict.rs:601`, `rev_dep.rs:1235-1240,1456-1464`), "and 2 more".
Fix: when pin comes from a built slot-op atom and `built_render` non-empty render only that; keep stripped for mask decisions. Re-pin `test_oracle_209` (pmtest `test_emerge_pretend_contract.py:28825`); add `--dynamic-deps=n` cell + unit test.
`force_reinstall` (`depgraph.py:5472-5497`, `_want_installed_pkg` `:7308-7330`) arm 2 missing in `want_installed_pkg` (`rev_dep_or.rs:461-488`); no probe. Risk low (render) / medium (force_reinstall). Closable.

## #654 — no longer reproduces at HEAD
`check_cross_mode` returns `[]` on p652try, virtual/p652foo, p651prov. Premise wrong: Portage `--tree` walks up installed parents and prints `[nomerge]` rows (`output_helpers.py:434-500`, prune `:503-529`).
Real gap (scratch zz654): Portage prints 2 `[nomerge]` rows, portuale none (`pretend/display/tree.rs:480-545`; #25 DONE-PARTIAL residue). Checker tolerance `output_invariants.py:429-480` masks it.
Plan: close #654 as not reproducing with corrected premise; file the `[nomerge]` chain as a new item (next free number).

## Side findings (unfiled)
- Duplicate conflict block printed on e604.
- `p652try`/`p652foo`: portuale 2/20 vs Portage 1/20, equal rows.
