# Evidence — backlog #265 (a+b, with #273), D2 S0

Fresh probe pairs, 2026-10-01 (agent date 2026-09-30 batch): host
`opencode` run, real Portage **3.0.82.2** in `localhost/test-portuale:latest`
(staged fixture via `l0-fixture-oracle/stage.sh`, the oracle's own staging)
vs this worktree's `portuale` release binary against the same staging
applied on the host (`/tmp/opencode/d2/stage-host.sh`, fixture-only deltas
identical to `stage.sh`).

Identical effective options on both sides (`--ignore-default-opts`,
`PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=<staged>`,
`PORTAGE_REPOSITORIES` from the staged `repos.conf`,
`LC_ALL=C.UTF-8 TZ=UTC PYTHONHASHSEED=0`, empty `EMERGE_DEFAULT_OPTS`).

## Cells (all rc 1 on both sides)

- `mia0a-1`: `emerge --ignore-default-opts --pretend "=dev-libs/mia0a-1"`
- `mia0a-2`: `emerge --ignore-default-opts --pretend "=dev-libs/mia0a-2"`
- `useflagpkg-autounmask-use-n`: `emerge --ignore-default-opts --pretend --autounmask-use=n 'dev-libs/useflagpkg[-foo]'`

## Real's bytes (the oracle)

- `real/mia0a-1.txt`: `emerge: there are no ebuilds built with USE flags
  to satisfy "dev-libs/mia0b[foo?]".` +
  `- dev-libs/mia0b-1::testrepo (Missing IUSE: foo)` + the two
  `(dependency required by …)` lines. (Leading Global Updates noise is
  real's first-run update pass, kept verbatim.)
- `real/mia0a-2.txt`: same shape for `"dev-libs/mia0b[foo?,bar]"`
  with `(Missing IUSE: foo)` — no `Change USE:` row. The brief's
  "`Missing IUSE: foo` + `Change USE: +bar`" is the real-vs-portuale
  contrast (portuale prints `Change USE: +bar` here, see below).
- `real/useflagpkg-autounmask-use-n.txt`: `emerge: there are no ebuilds
  built with USE flags to satisfy "dev-libs/useflagpkg[-foo]" for
  /tmp/d2stage/fixtures/.` +
  `- dev-libs/useflagpkg-1.0::testrepo (Change USE: -foo)` and **no**
  `(dependency required by…)` lines (myparent is the argument atom).

## Portuale's bytes (post-fix, this slice)

Re-probed after S1 against the same staging; all three match real's
rows above:

- `portuale/mia0a-1.txt`: the block with
  `- dev-libs/mia0b-1::testrepo (Missing IUSE: foo)` + chain.
- `portuale/mia0a-2.txt`: the block with
  `- dev-libs/mia0b-1::testrepo (Missing IUSE: foo)` + chain
  (was `Change USE: +bar`).
- `portuale/useflagpkg-autounmask-use-n.txt`: the block with
  `- dev-libs/useflagpkg-1.0::testrepo (Change USE: -foo)` and no
  chain lines (was the bare miss).

Deliberate, precedent-backed cuts vs real's bytes (unchanged by this
slice): portuale omits real's staging `for <root>.` suffix everywhere
(the `fixture-miss-message-unsuffixed` class), and a top-level abort
prints no merge-list header (pre-existing abort shape).

## Portuale's bytes (pre-fix, for the record)

- `portuale/mia0a-1.txt`: the bare `there are no ebuilds to satisfy
  "dev-libs/mia0b[foo?]"` miss + chain (no USE block).
- `portuale/mia0a-2.txt`: the USE block but
  `- dev-libs/mia0b-1::testrepo (Change USE: +bar)`.
- `portuale/useflagpkg-autounmask-use-n.txt`: the bare
  `there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".` miss.

## Real-source lines (vendored `3rdparty/portage`, 3.0.82.2)

- The `missing_use` gate: `lib/_emerge/depgraph.py:6593-6614`
  (`not pkg.iuse.is_valid_flag(atom.unevaluated_atom.use.required) or
  atom.violated_conditionals(…).use`).
- `use.required` includes conditional flags: `lib/portage/dep/__init__.py:1363`
  (`self.required = frozenset(no_default)` — every token without a
  `(+)/(-)` default marker, `?`/`=`/`!=`/`!?` included).
- The reasons loop: `depgraph.py:6714-6760` (`Missing IUSE:` from
  `pkg.iuse.get_missing_iuse(required_flags)` wins over `Change USE:` per
  candidate; `Package.py:795` for the IUSE+implicit lookup).
- The display: `depgraph.py:6969-6994` (the `show_missing_use` block) and
  `depgraph.py:7080-7090` (chain lines skipped when `myparent` is an
  `AtomArg`); the `for <root>` suffix: `depgraph.py:6500-6501`
  (`if root != running_root: xinfo += f" for {root}"`).
