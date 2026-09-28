# g195 report — Track G4, backlog #195 (USE-change suggestion + argument-order display)

Status: DONE_WITH_CONCERNS (round-2 scope landed and green; 1 contract
+ 2 Rust pfgraph spots fail on live-real contradictions outside the
brief's scope — question + options + evidence below for the
coordinator's ruling; beds pending coordinator verdict on
READY-FOR-BEDS d949ed4f)

## Round 2 (2026-09-28; branch `backlog/195-autounmask-parent-order`)

Worktree pair `/home/vivo/repo/PORTUALE/wt-195-autounmask-parent-order/{portuale,pmtest}`;
`main` merged into both first (fast-forward; portuale `be76ddfa`,
pmtest `112577b`). Commits: pmtest `e23a3e9` (pins), portuale
`d949ed4f` (product, quotes the pmtest sha). GPG: the one `GNUPGHOME`
I created (`/tmp/opencode/g195b/gnupg`) was `gpgconf --kill`ed +
removed per rule 17. One container probe (rule-13 exception) + host
3.0.82.2 staged-fixture probes for S0 (details below); beds are the
coordinator's (READY-FOR-BEDS in `g195-progress.md`).

### What landed (brief option (a), B18)

1. Faithful `aup0b` port (record-and-fail): new
`violated_parent_flags` ports real `Atom.violated_conditionals`'s
parent-side partition (`lib/portage/dep/__init__.py:1465`: `?` is
`P&&!C`, `=` is `P!=C`, `!?` is `!P&&C`, `!=` is `P==C`, child USE
filtered to valid IUSE per `Package.py:728-731`); new
`viable_parent_flip_targets` walks version-matching visible children
descending with real's masked-instance skip (`:6775`) and the
untouchable-child `continue` (`:6732-6736`). The backtrack-off arm
records the change and FAILS (no rows): `parent_flip_recorded`
suppresses the dep's disclosures (real `remaining_items`), the abort
renderer skips its row, and the change's dep chain is pre-filled with
just the argument line for argument-package changes (real
`_get_dep_chain` never prints the start node). Output for
`--pretend --autounmask =dev-libs/aup0b-1` is byte-identical to the
container probe: header-only stdout, `-foo` block + terminated-early
notice on stderr, rc 1. The display twin `use_unsat_parent_row` shares
the narrowing (this caught two real regressions during development:
the concrete gate must run on the *unevaluated* unconditional deps --
a parent-lacks-child-has `=` stays conditional-only in real -- and the
post-pass plain-miss sweep must skip collected deps).
2. `parentflipeqpkg` pin + Rust test corrected to real's bare miss and
renamed (`..._fails_like_real_when_the_child_flag_is_masked`); the
probe is pasted verbatim in the pin's docstring. Why the old parity
claim was wrong: commit `53859861` (2026-08-30) verified "both sides
byte-identical" with both sides being the Rust binary and
`python/emerge_pretend_reference.py` (the since-removed second Python
copy sharing the flip-then-resolve model). No live `emerge` ever ran:
no container probe (bed did not exist), fixture profile USE
unexamined, `--autounmask` defaults unexamined (bare `--pretend`),
no image version recorded.
3. New `aup0b` pin (`test_autounmask_parent_suggests_disabling_foo_
and_still_fails`) with the container probe verbatim in its docstring;
CASES prose for the two parentflip rows updated (rc assertions
unchanged).

### Probes (all staged fixtures, 3rdparty == host == 3.0.82.2)

- Container (`localhost/test-portuale:latest`, log
`/tmp/opencode/g195b/real-probe.txt`): parentflip bare miss under
default/`--autounmask`/`--autounmask-use=n` (rc 1, no rows, no
block); aup0b default `-foo` block + notice (rc 1, no rows); aup0b
`--autounmask-backtrack=y` `-foo` block with NO notice (rc 1, no
rows) -- real re-drives nothing (backtrack stays 0/20).
- Host (make.local neutralised, disposable GPG home, since removed):
`useflagpkg[-foo]` resolves with a ONE-line `# required by` chain
(real never prints the start node); `pfgraphparent` bare miss under
default AND backtrack=y; `useeqparentoffpkg` resolves via child flip
with the two-line parent+argument chain (non-argument change -- the
generic fill is already right there).

### Gates

- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`:
0 warnings. `cargo test --release` whole workspace: all green except
`autounmask_use_parent_flip_re_resolves_the_whole_graph` and the
`autounmask_backtrack_disabled_gate` pfgraph half (below).
- Full pmtest suite, fresh private `--basetemp=/var/tmp/pmtest-g195b-
full`: **2115 passed, 1 failed** (`..._re_resolves_the_whole_graph`),
37 skipped, 4 xfailed, no bless. Drift by name (corpus warning, for
review): the pfgraph pin's `#0` call plus the expanded parentflipeqpkg
+ pfgraphparent flag-matrix calls (`--emptytree`, `--json`,
`--quiet`, `--tree`, `--update` variants, `--verbose`, bare) -- all
expected output moves of this slice (resolve+rows to bare miss).

### BED-PENDING (coordinator runs these from the pmtest worktree)

- `differential-test-bed/run/l0-resolver.sh` (G guard)
- `differential-test-bed/run/l0-fixture-oracle-all.sh` (G guard)
- Expectation: pfgraph/aup0b/parentflip cells move TOWARD real (bare
misses, record-and-fail); the only known red spots are pins, not
behaviour.

### Question for the coordinator (judgment call, rule 4)

The faithful port necessarily moves `pfgraphparent` (masked-child
`use.mask pf`, same untouchable rule): live real reports the bare
miss under default AND `--autounmask-backtrack=y` (host probes
above), no rows, no block. Three spots still assert Slice-4 resolve
and fail: the contract pin
`test_autounmask_use_parent_flip_re_resolves_the_whole_graph` (both
halves), its Rust twin, and the gate test's pfgraph half (kept
asserting old behaviour with a pointer here -- deliberately not
silenced). Options: (a) authorise correcting those three spots to the
probed bare miss (a follow-up round; the probes above are the
grounding, container re-probe for the docstring if wanted); (b) keep
Slice-4 resolve for pfgraph as an accepted divergence (then the port
must split behaviour per-cell -- not real-grounded, do not pick
without saying so).
Related residue from the same probes (separate slice, same evidence):
the dep-chain fill prints an extra self-row for argument-package
changes (useflagpkg pins assert two `# required by` lines, live real
prints one); the new off-arm pre-fills the real one-liner, so only
old cells carry the divergence.

## Round 1 (2026-09-27; NEEDS_CONTEXT, no product commits)

