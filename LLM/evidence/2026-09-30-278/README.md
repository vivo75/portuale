# Evidence 2026-09-30 — #278 cycle block start node (O1 S0)

The #249 fixture variant where the puller also depends on the build
tool directly: `dev-libs/u249json-1` with

```
BDEPEND="virtual/u249make dev-util/u249make"
```

(committed fixture carries only `virtual/u249make`; the variant adds
the direct `dev-util/u249make` edge, staged per
`/tmp/opencode/o1/stage-variant.sh`). Both sides abort with the same
two-node ring (`u249make -BDEPEND-> u249json -BDEPEND-> u249make`,
plus the `virtual/u249make` runtime branch), but print the cycle from
different nodes depending on the staging (below).

## Probes

Identical argv on both sides, both stagings:

```
emerge --ignore-default-opts -p --backtrack=0 dev-util/u249make
```

Environment (both PMs): `PORTAGE_CONFIGROOT=$FX`,
`DISTDIR=$FX/distfiles`, `PORTAGE_REPOSITORIES` read from the staged
`$FX/etc/portage/repos.conf/repos.conf`, `LC_ALL=C.UTF-8`, `TZ=UTC`,
`PYTHONHASHSEED=0`, empty `EMERGE_DEFAULT_OPTS`. Default staging adds
`ROOT=$FX PORTAGE_RUNNING_ROOT=$FX`; `FX_HOST_ROOTS=1` (hostroots)
uses `ROOT=/ PORTAGE_RUNNING_ROOT=/` with the staged vdb copied to
`/var/db/pkg` (the `l0-fixture-oracle` `in-container.sh` shape).

- Real: host `/usr/sbin/emerge`, `Portage 3.0.82.2`, captured in the
  container (`/tmp/opencode/o1/cells-container.sh` staging
  `/tmp/o1stage`, 2026-10-01). Host-side re-run is not possible: host
  real reads the host `/etc/portage/make.profile`, which the staged
  fixture cannot satisfy.
- Portuale: worktree `rust/target/release/portuale` with the #278 port
  (`find_hard_cycles` starts where real's `shortest_cycle[0]` starts),
  captured host-side (`/tmp/opencode/o1/ev/`, 2026-10-01; identical
  bytes to the previous run's `portuale-*.txt` pair).

## Start nodes (rc 1 on all four)

| capture                | printed cycle                          | starts at          |
| ---------------------- | -------------------------------------- | ------------------ |
| `real-default.txt`     | `u249make -> u249json -> u249make`     | `dev-util/u249make` |
| `real-hostroots.txt`   | `u249json -> u249make -> u249json`     | `dev-libs/u249json` |
| `portuale-default.txt` | `u249json -> u249make -> u249json`     | `dev-libs/u249json` |
| `portuale-hostroots.txt` | `u249json -> u249make -> u249json`   | `dev-libs/u249json` |

So the rotation persists in the sense that matters for the LOCAL
decision: real's single-root rendering (`FX_HOST_ROOTS=1`) starts at
`u249json`, and portuale now starts there in both stagings. Real's
default-staging rendering differs twice over: the merge list gains the
cross-root top-level row (`[ebuild N] dev-util/u249make-1 … to $FX`,
`Total: 4 packages`) and the cycle starts at the other node. Both are
the cross-root graph (BDEPEND resolved against the running root plus
the extra requested-package merge node) that Track X (#242) owns —
the same split the `cyc0` strict-xfail reason already records
("live real builds the cycle over its cross-root graph … so the start
node differs"). Portuale renders the single-root graph in both
stagings (no cross-root split yet), so `u249json` everywhere is the
correct LOCAL outcome; full default-staging agreement waits for
Track X.

Real's rule (grounded in `3rdparty/portage`, same 3.0.82.2):
`digraph.get_cycles` (`lib/portage/util/digraph.py:387`, nodes in
insertion order, per node each child's shortest child-`>`node path)
fed to `_find_cycles`
(`lib/_emerge/resolver/circular_dependency.py:48`, first
strictly-shortest wins), printed from `shortest_cycle[0]`
(`_prepare_circular_dep_message`, `:76`). The port mirrors exactly
that: iterate the scheduler digraph's pre-bias insertion order (real
`_create_graph`'s LIFO order, threaded out of `serialize_merge_order`
as a rank), close each node's cycle through each child, keep the
first strictly-shortest. Verified against real's single-root
rendering on every pinned cycle shape, not just this variant:
`hardcyclea`, `slopcyca`, `usecyclea`, `gpcyclec`, `cyc4a`
(`dev-libs/cyc4b` first), `=dev-libs/cyc0b-1` (`dev-libs/cyc0a`
first), `=dev-libs/cyc0w-3` (`dev-libs/cyc0y` first, strict-xfail now
XPASSes), `=dev-libs/cyc0z-1` (`dev-libs/cyc0y` first), the three
`u251` orders, and this variant (`/tmp/opencode/o1/cells-cycle.sh`,
`cells-hostroots.sh`, `cells-cyc0.sh` captures plus a
`ResolverPlayground` oracle for the variant and the two-node rings).
