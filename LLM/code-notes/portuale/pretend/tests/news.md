# crates/portuale/src/pretend/tests/news.rs
News parsing, selection, notices, and the read-news prompt arms.

## fn news_item_format_and_valid_gate_the_1x_vs_2x_atom_split
- Portage's `DisplayInstalledRestriction.isValid`'s own `eapi="0"`/`eapi="5"` split, as a field-level (not crate EAPI parametrization) gate: 1.x rejects `:slot`/`:sub`/`:=`/`:*`/`[use]`, 2.x accepts them.
- Format extraction. First matching header wins (Portage's break-on-first-match).
- 2.x: everything portuale parses is legal (superset grammar). 1.x: plain only -- every slot / slot-op / use form is invalid. Both formats reject a malformed atom; a missing format header rejects too.

## fn filesystem_news_selector_evaluates_one_repo_through_the_seam
- The director's news slot carries the production `--check-news` read: one `FilesystemNews` per repo evaluates validity + relevance over `metadata/news`, minus the already-read/skipped ids. A relevant item (installed atom matches the vdb) lands in `unread` (and permanently in `skip`); an irrelevant one (atom matches nothing installed) lands in neither; a repo without a `metadata/news` dir evaluates to `None` (nothing pending).
- A repo with no news dir is nothing pending, never a panic.

## fn news_profile_and_keyword_restrictions_match_like_portage
- Backlog #258: Portage's `DisplayProfileRestriction` / `DisplayKeywordRestriction` (`news.py:388-423`). The l3 `l3-20260928T195717Z` container probe counted 15 gentoo items for Portage and 34 for portuale on `default/linux/amd64/23.0/systemd`; the 19 extras were all profile/keyword-restricted to other profiles and arches.
- Profile: exact, 2.x `/*` prefix, other profiles, 1.x `/*`. Keyword: `ARCH` verbatim, OR'd within the kind. AND across kinds: a matching keyword does not rescue a non-matching profile. No profile at all matches no profile restriction.
- Portage's `DisplayProfileRestriction.isValid`: a 1.x value with a `*` anywhere but a trailing `/*` fails the whole item; a 2.x one never does.

## fn news_profile_path_strips_the_main_repo_profiles_prefix
- Portage's `NewsManager.__init__` (`news.py:88-98`) with the fixture tree's own `etc/portage/make.profile -> ../../repo/profiles/default`: the realpath minus `<main repo>/profiles/`.
- No main repo, or no make.profile: no profile.

## fn news_counts_follow_portage_count_unread_news_gates
- Portage's `count_unread_news` (`news.py:470-479`): no profile path at all counts nothing. Portage's `updateItems` (`news.py:121-133`): an unusable state directory returns before the news dir is read, and `getUnreadItems` counts `.unread` as it stands -- `grabfile`, so comment lines and trailing comments drop out.
- A stale `.unread` in a state directory the caller cannot write still counts, even for a repo with no `metadata/news`. `ensure_dirs(mode=0o074)` adds only group/other bits, so an owner-read-only directory stays unwritable for a non-root caller (root writes anyway, so the check is skipped there).

## fn news_state_files_get_the_group_write_bit_like_portage
- Portage's `apply_secpass_permissions(..., mode=0o064, mask=0)` on each rewritten state file and `ensure_dirs(..., mode=0o074, mask=0)` on the directory (`news.py:121-200`): the bits are OR'd onto whatever the umask left. The l3 probe showed Portage's files at 0664 and portuale's at 0644.

## fn news_notice_gate_needs_news_in_resolved_features
- Portage's `display_news_notification` (`post_emerge.py:38`): the notice is skipped wholesale unless `news` is in the resolved `FEATURES` (no evaluation, no output). Same `Config::default` + `other_vars` shape `buildpkg_comes_from_the_cli_then_...` uses for its own feature gate.

## fn unread_news_counts_reuses_the_check_news_evaluation
- Backlog #196 S1's "no second news parser": the count notice reads through the same `FilesystemNews` evaluation (and the same `.unread`/`.skip` write-back) `--check-news` uses, so the hermetic fixture tree counts exactly what `test_check_news_counts_unread_relevant_items` pins over in the contract suite (5 relevant testrepo items). The ROOT is a temp dir sharing the fixtures' own vdb read-only (`Display-If-Installed` needs it) with a fresh `var/lib`, the same isolation `_check_news_isolated_root` gives the contract tests -- nothing is ever written into the git-tracked fixtures tree itself.
- The write-back landed in the temp ROOT (sorted, one id per line), and a second evaluation stays sticky at the same count (Portage's `getUnreadItems`'s `len(.unread)`).

## fn pretend_end_notice_prints_only_under_pretend
- Backlog #196 fix round 1: Portage's `post_emerge.py:112-117` prints the notice at the end of a `--pretend` run (for every `action_build` retval) and never otherwise. `pretend=false` evaluates nothing (no state dir appears); `pretend=true` evaluates and writes back the hermetic 5-count, exactly like `--check-news` does.

## fn merge_failure_notice_prints_only_when_the_vdb_changed
- Backlog #196 fix round 1: Portage's `post_emerge.py:155` prints the notice regardless of retval, but only when the vdb changed (`:112-117`). A total failure (`changed=false`) evaluates nothing; a partial one (`changed=true`) evaluates and writes back the hermetic 5-count.

## fn display_news_notice_reports_whether_it_printed
- Backlog #231 (c): the `--ask` + `--read-news` prompt fires only when the notice actually printed (Portage's `actions.py:4268-4281` short-circuits on `display_news_notification`'s falsy return), so the helper returns Portage's own boolean. Gated off: `false`, no evaluation. Gated on: `true`, the hermetic 5-count evaluated and written back.

## fn ask_read_news_yes_spawns_eselect_and_continues
- Backlog #231 (c): Portage's `_emerge/actions.py:4266-4281` -- once the pre-resolution notice printed, `--ask --read-news` prompts "Would you like to read the news items while calculating dependencies?" and spawns `eselect news read` on "Yes", then continues into the resolve. Answering "Yes" here runs the stub (argv pinned) and the declined merge prompt still exits 130 with `Quitting.`.
- The stub dir is separate from the run's base dir so the marker survives the cleanup below for the assertion.

## fn ask_read_news_no_skips_eselect_and_continues
- Backlog #231 (c), the "No" arm: Portage's `== "Yes"` check fails, so eselect is never spawned and the run continues into the resolve; the declined merge prompt still exits 130.

## fn ask_read_news_true_spellings_prompt_like_portage
- Backlog #234 (b): Portage's `true_y_or_n` (`main.py:321-322,625`) accepts `--ask=True` / `--read-news=True` and normalises them to the bare flags (`main.py:802-805,950-953`), so the pair prompts exactly like `--ask --read-news`.