Worktree branch (both repos): `backlog/195-autounmask-parent-order`.
portuale tip `660961f3`, pmtest tip `76bdda2`. Both worktrees clean
(`git status --short` empty). No `READY-FOR-BEDS` written (guard fires
"after each product commit"; there are none). GPG: every GNUPGHOME I
created (`/tmp/opencode/g195/gnupg`, playground copies) was
`gpgconf --kill`ed + removed per rule 17 (the four agents in `ps` are
other agents' `n194`/`upstream-gpg` homes — left alone).

## S0 reproductions (current portuale, release binary built from tip)

`=dev-libs/aup0b-1` (`--pretend --autounmask`, rc 1): stdout is the
merge header with no rows; stderr is the bare block with NO suggestion:

    emerge: there are no ebuilds to satisfy "dev-libs/aup0d[foo(-)?,bar(-)?]".
    (dependency required by "dev-libs/aup0b-1::testrepo" [ebuild])
    (dependency required by "=dev-libs/aup0b-1" [argument])

Six `aub0` orders (`--pretend --autounmask-backtrack=y`, rc 1 everywhere):
merge list with both `aub0d-1`/`aub0d-0` + slot-conflict block in ALL
orders; `aub0d-0` reads `USE="-foo"` in c/b/a, c/a/b, b/c/a, a/c/b and
`USE="foo"` in b/a/c, a/b/c; the `The following USE changes are
necessary` block (`=dev-libs/aub0d-0 -foo`, required by aub0a) is present
in the first four and absent in b/a/c and a/b/c.

## Real's answer (3rdparty/portage 3.0.82.2)

One container probe (rule 13), staged fixtures + all 7 cells in a single
`podman run --entrypoint /bin/bash localhost/test-portuale:latest`:

    podman run --rm -v <pmtest>/fixtures:/fixtures:ro \
      -v <pmtest>/differential-test-bed/layers/l0-fixture-oracle/stage.sh:/stage.sh:ro \
      -v /tmp/opencode/g195/in-probe.sh:/in-probe.sh:ro \
      --entrypoint /bin/bash localhost/test-portuale:latest /in-probe.sh

(in-probe.sh: `bash /stage.sh /tmp/fxstage`, then the 7 `emerge` cells
with `PORTAGE_CONFIGROOT/ROOT/DISTDIR` on the staged tree; full log
`/tmp/opencode/g195/real-probe.txt`.) Resolution text, verbatim modulo
the Global-Updates/news/FEATURES noise and the `for <root>` staging
suffix:

aup0b (`--pretend --autounmask =dev-libs/aup0b-1`, rc 1) — stdout header
only, then stderr:

    The following USE changes are necessary to proceed:
     (see "package.use" in the portage(5) man page for more details)
    # required by =dev-libs/aup0b-1 (argument)
    >=dev-libs/aup0b-1 -foo

     * In order to avoid wasting time, backtracking has terminated early
     * due to the above autounmask change(s). The --autounmask-backtrack=y
     * option can be used to force further backtracking, but there is no
     * guarantee that it will produce a solution.

Note: `-foo` ONLY (not the playground's `{foo:false,bar:false}`): the
fixture leaf profile (`repo/profiles/default/make.defaults`,
`USE="-bar ..."`, incremental-removal exercise) leaves the parent at
`{foo}`, so only `foo(-)?` is violated. Upstream
`test_autounmask_parent.py` expects both flags only because its minimal
profile enables both.

aub0 (all six `--autounmask-backtrack=y` orders, rc 1) — stdout header
with NO rows; NO `USE changes are necessary` block in any order; in
EVERY order stderr carries:

    emerge: there are no ebuilds built with USE flags to satisfy "dev-libs/aub0d[-foo]" for <FX>/.
    !!! One of the following packages is required to complete your request:
    - dev-libs/aub0d-0::testrepo (Change USE: -foo)
    (dependency required by "dev-libs/aub0a-0::testrepo" [ebuild])
    (dependency required by "dev-libs/aub0a" [argument])

plus, in exactly the two a-first orders, a slot-conflict prefix whose
entry order varies (a/c/b, backtrack 2/20: `aub0d-0` first; a/b/c,
backtrack 3/20: `aub0d-1` first; other orders 0–2/20, no prefix).
Deterministic across 3 repeat runs. So real is itself order-dependent
here — the entry's "argument-order-independent" framing does not match
live real.

Host-side fidelity note: the host `/etc/make.local` leaks
`EMERGE_DEFAULT_OPTS=... --binpkg-respect-use=y ...` (which forces
`autounmask_keep_use=True`) and `USE="-cuda"` into any host run of the
vendored 3.0.82.2 through the fixture `source /etc/make.local`. All
host iteration used a staged copy with that line pointed at `/dev/null`
(container-equivalent: the image has no `/etc/make.local`, stage.sh
touches an empty one); normalized host text is IDENTICAL to the
container probe (only spinner/width padding differs).

## Named mechanisms (3rdparty/portage 3.0.82.2, `lib/_emerge/depgraph.py`)

- aup0b: `_resolve` fails in `_create_graph`, then
  `_apply_parent_use_changes` (:5820) re-probes each unsatisfied dep
  with `_show_unsatisfied_dep(collect_use_changes=True)`, whose
  parent-conditional arm (:6768–6858) flips the parent via
  `_pkg_use_enabled(myparent, target_use)` (:6845) where `involved_flags`
  comes from `violated_conditionals(child, valid, parent)` — i.e. only
  parent-side-ACTIVE conditionals (`foo(-)?` with parent foo on;
  `bar(-)?` already inactive). With backtrack off the run then fails
  (`_have_autounmask_changes` → `_success_without_autounmask`, :5791)
  and `_display_autounmask` prints the block; the early-termination tail
  is `:11093`-adjacent, gated on `_autounmask_backtrack_disabled`
  (`need_config_change`:11752, cf. #217).
- Portuale gap: `suggested_parent_use_candidate` (portage-repo
  `lib.rs`:5771) toggles EVERY conditional flag `conditional_flags`
  returns (`lib.rs`:5699) — `{bar,foo}` — so the probe re-evaluates with
  `bar` wrongly enabled, `atom_currently_satisfiable` fails, and no
  suggestion forms. The documented "same narrowing" is also baked into
  the display twin `use_unsat_parent_row` (:10566).
- aub0: single-pass failure on the loser of two sibling USE-deps against
  profile-foo `aub0d-0` (c/b/a: A's `D[-foo]` matches after a `-foo`
  flip is recorded, then B's `D[foo]` is unmatchable — contradictory
  flip rejected at :7714; the pass fails, `need_config_change` ends
  backtracking, final state has `needed_use == {}` and, order-dependently,
  zero or two `aub0d` nodes). Display is `display_problems`
  (:11104): slot notice only if the tracker still holds the conflict,
  then the leftover `_unsatisfied_deps_for_display` item (:11279).
- B17/#242: not involved — none of the 7 cells has a BDEPEND/cross-root
  edge (RDEPEND/DEPEND only, staged ROOT == running ROOT).

## Why S1 stops here (judgment calls, rule 4)

1. Half 1 (aup0b) cannot land cleanly. Narrowing the repair path to
   parent-active conditionals makes the probe succeed — and portuale's
   Slice-4 backtrack-off arm then RESOLVES (merge rows), while real
   records-and-fails (no rows). Making it record-and-fail to match real
   collides with the `parentflipeqpkg` pin + Rust test
   (`autounmask_use_parent_flip_resolves_when_the_child_flag_is_masked`,
   contract `test_autounmask_use_parent_flip_resolves_when_the_child_flag_is_masked`),
   which assert resolve+rows+block AS REAL. A host-clean probe of real
   on that exact cell shows the BARE miss instead:
   `emerge: there are no ebuilds to satisfy
   "dev-libs/parentflipchildpkg[feat=]"` (no rows, no block) — real's
   untouchable-child `continue` (:6732–6736) skips the parent probe when
   the child's needed flag is masked/forced. So a faithful aup0b port
   also flips `parentflipeqpkg` to the bare miss, i.e. correcting a pin
   whose docstring claims live-verified real parity plus inverting a
   Rust test's core assertion. Options: (a) authorize the pin+test
   correction to real's bare miss and land aup0b record-and-fail;
   (b) keep Slice-4 resolve as an accepted divergence (then aup0b keeps
   merge rows — still divergent from real) and record it; (c) split the
   difference per-cell (not real-grounded — do not pick).
2. Half 2 (aub0) is not one rule. Reaching real's text needs fail-fast
   abort dynamics (real's `_create_graph` returns 0 at the first loser;
   portuale continues and graphs everything), order-accurate arg/dep
   processing, AND the order-dependent slot prefix — interacting
   resolver/display changes with suite-wide blast radius (every abort
   pin assumes continue-and-report), far past a single G-track commit
   under the L0 + fixture-oracle guard.

## BED-PENDING (no commits; for the slices that land later)

From the pmtest worktree (coordinator runs these):
- `differential-test-bed/run/l0-resolver.sh` (G guard: identical-or-better row by row)
- `differential-test-bed/run/l0-fixture-oracle-all.sh` (G guard: 0 unexplained)
- `python3 -m pytest pytests-contract-suite -q --basetemp=/var/tmp/pmtest-g195 -p no:cacheprovider`
  (private basetemp per rule 16)
- `cargo fmt --check` / `cargo clippy --release --all-targets` (zero
  warnings) / `cargo test --release` from `portuale/rust`, with
  `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu` (rule 15).

## Round 4 (g195d): Important-1 + minors 2-4

Branch `backlog/195-autounmask-parent-order`, worktree pair
`/home/vivo/repo/PORTUALE/wt-195-autounmask-parent-order/{portuale,pmtest}`.

Oracle: host run of real Portage 3.0.82.2 `Atom.violated_conditionals`
(`lib/portage/dep/__init__.py:1465`, branches `:1525-1576`) over the
full operator x validity x default x P x C matrix (72 single-token
cells; invalid rows C=0 only, since IUSE-filtered child USE never
holds an invalid flag — real `Package.py:728-731`, the same contract
the port documents) plus 10 two-token concrete-gate cells. Probe:
`docs/evidence/2026-09-28-g195/g195d-oracle-probe.py` (portuale
worktree); output `g195d-oracle-output.txt` (same dir). Columns:
`conditional` = union of the result's conditional.{enabled,equal,
not_equal,disabled} (depgraph.py:6787-6795 — what the port returns);
`enabled`/`disabled` feed the gate (`:6784`); `missing_iuse` =
required-but-not-in-child-IUSE (arm skipped at `:6721-6727` before
the call); `arm` = composed end-to-end outcome; `port_now` = the
port's pre-g195d uniform P/C math; `match` = port_now vs arm.

The probe found 10 divergences, in exactly the two corner classes the
review predicted (plus 4 more of the same invalid-`(+)` family — all
`(-)` cells already agreed). Both review corners confirmed:
`!=` invalid `(+)` P=1/C=0 is violated in real (missing_enabled
sub-branch) but satisfied by `p == c`; `-bar(+)` invalid with the
child lacking `bar` fails real's gate (disabled) but passed the
port's valid-only gate. Fixed one rule per commit (pins green
throughout):

- portuale `93954ad3` (quotes pmtest `e23fec7`): conditional branches
  model real's missing_enabled/missing_disabled sub-branches per
  operator; valid flags reduce to the old math (no fixture behavior
  change). Carries the 72-cell matrix test, the staged evidence dir,
  the `?` (`:1525-1531`) / `=` (`:1532-1549`) citation fixes, the
  `node is not start_node` (`:6357`) fix, the pfgraph probe repoint,
  and the latch/gate invariant comment (minors 2-4).
- portuale `b3420f99`: concrete gate models real `:1511-1524` for
  invalid-defaulted unconditionals (`bar(-)` absent violates,
  `-bar(+)` absent violates). Carries the 12-row gate test (G0-G10).
- pmtest `e23fec7` (docstring-only, committed first): both pins point
  at the staged evidence copies.

Oracle table (verbatim from `g195d-oracle-output.txt`; `port_now` /
`match` describe the pre-fix port — post-fix the port returns the
`arm` column on all 82 cells, pinned by `tests_195d`):

```
? valid none 0 0 | [] [] [] PASS [] [] [] OK
? valid none 0 1 | [] [] [] PASS [] [] [] OK
? valid none 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
? valid none 1 1 | [] [] [] PASS [] [] [] OK
? valid plus 0 0 | [] [] [] PASS [] [] [] OK
? valid plus 0 1 | [] [] [] PASS [] [] [] OK
? valid plus 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
? valid plus 1 1 | [] [] [] PASS [] [] [] OK
? valid minus 0 0 | [] [] [] PASS [] [] [] OK
? valid minus 0 1 | [] [] [] PASS [] [] [] OK
? valid minus 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
? valid minus 1 1 | [] [] [] PASS [] [] [] OK
? invalid none 0 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
? invalid none 1 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
? invalid plus 0 0 | [] [] [] PASS [] [] [] OK
? invalid plus 1 0 | [] [] [] PASS [] [] [foo] DIVERGE
? invalid minus 0 0 | [] [] [] PASS [] [] [] OK
? invalid minus 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
= valid none 0 0 | [] [] [] PASS [] [] [] OK
= valid none 0 1 | [foo] [] [] PASS [] [foo] [foo] OK
= valid none 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
= valid none 1 1 | [] [] [] PASS [] [] [] OK
= valid plus 0 0 | [] [] [] PASS [] [] [] OK
= valid plus 0 1 | [foo] [] [] PASS [] [foo] [foo] OK
= valid plus 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
= valid plus 1 1 | [] [] [] PASS [] [] [] OK
= valid minus 0 0 | [] [] [] PASS [] [] [] OK
= valid minus 0 1 | [foo] [] [] PASS [] [foo] [foo] OK
= valid minus 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
= valid minus 1 1 | [] [] [] PASS [] [] [] OK
= invalid none 0 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
= invalid none 1 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
= invalid plus 0 0 | [foo] [] [] PASS [] [foo] [] DIVERGE
= invalid plus 1 0 | [] [] [] PASS [] [] [foo] DIVERGE
= invalid minus 0 0 | [] [] [] PASS [] [] [] OK
= invalid minus 1 0 | [foo] [] [] PASS [] [foo] [foo] OK
!= valid none 0 0 | [foo] [] [] PASS [] [foo] [foo] OK
!= valid none 0 1 | [] [] [] PASS [] [] [] OK
!= valid none 1 0 | [] [] [] PASS [] [] [] OK
!= valid none 1 1 | [foo] [] [] PASS [] [foo] [foo] OK
!= valid plus 0 0 | [foo] [] [] PASS [] [foo] [foo] OK
!= valid plus 0 1 | [] [] [] PASS [] [] [] OK
!= valid plus 1 0 | [] [] [] PASS [] [] [] OK
!= valid plus 1 1 | [foo] [] [] PASS [] [foo] [foo] OK
!= valid minus 0 0 | [foo] [] [] PASS [] [foo] [foo] OK
!= valid minus 0 1 | [] [] [] PASS [] [] [] OK
!= valid minus 1 0 | [] [] [] PASS [] [] [] OK
!= valid minus 1 1 | [foo] [] [] PASS [] [foo] [foo] OK
!= invalid none 0 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
!= invalid none 1 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
!= invalid plus 0 0 | [] [] [] PASS [] [] [foo] DIVERGE
!= invalid plus 1 0 | [foo] [] [] PASS [] [foo] [] DIVERGE
!= invalid minus 0 0 | [foo] [] [] PASS [] [foo] [foo] OK
!= invalid minus 1 0 | [] [] [] PASS [] [] [] OK
!? valid none 0 0 | [] [] [] PASS [] [] [] OK
!? valid none 0 1 | [foo] [] [] PASS [] [foo] [foo] OK
!? valid none 1 0 | [] [] [] PASS [] [] [] OK
!? valid none 1 1 | [] [] [] PASS [] [] [] OK
!? valid plus 0 0 | [] [] [] PASS [] [] [] OK
!? valid plus 0 1 | [foo] [] [] PASS [] [foo] [foo] OK
!? valid plus 1 0 | [] [] [] PASS [] [] [] OK
!? valid plus 1 1 | [] [] [] PASS [] [] [] OK
!? valid minus 0 0 | [] [] [] PASS [] [] [] OK
!? valid minus 0 1 | [foo] [] [] PASS [] [foo] [foo] OK
!? valid minus 1 0 | [] [] [] PASS [] [] [] OK
!? valid minus 1 1 | [] [] [] PASS [] [] [] OK
!? invalid none 0 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
!? invalid none 1 0 | [foo] [] [] PASS [foo] SKIP-missing-iuse []POISON OK
!? invalid plus 0 0 | [foo] [] [] PASS [] [foo] [] DIVERGE
!? invalid plus 1 0 | [] [] [] PASS [] [] [] OK
!? invalid minus 0 0 | [] [] [] PASS [] [] [] OK
!? invalid minus 1 0 | [] [] [] PASS [] [] [] OK
G1 foo? bar bar_valid=1 bar_use=0 | [foo] [bar] [] FAIL [] FAIL-gate []GATE OK
G2 foo? -bar bar_valid=1 bar_use=1 | [foo] [] [bar] FAIL [] FAIL-gate []GATE OK
G3 foo? bar(-) bar_valid=0 bar_use=0 | [foo] [bar] [] FAIL [] FAIL-gate [foo] DIVERGE
G4 foo? -bar(+) bar_valid=0 bar_use=0 | [foo] [] [bar] FAIL [] FAIL-gate [foo] DIVERGE
G5 foo? bar(+) bar_valid=0 bar_use=0 | [foo] [] [] PASS [] [foo] [foo] OK
G6 foo? -bar(-) bar_valid=0 bar_use=0 | [foo] [] [] PASS [] [foo] [foo] OK
G7 foo? bar bar_valid=0 bar_use=0 | [foo] [bar] [] FAIL [bar] SKIP-missing-iuse []POISON-req OK
G8 foo? -bar bar_valid=0 bar_use=0 | [foo] [] [bar] FAIL [bar] SKIP-missing-iuse []POISON-req OK
G9 foo= bar(-) bar_valid=0 bar_use=0 | [foo] [bar] [] FAIL [] FAIL-gate [foo] DIVERGE
G10 foo= -bar(+) bar_valid=0 bar_use=0 | [foo] [] [bar] FAIL [] FAIL-gate [foo] DIVERGE
```

Notable oracle readings beyond the divergences: all 48 valid cells
match the old uniform math (no fixture impact); invalid `(-)` cells
match everywhere (only `(+)` diverges); invalid-defaultless
conditionals report `conditional` in isolation but end-to-end real
skips the arm via `missing_iuse` — the port's poison returns `[]`,
same outcome.

Gates on the final tree (`b3420f99`): `cargo fmt --check` clean,
`cargo clippy --release --all-targets` 0 warnings, `cargo test
--release` whole workspace green (877 portage-repo incl. the 2 new
tests_195d; 711 portuale binary), full pmtest suite on fresh private
`--basetemp=/var/tmp/pmtest-g195d-full2`: 2116 passed, 0 failed, 37
skipped, 4 xfailed; corpus drift (listed, never blessed) is exactly
the same 18 expanded-matrix parentflipeqpkg+pfgraphparent calls as
round 3. Rule 13: L0 + fixture-oracle beds are the coordinator's.
