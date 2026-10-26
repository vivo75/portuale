# crates/portage-repo/src/tests/autounmask.rs
Autounmask suggestion, abort-path, cascade, and breakage tests.

## fn autounmask_suggests_a_keyword_only_when_enabled
- Fixture `dev-libs/autounmaskkeywordpkg`: KEYWORDS `~amd64`, not accepted by the fixture profile's ACCEPT_KEYWORDS, no `package.accept_keywords` entry; masked by KEYWORDS alone (package.mask/license/properties/restrict pass): the exact "keyword_masked_only" shape autounmask's v1 suggestion targets.
- With `autounmask_suggest_keywords` off (Portage's correct default; see `resolve_pretend_graph`'s doc comment for the full on/off default-resolution logic this mirrors), no suggestion is appended, matching portuale's pre-existing behavior exactly.
- Keyword-masked-only renders Portage `_show_unsatisfied_dep`'s "All ebuilds … have been masked" block (`~amd64 keyword`), not the bare "there are no ebuilds to satisfy" line.
- With `--autounmask` explicit the target resolves (Portage applies the implicit `=cpv ~arch` change) as a normal `New` entry, recorded for the keyword-changes block.

## fn autounmask_use_suggests_a_flag_flip_only_when_enabled
- Fixture `dev-libs/useflagpkg`: `foo` enabled globally by the fixture profile's make.conf (see use_dep_enforcement's tests: `-foo` is genuinely unsatisfiable against it). `dev-libs/useflagpkg[-foo]` masked by this one USE-dep alone: the exact "use_masked_only" shape. With `autounmask_suggest_use` off (Portage's default) the abort is Portage's own USE block (backlog #265(b)), not the bare miss.
- With `--autounmask-use` (on by default) the atom resolves via the implicit `package.use` flip.

## fn fixture_dependency_use_dep_mismatch_resolves_via_the_flip_once_autounmask_use_is_on
- Fixture `dev-libs/usedeprejectedpkg` RDEPENDs `dev-libs/useflagpkg[-foo]`.

## fn autounmask_license_resolves_a_eula_masked_dependency
- Fixture: LICENSE `SomeEula`, no `package.license` unmask; change atom `>=cpv`, token the license, two-line dep chain.

## fn autounmask_keep_masks_n_unmasks_a_package_mask
- `graph_result_autounmask` has `autounmask_suggest_masks` on, so the dependency resolves and the `=<cpv>` mask change is recorded (empty token); without autounmask the dep stays NoVisibleCandidate.
- Fixture `dev-libs/maskmaskedconsumer` RDEPENDs package.mask'd `dev-libs/hardmaskedpkg`; change atom `=<cpv>`, empty token.

## fn abort_outcome_membership_and_order
- Slice 3 pins (`docs/history/abort-path-spec.md` §6, live captures in `fixtures/abort-captures/`); the renderer switches to `partial` in Slice 4. The `-last` sibling is identical (declared position is unobservable).
- The `abort-au-*` tops resolve with `--autounmask-use` on (the CLI default); `graph_result_real` runs with autounmask off.
- With `--autounmask-use=n` the `[auflag]` dep wins over the cycle (spec §4e third bullet: Portage prints the "no ebuilds built with USE flags" block, no list).
- Precedence oracle `abort-masked-cycle`: no list, no circular block, even though the cycle is still detected.
- Portage `_success_without_autounmask` shows the full list for changes-alone and ordinary resolves.
- Expected value from Portage: the outcome is produced on the settled graph while `entries` stays the full graph.
- Masked-only dep (§4a): empty partial, the report's display atom, the top as the first merge-bound requirer -- and the `-last` sibling identical (declared position is unobservable).
- Unserializable cycle (§4c): the `_serialize_tasks` stuck remainder -- top + both arms, both leaves drained out -- in leaf-drain order, `partial` one entry per member in that order. The autounmask+cycle fixtures (§4d) are the same shape: the autounmask changes only stop Portage's backtracking at try 0.

## fn autounmask_backtrack_disabled_gate
- Backlog #217, both branches of `GraphResult::autounmask_backtrack_disabled` (Portage `_autounmask_backtrack_disabled`, `depgraph.py:11752`).
- `abort-au-plain`'s USE block has NO notice (lone change: `_success_without_autounmask`); `abort-au-cycle`'s and `aucasctop`'s blocks DO (a cycle / a restart-worthy flip on the already-graphed `aucascmid` coincide).
- Post-failure parent collects keep the notice too (Portage `_apply_parent_use_changes`, `depgraph.py:5820`): the aup0b cell records `-foo` on the failed dep and fails with the notice.
- `dev-libs/parentflipeqpkg`: round-2 correction of the old resolve+rows expectation, which was never live-probed.
- `dev-libs/pfgraphparent`: live Portage (`localhost/test-portuale:latest`, staged fixtures, g195c probe `LLM/evidence/2026-09-28-g195/g195c-real-probe.txt`). Corrected under coordinator ruling B20, option (a); the old flip+notice assertions were a source-reading inference, never a live measurement.
- `dev-libs/pfgraphparent`: same masked-child shape as parentflipeqpkg (`use.mask pf`), so the same untouchable-child `continue` (`depgraph.py:6732-6736`) applies; Portage reports the bare miss under default AND `--autounmask-backtrack=y`: `emerge: there are no ebuilds to satisfy "dev-libs/pfgraphchild[pf=]"`, no rows, no block, rc 1.
- Config conjuncts: `--backtrack=0` leaves Portage's `_allow_backtracking` False so the flag is never set. No changes, no notice is the predicate's first conjunct.

## fn autounmask_cascade_flip_before_dep_walk_pulls_the_gated_leaf
- Backlog #218: Portage `_add_dep` only pushes onto `_dep_stack` (`lib/_emerge/depgraph.py:3254-3271`); the flip itself is `_pkg_use_enabled(pkg, target_use)` (`:7669`). Live-captured in `fixtures/abort-captures/dev-libs_aucasctop.pretend-debug.stdout` (one continuous walk).
- Portuale's already-resolved-slot re-check re-expands via `expand_resolved_slot_with_flipped_use`, so the default (no-backtrack) graph carries the full 4-package list. The #217 notice gate reads the same accumulator.
- L0 20260928T063310Z: the delta never records a phantom owner: every `required_by` set is exactly the forward-walk owners, with no flipped-package residue for `build_digraph`'s fallback to stitch a phantom edge from.
- The fresh-flip shape is untouched: `aucasclate` alone resolves `aucascmid[cascade]` on first encounter, leaf included, with no delta pass involved.
- L0 20260928T063310Z: `aucascleaf` queues only through the flipped package's re-expansion (a genuinely new slot), and no text-new delta atom re-names a settled slot here; Portage carries no such edge (the flip lands via the later atom, never via a re-walk of the flipped package).
- The delta only anticipates what the `--autounmask-backtrack=y` whole-walk re-drive finds.

## fn autounmask_keywords_records_the_dep_chain_for_a_keyword_masked_dependency
- Fixture `dev-libs/autounmaskdepconsumer`; change atom `=cpv`, token `~amd64`, two-line dep chain.

## fn autounmask_keyword_backward_cascade_re_resolves_a_slot_to_a_masked_version
- `dev-libs/kwbacktop` RDEPENDs `dev-libs/kwbackmid` (bare -> stable `1.0` wins the slot first) AND `>=dev-libs/kwbackmid-2.0` (only `2.0` exists there, and it is `~amd64`).
- Slice 6: the slot conflict folds both atoms into `slot_constraints`, and on retry `resolve_pretend`'s `*_masked_only` fallback fires because no is_visible candidate satisfies the folded `>=2.0`. Fixture: bare atom lets stable `1.0` win first; only `2.0` exists above, `~amd64`. Portage `_select_pkg` weighing every atom pulling the slot + `_autounmask_levels`.

## fn autounmask_levels_unmask_two_categories_at_once_on_the_same_version
- `dev-libs/multimaskdep-2.0` is `~amd64` keyword-masked AND `@EULA` license-masked; `multimaskdep-1.0` is only keyword-masked. Portage `_autounmask_levels`: level 1 (+license) yields nothing (both still keyword-blocked); level 2 (+~arch +license) unmasks BOTH categories, and re-scanning every version picks the higher `2.0`.
- Fixture: `multimaskdep-2.0` `~amd64` AND `@EULA` license-masked; `1.0` only keyword-masked. Before the per-level re-scan, portuale's flat `keyword_masked_only` fallback excluded `2.0` (it has a license problem too) and settled on `1.0` with only a keyword change.

## fn autounmask_keywords_top_level_dep_chain_is_the_argument_line
- Backlog #248, expected value from Portage: the change sits on the argument package itself, and Portage's `_get_dep_chain` never prints the start node -- just the `(argument)` line (fresh host 3.0.82.2 staged-fixture probe 2026-09-29).

## fn autounmask_use_resolves_a_use_dep_mismatch_and_applies_the_flip_to_the_display
- Fixture `foo` default-on, atom wants it off. Change recorded with the `>=<cpv>` atom form (Portage `check_if_latest` for USE, bug #536392). Backlog #248: the dep chain is just the `(argument)` line -- Portage's `_get_dep_chain` never prints the start node (fresh host 3.0.82.2 staged-fixture probe 2026-09-29).
- `dev-libs/useflagpkg[-foo]` -- `foo` is default-on, the atom wants it off. `--autounmask-use` (on in `graph_result_autounmask`) applies the flip so the entry resolves as `New`, the `-pv` USE display shows `foo` disabled.

## fn autounmask_use_flip_is_not_blocked_by_a_default_satisfied_missing_flag
- Fixture atom needs `missingflag` on (in IUSE, default-off: a genuine flip) and `nonexistentflag(+)` (not in IUSE). `suggested_use_flip` must skip the default-satisfied flag rather than bail on the whole atom.

## fn autounmask_use_dependency_records_the_two_line_dep_chain
- Fixture `dev-libs/usedeprejectedpkg` RDEPENDs `dev-libs/useflagpkg[-foo]`.

## fn autounmask_use_parent_flip_fails_like_portage_when_the_child_flag_is_masked
- Expected value from Portage: live probe (`localhost/test-portuale:latest`, staged fixtures, `emerge --pretend dev-libs/parentflipeqpkg`, rc 1) reports the bare miss (`emerge: there are no ebuilds to satisfy "dev-libs/parentflipchildpkg[feat=]"`), no rows, no USE block. Portage's untouchable-child `continue` (`lib/_emerge/depgraph.py:6732-6736`) skips the parent probe when the child's needed flag is masked/forced.
- History: the old resolve+rows+block expectation was verified against the since-removed second Python copy, never live Portage.

## fn autounmask_use_parent_flip_records_minus_foo_and_fails_for_aup0b
- Expected value from Portage: upstream `test_autounmask_parent.py` (`aup0b`): `dev-libs/aup0b` (IUSE `+bar +foo`) DEPENDs `dev-libs/aup0d[foo(-)?,bar(-)?]`; the fixture profile leaves the parent at `{foo}`, so only the `foo(-)?` conditional is parent-active. Live Portage (`localhost/test-portuale:latest`) records `-foo` on the parent and FAILS; live probe (`emerge --pretend --autounmask =dev-libs/aup0b-1`, rc 1): stdout is the merge header with no rows, stderr is the `-foo` USE block plus the "backtracking has terminated early" notice. Portage `_apply_parent_use_changes` (`lib/_emerge/depgraph.py:5820`) re-probes with `violated_conditionals` (`:6779-6798`), and the `_success_without_autounmask` tail (`:5793`) returns False. Fixture profile leaves the parent at `{foo}`.

## fn autounmask_use_parent_flip_pfgraph_reports_the_bare_miss_like_portage
- Expected value from Portage: the quoted verbatim output above (identical text with `--autounmask-backtrack=y` and `--autounmask-use=n`; Global-Updates/news/FEATURES noise cut; portuale omits Portage's staging `for <root>.` suffix, the long-standing `fixture-miss-message-unsuffixed` class).
- `dev-libs/pfgraphparent` (IUSE +pf) RDEPENDs `dev-libs/pfgraphchild[pf=]` AND `pf? ( dev-libs/pfgraphextra )`. The child's `pf` is `use.mask`'d. Live Portage (`emerge --pretend dev-libs/pfgraphparent`, rc 1) prints `(dependency required by "dev-libs/pfgraphparent-1.0::testrepo" [ebuild])` and `(dependency required by "dev-libs/pfgraphparent" [argument])` after the miss line.
- `pfgraphextra` is still walked (`pf? (...)` stays active with the parent's +pf unflipped) but the `Aborted` outcome prints no rows -- Portage's merge list is likewise empty.
- Portage's untouchable-child `continue` skips the parent probe, so neither the default single-dep re-resolve (which kept `pf? ( pfgraphextra )` in the list) nor the `--autounmask-backtrack=y` whole-graph re-resolve (which dropped it) ever happens; Portage's merge list is likewise empty.
- History: renamed from `autounmask_use_parent_flip_re_resolves_the_whole_graph`, which stated the opposite of what Portage does (coordinator ruling B20, option (a)).

## fn existing_slot_use_change_chain_starts_at_the_requester
- Backlog #244, expected value from Portage 3.0.82.2 (`LLM/evidence/2026-09-28-244/fixture-oracle-2/real/dev-libs_aubreaktop.txt`): `aubreaktop` pulls `aubreaksub` plain first, then `aubreakwant` needs `aubreaksub[brk]` on the already-resolved package. The change chain starts at the requester whose atom needed the change, not at the package that pulled it in first.

## fn autounmask_use_breakage_abandons_autounmask_and_re_resolves_clean
- `dev-libs/aubreaktop` RDEPENDs `aubreaksub` (plain, resolves `brk` off first), then `aubreakwant` (needs `aubreaksub[brk]`) and `aubreakunwant` (needs `aubreaksub[-brk]`). Autounmask folds `brk` on for `aubreakwant`; the next pass's `aubreakunwant[-brk]` asks for it back off -- a flag wanted both ways.
- Expected value from Portage `_autounmask_breakage` (`depgraph.py:12262`): drop every change and re-resolve one final pass with suggestion off; `aubreaksub` settles at default `brk` off, and `aubreakwant`'s `[brk]` is an ordinary (non-fatal) `NoVisibleCandidate`, exactly what `--autounmask-use=n` would have produced from the start. The default collect-only path never gets a second pass to notice the flag wanted both ways.

## fn non_use_changes_keep_no_trigger
- Backlog #205 R4, negative legs, on the same fixture shapes their own tests pin.

## fn abort_outcome_membership_and_order (#517 (b))
- Portage's merge-order bias (`_merge_order_bias`, parent count descending) puts a parentless top target last; the cycle arms (one parent each) drain first. Verified against host Portage's re-display (`[ebuild] top`, `[nomerge] a`, `[ebuild] b`, `[ebuild] a`) for the `mid` and `last` tops.
