# Review — n196 fix round 1 (re-review)

**Verdict: APPROVE.** All three Important items from `n196-review.md` are resolved against real Portage 3.0.82.2 source (verified below against `../pmtest/3rdparty/portage`, pinned 3.0.82.2). No new Critical or Important defects. Backlog #196 may now be flipped to DONE at the owner's discretion — the fix brief's owner rule ("port real exactly") is satisfied on every reachable print point; the remaining residues (`--resume`, remote-exec early return, `--ask`+`--read-news` eselect prompt/spawn) are genuinely unreachable-or-interactive and correctly tracked.

## Fix-round-1 verification (each first-review Important item)

- **Important #1 (`--pretend`-end arm) — RESOLVED.** Real `post_emerge` early-returns when `not vardbapi._pkgs_changed` but prints the notice first iff `"--pretend" in myopts` (`lib/_emerge/post_emerge.py:112-117`, confirmed), and real `run_action` calls `post_emerge` unconditionally after `action_build` (`lib/_emerge/actions.py:4289-4297`, confirmed: `retval = action_build(...); post_emerge(...); return retval`). Portuale's `pretend_end_news_notice` (`rust/portuale/src/pretend.rs:7064`) is now called at every `--pretend` exit of the build path: three resolve-error arms (`pretend.rs:12007,12055,12068`), autounmask-only / autounmask-changes / blocker / slot-conflict / buildpkgonly-unsat / large-cycle / abort returns (`pretend.rs:13139,13179,13215,13265,13479,13529,13552`), and the final tail (`pretend.rs:13957`, no-op when `!pretend`). Usage-error returns above the resolve stay silent — correct, real never reaches `post_emerge` there either. `--json` stays silent — correct *and* structurally enforced: the `--json` path returns early (`pretend.rs:12213-12280`) before any notice call. Pinned both directions including failed-resolve (`test_portuale.py:4795,4824`). The old R1 pollution fear is correctly retired: real's `count_unread_news` runs with `update=True` (`lib/portage/news.py:447-506`), so writing `.unread`/`.skip` on pretend is faithful, and the suite strips `FEATURES` so zero corpus drift results.
- **Important #2 (post notice on merge failure) — RESOLVED.** Real prints at the `post_emerge.py:155` tail regardless of retval (retval only feeds `exit_msg`, `post_emerge.py:104-108`, confirmed). Portuale's `merge_failure_news_notice(changed, …)` (`pretend.rs:7084`) on both merge arms (`pretend.rs:13794,13865`) with `changed = unmerged.len() < entries.len()` is a sound structural reading of real's `_pkgs_changed` gate. Pinned both directions (`test_portuale.py:4890` partial-failure prints twice post-after-last-`>>>`; `test_portuale.py:4914` total-failure prints once). Digest/missing-failure early returns staying silent is consistent (nothing merged → `_pkgs_changed` false).
- **Important #3 (post notices after uninstall actions) — RESOLVED.** Real calls `post_emerge` after uninstall unless deselect/buildpkgonly/fetchonly/pretend (`lib/_emerge/actions.py:4164-4175`, confirmed). Portuale funnels all removals through `execute_unmerge` (`pretend.rs:5253`), now printing the notice at the tail (`pretend.rs:5384`, after the preserved-libs notice, mirroring real's tail order) with correct structural gates: `pretend` returns before any removal (pinned: `-p -C` silent, `test_portuale.py:4940`), empty selections return before removal (Rust unit test), `run_deselect` never reaches `execute_unmerge` (`pretend.rs:11186` vs `execute_unmerge` call sites — deselect has its own return). Failure nuance (`idx > 0` prints, first-removal failure silent, `pretend.rs:5322`) correctly models the vdb-changed gate.
- **Minor #4 — RESOLVED.** All-zero guard moved inside `print_news_notifications` (`pretend.rs:6994`), matching real `news.py:526` (`if news_reader_display`, confirmed).
- **Minor #6 — RESOLVED.** `--quiet`-still-prints and resolve-failure-pre-notice pinned (`test_portuale.py:4848,4872`); empty-mergelist-no-post deliberately unpinned with a valid reason (no CLI shape produces it).
- **Minor #7 — RESOLVED.** Citations now read `actions.py:4264-4281`/`4289-4297`/`4164-4175`, all within the confirmed ranges.

## Strengths

- Real-source fidelity is exact on every re-checked claim, including the subtle ones (unconditional `post_emerge` tail, `update=True` write-back on pretend, deselect exclusion).
- The `changed`/`idx > 0`/structural-empty gates are honest approximations of `_pkgs_changed`, each labeled as such in comments with real file:line anchors.
- Test matrix is genuinely strong: 3 Rust unit tests pin gates via state-dir absence/presence on isolated ROOTs; 7 contract pins cover user-visible behavior (once-vs-twice, ordering vs `Calculating...`/`>>>`/`[ebuild`, both FEATURES directions, `.unread` contents). No fixture-tree pollution by construction.

## Issues

### Minor

1. **Reload caveat documented in report but not in code.** The fix-round report correctly notes real reloads settings (`post_emerge.py:92-95`, confirmed) while portuale reuses resolve-time `&config`, but no code comment at the post-merge (`pretend.rs:13920-13948`) or unmerge (`pretend.rs:5384`) call sites says so — a future reader could mistake the reuse for the reload. One-line comment each. — `pretend.rs:13920`, `pretend.rs:5384`
2. **`--json`-stays-silent has no pin.** The silence is structural today (early return at `pretend.rs:12213-12280`), but a refactor that merges the `--json`/text tails would pollute machine-readable output with nothing catching it. One contract test with `FEATURES="news" --json --pretend` asserting no notice text. — `pretend.rs:12213`

## Spec-compliance verdict

Compliant. All four real print-point families (pre-resolution non-pretend, post-merge incl. failures gated on vdb-changed, post-uninstall excl. deselect/pretend, `--pretend`-end incl. failed resolves) now match real 3.0.82.2 in gates, text, stream, and order. Deliberate omissions (`--ask`+`--read-news` eselect prompt/spawn, `--resume`, remote-exec early return) match the fix brief's scope and are correctly tracked as residues.

## Test-quality verdict

Very good. Hermetic (tmp ROOTs, symlinked vdb), behavior-pinning (counts, ordering, both gate directions, failure shapes both ways), zero-drift discipline held (full suite green, no bless). Deductions only for the two Minor gaps above.

## Task-quality verdict

Strong. The fix round closed everything asked, corrected its own round-0 rationale in writing (R1 fear retired with evidence, not silently), kept residues explicit with shrunken scope, and held the no-bless rule. Process note, non-blocking: the report's "Minor #5 noted in the post-merge comment" claim is slightly overstated (it's in the report, not the code) — see Minor 1.

Read-only review; no files written, no tests executed (gate logs cited from `n196-report.md` §Fix round 1, not re-run).
