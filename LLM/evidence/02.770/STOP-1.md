# STOP-1 (runbook 02.770 Task 5 Step 4)

Rule: "Any other difference: STOP-1" and "a NOTE or refusal line that vanished is STOP-1 until explained";
the table requires `tree-*` probes byte-identical to before.

Command: `bash LLM/evidence/02.770/probe.sh /var/tmp/770/bin-after /var/tmp/770/raw/after`, diffed against
`before/probes.txt` (`after/probes.diff`).

Expected: `tree-uD-pull/pg` and `tree-uD-bpull/pg` byte-identical to before (false #761 NOTE stays, #781).

Actual (verbatim, before -> after):

tree-uD-pull/pg (`emerge --solver=pubgrub --pretend --verbose --update --deep --getbinpkg=n dev-libs/p569pull`, root ctl)
  before:
    [ebuild  N     ] dev-libs/p569pull-1::testrepo to /var/tmp/770/roots/ctl 0 KiB
    NOTE: One or more updates were not merged because an installed package cannot follow them:
      not merged: 5.44 (dev-libs/p569virt-1 needs =dev-libs/p569prov-5.38*)
      No visible version of dev-libs/p569virt accepts dev-libs/p569prov-5.44.
  after:
    [ebuild     U  ] dev-libs/p569prov-5.44:0/5.44::testrepo [5.42:0/5.42::testrepo] ...
    [ebuild  N     ] dev-libs/p569core-1.0::testrepo ...
    [ebuild  rR    ] dev-libs/p569virt-1::testrepo ...
    [ebuild  N     ] dev-libs/p569pull-2::testrepo ...
      (dev-libs/p569prov-5.44:0/5.44 ...) causes rebuilds for:
        (dev-libs/p569virt-1:0/0 ...)
tree-uD-bpull/pg: the same change with p589bprov / p589bcore / p589bvirt / p589bpull.

The default solver's rows on the same probes (unchanged) are the "after" rows plus the `r` tag on the provider row.
So pubgrub now prints the default's plan on both shapes and the false NOTE / held-back plan (#781 shape) is gone.

Explanation (a reading of the code, not traced): the scan now files the walked `p569virt` rebuild before
`holder_fix` judges the `p569prov` move, so `holder_fix` finds nothing broken and no longer withholds it.
The plan (item 4) and the runbook called these shapes "different cause, byte-unchanged"; that part was wrong for
the `--update --deep` shapes. `-u @world` (no `--deep`) is still byte-unchanged.

Every other hunk matched the table:
  pull-json, pull-uDN, pull-uD: pubgrub gains `rR p569virt-1` + the pair, provider row still `U` without `r`.
  pull-bt0: pubgrub gains `rR p569virt-1` (intended, Q2).
  pull-DN, pull-D, pull-u, tree-D-pull, tree-D-bpull, ctl/prov/r703 json: byte-identical. Every `df` block identical.

Tree state: product change, tests, code notes and evidence are in place and uncommitted. Nothing reverted.
Decision for the owner/coordinator: accept this improvement (list the two tree-uD probes as an expected difference
and adjust the #781 residue text) or reject it.

## Resolution (coordinator, 2026-11-22)

Accepted as an expected improvement, not a defect. Checked against the raw probe outputs
(`after/probes.txt`): on `tree-uD-pull` and `tree-uD-bpull` pubgrub now prints exactly the
default solver's plan (provider `U`, core `N`, virt `rR`, pull-2 `N`, same "causes rebuilds
for" pair); the only difference to the default is the provider's `r` tag, which is the
separate open gap already named in the plan. Before, pubgrub printed a held-back plan
(`N p569pull-1` only) with a false NOTE ("No visible version of ... accepts ...-5.44"):
that was wrong by the charter (the installed consumer can in fact follow the new provider
through its tree ebuild). The false NOTE and the held-back plan are gone on the
`--update --deep` shapes; `-u @world` without `--deep` is byte-unchanged, so residue #781
stays open for it (its text is reworded at close-out). The two `tree-uD-*` probes are
listed as expected differences; the runbook's Task 5 Step 4 table is amended accordingly.
Whether a pinned pubgrub output in pmtest moves is checked by the full suite (Task 6a).

Also: the Task 4 red state was invalid (the test helper dropped its TempDir early, see
`after/red-note.txt`); the corrected helper's real red is recorded there.
