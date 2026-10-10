# n196 report — backlog #196: the GLEP 42 news-count notice

Branch `backlog/196-news-count-notice` in both worktrees
(`/home/vivo/repo/PORTUALE/wt-196-news-count-notice/`).
Status: DONE (with BED-PENDING items for the coordinator; no bless needed —
zero drift). Backlog entry #196 intentionally left unflipped per the brief.

## S0 — real source grounding (3rdparty/portage = 3.0.82.2; symbols, not lines)

- `lib/_emerge/post_emerge.py::display_news_notification`: returns False
  unless `news` is in `root_config.settings.features`; otherwise
  `count_unread_news(portdb, vardb)`, returns False when every count is 0,
  else `display_news_notifications(news_counts)`. No `--quiet`/`--ask`/
  `--nodeps` consult anywhere in this function.
- `lib/_emerge/post_emerge.py::post_emerge`: when `vardbapi._pkgs_changed`
  is false, prints the notice only under `--pretend` ("GLEP 42 says to
  display news *after* an emerge --pretend") and returns; otherwise
  (info update, preserved-libs notice, `chk_updated_cfg_files`,) calls
  `display_news_notification` unconditionally at the end (no retval check
  — it also prints when the merge failed, and under `--quiet`).
- `lib/_emerge/actions.py::run_action`: the pre-resolution call, gated on
  `"--pretend" not in myopts`, sits after argument validation and before
  `action_build`; when the notice printed *and* `--ask` *and* `--read-news`
  are given, real prompts "Would you like to read the news items while
  calculating dependencies?" and spawns `eselect news read` on Yes.
  `post_emerge` is also called after uninstall actions (clean/depclean/
  prune/unmerge/rage-clean, except deselect/buildpkgonly/fetchonly/pretend).
- `lib/portage/news.py::count_unread_news` (per-repo `NewsManager`
  `updateItems` with `update=True` + `getUnreadItems` = `len(.unread)`)
  and `lib/portage/news.py::display_news_notifications`: leading blank
  line, one ` * IMPORTANT: <N> news items need reading for repository
  '<repo>'.` line per repo with N > 0, then ` * Use eselect news read to
  view new items.` plus a trailing blank line; all via `print()` (stdout).
- No fresh container probe was spent: the m185 probe already captured the
  ordering on a non-pretend `emerge --oneshot` (news, blank, header,
  `Calculating...`), and the notice text/ordering above comes from the
  pinned 3.0.82.2 source, which has no version skew on these lines (the
  only known 3.0.81.3-vs-3.0.82.2 skew is the `Calculating` spacing).

Portuale's existing machinery (`rust/portuale/src/pretend.rs`):
`FilesystemNews` (`NewsManager.updateItems` + `getUnreadItems` semantics
over `metadata/news`, with `.read`/`.skip`/`.unread` state files and
`write_news_state_if_changed`), `news_item_valid` / `news_item_relevant`,
and `run_check_news` (the `actions.py:3844` standalone action, which has
no FEATURES gate — unlike `display_news_notification`).

Correction to the m185 report: fixture repos DO carry `metadata/news`
(`fixtures/repo/metadata/news/`, eleven items); the hermetic unread count
is 5, not 0 (pinned below by both a Rust test and a contract test).

## S1 — fix (`rust/portuale/src/pretend.rs`, +197/−29)

- Extracted `unread_news_counts(repos, root)` (evaluate + write-back, byte-
  identical to what `run_check_news` did) and `print_news_notifications`
  (real `display_news_notifications` text), both shared by `--check-news`
  and the new notice so the two can never drift apart; new pure predicate
  `news_notice_enabled` (real `post_emerge.py:38`, `news` ∈ FEATURES).
- `display_news_notice_if_any`: the `display_news_notification` port
  (FEATURES gate + nonzero-count gate, no `--quiet` gate, stdout).
- Call site 1 (pre-resolution): just before `run_resolve`, gated on
  `!pretend` — after all usage-error returns, before resolution, so it
  fires even when resolution later fails, exactly like real's
  pre-`action_build` call; real's observed pre-`>>>` order (notice first,
  then header/`Calculating...`) falls out of the placement.
