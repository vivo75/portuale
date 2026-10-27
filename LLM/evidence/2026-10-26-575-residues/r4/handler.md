# R4 S0: Portage's slot_conflict_handler and the need_config_change stop (implementer notes)

Portage 3.0.83. `3rdparty/portage` is at tag `portage-3.0.83`. `slot_collision.py` and
`depgraph.py` are byte-identical (`cmp`) to the host's `/usr/lib/python3.14/site-packages`.
SC = `lib/_emerge/resolver/slot_collision.py`, D = `lib/_emerge/depgraph.py`,
PKG = `lib/_emerge/Package.py`, SETS = `lib/portage/_sets/base.py`.
Probes are under `captures/` (see `captures/README` lines in cases.md).

## 0. The rule in two sentences

On every backtracking pass that has slot conflicts (and backtracking is allowed, or none of
`--buildpkgonly/--fetchonly/--fetch-all-uri/--nodeps` is set), Portage builds the handler once
for that pass. If the handler's search finds USE changes (`changes` non-empty), the backtracking
loop ends right there. On try 0 that pass is the report, and it prints `get_explanation()` instead
of the package.mask advisory. On a later try the stop is followed by a final `allow_backtracking=False`
pass from `get_best_run()`, which throws the suggestion away (§4, quirk Q1).

## 1. Construction (SC:81-182)

- `all_conflicts` = `(root, slot_atom, pkgs)` for every `_package_tracker.slot_conflicts()` (SC:89-91).
  `all_parents` = `depgraph._dynamic_config._parent_atoms` (SC:95).
- For each conflict *i*: `conflict_pkgs[i]` = `list(pkgs)`, and `all_conflict_atoms_by_slotatom[i]` =
  the union of `(ppkg, atom)` over every pkg of that conflict. `conflict_nodes` = every pkg of every
  conflict (SC:108-115). Note SC:114 iterates `all_parents.get(pkg)` without a None guard
  (the "(no parents)" display arm SC:700-701 is guarded; see §6).
- `_prepare_conflict_msg_and_check_for_specificity()` (SC:127, SC:241-726) builds the conflict
  text and sets the two flags (§3).
- The configuration search (SC:137-179):
  - `_configuration_generator(conflict_pkgs)` (SC:1191-1237). Per conflict it reorders the pkgs:
    non-installed first, installed last, each group in the original order (SC:1193-1203).
    Configurations are enumerated odometer-style, with the **last** conflict varying fastest
    (SC:1222-1237). The first configuration takes index 0 of every conflict, which is the
    "all-ebuild" one only when every conflict has a non-installed pkg.
  - For each configuration: `new_solutions = _check_configuration(...)` (SC:152). If it is
    non-empty, they are appended to `self.solutions`. If this was the **first** configuration, the
    loop breaks ("All-ebuild configuration has a solution", SC:156-167).
  - `first_config = False`. Then `if len(conflict_pkgs) > 4: break` (SC:170-179).
    `conflict_pkgs` has **one entry per slot conflict**, so this means "more than 4 slot conflicts",
    despite the comment's "number of conflict packages". With more than 4 conflicts only the first
    configuration is ever checked.
- `changes`: for every solution, `_add_change(_get_change(solution))` (SC:181-182).
  - `_get_change` (SC:226-239) maps each flag through `pkg.iuse.get_flag(flag)`. A None result
    (a use-dep default) is skipped. It emits `True` for "enabled" when the flag is not in
    `_pkg_use_enabled(pkg)`, and `False` for "disabled" when it is. Flags already in the wanted
    state are dropped.
  - `_add_change` (SC:205-224) keeps a minimal set. A new change is ignored if an existing change
    is a subset of it (`_is_subset`, SC:187-203: same pkgs, same flag values). Existing changes that
    are supersets of the new one are removed. Order is first-kept, then appended.

## 2. `_check_configuration(config, atoms_by_slot, conflict_nodes)` (SC:786-982)

Returns a list of solutions. `False` rejects the configuration (falsy either way).

1. **Installed pkg with pending USE** (SC:796-816). For each installed pkg in `config` and each
   other pkg of its conflict: if `iuse.all` differs (symmetric difference) or `_pkg_use_enabled`
   differs, return False.
