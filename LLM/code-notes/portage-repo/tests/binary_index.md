# crates/portage-repo/src/tests/binary_index.rs
Binary candidate tests: local/remote indexes, multi-instance dedup, slot triggers.

## fn build_local_binpkg_index_uses_the_cli_scanned_binpkgs
- Backlog #123: honours the CLI layer's `$PKGDIR` directory scan (`config.scanned_binpkgs`) when one exists.
- History: the old per-inputs memo is gone; the index is built once per resolution and threaded as an `Arc`, never re-derived from `&Config` at a call site.

## fn list_remote_binary_candidates_reads_each_binrepos_own_packages_index
- `dev-libs/remotebinpkg` exists only in the binhost's Packages index (no ebuild, no local `$PKGDIR` entry); a binhost candidate is marked remote with the index's own REPO field.
- An already-local cpv+version is dropped as "already downloaded" (Portage `bintree.isremote`); passing the binhost index as its own `local` makes every entry look already-local, so nothing is returned.

## fn dedup_binary_instances_keeps_newest_build_time_per_group
- Bare atom: newest BUILD_TIME of the group wins, regardless of input order or BUILD_ID.
- Atom `[feat]`: the satisfying instance wins even when a newer instance in the same group does not.

## fn multi_instance_local_build_does_not_shadow_a_newer_remote_build
- B1 bug 2: a version already present in the local `$PKGDIR` hid every remote build of that version, including a newer `BUILD_ID`. Portage `bintree.isremote` is per instance (`cpv` + `build_id` under binpkg-multi-instance), so the newer remote build must stay a candidate and the stale local one must not shadow it.

## fn multi_instance_download_lookup_returns_the_newest_build
- B1 bug 3: `find_remote_binpkg` matched by `CPV` only and returned the first `Packages` entry (the oldest build, listed first), so `-7` was planned but `-6` fetched into `$PKGDIR` (which then triggered bug 2). With no `BUILD_ID` to ask for, the lookup must at least return the newest build (highest `BUILD_TIME`, Portage `_cmp_cpv` order); the fix adds a `build_id` parameter and this stays the fallback.

## fn quickpkg_direct_index_entries_reads_the_source_root_vdb
- Fixture `fixtures/quickpkgroot` holds one installed package, `dev-libs/quickpkgdirectpkg-1.0` with RDEPEND on `dev-libs/newpkg`.
- `fixtures_root()/distfiles` has no vdb at all, proving the empty-list tolerance.

## fn changed_deps_and_changed_slot_combine_in_one_reinstall_outcome
- Fixture `dev-libs/changedslotpkg`: vdb has both a stale RDEPEND (samepkg vs the ebuild's newpkg) and a stale SLOT ("0" vs "0/2").

## fn fixture_repo_constrained_atom_finds_the_named_repo_only
- Fixture `dev-libs/overlayonlypkg` exists only in the overlay repo: proves portage-repo's candidate strings carry `::reponame` end to end, not just that portage-dep parses and matches it in isolation.