- Call site 2 (post-merge): inside the `!pretend` execution block after
  the preserved-libs notice (mirroring `post_emerge.py:141-155` order),
  gated on `!buildpkgonly && !entries.is_empty()` (real's `_pkgs_changed`
  gate: `--buildpkgonly` builds but never merges; an empty mergelist
  changes nothing).
- Deliberately NOT ported (residues, see below): the `--pretend`-end arm,
  the `--ask`+`--read-news` eselect prompt/spawn, uninstall-action
  post notices, `--resume`, the remote-exec early return, and the
  post notice on merge-failure paths.

Why the `--pretend`-end arm stays out is the slice's main judgment call,
documented not defaulted: real prints the notice at the end of a pretend
run too (`post_emerge.py:112-117`), but porting it would (a) write
`.unread`/`.skip` into the shared, git-tracked fixture ROOT on every one
of the ~1000 pretend contract tests (the pattern
`_check_news_isolated_root` exists to forbid exactly that) and (b) drift
the entire pretend corpus for no bed benefit (beds compare non-pretend
merges). Needs an owner ruling + corpus plan; filed as residue §R1.

A consequence the gates verify: under a fixture config root
`make.globals` stays absent (portage-profile reads it config_root-
relative) and the suite strips calling-env `FEATURES`, so the resolved
FEATURES has no `news` unless a test opts in — the existing suite is
provably silent (full suite: 0 drift, 0 pollution; see Gates).

## S2 — tests

Rust (`pretend::tests`, all pass):
- `news_notice_gate_needs_news_in_resolved_features` (pure FEATURES gate).
- `unread_news_counts_reuses_the_check_news_evaluation` (fixture testrepo
  → 5, overlay → 0, through a tmp ROOT sharing the fixtures' vdb
  read-only; pins sorted write-back + sticky re-evaluation; nothing
  touches the git-tracked tree).

pmtest (`pytests-contract-suite/test_portuale.py`, both pass):
- `test_emerge_oneshot_prints_news_count_notice_twice_like_real`
  (`FEATURES="news"`, isolated ROOT): `--buildpkgonly` shows the notice
  exactly once, before `Calculating...` (no post notice — real's
  `_pkgs_changed` gate); `-k --oneshot` shows it exactly twice, pre
  before `Calculating...` and post after the last `>>>` line; exact
  `* IMPORTANT: 5 news items ... 'testrepo'.` + eselect text; merge
  still lands; `.unread` has 5 lines.
- `test_emerge_buildpkgonly_without_news_feature_prints_no_notice`:
  no FEATURES → no notice text and no `var/lib/gentoo/news` dir at all.

## Gates (all green; no bless; nothing edited to make anything pass)

- `cargo fmt --check` clean; `cargo clippy --release --all-targets` zero
  warnings (toolchain `stable-x86_64-unknown-linux-gnu`).
- `cargo test --release` rc 0: all 26 suites green (portuale 666 passed
  incl. the 2 new tests; portage-repo 847). Log: `/tmp/opencode/n196/cargo-test.log`.
- pmtest full suite (registry-rebuilt release binary):
  `2071 passed / 37 skipped / 8 xfailed / 0 failed` (rc 0, 430 s),
  basetemp `/var/tmp/pmtest-n196full`, `-p no:cacheprovider`.
  No failures, no `corpus drift` lines. Log: `/tmp/opencode/n196/pmtest-full.log`.
- `git status` clean apart from the two intended files; no untracked
  fixture pollution (news state only ever lands in tmp ROOTs).

## BED-PENDING (coordinator only; run from the pmtest worktree)

