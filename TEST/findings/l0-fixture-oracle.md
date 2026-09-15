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

## What a fixture addition must not break

A new fixture that real will read needs: valid bash in its ebuild
description (no backquotes), a digest for **every** ebuild in its package
dir, a category in `etc/portage/categories`, and no `*/pkg` atoms or
`profiles/updates` comments. `dev-libs/blockusedeptarget`'s description
was the backquote case (fixed in staging); a new fixture that trips one
of these shows up as a real-only staging diagnostic, not as a portuale
difference — check `stage.sh`'s header before filing a finding.
