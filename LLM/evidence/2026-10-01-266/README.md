# #266 — real Portage probes (backlog #266, 2026-10-01)

Real Portage 3.0.82.2 (`3rdparty/portage` @ `1d95fc2c` for the
playground, host `/usr/sbin/emerge` for the staged-fixture probes) on
the entry's shape: `>=foo-1[bar]` alongside `<foo-3` on an ebuild
parent, both satisfiable by the lower pick.

Shape: `dev-libs/r266mid-1.0` with
`RDEPEND=">=dev-libs/r266lib-1[bar] <dev-libs/r266lib-3"`; the provider
ships 1.0/2.0/3.0 in distinct slots 1/2/3 (so the un-collapsed pair can
co-merge instead of slot-conflicting) with `bar` default-off in
1.0/2.0 (`IUSE="-bar"`) and default-on in 3.0 (`IUSE="+bar"`). The `<3`
atom always picks 2.0; the `>=` atom always picks 3.0 on its own.

- **Profile B (defaults):** 2.0 builds `-bar`, so the `>=` atom matches
  only 3.0 and the pair does NOT collapse. Real merges all three and
  reports the `bar` enablement as a USE-changes block (rc 1).
- **Profile A (`dev-libs/r266lib bar` in `package.use`):** 2.0 builds
  `+bar`, so it satisfies both atoms and real collapses the pair onto
  2.0 (rc 0, two rows, no block).

## `playground/` — real's own ResolverPlayground

Run from `3rdparty/portage` with `PATH=$PWD/bin:$PATH` and a copy of
`lib/portage/tests/.gnupg` as `PORTAGE_GNUPGHOME` (same staging as
`docs/evidence/2026-09-29-236/`).

- `r266_pg.py` → `r266.log`: profile B mergelist
  `[r266lib-3.0, r266lib-2.0, r266mid-1.0]` (success True — the
  playground does not fail on the autounmask USE-changes notice);
  profile A (`USE="bar"` in `make.conf`) mergelist
  `[r266lib-2.0, r266mid-1.0]`.

## `staged-probes/` — host `/usr/sbin/emerge` against fixture copies

`PORTAGE_CONFIGROOT`/`ROOT`/`PORTAGE_RUNNING_ROOT` at a copy of
`../pmtest/fixtures` (including the committed `r266lib`/`r266mid`
fixtures), cwd at that copy for the relative `repos.conf` locations,
argv `--ignore-default-opts --pretend [--color=n] dev-libs/r266mid`.

- `profile-B-default.log`: rc 1, `backtrack: 0/20`, rows
  `N r266lib-3.0 USE="bar"` + `N r266lib-2.0 USE="-bar"` +
  `N r266mid-1.0`, plus `The following USE changes are necessary to
  proceed: ... >=dev-libs/r266lib-3.0 bar`.
- `profile-A-bar-on.log` (same plus `dev-libs/r266lib bar` appended to
  `etc/portage/package.use`): rc 0, rows `N r266lib-2.0 USE="bar"` +
  `N r266mid-1.0`, no block.
- `r25-reground.log`: the #25 r25 shape re-probed the same way
  (`FX_WORLD_EXTRA=dev-libs/r25consumer` equivalent: `r25consumer`
  appended to the copy's world; `--update --deep --newuse --oneshot
  dev-libs/r25target`): rc 0, `backtrack: 0/20`, rows `U r25up-2.0` +
  `N r25target-1.0`, no skipped-update block — the existing
  `test_236_r25_default_backtracking_settles_in_one_silent_pass`
  expectation is still real's output (the pin did not move for #266).

Before the fix portuale printed three rows in both r266 profiles (a
use-dep atom only ever matched its own pick for an ebuild candidate);
after it, portuale matches real in both.
