# crates/portage-repo/src/tests/reinstall.rs
Reinstall-trigger pins: binary identity, --newuse/--changed-use/--changed-deps/--newrepo.

## fn ebuild_visible_at_and_binary_reinstall_warranted_match_portage_identical_binary
- `ebuild_visible_at` asks the tree, not the pool: `newpkg` has a visible ebuild, `rebuiltbinarypkg` does not (the ebuild-gone bolt).
- Portage `identical_binary` (same BUILD_TIME) vs `identical_binary`+`_equiv_ebuild_visible` (differing, ebuild gone, --update): the latter only fires under `--update` (Portage's `not avoid_update`).

## fn newuse_forced_flags_suppresses_a_spurious_reinstall
- Fixture: `dev-libs/usemaskreinstallpkg` installed with an empty vdb IUSE; its ebuild now declares `IUSE="masked_newly_added_flag"`, which `fixtures/repo/profiles/base/use.mask` masks off, so the flag is never enabled before or after.
- Without `reinstall_flags_for_use_change`'s `flags -= forced_flags` step (Portage depgraph.py's own line, ported exactly), this would spuriously reinstall just because the flag now exists in IUSE.

## fn changed_deps_detects_an_atom_moved_between_two_dep_keys
- Fixture: `dev-libs/movedkeydepspkg` recorded `RDEPEND="dev-libs/samepkg"`; its ebuild has that atom in PDEPEND instead, nothing else on either side. The pre-slice merged-into-one-string comparison saw no change; Portage `_changed_deps` compares `built_deps` to `unbuilt_deps` element-wise.

## fn changed_deps_ignores_a_built_slot_operator_deps_resolved_slot
- Fixture: `dev-libs/slotopdepspkg` ebuild `RDEPEND="dev-libs/slotoptarget:="`; its vdb recorded the built `dev-libs/slotoptarget:2=` (the slot it merged against). Without the normalization every `:=` dep would spuriously trigger a --changed-deps reinstall.

## fn changed_deps_detects_an_any_of_group_reorder
- Fixture: `dev-libs/anyofreorderdepspkg` vdb `RDEPEND="|| ( dev-libs/reorderdepa dev-libs/reorderdepb )"`, ebuild swaps the alternatives. Portage `_changed_deps` compares structured `use_reduce(token_class=Atom)` output; the pre-slice flat set comparison missed it.

## fn changed_deps_detects_a_plain_atom_reorder_but_not_a_redundant_bracket
- Fixture pair: `orderchangeddepspkg` (vdb `reorderdepa reorderdepb`, ebuild `reorderdepb reorderdepa`) vs `redundantbracketdepspkg` (vdb `reorderdepa reorderdepb`, ebuild `( dev-libs/reorderdepa dev-libs/reorderdepb )`).

## fn changed_deps_report_is_silent_when_changed_deps_is_also_given
- Portage: "This is completely silent... if --changed-deps ... is enabled" -- the Vec stays empty even though the underlying change reinstalls (proven by the second assertion).

## fn changed_deps_ignores_a_libc_only_dependency_change
- Fixture: `dev-libs/libcnoisepkg` vdb RDEPEND names sys-libs/glibc, ebuild names sys-libs/musl; both are virtual/libc providers (the fixture vdb's virtual/libc entry RDEPENDs `|| ( sys-libs/glibc sys-libs/musl )`). Portage `strip_libc_deps` strips both, leaving only identical `dev-libs/samepkg`.

## fn newrepo_fires_via_the_unknown_repo_sentinel_when_no_repository_file_was_ever_recorded
- Portage predates this tracking (or a hand-installed/synthetic entry): `portage.versions._unknown_repo` (`__unknown__`) never equals a repo name, so --newrepo fires though nothing changed.

## fn newuse_vs_changed_use_diverge_on_a_newly_added_iuse_flag
- Fixture: `dev-libs/changedusepkg` installed with empty vdb IUSE; its ebuild declares `IUSE="brandnewflag"` (unmasked, not globally enabled). --newuse's presence-diff term reacts to the flag existing in IUSE; --changed-use's narrower formula only looks at enablement of flags declared on both sides.
