# #391 oracle: `movedepconsumer` with package moves on (2026-10-19)

Backlog #391 (`LLM/backlog-tasks-2026-10.md:179`): fixture
`dev-libs/movedepconsumer` has `RDEPEND="dev-libs/oldmovepkg"`, which the
fixture `profiles/updates/2Q-2024` moves to `dev-libs/newmovepkg`
(`move dev-libs/oldmovepkg dev-libs/newmovepkg`; vdb still holds
`dev-libs/oldmovepkg-1.0` under its pre-move dir).

## Staging

`../pmtest/fixtures` copied to `/var/tmp/pt-391/st/fixtures` with
`differential-test-bed/layers/l0-fixture-oracle/stage.sh` (adapted for the
host: `$SRC` instead of `/fixtures`; full deltas: absolute `repos.conf`
locations, `binrepos.conf` interpolation, `categories`, per-package
`Manifest`s, news dropped, `common-1.0` staled, `layout.conf` masters,
updates comments dropped, `*/<pkg>` rewrite, `repo_name` fill). Fresh
stage per cell: global updates mutate the staged vdb.

Host Portage 3.0.82.2 env (same shape as `run-s0-portage.sh`, minus
`PORTAGE_REPOSITORIES`: inlining the staged `repos.conf` content breaks
the host running-root config's own `gentoo:` parent resolution, so each
config reads its own file):
`PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=$FX`,
`DISTDIR=$FX/distfiles`, `PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC`,
`CLEAN_DELAY=0`. portuale env identical plus `HOME=/var/tmp/pt-391`.

## Verdict: DIVERGED (pre-fix), AGREE (post-fix)

| cell | Portage 3.0.82.2 | portuale pre-fix | portuale post-fix |
|---|---|---|---|
| `-pv --deep dev-libs/movedepconsumer` | rc 1, `no ebuilds to satisfy "dev-libs/oldmovepkg"`, backtrack 1/20 (`portage-deep.txt`) | rc 0, `N newpkg` + `N movedepconsumer` (`portuale-deep-before.txt`) | rc 1, same error text modulo the ` for <root>` suffix + backtrack rendering (pre-existing cuts) (`portuale-deep.txt`) |
| `--pretend dev-libs/movedepconsumer` | rc 1, same error (`portage-p.txt`) | rc 0, `N movedepconsumer` (`portuale-p-before.txt`) | rc 1, same error (`portuale-p.txt`) |
| `-pv --package-moves=n --deep` (control) | rc 0, `N movedepconsumer` (S0 `s0-d-movedepconsumer-deep-nomoves.txt`; a #360 installed-only instance, out of scope) | rc 0, `N movedepconsumer` | unchanged, rc 0 `N movedepconsumer` (`portuale-deep-nomoves.txt`) |
| `--pretend --package-moves=n` (control) | rc 0, `N movedepconsumer` | rc 0 | unchanged (`portuale-p-nomoves.txt`) |

Note: each `portuale-*.txt` header records the binary md5; the pre-fix
captures were taken with the 2026-10-19 01:13 binary (HEAD `9a909749`
clean), the post-fix captures after rebuilding with the #391 diff.
`target/release/emerge` is a symlink to `target/release/portuale`.

Host-Portage noise (recorded verbatim, not part of the verdict): the
host `gentoo` repo's own updates passes, `Unsupported binary package
format from ''` × 13 (fixture binhost refresh), and no host-profile
complaints (each config reads its own repos).