2. Per conflict `idx`, with `pkg = config[idx]`, build `involved_flags` (SC:822-943). For each
   `(ppkg, atom)` of that conflict:
   - `not atom.package` (soname) → skip (SC:825).
   - `ppkg` is a conflict pkg that is not in `config` → skip (SC:828-831).
   - `atom` matches `pkg` with its current USE → skip (SC:833-835).
   - `atom.without_use` does not match → return False ("version requirements", SC:837-846).
   - `pkg.iuse.is_valid_flag(atom.unevaluated_atom.use.required)` is false → return False
     ("misses needed flags from IUSE", SC:848-856).
   - `violated_atom`:
     - if `ppkg` is not a Package or is installed: `atom.violated_conditionals(pkg_use, valid)`,
       i.e. the evaluated atom (SC:858-863);
     - else: `atom.unevaluated_atom.violated_conditionals(pkg_use, valid, parent_use=ppkg_use)`.
       If its `.use` is None → skip (bug 374423, SC:864-878).
   - `pkg.installed` and the violation has enabled/disabled flags → return False (SC:880-891).
   - For every flag in `violated_atom.use.required` (SC:897-914). `required` is the whole atom's
     `required` set (PD `violated_conditionals` passes `required=self.required`), **not** only the
     violated flags:
     - flag in `.use.enabled`: "" / cond / enabled → `enabled`, otherwise `contradiction`;
     - flag in `.use.disabled`: "" / cond / disabled → `disabled`, otherwise `contradiction`;
     - otherwise: "" → `cond` (an existing state is kept).
3. If `pkg.installed`, involved flags are pinned to its USE (SC:916-930): enabled-but-off →
   contradiction; disabled-but-on → contradiction; cond → its actual state.
4. Any `contradiction` → return False (SC:932-941).
5. Candidates: `_solution_candidate_generator(all_involved_flags)` (SC:1240-1301). Every `cond`
   becomes a shared `_value_helper` starting at "disabled". The helpers are enumerated as binary
   counting with the **last** cond varying fastest (SC:1284-1301); with no cond there is one
   candidate. Each candidate goes through `_check_solution`. Truthy results are collected. Stop
   after `_check_configuration_max = 1024` candidates (SC:79, SC:954-975).

## 3. `_check_solution(config, flags, atoms_by_slot)` (SC:1012-1188) and `_force_flag_for_package` (SC:984-1010)

- `required_changes = {pkg: {flag: state}}`. For each `idx, pkg`:
  - if `pkg` is not installed: for every involved flag that `pkg.iuse.is_valid_flag(flag)`,
    `_force_flag_for_package(rc, pkg, flag, state)` (SC:1045-1052);
  - for each parent `(ppkg, atom)` of that conflict with `atom.package` and a non-empty
    `atom.unevaluated_atom.use`, and each involved flag that is in `use.required` **and** with
    `use.conditional` non-empty (SC:1054-1067):
    - `[flag?]`: state disabled → force ppkg `disabled`;
    - `[!flag?]`: state enabled → force ppkg `disabled`;
    - `[flag=]`: enabled → force ppkg `enabled`; disabled → force ppkg `disabled`;
    - `[!flag=]`: enabled → force ppkg `disabled`; disabled → force ppkg `enabled` (SC:1068-1113).
  - `ppkg` may be **installed**: nothing stops a change on an installed parent (seen in upstream
    case 7, `captures/upstream-sc.txt`: solution 1 forces `Q-2 installed {bar: disabled}`).
- `_force_flag_for_package` is asymmetric:
  - "disabled": a prior "enabled" → `contradiction`; flag currently on → `disabled`; otherwise the
    prior value is kept, **"" when there is none**;
  - "enabled": a prior "disabled" → `contradiction`; otherwise always `enabled`.
- Valid only if every state is exactly "enabled" or "disabled" (SC:1115-1122). So `""` (forcing
  "disabled" on a flag that is already off) **invalidates the candidate** (quirk Q2).
- Match check (SC:1124-1167): for each pkg, `new_use` = `_pkg_use_enabled(pkg)` ± its changes
  (`old_use` reused when equal). Then every parent atom whose `ppkg` has a `.use` attribute
  (SetArg/AtomArg skipped, SC:1142-1144) is re-evaluated: `atom.unevaluated_atom.evaluate_conditionals(ppkg_new_use)`.
  It must match `pkg` with `new_use` (`InternalPackageSet.findAtomForPackage(pkg, new_use)`),
  else the candidate is invalid. See quirk Q3 for the empty-USE case.
- REQUIRED_USE (SC:1169-1184): for every changed pkg with REQUIRED_USE,
  `check_required_use(req, use±changes, iuse.is_valid_flag)` must hold.
- Returns `required_changes` only if valid **and non-empty** (SC:1186-1188).

## 4. The flags and `get_explanation()` (SC:728-784)

