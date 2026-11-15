# #757 — the vdb-lockfile race at four suite copy sites

## What was observed (not engineered)

Two independent runs failed on the same file, at two different tests, both
passing solo immediately afterwards:

- `observed-failure-test_733-parallel.log` — the #755 parallel gate run
  (`scripts/pytest-parallel.sh`, 8 workers): `test_733_tree_does_not_nest_a_row_under_a_hidden_leaf[pubgrub]`
  died in setup with
  `shutil.Error: [('…/fixtures/var/db/.pkg.portage_lockfile', …/root/var/db/.pkg.portage_lockfile', …)]`.
  The excerpt is also isolated in `observed-failure-shutil-error.txt`.
- `observed-failure-test_698-bless.txt` — the #755 corpus-bless run:
  `test_698_a_holder_without_a_route_still_withholds_the_upgrade[portage]`
  failed the same way; it passes 2/2 solo.

No engineered reproduction was attempted. The race needs a concurrent
`emerge` to take and release the vdb lock inside the copy's listing window,
which is a load property, not an input property; and the deterministic pin
does not need one, because a *copied* lock is wrong whether or not it
vanishes mid-listing (lock state is not transferable, and a copy that lists
it as its owner unlinks it dies).

## What makes the pin deterministic

`pytests-contract-suite/test_fixture_copy.py` plants the five volatile names
in a scratch tree and asserts none travels through
`fixtures_copytree_ignore()` — red against a bare `shutil.copytree` (the lock
travels) and red against the old three-literal predicate (the hardlink
sibling and the inode-test probe travel). It also scans this suite's own
sources for a `var`/whole-ROOT copy without the filter, so a *new* site
cannot be written without one.

Red-first evidence: with the fix stashed, 2 of its 4 tests fail
(`test_the_copy_filter_drops_every_volatile_name_and_nothing_else`,
`test_no_copy_of_the_vdb_or_the_whole_root_is_unfiltered`); with the fix,
4 pass.
