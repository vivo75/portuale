# R4 S0: pmtest contract cases whose output changes once the handler is ported

Portage 3.0.83 (host emerge, `cmp`-identical to `3rdparty/portage`); portuale
`target/release/portuale` as built (not rebuilt), 2026-10-27.

## Result

**Exactly one contract call changes**: the cgp USE keeper. Portage stops through the handler arm
there and prints its explanation. No other case or pin, in the contract suite or in the expanded
corpus, gets a handler stop, a `Change USE` explanation or the `Enabling --newuse` text.

| case | argv (+ world) | Portage rc / count | portuale rc / count today | what changes |
|---|---|---|---|---|
| `test_complete_graph_use_break_fails_with_world_consumer` (`test_emerge_pretend_contract.py:14906`, call at :14937) | `--pretend dev-libs/cgp0x`, world += `dev-libs/cgp0q` | 1, `backtrack: 0/20`, handler stop on try 0 | 1, `backtrack: 0/20` | the package.mask advisory (with the `--backtrack=30` sentence) and the "For more information" lines are replaced by the explanation below |

Portage's text after the conflict block (raw stderr, `captures/cgp/k0105.portage.stderr`; `⏎` = newline):

```
                     ^^^^          …                                                              ⏎
⏎
⏎
It might be possible to solve this slot collision⏎
by applying all of the following changes:⏎
   - dev-libs/cgp0x-2.8.0 (Change USE: -icu)⏎
⏎
```

- With `--color=y` the flag is `\x1b[34;01m-icu\x1b[39;49;00m` (SC:766).
- Nothing follows: no advisory, no "For more information" (D:1747-1750).
- Trace (`captures/cgp/k0105.trace.jsonl`): `need_config_change` ret True, built_here,
  slot_arm_stop, `changes=[{cgp0x-2.8.0 ebuild: {icu: False}}]`, `n_solutions=1`, not a version
  conflict, not unspecific; `_backtrack_depgraph` backtracked 0; the display reuses the handler.

portuale's current text in the same place (`captures/cgp/k0105.portuale.stdout`): one blank line, then
`It may be possible to solve this problem by using package.mask to` … `not be installed simultaneously. You may want to try a larger value of`
`the --backtrack option, such as --backtrack=30, in order to see if` `that will solve this conflict automatically.`
blank, `For more information, see MASKED PACKAGES section in the emerge man` / `page or refer to the Gentoo Handbook.`, blank.

Effect on the pin: it asserts rc 1, stderr `""`, the one `R` row, `slot conflict`, `dev-libs/cgp0q-4.8.2`
and `dev-libs/cgp0x:2[-icu]` (:14937-14945). All of these still hold, so the pin passes as is. Add a
pin on the explanation lines (RED in the plan). The keeper has **no corpus entry** (tmp ROOT), so
there is **no corpus drift** from R4.

Not R4 (probe artefacts, also in this capture):
- `for <FX>/`, `to '<FX>/'`, `installed in '<FX>/'`: Portage's running root is `/` here
  (it ignores `PORTAGE_RUNNING_ROOT`);
- the blank-line count before the advisory/explanation: Portage has two newlines (SC:702 + SC:726),
  portuale one. That holds on the generic path too, e.g. `census/j0929.*` slotusegroup. Check this
  against the container oracle before copying it: the bed diff of gimp shows portuale with an
  *extra* blank after the NOTE line instead.

## The other slot-conflict cases (no handler text, no stop: unchanged by R4)

Every handler built in a probe except k0105 had `is_a_version_conflict=True` and 0 solutions
(181 handler records, 179 version conflicts, `captures/census*/*.trace.jsonl`). The contract and
corpus calls with a slot conflict in either PM's output (`captures/census-summary.txt`, 40 jobs;
`captures/census-expanded-summary.txt`, 70 jobs):

- the slotconfgroup / slotconflictunsolvable / scuseparent / slotusegroup families (every option variant);
- `btparent --backtrack=0`, `orbtblocked`, `mgfa`, `mg2top`, `scm0a`, `p569pull`;
- the needer/othermod triangle (± world keeper), `slotconflictnewpin+oldpin`, the aub0 permutations,
  `r25lib` with world `r25consumer`;
