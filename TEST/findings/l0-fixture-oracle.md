# L0 fixture oracle — real `emerge` on the checked-in fixture tree

Backlog #49 / `docs/second_python_copy_removal.md` §6. Runs the real
`emerge -p` on a staged copy of `fixtures/` and compares it to
portuale's own output through the same comparator the L0 bed uses.

```sh
TEST/run/l0-fixture-oracle.sh          # atom list: TEST/atomlists/l0-fixture-oracle.txt
# -> TEST/logs/l0-fx-<stamp>/l0-report.txt + l0-report.json
#    rc 0 green (every finding explained), 1 unexplained
```

## What shipped

| piece | path |
|---|---|
| staging (container-side) | `TEST/layers/l0-fixture-oracle/stage.sh` |
| runner (container-side) | `TEST/layers/l0-fixture-oracle/in-container.sh` |
| entry point (host) | `TEST/run/l0-fixture-oracle.sh` |
| cases | `TEST/atomlists/l0-fixture-oracle.txt` |
| allowlist | `TEST/compare/known-divergences-fixture-oracle.yaml` |

The staging deltas (each one a real-Portage requirement the checked-in
fixture tree does not satisfy; the checked-in tree stays untouched):

1. `repos.conf` relative `location =` -> absolute staged paths (real
   rejects the relative form portuale accepts);
2. `binrepos.conf`'s `${PORTAGE_CONFIGROOT}` resolved (portuale-only
   interpolation);
3. `etc/portage/categories` added (real masks a package whose category is
   not listed);
4. one `Manifest` per package dir with **every** ebuild's
   BLAKE2B/SHA512 (real masks a digest-less ebuild as corruption and a
   multi-version dir missing a line as "not listed in the Manifest");
5. `repo/metadata/news` dropped (portuale-format fixture news, real
   rejects them);
6. `/etc/make.local` touched in the throwaway container (fixture
   `make.conf` sources it);
7. backquotes in staged ebuilds become single quotes (fixture
   `DESCRIPTION`s use `` `flag` `` prose, which bash evaluates as command
   substitution when real's depend phase sources the ebuild; metadata is
   prose, no resolution input changes);
8. `masters = testrepo` appended to the overlay repos' `layout.conf`
   (real warns otherwise; `repnamerepo` has a `layout.conf` without it);
9. comment lines stripped from staged `profiles/updates/*` (real's parser
   errors on them);
10. `*/<pkg>` profile atoms rewritten to the package's real category
    (real reports "Invalid atom" otherwise);
11. `profiles/repo_name` added where missing (real warns otherwise);
12. after the portage pin upgrade, the container's `/var/db/pkg` is
    replaced with the fixture vdb, so the running root's installed set is
    the fixture set -- otherwise real prints "The following installed
    packages are masked" (the image's gentoo set is not in the staged
    fixture repos) on every probe. Portuale's `fixture_env` pins
    `PORTAGE_RUNNING_ROOT` to fixtures for the same reason.

Portage is pinned/upgraded to `3.0.82.2` before the cases run, like the
L0 bed.

## Result (2026-09-15, `TEST/logs/l0-fx-20260915T101508Z`)

12 cases, **5 clean** (no findings at all), 20 findings explained, 0
unexplained:

- clean: `dev-libs/diamond`, `dev-libs/anyof` (`||` group),
  `dev-libs/iusedefaultpkg` (REQUIRED_USE), `dev-libs/dualslotpkg`
  (multi-slot), `dev-libs/blockusedeptarget`;
- explanation-only (wording/staging path, see the allowlist):
  `dev-libs/autounmaskkeywordpkg`, `dev-libs/kwneedpkg`,
  `dev-libs/requiredusebadpkg`;
- **genuine resolver differences, filed as backlog #54** (suppressed by
  two `owner: portuale-bug` allowlist entries so the layer can gate other
  changes): `--update dev-libs/paired`, `dev-libs/needer`,
  `=dev-libs/paired-2.0`, and the `needer + othermod` triangle's block
  content/exit.

## #54 — the installed-consumer pin over-approximation

Real 3.0.82.2 in the staged (hermetic) environment:

| case | real | portuale |
|---|---|---|
| `--update dev-libs/paired` | `[ebuild U] paired-2.0 [1.0]`, rc 0 | silent, keeps 1.0 |
| `dev-libs/needer` | `U 2.0` + `N needer`, **no block**, rc 0 | same list **+ block** |
| `=dev-libs/paired-2.0` | `U 2.0`, **no block**, rc 0 | `U 2.0` **+ block** |
| `needer dev-libs/othermod` | `U 2.0` + `N needer` + `N othermod` + block, rc 1 | same list + block, rc 0 |