- `is_a_version_conflict` is set only while rendering a **Package/SetArg** parent line
  (the last `else` at SC:639-664) whose `(parent, atom)` is in a `("version", *)` or `("slot", *)`
  reason (SC:645-664). It is never set from AtomArg, PackageArg or soname lines (SC:630-638),
  nor from parents that were not selected for display.
- `conflict_is_unspecific` is set when a conflict pkg has parent atoms but
  `selected_for_display` is empty (SC:681-686): it prints
  `(no parents that aren't satisfied by other packages in this slot)`.
- `get_explanation()`:
  1. `is_a_version_conflict` → None (SC:731-732);
  2. `conflict_is_unspecific and not ("--newuse" in myopts and "--update" in myopts)` → returns
     `"!!! Enabling --newuse and --update might solve this conflict.\n!!! If not, it might help emerge to give a more specific suggestion.\n\n"`
     (SC:734-739). Plain text, no colour. It is printed even when there are no solutions;
  3. `not self.solutions` → None (SC:741-743). This tests **solutions**, not `changes`;
  4. Header (SC:745-756):
     - `len(solutions) == 1`: `It might be possible to solve this slot collision\n` (`these slot collisions\n`
       when `len(all_conflicts) > 1`), then `by applying all of the following changes:\n`;
     - else: the same first line, then `by applying one of the following solutions:\n`.
  5. Body (SC:758-783):
     - `len(self.changes) == 1`: `print_change(changes[0], "   ")`;
     - else, for each change: `  Solution: Apply all of:\n` + `print_change(change, "     ")`.

     `print_change` writes one line per pkg (dict insertion order):
     `<indent>- <pkg.cpv> (Change USE: <flags joined by ' '>)\n`, then one `\n` after all pkgs.
     Each flag (dict order) is `colorize("red", "+flag")` = `\x1b[31;01m+flag\x1b[39;49;00m` or
     `colorize("blue", "-flag")` = `\x1b[34;01m-flag\x1b[39;49;00m`. With `--color=n` both are plain.
     The cpv is bare: no repo, no slot.
  - **Header/body mismatch (quirk Q4)**: the header counts `solutions` but the body counts
    `changes`. Two solutions minimised to one change give "one of the following solutions:"
    followed by a single 3-space-indented change with no `Solution:` line. Upstream case 7 shows it
    live (`captures/upstream-sc.txt`).

## 5. Where it is used (D)

- `need_config_change` (D:11838-11885), every arm, in order:
  1. `_success_without_autounmask or _required_use_unsatisfied` → True (D:11843-11847). Set at
     D:5808 and D:3667.
  2. `_slot_conflict_handler is None and not _accept_blocker_conflicts() and any(slot_conflicts())`
     → build the handler (D:11849-11854). If `.changes` → True (D:11855-11863).
     - `_accept_blocker_conflicts` (D:9302-9315) is False whenever `_allow_backtracking`, otherwise
       True for `--buildpkgonly/--fetchonly/--fetch-all-uri/--nodeps`;
     - the handler lives in `_dynamic_config` (D:675), so it is built at most **once per depgraph
       instance (= once per pass)**, not once per run. The arm returns True **only in the call that
       built it**.
  3. `_allow_backtracking and --autounmask-backtrack != "y" and _have_autounmask_changes()`
     (D:11865-11883): with `--autounmask-continue` (and `--autounmask-backtrack` not "n") → False;
     else set `_autounmask_backtrack_disabled` → True. This is the "In order to avoid wasting
     time, backtracking has terminated early" notice (D:11249-11256).
  4. → False (D:11885).
- The loop (D:12401-12418) is `if success or need_config_change() or not allow_backtracking or
  backtracked >= max_retries: break`. `need_config_change()` therefore runs on **every** failed
  pass, including `--backtrack=0` (it is evaluated before `not allow_backtracking`), and it builds
  the handler on that pass's graph.
- After the loop (D:12420-12439): `if backtracked and not success and not mydepgraph.need_display_problems():`
  rerun with `allow_backtracking=False, backtrack_parameters=backtracker.get_best_run()`.
  `need_display_problems` (D:11827-11836) calls `need_config_change()` **again**. Arm 2 is skipped
  this time (the handler is not None), so after an arm-2 stop it returns False unless arm 1/3 or
  circular deps hold.
  - **Try 0** (`backtracked == 0`): no rerun; the stopped pass is the report (upstream cases 0/6/7,
    cgp USE keeper).
  - **Try N > 0**: the rerun happens. `get_best_run()` (B:287-298) is the deepest *terminal* node
    (no masks), usually the root. The report is that graph, and its handler is rebuilt at display
    time (quirk Q1, `captures/pg-late-pass4.txt`).
