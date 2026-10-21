# crates/portage-repo/src/reinstall.rs
When an installed package needs a reinstall (changed deps, slot, repo, USE).

## fn canonical_dep_key
- Ports Portage `_changed_deps` (`uselist=pkg.use.enabled`, used for both sides of the comparison) with `with_bdeps` respected exactly like `enqueue_dependencies`' dep-key list (Portage `depgraph.py`'s `if self._dynamic_config.myparams.get("bdeps") in ("y", "auto"): depvars = Package._dep_keys ... else: depvars = Package._runtime_keys`).
- `portage_use_reduce::use_reduce_structured` ports Portage `use_reduce`'s own `flat=False` bracket-optimization pass. Each serialized atom token gets Portage `Atom.evaluate_conditionals` (`evaluate_atom_conditionals`, resolving 2-/4-style USE-deps against the installed package's own `USE`) and Portage `strip_slots` (`strip_slot_operator_one`); Portage `strip_libc_deps` iterates just the outer per-key list, never recursing into `||`/nested groups, and matches by `(category, package)` identity (`libc_provider_cps`). The pre-slice merged-into-one-string comparison missed `DEPEND`-to-`RDEPEND` moves.
- A vdb-side string that fails to parse -- including an individual atom token Portage `use_reduce(token_class=Atom)` would reject -- counts as "changed" unconditionally, matching Portage's `except InvalidDependString: changed = True`; the repo side is assumed always well-formed (Portage has no equivalent fallback there).

## fn binary_deps_changed
- Portage `create_depgraph_params.py:196-203`; Portage `_wrapped_select_pkg_highest_available_imp` (`depgraph.py:8288-8300`): `self._changed_deps(binpkg)` -> `ignored_binaries[pkg]["changed_deps"] = True; continue`. A binary whose metadata can't be read, or whose deps parse but the ebuild's don't, stays "unchanged" (`false`); an unparsable binary dep string is "changed" (`true`), matching Portage's `except InvalidDependString`.

## fn deps_changed
- Vdb side mirrors Portage `_changed_deps`'s `built_deps` loop; repo side mirrors its `unbuilt_deps` loop. Per-key comparison mirrors `built_deps` vs `unbuilt_deps` element-wise, each a per-key struct.
- Portage `_changed_deps` compares its per-key `use_reduce(token_class=Atom)` output key by key (`built_deps != unbuilt_deps`, each a list of one struct per dep key), after `strip_slots` and `strip_libc_deps`. `strip_slot_operator_slots` (Portage `strip_slots`) keeps a built `cat/pkg:2=` vdb dep from spuriously differing from the ebuild's own `cat/pkg:=`.
- The comparison is structured, matching Portage `_changed_deps` (`built_deps != unbuilt_deps`, each a list of one `use_reduce(token_class=Atom)` -- i.e. `flat=False` -- struct per dep key): a `||`-group reorder (`|| ( a b )` -> `|| ( b a )`) and -- since Portage compares Python lists, which are order-sensitive everywhere -- a plain `RDEPEND="a b"` -> `"b a"` reorder both register as changed, while redundant-bracket differences (`a b` vs `( a b )`) do not.

## fn split_slot
- Portage `portage.versions._pkg_str`'s slot-parsing branch. Splits a raw `SLOT` string into `(slot, sub_slot)`: `SLOT="main/sub"`, `sub_slot` defaulting to the slot itself when no `/` is present (Portage `portage.versions._pkg_str`'s own slot-parsing branch).

## fn read_vdb_slot
- Portage `vardbapi`'s `SLOT` file is written verbatim from the same `SLOT` variable the ebuild declares; identical format to what `list_candidates` parses repo-side.

## fn slot_changed
- Portage `depgraph.py`'s `_changed_slot`: `ebuild = self._equiv_ebuild(pkg); return ebuild is not None and (ebuild.slot, ebuild.sub_slot) != (pkg.slot, pkg.sub_slot)`.
- Known, documented scope cut: Portage's consumers of `_changed_slot` live deep inside binary-package/slot-operator-rebuild scheduling portuale has none of (`_slot_operator_replace_installed`, `built`/`useoldpkg` branches in `_privileged_size_fetch`/candidate selection) -- rejecting a matched installed candidate and, depending on context, either aborting the search or continuing to look for a binary package with the right `SLOT`. Captures the dominant real-world effect (a package whose `SLOT` metadata changed upstream, e.g. an ABI-bump `SLOT="0"` -> `SLOT="0/2"`, gets flagged for reinstall) without replicating Portage's considerably messier, binpkg-entangled control flow. Same "re-read metadata portuale's general `Candidate` model doesn't carry" approach `deps_changed` uses for `DEPEND`/`RDEPEND`.

## fn new_repo_changed
- Portage `depgraph.py`: `"--newrepo" in myopts and myeb.repo != pkg.repo` / `pkg.repo != inst_pkg.repo`. A vdb entry with no `repository` file at all (Portage predates this tracking, or a hand-installed/synthetic entry) is treated as Portage `portage.versions._unknown_repo` (`"__unknown__"`). Unlike `slot_changed`/`deps_changed`'s missing-data fallbacks, Portage's comparison has no tolerant fallback at all. Resolved `Candidate::repo_name` at each of the two call sites, not re-derived here the way `slot_changed`'s re-lookup works, since the caller already has it in hand.

## struct RebuiltBinaryChangedArgs::binary_build_time
- Portage `matched_packages`' `built_pkg` is whichever the pool actually offers; Portage's own falsy guard (`bug #306659`).

