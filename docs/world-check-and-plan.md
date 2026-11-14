# Checking and planning `@world` without the resolver

`mrg` has two actions that read `@world` with the `portage-world` crate and
never start the resolver: `--check-world` and `--plan-world`. They take about
half a second on a full tree. Neither exists in `emerge`.

Both follow `@world` (the world file, `@system` and `@profile`) and, level by
level, the dependencies of the version they would pick for each atom. An
installed package that stays as it is contributes only its `RDEPEND` and
`PDEPEND`; a package that is built (new, upgraded, rebuilt) contributes every
dependency variable. `--with-bdeps=y` also follows the build-time ones of the
kept packages. The Portuale-only switches of these actions are in
[future-options.md](future-options.md).

The fixture tree used below is `pmtest/fixtures`. Run on a real host, the
numbers are different and the shape is the same.

## `mrg --check-world`: can `@world` be satisfied?

It answers one question: does every package slot that `@world` reaches have a
visible ebuild that satisfies every atom aimed at it?

It does not check blockers, `REQUIRED_USE`, slot conflicts or backtracking.
A clean report is not a promise that `emerge` succeeds.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | every slot is satisfied and every ebuild on the path was read |
| 1 | an `INVALID` slot, a `NO MATCH` atom, or an `UNREADABLE` ebuild |
| 2 | the repositories or the configuration cannot be read |

Text output, a clean tree:

```
$ mrg --check-world
check-world: 7 atoms, 8 package slots, 0 unsatisfiable
```

With `app-misc/doesnotexist` added to the world file:

```
$ mrg --check-world
NO MATCH app-misc/doesnotexist  <- @world
check-world: 8 atoms, 8 package slots, 1 unsatisfiable
$ echo $?
1
```

The rows are `INVALID cat/pkg:slot` (followed by one `wants ATOM  <- who` line
per atom), `NO MATCH ATOM  <- who`, `UNREADABLE cat/pkg-ver` (the metadata
could not be read, so its dependencies were never followed) and, with
`check-world-unneeded`, `UNNEEDED cat/pkg:slot`. The last line counts atoms,
slots and unsatisfiable items, and ends with `, N not read` when some ebuild
could not be read.

`--json` prints one health-checker finding per line, then one summary object:

```
$ mrg --check-world --json
{"property":"world-no-match","package":"@world","atom":"app-misc/doesnotexist","reason":"no ebuild matches; asked by @world","pre_existing":false}
{"atoms":8,"slots":8,"unsatisfiable":1,"not_read":0}
```

The finding properties are `world-invalid`, `world-no-match`, `world-unneeded`
and `checker-limitation` (an unreadable ebuild). The summary object has
`atoms`, `slots`, `unsatisfiable` and `not_read`. A clean tree prints the
summary alone.

Future options: `check-world-unique` (two satisfying versions make a slot
unsatisfiable), `check-world-threads=N` (the answer does not depend on it) and
`check-world-unneeded` (list the slots another slot serves; the exit code does
not change).

## `mrg --plan-world`: what would change?

`mrg --plan-world[=update|emptytree] [--sort alpha|depend|resolver] [--json]`
answers: which slots would a plan install for `@world`, and in what order?

| Mode | Rows | Full plan |
|---|---|---|
| `update` (default) | a slot whose best visible version is not the installed one (`N`, `U`, `D`), and an `R` row for a kept package whose recorded `:slot/sub-slot=` dependency the plan breaks or one that is already stale | `mrg -puDN @world` |
| `emptytree` | every slot `@world` reaches, `R` for an installed version that is kept; build dependencies are followed for all of them | `mrg -pe @world` |

Any other mode is a usage error (exit 2) that names the two.