- Then `autounmask_breakage_detected()` may rerun once more with autounmask off (D:12441-12458);
  that pass gets its own handler at display.
- Display: `display_problems` (D:11260) → `_show_slot_collision_notice` (D:1727-1790), only when
  `any(slot_conflicts())` (D:11274-11276).
  - `_show_merge_list()` first (D:1738). It reuses the cached handler or builds one (D:1740-1742).
  - It writes `get_conflict()` (D:1744-1745). `conflict_msg` ends with the per-slot `"\n"` (SC:702)
    and the final `"\n"` (SC:726), i.e. two newlines after the last parent block, plus the
    `NOTE: ... --verbose-conflicts` / need_rebuild trailers in between when present (SC:704-724).
  - If the explanation is truthy it is written and the function **returns** (D:1747-1750). No
    package.mask advisory, no `--backtrack=30` sentence, no "For more information" block. `--quiet`
    does not suppress the explanation (the `--quiet` return is after it, D:1752-1753).
  - Otherwise the advisory follows. Its `--backtrack=30` sentence needs `not _allow_backtracking`
    on the **displayed** depgraph and `--backtrack` unset or in 1..29 (D:1765-1775).
  - Everything goes to **stderr** (`writemsg`).

## 6. Quirks (candidates for `LLM/portage-quirks.md`)

- **Q1 — a stop after try 0 drops its own suggestion.** Arm 2 is not idempotent (D:11849-11863),
  so `need_display_problems()` at D:12420 is False for the stopped pass. The final pass reruns from
  `get_best_run()` with no masks and reports the pre-backtrack graph. Live:
  `captures/pg-late-pass4.txt`. On try 4 the handler finds `{K-1: -foo, M-1: -foo}` and the loop
  breaks. The 6th `need_config_change` call returns False. The final pass (allow_bt False, no masks)
  shows the `dev-libs/Z:0` version conflict with the package.mask advisory **and** the
  `--backtrack=30` sentence, `backtrack: 4/20`, and no `Change USE` line. The D:11856-11862 comment
  says the handler's changes "are more likely to be useful", which this defeats. Mimic: yes (it
  decides the output).
- **Q2 — forcing "disabled" on a flag that is already off invalidates the candidate**
  (SC:991-1000 vs SC:1001-1010, checked at SC:1115-1122). The result is a spurious `+flag`.
  Live: `captures/pg-force-disabled.txt`. L's `K[-foo,bar?]` (L has bar off) makes `bar` a `cond`
  flag. Candidate `bar=disabled` → `""` → invalid. Candidate `bar=enabled` → valid. Portage then
  suggests `sci-libs/K-1 (Change USE: -foo +bar)`, though `-foo` alone (with M `-foo`) solves it.
  Mimic: yes.
- **Q3 — an unbuilt package can never be tested with an empty USE.**
  `findAtomForPackage(pkg, modified_use)` copies the pkg and sets `_metadata["USE"] = " ".join(use)`
  (SETS:122-124). PKG:746-754 treats an empty `USE` as "not stored" and recomputes it from the
  config. A solution that turns off a package's last enabled flag therefore fails the match check
  ("new conflict introduced"). Live: `captures/pg-k-eapi8-debug.txt`. Upstream case 6 rebuilt with
  EAPI 8 ebuilds (no `x86` in USE) loses its solution, while the EAPI-1/2 original keeps it because
  `x86` stays in USE. Irrelevant for the pmtest fixtures (`ELIBC="glibc"` keeps `elibc_glibc` in USE).
  Mimic: only if portuale ever renders an empty-USE ebuild; record it.
- **Q4 — the header counts solutions, the body counts changes** (SC:745-783). See §4. Live: upstream case 7.
- **Q5 (not reproduced)**: SC:114 would raise `TypeError` for a conflict pkg with no parent atoms,
  which would make the "(no parents)" arm (SC:700-701) unreachable through the handler. Not probed.

## 7. Escapes and edge cases

- No slot conflicts in the pass → arm 2 never runs (D:11852).
- `--nodeps` → `max_retries = 0` (D:12366), so `allow_backtracking` is False and
  `_accept_blocker_conflicts` is True (D:9311-9313): no handler in the loop. The display still builds
  one (D:1740-1741) and can print the explanation.
- `--backtrack=0` without those options → the handler is built in the loop and can stop it; the
  pass is the report either way.
- A solution needs at least one involved flag. Without any USE dep among the conflict's parents,
  `required_changes` stays empty → None (SC:1186-1188). Pure version/slot conflicts never stop
  here and never explain (unless unspecific → the `--newuse` text).