## fn rebuilt_binary_changed
- Portage `depgraph.py`'s own reinstall trigger (lines ~8394-8429, confirmed by reading it), comparing a binary candidate's own `BUILD_TIME` against the already-installed package's own recorded `BUILD_TIME` -- "replace installed packages with binary packages that have been rebuilt" (Portage `main.py`'s own help text). The common real-world case is a same-version binary rebuilt against updated dependencies (a toolchain/ABI bump), not a version change at all. Portage's own "skip the check if a newer source (unbuilt) candidate exists" branch has no equivalent here: this function is only ever called once the caller has already established `version` is both the best visible candidate and what's already installed, so nothing newer (built or unbuilt) can exist by construction -- see the two call sites in `resolve_pretend`. Portage asymmetry, not a simplification: "use `--rebuilt-binaries-timestamp 0` if you want only newer binaries pulled in" vs "don't care ... this is for closely tracking a binhost". A missing local/remote `BUILD_TIME` must never cause a spurious reinstall (an empty string is falsy in Python too; Portage's own comment on `bug #306659`). Both call sites gate this on `usepkg || usepkgonly`, not `usepkg` alone -- Portage `built_pkg` only ever exists in `matched_packages` when the binary db is part of the candidate pool at all, matching Portage `depgraph.py`'s own `dbs` construction (see `resolve_pretend`'s doc on `--usepkg`/`--usepkgonly`).

## fn best_binary_build_time
- What Portage `matched_packages`' `built_pkg.build_time` would be (local `$PKGDIR` or a `--getbinpkg` remote binhost).

## fn ebuild_visible_at
- Portage queries `=cat/pkg-ver` (no slot comparison). Portage `_equiv_ebuild_visible(pkg)` (`depgraph.py:7399`) at a bare version: whether a visible ebuild still exists in the tree at `version`. Also read at render time by portuale's slot-collision `need_rebuild` check (Portage `slot_collision.py`'s `_equiv_ebuild_visible(ppkg)` gate).

## fn binary_reinstall_warranted
- Without `--rebuilt-binaries`: the `identical_binary` + `_equiv_ebuild_visible` rejection of an installed built instance (`depgraph.py:7999-8030`). Portage only rejects the installed instance this way when `not avoid_update` (`avoid_update = "--update" not in myopts`, `depgraph.py:7826`); without `--update` Portage's own `_select_pkg_highest_available` final `if avoid_update:` step keeps the installed package, so `update` gates this branch. (With an identical `BUILD_TIME` -- `identical_binary` -- or a still-visible ebuild, the installed instance is left alone.) The `update`-less "bare top-level always replaces" case is a separate mechanism (`is_top_level && !selective`), not this one.

## fn valid_iuse
- `USE_EXPAND_IMPLICIT`-derived `elibc_*`/`kernel_*`/... and `IUSE_IMPLICIT` flags. A USE-dep like `foo[elibc_glibc]` matches a `foo` that never lists `elibc_glibc` in its own `IUSE`. Used only for a USE-dep's own `.required`/`(+)`/`(-)` check (`use_deps_satisfied`, portage-dep) -- deliberately NOT for `--newuse`'s own IUSE-presence diff. An implicit flag would otherwise read as "newly added to IUSE" and spuriously trigger a reinstall. Cheap no-op clone when `iuse_effective` is empty (every fixture/profile without an explicit `USE_EXPAND_IMPLICIT`).

## fn candidate_iuse_and_use
- Portage `_iter_match_pkgs` checks an atom's `[use]` deps against `pkg.use.enabled` (the baked set) for a built package. A binary candidate has no `repo_location` md5-cache entry to read, and -- unlike an ebuild -- its USE isn't recomputed from the profile: it carries the flags it was actually built with (`Packages` `USE:` field, `binary_use`) over its own recorded `IUSE`.
- Same "absence is real, not an error" precedent `read_vdb_flag_set` sets for a missing vdb IUSE/USE file: a missing IUSE key is a valid "declares no USE flags at all" state, unlike a missing md5-cache entry entirely, which means "can't tell anything about this candidate". Owned inside a fresh `Rc` (no shared source exists for the baked set): one small allocation, no content clone saved or added versus before #103.

## fn reinstall_flags_for_use_change
- Ports both the `newuse` and `elif changed_use` branches of Portage `depgraph.py`'s `_reinstall_for_flags`. Only ever called when at least one of `newuse`/`changed_use` is set; Portage checks `newuse` first (`if newuse or (...): ... elif changed_use or (...): ...`, which checks `newuse` first).
- Both branches share one term: `(orig_iuse∩orig_use) ^ (cur_iuse∩cur_use)` (which flags were enabled, among those actually declared on each side) -- this alone is the entire `--changed-use` formula, deliberately narrower than `--newuse`: it only ever reacts to an enablement change of a flag that exists in IUSE on both sides, never to IUSE gaining or losing a flag entirely. `--newuse` adds `(orig_iuse ^ cur_iuse) - forced_flags` on top -- Portage's own `flags -= forced_flags` line sits between the `^=` and the final `|=`, so a flag forced on/off by the profile never by itself triggers a `--newuse` reinstall, but can still contribute via the shared enablement term above.

## fn binary_deps_changed (key slimming, backlog #441 S1 / #457)
- The memo key no longer carries the libc provider set or dep-string clones: `vdb_generation` (cheap under the freeze scope, `generation()` is epoch-memoised) stands in for the provider set (a pure function of VDB state, same discipline as `installed_candidates_by_cp`), and the built-USE set and five recorded dep strings ride as order-independent/len-prefixed `u64` fingerprints (`fingerprints.rs` shape). Key build per call is small clones plus string walks, no VDB scan, no sorts, no big clones.