- the cgp VER and igw keepers: `>=/<dev-libs/cgp1x`, `>=/<dev-libs/igw0x`, `=dev-libs/paired-2.0` with world keeper.

`j0411` (`--verbose-conflicts=n`) is Portage rc 2 (argparse), a known separate surface.

### Side observation, not R4: backtrack counts differ, not investigated

- Portage `6/20` vs portuale `5/20` on every slotconfgroup/slotconflictunsolvable/scuseparent cell
  (24 jobs).
- `4/20` vs `3/20` on slotusegroup, needer/othermod and newpin/oldpin (13 jobs).
- `3/20` vs `0/20` on the cgp1x/igw0x/paired keepers. That one matches s0b's Task-10a prediction.
- `10/20` vs `14/20` on mg2top.
- `4/20` vs `1/20` on needer + world keeper.

The contract pins do not assert these counts. They come from my namespace probe (§ method) and
are **not** confirmed in the container oracle. Treat them as a lead.

## Method (so the census can be rerun)

- Jobs (scratchpad `r4/`):
  - `jobs-all.json`: 1030 jobs = every pretend call in the contract corpus whose env is the
    fixture tree (1310 entries minus 143 tmp/custom-root ones) ∪ all 908 `CASES` rows;
  - `jobs-extra.json`: 123 jobs = statically resolvable `_run(..., fixture_env | _world_extra_env(...))`
    calls not already covered, plus the loop-driven cgp/igw cells;
  - `jobs-expanded.json`: 4688 jobs = every pretend entry of `corpus/expanded.json.xz`.
- Driver `playground/census.py`:
  - a fresh `cp -a` of `pmtest/fixtures` per run, under `/var/tmp/claude-r4/j/<id>/fx`;
  - the mtimedb `updates` record is seeded as `conftest.py` does; world += the test's atoms;
  - `cwd=<fx>`; env as `fixture_env` (CONFIGROOT=ROOT=RUNNING_ROOT=fx, DISTDIR, CLEAN_DELAY=0),
    the suite's `_ENV_CONFIG_VARS` stripped, PYTHONHASHSEED=0;
  - argv = `--ignore-default-opts` + the suite's argv;
  - both PMs run inside `playground/ns-run.sh`, a private mount namespace adapted from pmtest
    `repos_filter.sh`:
    - comments-only `/usr/share/portage/config/repos.conf` (no host gentoo repo, like the suite);
    - `/etc/portage` replaced by a dir holding only `make.profile -> <fx>/repo/profiles/default`.
      Portage parses the `/` config's profile against the shared repo set, and the host profile
      needs gentoo;
    - `<fx>/var/db` on `/var/db` and `<fx>/var/cache/edb` on `/var/cache/edb`. The host is
      never touched.
  - Portage runs through `playground/sc-trace.py`, a call-through wrapper of `need_config_change`,
    `_show_slot_collision_notice` and `_backtrack_depgraph` that writes JSON lines;
    `playground/analyze.py` summarises.
- **Coverage gaps (not probed):**
  - 441 `_run` calls whose argv/env are built dynamically;
  - the 143 tmp-tree corpus entries;
  - the three tmp-tree slot-conflict tests `test_oracle_backtrack_masks_are_discarded_with_their_reason` (:24023),
    `test_or_choice_avoids_downgrade_into_the_graphed_update` (:24456) and
    `test_oracle_slot_conflict_abi_rebuilds_the_built_parent` (:25152). Their ebuilds carry **no USE deps**,
    and without one `_check_solution` cannot return a solution (SC:1186-1188), so they cannot stop
    or explain. They could only print the `--newuse` text if a conflict were unspecific, and
    portuale already renders the "(no parents that aren't satisfied…)" line
    (`problem_reports.rs:943`), which no census output contains.
  - Two `-pC`/`-pc` jobs timed out under Portage (unmerge actions: no resolver, irrelevant).