- `is_a_version_conflict` suppresses the explanation but **not** the stop (D:11855 looks only at
  `.changes`). If a version-mismatch parent is displayed while another configuration still has a
  USE solution, Portage would stop and print the generic advisory. Not seen in any probe (§ cases.md).
- `changes == [{}]`: a solution whose every forced state already matches gives `_get_change` → `{}`.
  `_add_change` then appends `{}`, which is truthy, and stops backtracking with an empty body.
  Reasoned from SC:226-239 and SC:205-224 only; not reproduced.

## 8. Upstream shapes usable as Rust unit tests (`lib/portage/tests/resolver/test_slot_collisions.py`)

Run live via `playground/sc_upstream.py` (ResolverPlayground, PYTHONHASHSEED=0;
`captures/upstream-sc.txt`). All results match the test's `slot_collision_solutions`.

| # | request / options | conflict | Portage result | use as |
|---|---|---|---|---|
| 0 | `dev-libs/A B C D`, `--autounmask=n` | `dev-libs/A:0`: ebuild A-1 vs installed A-1 (`C: A[foo]`, `D: A[foo=] B[foo=]`) | backtracked 0, stop on try 0; changes `[{A-1: foo True, D-1: foo True}]`; one solution → `by applying all of the following changes:` / `   - dev-libs/A-1 (Change USE: +foo)` / `   - dev-libs/D-1 (Change USE: +foo)` | stop + explanation, `+flag` on a parent through `[foo=]` |
| 1 | `sys-libs/A B C D E F`, `--autounmask=n` | `sys-libs/J:0`: J-2 vs J-1 | handler built try 0, changes `[]`, not a version conflict; backtracked 2; no explanation | negative case: USE-only conflict without a solution (`J[foo(+)]`, defaults) |
| 2 | `=app-misc/X-1 =app-misc/Y-1` | `app-misc/Z:0` Z-1/Z-2 | `is_a_version_conflict` True, changes `[]`, backtracked 4 | version conflict → None |
| 3 | `=app-misc/X-2 =app-misc/Y-2` | same | same, backtracked 4 | version conflict (`>`/`<` operators) |
| 4 | `=app-misc/E-1 =app-misc/F-1` | `dev-libs/E:0` E-1/E-2 | version conflict, changes `[]`, backtracked 3 | mixed USE + version parents → None |
| 5 | `dev-lang/perl:0/5.12 dev-lang/perl:0/5.16 =dev-lang/perl-5.12*` | `dev-lang/perl:0` | changes `[]`, `is_a_version_conflict` **False** (AtomArg parents never set it, SC:636-638), backtracked 2, no explanation | sub-slot conflict with argument parents only |
| 6 | `sci-libs/L sci-libs/M` | `sci-libs/K:0` ebuild K-1 (foo) vs installed K-1 (-foo) | stop on try 0, changes `[{K-1: foo False, M-1: foo False}]`, explanation `   - sci-libs/K-1 (Change USE: -foo)` / `   - sci-libs/M-1 (Change USE: -foo)` | the canonical stop (the D:11856-11862 comment names it) |
| 7 | `sci-libs/P sci-libs/Q:2` (both orders), `--update --complete-graph --autounmask=n` | `sci-libs/Q:1` ebuild vs installed | 2 solutions → 1 change `[{Q-1: foo True, P-1: foo True}]`; header `by applying one of the following solutions:` with one 3-space change and no `Solution:` line (Q4); solution 1 also forces installed Q-2 `bar` (§3) | `_add_change` minimisation + Q4 |
| (commented out upstream) | `=app-misc/C-1 =app-misc/B-1` | REQUIRED_USE `^^ ( foo bar )` | upstream says it varies between runs; not used | — |

`testConnectedCollision`, `testDeeplyConnectedCollision` and `testSelfDEPENDRemovalCrash` succeed
through `_solve_non_slot_operator_slot_conflicts`. They do not reach the handler with a result.

Extra shapes probed here, for unit tests of the quirks:
- `playground/shape_force_disabled.py` (Q2): changes `[{K-1: foo False, bar True}, M-1: foo False]`,
  explanation `   - sci-libs/K-1 (Change USE: -foo +bar)` / `   - sci-libs/M-1 (Change USE: -foo)`.
- `playground/shape_late4.py` (Q1): the stop on try 4 is discarded; final `backtrack: 4/20`, the
  Z:0 version conflict, the advisory with the `--backtrack=30` sentence.
- `playground/shape_k.py` vs `shape_k_up.py` (Q3): the same case with EAPI 8 loses its solution.
