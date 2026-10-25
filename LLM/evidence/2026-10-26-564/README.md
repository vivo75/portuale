# Task 10 (#564) Step 1 evidence — reproduce + filter (2026-10-26)

## Reproduction: shape `30-regen-user` on both managers

Probe rebuilt from `LLM/evidence/2026-10-24-513/probe.sh` (single shape),
`PORTAGE_CONFIGROOT` + `ROOT` under `/var/tmp/ev513-tree/30-regen-user`,
both sides rc 0 (`30-regen-user.{portage,portuale}.rc`):

- Portage `--regen`: 20365 unique `Processing` lines (`portage-processing.txt`)
- portuale `--regen`: 926 unique `Processing` lines (`portuale-processing.txt`)
- `comm`: 19439 host-only cps, 926 shared, 0 portuale-only
  (the Oct-24 `kde-base/ark` 1-cp delta is gone on today's tree — host or
  fixture categories shifted; traversal gating is Task 11's question, not
  this one).

The delta comes solely from the global-file merge: Portage's
`--info` repo set is `{testrepo, gentoo @ /.gentoo/repos/gentoo}`
(`portage-info-repos-unfiltered.txt` — `gentoo` from the installed
`/usr/share/portage/config/repos.conf`, `repository/config.py:1490-1505`),
while portuale (pre-Step-2, config-root only) sees `{testrepo}`.

## Filter check (mount namespace, comments-only bind)

Same Portage `--info` with the filter bind over the installed file:
repo set `{testrepo}` only (`portage-info-repos-filtered.txt`) — the host
`gentoo` is invisible inside, the host file untouched outside, uid 1000
preserved (see the `test_repos_filter.py` self-test for the concurrent
case). Mount propagation verified private (host reads its own file
throughout a filtered sleep).

## Two Portage-under-filter frictions found (both probe-env, not suite)

1. With `ROOT != /`, Portage also loads a *running-root* config
   (`config_root=None`, `portage/__init__.py:524-527`) whose env inherits
   the target's `PORTAGE_REPOSITORIES` (`config.py:613`). Hiding the
   global file empties the running repo set, so the host profile
   (`/etc/portage/make.profile` → buildovl chain with a `gentoo:` parent)
   fails to parse and `profile_check` (`_emerge/main.py:1204`) refuses
   `--regen` (rc 1). The contract suite never runs Portage, so this binds
   only manual Portage probes: compare repo sets via `--info` (which needs
   no valid profile) and attribute the `Processing` delta with `comm`
   against the unfiltered `--regen` pair, as above.
2. Inside the user namespace, `chown` to the `portage` group fails
   (`EINVAL`), so Portage `--regen` with `ROOT=/` cannot create its
   `/.gentoo/cache/edb/dep` dir there either. Same scope note as above.

## Step 2 live proof (2026-10-26, post-change binary)

- Unfiltered `portuale emerge --info` on the probe root: repos
  `{testrepo, gentoo @ /var/db/repos/gentoo}` -- the merge is live
  (`portage-info-repos-unfiltered.txt` shows Portage's identical set).
- Unfiltered `portuale emerge --regen`: 20348 unique `Processing` lines
  (`portuale-processing-unfiltered.txt`) vs Portage's 20365. `comm`: zero
  portuale-only lines; the 17 Portage-only lines are all non-category
  top-level dirs of the host repo (`Processing profiles/...`,
  `Processing metadata/...`, `Processing eclass/tests`) -- a traversal
  shape (`_iter_every_cp` vs the dir-walk `all_cp`), Task 11's question,
  not repo selection: the repo sets match exactly.
- Filtered `portuale emerge --regen`: 926 lines, byte-identical count to
  the pre-change binary -- the suite's `Processing` counts are unchanged.
