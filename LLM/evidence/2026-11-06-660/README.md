# #660: the 38 drifting expanded-corpus rows against Portage

Run: `differential-test-bed/run/l0-fixture-oracle.sh` (pmtest) with `atomlist.txt` (the 38 drifting
cases), Portage 3.0.83 vs portuale at `a917480e`, 2026-11-06. `real/` is Portage, `portuale/` is
portuale (stdout and stderr merged), `meta.tsv` has the exit codes (all 38 equal), `l0-report.txt` the
comparator's report.

What the rows show, and why the bless in pmtest `f4580d1` is right:

- 32 `--pretend --quiet` rows: Portage prints neither `These are the packages that would be merged` nor
  `Calculating dependencies ... done!`; portuale now prints neither (the old corpus value had both).
- 4 `--pretend --verbose` rows: `::testrepo` where the old value had `::`.
- 2 `omasked` rows: exit 0 and `[ebuild  N     ] dev-libs/omasked-1.0` in both. Portage also prints a
  `!!! The following installed packages are masked:` block that portuale does not (filed as #662).

The report's 11 unexplained findings are not drift: 2 are the #662 block, 4 are the comparator reading
the `package.mask` comment dump as a block (#664), 1 is `implicitiusepkgmusl` (#663), the rest are
wording lines on the explained list.