1. `l32` candidate (this slice's bed: every real log shows the notice):
   `differential-test-bed/run/l32-lifecycle.sh`
   Expect: portuale-side logs gain the notice block before the first
   `>>>` (blank, ` * IMPORTANT: 15 news items need reading for
   repository 'gentoo'.`, ` * Use eselect news read to view new items.`,
   blank — 15 = the bed image's unread gentoo items per m185) plus one
   after each merge that changed the vdb. Flag: if the comparator does
   byte comparison on those regions, the count (15, evolving as news
   ages) may need normalising, the same class as the cut wall-clock
   seconds; if it normalises output, say so.
2. Merge-path safety gate (display-only change, but it runs on the
   non-pretend path and writes news state under ROOT — no merged-file
   writes): the `l1` merge gate per `docs/agent-context.md`
   (`differential-test-bed/atomlists/l1-merge-gate.txt` with
   `L1_CONSUME_REINSTALL=1`), plus `l1-merge-from-binpkg.sh` if the
   coordinator wants the full merge report. Expect: green.

## Residues (all deliberate cuts; none blocks this slice)

- R1 (needs owner ruling): the `--pretend`-end notice (`post_emerge.py:
  112-117`). Real prints it; porting it needs a corpus plan (whole
  pretend corpus gains 3 lines wherever the hermetic count is nonzero)
  and a state-write policy for the shared fixture ROOT.
- R2: the `--ask` + `--read-news` "read while calculating?" prompt and
  `eselect news read` spawn (`actions.py:4271-4288`). Portuale has no
  eselect integration and must never block on an interactive prompt.
- R3: post notices after uninstall actions (real `actions.py:4166-4175`
  calls `post_emerge` there too), on the `--resume` path (returns before
  the pre-resolution point), on the remote-exec early return, and on
  merge-failure early returns (real prints regardless of retval).
- R4: `emerge --check-news` still has no FEATURES gate, matching real
  (`actions.py:3844`) — unchanged by this slice, noted so a future
  reader doesn't "fix" the asymmetry.

## Fix round 1 (2026-09-27; portuale ad753912, pmtest 55137b2)

Closed review Important issues #1–#3 (the three missing print points),
plus Minor #4, #5(doc), #6 (quiet + resolve-fail pins) and #7 (line
refs). Residue status after this round: R1 adapts (see below); R2
unchanged (eselect prompt/spawn still out); R3 shrinks to `--resume`,
the remote-exec early return, and `post_emerge`'s info/cfg-file halves
after uninstalls (the news tail is now ported); R4 unchanged.

- `--pretend` end (`post_emerge.py:112-117`): new
  `pretend_end_news_notice` helper, called at every `--pretend` exit
  of the build path — success, all three resolve-error arms, and the
  autounmask-only/autounmask-changes/blocker/slot-conflict/
  buildpkgonly-unsat/circular/abort returns. Real reaches this arm
  for every `action_build` retval (`run_action` calls `post_emerge`
  unconditionally, `actions.py:4289-4297`), so failed `--pretend`
  resolves print it too (pinned). Usage-error returns above the
  resolve stay silent (real never reaches `post_emerge` there either);
  `--json` stays silent (portuale-only format, must stay parseable).
  Corpus drift: NONE — the whole pretend suite runs with the
  calling-env FEATURES stripped, so the `news` ∈ FEATURES gate keeps
  every existing pretend run silent (full suite: 0 `corpus drift`
  lines). State writes: real's `count_unread_news` runs with
  `update=True`, so real DOES write `.unread`/`.skip` under ROOT on a
  pretend run too — portuale writing them is faithful, not extra; new
  pins use isolated tmp ROOTs, so the git-tracked fixture tree is
  untouched (the old R1 pollution fear does not materialize).
- Failed merge (`post_emerge.py:104-108,155`): new
  `merge_failure_news_notice(changed, …)` on both merge arms (source
  + getbinpkg-plan), with `changed = unmerged.len() <
  entries.len()` — real's `_pkgs_changed` gate, reusing the
  already-computed `entries_not_merged` (same structural
  approximation as the success path's `!entries.is_empty()`).
  Digest/missing-failure early returns stay silent (nothing merged —
  verified: the #174 truncated-binpkg test still passes unmodified).
  Pinned both directions: `[schedok, schedbad]` → rc 1, schedok
  landed, notice twice (pre + post-failure, post after the last
  `>>>`); `[schedbad, schedok]` → rc 1, nothing landed, notice once.
- Uninstall actions (`actions.py:4164-4175`): the notice now ends
  `execute_unmerge` (after the preserved-libs notice, mirroring
  `post_emerge`'s tail order) — the single funnel for every real
  clean/depclean/prune/unmerge/rage-clean removal. `repos` + resolved
  config threaded through `run_unmerge_pretend`,
  `run_depclean_pretend`, `run_prune_pretend`,
  `run_prune_nodeps_pretend`, `run_clean_pretend`,
  `run_prune_nodeps_or_clean` (+2 test call sites). Gates are
  structural: `pretend` returns before any removal (so `-p -C`
  stays silent — pinned); empty selections return before removal
  (pinned by unit test); `run_deselect` never reaches
  `execute_unmerge` (real excludes deselect). Failure nuance, ported
  exactly: loop failure at `idx > 0`, `deselect_from_world` / info-
  regen failures (removals already landed) print; a first-removal
  failure stays silent.
- Minor #4: the all-zero gate moved INSIDE
  `print_news_notifications` (like real `news.py:526`); both callers
  simplified. Minor #5: the resolve-time-config reuse is now noted
  in the post-merge comment (real reloads, `post_emerge.py:92-95` —
  still negligible, still not ported). Minor #6: pinned
  `--quiet`-still-prints (`-q --buildpkgonly`, notice once) and
  resolve-failure-still-prints-pre-notice (plain `emerge
  dev-libs/anyofunresolvable`, rc 1, notice once before
  `Calculating...`); empty-mergelist-no-post is NOT pinned (no CLI
  shape produces an empty mergelist on the build path — an installed
  atom reinstalls; the gate stays structural). Minor #7: citations
  fixed to `actions.py:4266-4281`.

Tests: 3 new Rust unit tests (`pretend_end_notice_prints_only_
under_pretend`, `merge_failure_notice_prints_only_when_the_vdb_
changed`, `unmerge_end_notice_skips_an_empty_removal_list` — each
pins its gate through state-dir absence/presence on an isolated
ROOT) + 7 new contract pins in `test_portuale.py` (one `_news_env`
helper). Judgment call reused from round 0, unchanged: the
`--ask`+`--read-news` eselect prompt/spawn stays out (R2).

Gates (all green; no bless): `cargo fmt --check` clean; `cargo
clippy --release --all-targets` zero warnings (toolchain
`stable-x86_64-unknown-linux-gnu`); `cargo test --release` rc 0
(26 suites green, incl. the 3 new tests); pmtest full suite
(registry-rebuilt release binary) `2078 passed / 37 skipped / 8
xfailed / 0 failed` (rc 0, 517 s), basetemp
`/var/tmp/pmtest-n196fix-full`, `-p no:cacheprovider`, zero
`corpus drift` lines. Logs: `/tmp/opencode/n196fix/cargo-test.log`,
`/tmp/opencode/n196fix/pmtest-full.log`. `git status` clean apart
from the two intended files; no fixture pollution.

BED-PENDING (coordinator only; run from the pmtest worktree):
1. `differential-test-bed/run/l32-lifecycle.sh` — expect the notice
   block in portuale-side logs at every real print point now
   (pre-resolution, post-merge incl. failures with partial merges,
   post-unmerge, `--pretend` end); the count (15 unread gentoo
   items per m185, evolving as news ages) may need normalising.
2. Merge-path safety gate per `docs/agent-context.md`
   (`differential-test-bed/atomlists/l1-merge-gate.txt` with
   `L1_CONSUME_REINSTALL=1`), plus `l1-merge-from-binpkg.sh` — the
   merge-failure arms changed, so the failure shapes must stay byte
   exact (the digest-failure early return is untouched, but confirm).
