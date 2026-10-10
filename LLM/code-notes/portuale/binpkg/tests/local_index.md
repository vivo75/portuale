# crates/portuale/src/binpkg/tests/local_index.rs
Local `$PKGDIR` scan tests: fast path, stale vouch, reindex, orphans.

## fn populate_local_pkgdir_synthesizes_a_packages_style_entry_per_binpkg_file
- The fixture dir holds a `.tbz2` and a `.gpkg.tar` plus an index-only `Packages` file covering unrelated CPVs. The scan walks unconditionally (Portage's `_populate_local` default `reindex=True`), so both files are freshly parsed from disk regardless of what the index claims.
- `repository` maps to the `Packages` field name `REPO`. Entries are CPV-sorted for a deterministic candidate pool.

## fn populate_local_pkgdir_skips_invalid_files_and_keeps_the_pool
- Ports Portage's `_populate_local` two classes at the scan boundary (`bintree.py:1185-1199`): an `Invalid` file is skipped (with Portage's warning, whose exact line is pinned by the CLI contract test, not here) and the rest of the pool still resolves.

## fn populate_local_pkgdir_aborts_on_fatal_files
- The other class: a `Fatal` file (Portage would traceback) still stops the whole scan.

## fn populate_local_pkgdir_trusts_an_index_entry_even_when_stale
- Backlog #174: Portage's default (`FEATURES=pkgdir-index-trusted`, in Portage's `make.globals` default `FEATURES`) runs `_populate_local` with `reindex=False` -- the `<pkgdir>/Packages` stanza vouches for the file even when its `_mtime_`/`SIZE` no longer agree with the live file, and the file's bytes are first touched at merge by `BinpkgVerifier`. So a stale stanza is reused verbatim here (proven by a bogus SLOT the index claims that the archive itself does not carry) instead of being re-derived; the corruption surfaces at merge via `verify_binpkg_against_index` with Portage's `!!! Digest verification failed:` block. This test previously asserted the reindex behaviour (stale means re-parse), which is only Portage with an explicit `-pkgdir-index-trusted`.
- Portage's default `FEATURES=pkgdir-index-trusted` is in Portage's `make.globals`' own default `FEATURES`; the file's own bytes are first touched at merge by `BinpkgVerifier`, and the stale stanza is reused verbatim, proven by a bogus SLOT the index claims that the real `.tbz2` itself does not carry.

## fn populate_local_pkgdir_accepts_a_truncated_binpkg_the_index_vouches_for
- Backlog #174 (the F2 shape): Portage's `emerge --oneshot --usepkgonly l32/faultpkg` with the gpkg truncated to half selects the binary -- the `<pkgdir>/Packages` stanza (written by `--regen` before the truncation, so its `SIZE` is the whole-file size) vouches for it at scan under Portage's default `FEATURES=pkgdir-index-trusted`, and the failure surfaces at merge via `BinpkgVerifier`. Portuale used to fully parse the container at scan, hit `unexpected EOF during skip`, and drop the candidate with `!!! Invalid binary package`, ending in `there are no ebuilds to satisfy`.

## fn populate_local_pkgdir_vouches_by_category_plus_basename
- Backlog #174 review: the index-vouched branch matches the stanza by category + basename (`CPV` starting with `{category}/`), not the first same-basename stanza. The same `PF` can exist in two categories with the same basename (`cat-a/same-1.0.tbz2` vs `cat-b/same-1.0.tbz2`); each file reuses its own category's stanza. The vouched path never parses the container, so the files' bytes are irrelevant.

## fn populate_local_pkgdir_reinjects_a_removed_files_index_stanza
- Backlog #187 (the bed's F3a shape): the local `Packages` still lists a binary whose file was removed while a sibling binpkg is still on disk. Portage trusts the index under its default `FEATURES=pkgdir-index-trusted` (`_populate_local(reindex=False)` `cpv_inject`s every stanza without stating any file, `bintree.py:1057-1064`), so the removed package stays a selectable binary and only fails at merge (`BinpkgVerifier`). Portuale's file-driven scan used to drop the orphan outright -- and with any other file present the non-empty scan shadowed the raw-index fallback too, so the resolver silently fell back to the ebuild (rc 0) where Portage aborts (rc 1).

## fn populate_local_pkgdir_reindex_reparses_a_changed_file
- Backlog #199: Portage's `_populate_local(reindex=True)` (explicit `FEATURES=-pkgdir-index-trusted`, `bintree.py:1110-1136`) only reuses an index stanza whose `_mtime_` and `SIZE` still agree with the live file's `lstat(2)` and which carries Portage's `minimum_keys`; a changed file is re-parsed from disk (`bintree.py:1179-1351`). A stale stanza (bogus `_mtime_`, proven by a fabricated SLOT the archive does not carry) is trusted verbatim in trusted mode (backlog #174) but re-derived in reindex mode.
- A stale stanza (bogus `_mtime_`, proven stale by a fabricated SLOT the real archive does not carry) is trusted verbatim with `trust_index == true` (backlog #174) but re-derived with `false`: the real SLOT, SIZE and `_mtime_` come back.

## fn populate_local_pkgdir_reindex_reparses_a_key_incomplete_stanza
- Backlog #199 fix round 1: Portage's fast path reuses a stanza only when the stat fields agree and every `minimum_keys` entry is present (`bintree.py:1115-1126`: `_mtime_`, `SIZE`, then `if not minimum_keys.difference(d)`). A stanza whose `_mtime_`/`SIZE` match but which lacks a minimum key (no `EAPI` here) is re-parsed under `FEATURES=-pkgdir-index-trusted` (`trust_index == false`, Portage's `_populate_local(reindex=True)`); in trusted mode it is still vouched verbatim via the stale-vouch arm (backlog #174), so the trusted default is unchanged.

## fn populate_local_pkgdir_reindex_drops_a_removed_files_index_stanza
- Backlog #199: Portage's `_populate_local(reindex=True)` deletes every index stanza with no live file (`bintree.py:1353-1356`: `del metadata[instance_key]` for `instance_key not in pkg_paths`). Removed stanzas re-inject in trusted mode (backlog #187) but drop in reindex mode; the survivor's stanza carries every `minimum_keys` entry with live `_mtime_`/`SIZE`, so the fast path vouches it verbatim in both modes.

## fn populate_local_pkgdir_parses_unindexed_files_in_both_modes
- Backlog #199: Portage's `_populate_local(reindex=True)` parses every file the fast path missed (`bintree.py:1179-1351`). Portuale's walk runs in trusted mode too (unlike Portage's `reindex=False`, which never walks; kept so a `Packages`-less `$PKGDIR` still resolves), so an unindexed file parses identically either way.