Mechanism, from real's source: complete mode is only enabled when the
package tracker already holds a **slot conflict**
(`depgraph.py:9446-9453` `_resolve_conflicts`), and the end-of-walk loop
that pulls an installed satisfier into the graph only sees deps of nodes
already in the graph (`depgraph.py:8562+`, the `_unsatisfied_deps` loop).
`keeper-1.0`'s `=paired-1.0` pin is outside every target closure, so it
is only walked when a competing constraint (`othermod`'s `<paired-2.0`)
keeps the installed `paired-1.0` in the graph — the triangle. In the
single-sided shapes real replaces `paired-1.0` and never sees keeper.
Portuale applies the pin (and renders the residual block) unconditionally.

The three differences are pinned the wrong way in the contract suite
(`test_satisfiable_installed_pin_still_holds_the_upgrade`,
`test_explicitly_pinned_upgrade_breaks_an_installed_pin_and_reports_it`,
`test_hard_dependency_requirement_breaks_an_installed_pin_and_reports_it`)
with docstrings claiming live verification; those pins must move with the
#54 fix, and the triangle's expected parents include keeper where real
names only `othermod`.

## #54 S0 — oracle matrix: world state × case × complete-graph gate

`docs/05.054-residual_conflict.opus.md` §S0's hypothesis H: the 511e659
docstrings ("verified live") came from a probe where `keeper` was
reachable from `@world`; both oracles are right under different world
states, and the fix is to restrict the consumer scan to reachable
consumers.

Bed: `TEST/layers/l0-fixture-oracle/stage.sh` now honours
`FX_WORLD_EXTRA` (space-separated atoms appended to the staged
`var/lib/portage/world`); a throwaway vdb-only fixture
`fixtures/var/db/pkg/dev-libs/keeperroot-1.0` (`RDEPEND=dev-libs/keeper`,
no matching ebuild — the same "vdb orphan" pattern as `oldmovepkg` etc.)
gives a transitive-reachability cell. Real: `emerge -p --pretend` inside
`localhost/test-portuale:latest`, staged via `in-container.sh` directly
(not through the atomlist-comparator wrapper — this is an oracle-only
exploration, not a pinned case set), one throwaway run dir per cell.

| world state | `--update paired` | `needer` | `needer othermod` | `=paired-2.0` |
|---|---|---|---|---|
| hermetic | `[U] 2.0`, silent, rc 0 | `U`+`N needer`, no block, rc 0 | 3 merges + block (`othermod` only), rc 1 | `U`, no block, rc 0 |
| keeper in `world` | **new shape**: no merge, `WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict`, rc 1 | `U`+`N needer`+block (parent: keeper), rc 1 | 3 merges + block (parents `othermod` **and** keeper), rc 1 | `U`+block (parent: keeper), rc 1 |
| `keeperroot` (RDEPEND keeper) in `world` | (not re-run: same reachability closure as the row above) | byte-identical to "keeper in world" (only `Dependency resolution took` differs) | byte-identical | byte-identical |

Run dirs: `TEST/logs/l0-fx-054-hermetic`, `l0-fx-054-keeperworld`,
`l0-fx-054-keeperroot` (all four cases each).

Extra cells:

- hermetic, `--complete-graph-if-new-ver=n` (`l0-fx-054-noauto`):
  `--update dev-libs/paired` merges `[U] 2.0` with **no** block (auto-enable
  off means complete mode never triggers, so the pin -- reachable or not --
  is never even scanned); `dev-libs/needer` the same, `U`+`N needer`, no
  block. Confirms the flag fully bypasses the mechanism, independent of H.
- keeper in `world`, `--nodeps` (`l0-fx-054-nodeps`): `=dev-libs/paired-2.0`
  merges `[U] 2.0` with no block -- `--nodeps` disables the dependency
  walk entirely, so complete mode's consumer scan never runs even with a
  reachable pin present.

**Verdict: H holds for three of the four cases** (`needer`, `=paired-2.0`,
the `needer`+`othermod` triangle) — with `keeper` reachable from `@world`
(directly or transitively through `keeperroot`), real reproduces
511e659's exact shapes byte-for-byte (module the volatile "Dependency
resolution took" line), confirming the fix is reachability gating, not a
mechanism real doesn't have. The `--update dev-libs/paired` case is a
**new third shape** under keeper-in-world that 511e659 never observed: no
merge at all plus a `WARNING: ... skipped due to a dependency conflict`
notice (portage's "the entry never enters the graph, so it reports a
skip instead of a slot collision" branch — a *different* real code path
than the `!!! Multiple package instances...` block the other three
cases render, since there is no competing hard requirement forcing the
upgrade in this shape). `test_satisfiable_installed_pin_still_holds_the_
upgrade` only pins the hermetic (silent) shape and needs no change; the
keeper-in-world shape for this one case is new coverage, not a
correction, if S1 adds it.

Commit: bed switch + throwaway fixture + this section (no code change).

## What a fixture addition must not break

A new fixture that real will read needs: valid bash in its ebuild
description (no backquotes), a digest for **every** ebuild in its package
dir, a category in `etc/portage/categories`, and no `*/pkg` atoms or
`profiles/updates` comments. `dev-libs/blockusedeptarget`'s description
was the backquote case (fixed in staging); a new fixture that trips one
of these shows up as a real-only staging diagnostic, not as a portuale
difference — check `stage.sh`'s header before filing a finding.
