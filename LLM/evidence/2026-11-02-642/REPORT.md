# #642 S0 oracle: Portage fires the conflict probe on `-pvD p569pull`

Host Portage 3.0.83, scratch cell `/var/tmp/s642` (copy of `pmtest/fixtures`,
same layout as `LLM/evidence/2026-10-26-575-residues/r5/run-portage.sh`):

```sh
S=/var/tmp/s642   # + fx.cfg sibling, repos.conf as in r5/run-portage.sh
env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$S/fx.cfg ROOT=$S/fx \
  PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 PYTHONUNBUFFERED=1 /usr/bin/emerge \
  --ignore-default-opts --getbinpkg=n --usepkg=n --color=n \
  -pv --deep --debug dev-libs/p569pull >D-debug.log 2>&1
```

Findings (`D-probe-filing-excerpt.log`):

1. The initial pass records the prov conflict with three parent edges --
   `pull-2 =5.44* -> 5.44`, installed `virt-1 0/5.42= -> 5.42`, installed
   `virt-1 := -> both`. No menu arm (`=5.42*` etc.) is recorded as a parent
   atom: the walk selected the `~p569core-1.0` arm (core merges `N`), exactly
   what portuale's `selected_parent_atoms` (in-graph first) selects.
2. The probe fires twice (`backtracking due to missed slot abi update`,
   parent `virt-1`, child installed `prov-5.42`, atom `0/5.42=`) and the
   search restarts exactly once (`backtracking try 1`, final `1/20`).
3. Try 1 has a single prov node (Upgrade 5.42 -> 5.44, tagged `r U`); no
   same-version reinstall of 5.42 exists beside it. The filed child does not
   materialize a second node: Portage formulates the reinstall as a slot
   atom (`prov:0`) re-selected post-restart, and selection picks 5.44.

Consequences for the fix: the probe must file on this shape (the pre-#642
veto was unfaithful), the `r` tag on the upgrade row comes from the filing,
and the single restart requires no ghost reinstall node -- hence the
logic/display split for a subsumed child.