Choosing a `||` branch in plan mode: a branch ranks by whether every atom
it names is matched by the best visible candidate of its slot — the
package's best visible version when the atom names no slot, the best
visible version in that main slot when it does (a `:1` arm is judged
against slot 1's best, never a parallel slot 2). A branch that qualifies
outranks one that is merely installed, so the plan takes the arm that
admits the newest version; when both qualify the installed one wins, and
the check keeps the installed-first choice. A changed USE still needs
`plan-world-newuse` to become a rebuild row.

It does not check blockers, `REQUIRED_USE` or slot conflicts, so it can list
more or fewer rows than the resolver. A slot with no visible version has no
row. A downgrade is shown as `D` and is not refused.

`--sort` orders the rows. `alpha` is by `category/package:slot` and claims
nothing about build order. `depend` puts a package after the packages it needs
before it is built or usable (`DEPEND`, `BDEPEND`, `RDEPEND`, `IDEPEND`); a
runtime-only edge out of a package the plan builds with still orders, because
a build tool must be usable (PMS 8.1); a `PDEPEND` orders nothing, as the PMS
says (`dependencies.tex`). A cycle with no such edge is broken by building
its consumer twice: the slot gets a second row, and only the second `--json`
entry carries `built_twice: 2` (no `from_version`, it is the same version
built again). `resolver` runs
`emerge --pretend` as a child process and uses its order, which takes seconds
to a minute.

Exit codes: 0 when the plan was worked out (even if it lists nothing), 2 when
the tree cannot be read, the mode is wrong, or the resolver order was asked for
and could not be had.

Text output goes to stdout and is rows only. Two lines go to stderr: the
count and the order, then what was not checked and the command that checks it.
The count names unique slots, with `, N slot(s) built twice: cat/pkg:slot`
appended when a cycle was broken by building a package twice. A third line
names each stale slot-operator binding the plan does not repair,
once, with the atom and the reason -- a kept consumer whose recorded
`:slot/sub-slot=` no longer matches the final provider version (the chosen
one when the plan changes the provider's slot, else the installed one) but
whose rebuild would hold a changed provider back (a recorded versioned pin
forbids the chosen version), or which has no visible ebuild to rebuild to:

```
plan-world: 1 stale slot-operator binding(s) not rebuilt: dev-libs/w741user (=dev-libs/w741prov-1* not satisfied by dev-libs/w741prov-2)
```

```
$ mrg --plan-world --sort depend
[ebuild  N     ] dev-libs/innernestedsetpkg-1.0:0
[ebuild  N     ] dev-libs/newpkg-1.0:0
[ebuild     U  ] dev-libs/upgradepkg-2.0:0 [1.0]
[ebuild  N     ] dev-libs/withdeps-1.0:0
plan-world: 4 to install, order depend
not checked: blockers, REQUIRED_USE, slot conflicts, USE rebuilds; run 'mrg -puDN @world' for the full plan
```

The `, USE rebuilds` part is there while `plan-world-newuse` is off. In
`emptytree` mode it is never named, because every kept package is rebuilt
anyway:

```
$ mrg --plan-world=emptytree --sort depend
[ebuild   R    ] dev-libs/dualslotpkg-1.0:1
[ebuild   R    ] dev-libs/dualslotpkg-2.0:2
[ebuild  N     ] dev-libs/innernestedsetpkg-1.0:0
[ebuild   R    ] dev-libs/nestedsetpkg-1.0:0
[ebuild  N     ] dev-libs/newpkg-1.0:0
[ebuild   R    ] dev-libs/systempkg-1.0:0
[ebuild     U  ] dev-libs/upgradepkg-2.0:0 [1.0]
[ebuild  N     ] dev-libs/withdeps-1.0:0
plan-world: 8 to install, order depend
not checked: blockers, REQUIRED_USE, slot conflicts; run 'mrg -pe @world' for the full plan
```

If some ebuild could not be read, the first line ends with `, N not read`.

`--json` prints one line in the shape of `emerge -pv --json`, so the health
checker reads it unchanged:

```
{"entries":[ENTRY,...],"kept":[KEPT,...],"stale-bindings":[STALE,...],"request":["@world"],"aborted":null}
```

`stale-bindings` is present only when a stale binding goes unrepaired; a
plan with none to name omits the key, so the document is byte-identical
to one from before the field existed.

Each `ENTRY` has `category`, `package`, `version`, `slot`, `sub_slot`,
`outcome` (`new`, `upgrade`, `downgrade` or `reinstall`), `merge_order` (the
row position, from 0), `source` (always `ebuild`), `from_version` (upgrades and
downgrades only, never on a second build), `built_twice` (`2`, on the second
entry of a slot the plan builds twice only), `dependencies` (the dependency
variables of the chosen version), `required_use`, `iuse` and `use` (the USE
the plan would build with). A kept installed package that is also a row is a
`reinstall` entry and is not repeated in `kept`. `KEPT` has `category`, `package`, `version` and
`slot` of every other installed package. Each `STALE` has `consumer`
(`cat/pkg`), `atom` (the recorded atom that forbids the repair, or the stale
`:=` atom itself when the consumer has no visible candidate), `provider` (the
`cat/pkg-ver` the plan leaves behind, or `null`) and `reason`. One entry, from the plan above:

```
{"category":"dev-libs","package":"withdeps","version":"1.0","slot":"0","sub_slot":"0","outcome":"new","merge_order":3,"source":"ebuild","dependencies":{"RDEPEND":"dev-libs/newpkg dev-libs/upgradepkg"},"required_use":"","iuse":[],"use":[]}
```

Future options: `plan-world-newuse` (an installed package whose `USE`, cut to
the `IUSE` of the version kept, differs from the configured one gets an `R`
row; in `update` mode only) and `check-world-threads=N`, which both actions
read.
