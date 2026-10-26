# crates/portuale/src/global_updates/binpkg.rs
The binpkg half of `_do_global_updates` (backlog #576): `move_ent` /
`move_slot_ent` over `$PKGDIR` files and `update_ents` over their embedded
`*DEPEND` metadata, plus the `Packages` index rewrite.

## fn apply_binpkg_moves
- Ports the bindb move loop (`_global_updates.py:155-206`): iterate the repo
  map in order, skip `DEFAULT` and empty lists, build the per-repo closure
  over `RepoMatch::matches`, and count `move` copies and `slotmove`
  rewrites separately. The driver prints `%`/`S` from the report counts.
- The whole binpkg half is skipped when `$PKGDIR` is not writable
  (`writable`, `bintree.py:152`; the `first_existing` ancestor walk stays
  local, the one-probe `W_OK` check is the shared
  `privileges::path_writable`). Per-file `W_OK` refusals skip the entry
  the same way (`bintree.py:702`).
- No vdb lock: binpkgs live outside the vdb. No `bindb.populate()` either:
  the entry list comes from the `Packages` index (`cpv_all` order), which
  is what a trusted-index populate leaves behind.
- Deliberate cuts: no `isvalidatom(newcp, eapi=...)` per-entry check (the
  parse-time `bare_cp` check already rejected malformed names, same cut as
  the vdb moves); no `_file_permissions` gid/mode fixup after inject
  (group/mode bits ride the file copy); the moved gpkg's `new_basename`
  has no build id while the `aux_update` basename gains one (Portage
  quirks, mirrored exactly — see `new_pf_for_basename`).

## fn move_binpkg
- Ports `bintree.move_ent` (`bintree.py:639-831`): candidates are indexed
  binpkgs whose `CPV` is on `oldcp` (`PROVIDE`-virtual matches cannot occur
  — matching is on the stored `CPV`, not the atom index); `repository`
  gating reads the file's own `repository` key (the aux truth Portage
  matches against), falling back to the stanza `REPO`.
- Faithful quirks, all pinned by the oracle test: the count lands before
  the fallible file work (an occupied destination skips silently after
  counting); the already-applied check compares `BUILD_TIME` against every
  same-`CPV` stanza (`:674-680`); the old file stays behind for
  `eclean-pkg`; the move goes through a `.partial` copy (`update_path =
  allocated + ".partial"`, `copyfile`, rewrite, rename) under a
  `wantnewlockfile` lock on the partial path; a signed gpkg skips without
  counting (`:653-663`, covered by `signed_gpkg_move_is_skipped`).
- The single-command rewrite runs `update_dbentries` over every metadata
  value in place (order kept), then resets `PF`/`CATEGORY` with trailing
  newlines and renames the ebuild (`dict.pop` + reinsert moves it to the
  end). `xpak_mem` + `recompose_mem` are the existing `helpers::xpak`
  writers, made `pub(crate)` — no new xpak writer.
- The allocator is `BinpkgState::allocate_binpkg_path` (multi-instance
  preferred path with the inherited `BUILD_ID`, else max+1; Portage
  creates an empty placeholder to reserve the id, which a single-threaded
  run does not need). Single-instance keeps `<pkgdir>/<cpv>.<suffix>`.
- CPV handling reuses `portage-repo::split_cpv` (the 3-part
  `resolve/cycles.rs` copy): the old `cat/pkg` is replaced inside the
  stored `CPV` string directly, `cpvs_on_cp` compares the `(cat, pkg)`
  pair, `new_pf_for_basename` rebuilds `pf` as `pkg-ver`.
  `portage-repo` has no 2-part `cat/pf` helper and none was added (the
  3-part split covers every call site); `mtimedb.rs::split_cpv` keeps its
  own borrowed `(&str, &str)` first-digit-dash split (different rule and
  return shape — repointing it would change resume parsing).

## fn move_binpkg_slot
- Ports `dbapi.move_slot_ent` (`dbapi/__init__.py:449-494`) over the
  binpkgs: match on `cp` plus the file `SLOT`'s slot part, keep the shared
  `newslot` reassignment across entries (the sub-slot inheritance quirk),
  one `aux_update` per hit. Sorted `CPV` order, like `match()` order.
- A versioned atom narrows the indexed cpvs first through
  `portage_repo::slotmove_version_matches` (backlog #583, shipped),
  carried on the existing `MoveSlotArgs` struct.

## fn aux_update
- Ports `binarytree.aux_update` (`bintree.py:253-335`): fold the new values
  into the ordered pairs, drop falsy ones, rewrite the file in place
  (xpak: in-place `recompose_mem`, which breaks hardlinks; gpkg: temp +
  rename of the whole container), then re-inject the entry.
- A signed gpkg is removed instead (file and stanza), never rewritten
  (`:286-297`, covered by `signed_gpkg_is_removed_not_rewritten`). The
  digest layer is verified before every unsigned rewrite
  (`verify_gpkg_manifest` with an empty-features policy); signature
  detection (any `.sig` member or armored `Manifest`, `gpkg.py:1717-1751`)
  runs first, so no `gpg` ever spawns here.
- Known corner: a gpkg that is both digest-broken and signed is removed by
  this port, where Portage aborts on the digest error first. Unreachable
  in practice (such a file is unusable either way); unpinned.

## fn update_binpkg_ents
- Ports `dbapi.update_ents` (`dbapi/__init__.py:383-447`) for the binpkgs:
  sorted `CPV` walk, per-entry updates from `repo_dict[pkg.repo]` else
  `repo_dict["DEFAULT"]` (first matching map entry, empty lists yield
  nothing), dep values through the shared `collapse_ws` (`aux_get`
  collapse), one `aux_update` + inject per changed file, count returned.
- `on_update(maxval, curval)` mirrors the `onUpdate` protocol (`(maxval,0)`
  up front, `(maxval,pos+1)` per rewritten entry, firing even when the
  entry was a signed removal). The `*` printing is the output piece's.
- Reuses `portage_repo::apply_cmds_to_dep_string` and the vdb `DEP_KEYS`
  order (`Package._dep_keys` is the same five). The bintree `aux_get`
  collapse (`" ".join(myval.split())`) is the same rule as the vdb one,
  so rewritten values lose their trailing newlines exactly like the
  capture's. A versioned `slotmove` directive is skipped by the shared
  helper (backlog #583, shipped), like Portage's `update_dbentry`.

## fn rewrite_gpkg_file
- Ports `gpkg.update_metadata` (`gpkg.py:1140-1244`): fresh `gpkg-1`,
  recompressed `metadata.tar` (member name follows `BINPKG_COMPRESS`;
  bytes come from the shared `ebuild_package::compression_command_argv`
  — the same template, `BINPKG_COMPRESS_FLAGS` (per-compressor override
  first) and `{JOBS}`-from-`MAKEOPTS` the fresh build resolves, threaded
  through the writer halves — so a moved gpkg compresses exactly like a
  freshly built one), every other member
  streamed through byte-identical under the new prefix, recomputed
  `Manifest`, temp + rename, mode `0644`.
- Fresh `Manifest` records are `BLAKE2B`-first (the shared
  `helpers::digest_of` order, `checksum_helper.libs` for
  `MANIFEST2_HASH_DEFAULTS`): same line a fresh build writes. Carried-over
  members keep their OLD record verbatim, checksum order included.
- Two traps this port hit, both pinned by the oracle test: `tar::Builder`
  `append` (unlike `append_data`) trusts the header as-is, so the renamed
  image header needs an explicit `set_cksum` or the rewritten container
  fails checksum validation; and carried-over members keep their OLD
  `Manifest` record verbatim (checksum order included — the fixture's
  image line is `BLAKE2B`-first while fresh records are `SHA512`-first),
  exactly like `self.checksums.append(m)`.
- Inner `metadata.tar` is built with the `tar` crate (USTAR, fresh mtimes,
  mode `0644`, two zero blocks appended — the writer the `binpkg`
  readers already accept). Long names fall back to `set_path` errors;
  GNU/PAX extension headers are not round-tripped (fixture names are
  short; container bytes are normalized in comparison anyway).
- Kept members stream through `Builder::append` (never held in memory),
  so multi-hundred-megabyte images do not balloon the process — the same
  shape as Portage's `tar_stream_writer`.

## fn write_binpkg_index
- Ports `PackageIndex.write` + `_pkgindex_write` (`getbinpkg.py:153-177`,
  `bintree.py:2237-2273`): every stanza sorted by `CPV` (category, package
  name, `vercmp`), falsy and default-valued fields dropped (`SLOT`/`EAPI`
  `"0"` via the shared `omit_stanza_default`), header-inherited
  `CHOST`/`repository` dropped when equal, keys sorted by the shared
  internal-name mapping, `_mtime_`/`repository`/`DESCRIPTION` translated
  to `MTIME`/`REPO`/`DESC` on write.
- Stanza keys pass when they are in the allowed set (plus the fresh
  entry's `REPO_REVISIONS`); header keys are limited to
  `PACKAGES`/`TIMESTAMP`/`VERSION`. An earlier static key table left out
  `DESCRIPTION`, so a loaded stanza's `DESC` was dropped on write, while
  Portage writes every key the stanza carries; the table is gone and
  `DESC` is written back.
- Reuses the `ebuild_package` stanza writers (`format_packages_entry`,
  `sort_stanza_fields_for_write`) and the gzip half
  (`write_packages_gz`), all made `pub(crate)`. The header keeps the
  stored keys with fresh `TIMESTAMP`/`PACKAGES`/`VERSION`; `Packages.gz`
  is written under `compress-index` (stale copy removed otherwise).
- `inject` dedups by `PATH` (else same-`CPV`-at-same-`PATH`) before
  appending, like `_inject_file`; loaded stanzas are filtered to the
  allowed key set first (`_pkgindex_allowed_pkg_keys`, read-time only —
  fresh entries keep `REPO_REVISIONS` like `_inject_file`'s unfiltered
  append). File mtimes follow `TIMESTAMP` (`set_modified`; `os.utime`
  also sets the atime, which has no reader here).
- No second copies in the tree (dedup round, verified by `grep`): the
  allowed-key set, the `MTIME`/`REPO`/`DESC`
  translation, the `VERSION` gate and `cmp_cpv` exist only here.
  `ebuild_package::write_packages_index_entry` is the single-entry
  `inject` shape (append, header snapshot merge) — a different Portage
  function from this whole-index `PackageIndex.write` (sort by `_cmp_cpv`,
  header inherited from the stored header) — so the four stanza helpers
  (`format_packages_entry`, `omit_stanza_default`,
  `sort_stanza_fields_for_write`, `write_packages_gz`) stay the factored
  common pieces, not a merged writer. `emerge_getbinpkg.rs`'s `VERSION`
  check is the remote-fetch gate (different job, different error text).
- `BinpkgState::remove` mirrors `bintree.remove` for the signed-gpkg
  drop (stanza + file; the in-memory `cpvdict` bookkeeping has no
  equivalent — the index is reloaded per top-level call).

## visibility changes and shared-helpers reuse (dedup round)
- `helpers::xpak::{xpak_mem, recompose_mem}` and `helpers::mod::xpak`
  are `pub(crate)` (the move path's trailer rewrite reuses them).
- `ebuild_package::{format_packages_entry, omit_stanza_default,
  sort_stanza_fields_for_write, write_packages_gz}` are `pub(crate)`
  (the index rewrite reuses the build path's stanza ordering).
- New `ebuild_package::{CompressionArgvArgs, compression_command_argv}`:
  the one template-to-`argv` resolution; `resolve_compression_command_jobs`
  delegates to it, and `compress_metadata` calls it with the writer halves
  (`BINPKG_COMPRESS`, flags, bzip2 command, `MAKEOPTS`). The
  `helpers/gpkg.rs::compression_cmd` settings-`varexpand` variant stays
  (different expansion context: full helper environment).
- `helpers::digest_of` is `pub(crate)` (re-exported through `helpers`):
  the `Manifest` pair, replacing the local copy (which also wrote fresh
  records `SHA512`-first; shared order is `BLAKE2B`-first like a fresh
  build).
- `binpkg::bytes_hex_digest` (new, `digest.rs`; `file_hex_digest` reads
  the file then calls it): the stanza `MD5`/`SHA1` pair, replacing the
  local hasher pair.
- `global_updates::collapse_ws` (new, `mod.rs`): the `aux_get` collapse;
  `vdb.rs::dep_content` and every binpkg metadata read use it.
- `portage-repo::split_cpv` (existing, `resolve/cycles.rs`): replaces the
  local `split_cpv`/`cpv_cp`; no new `cat/pf` helper was added.
- `privileges::path_writable` (new): the one `W_OK` probe; `pkgdir_writable`
  (kept: the `first_existing` ancestor walk is the `bindb.writable` half)
  and the per-file gate use it, as do `fetch`/`info_files`/`news`.
- `binpkg::classify_inner_member` is `pub(crate)` (metadata/image member
  lookup). `global_updates::vdb::DEP_KEYS` is `pub(crate)` (shared dep
  key order). No behaviour changed in any of those modules.

## tests
- `binpkg_files_and_index_match_the_portage_capture`: scratch `cp -a`
  of the Task 5 fixture, moves then `update_ents`, against the committed
  Portage capture. Xpak files byte for byte; gpkg by decoded metadata
  members, `Manifest` shape (member order, image size + digests proving
  byte-identical image bytes) and a passing `verify_gpkg_manifest`;
  `Packages` by text fields (`TIMESTAMP`/`MTIME` normalized;
  `SIZE`/digests normalized only for the gpkg row); `Packages.gz`
  gunzips to `Packages`; no `.partial`/lockfile leftovers; world and vdb
  byte-identical (untouched). Progress hook asserts the `(3,0..3)` shape.
- `read_only_pkgdir_skips_binpkg_updates`: `chmod 555` on the scratch
  `pkgdir` (non-root test user, so `access(W_OK)` refuses): zero moves,
  zero rewrites, tree byte-identical. Restores the mode afterwards.
- `signed_gpkg_is_removed_not_rewritten`: appends a fake
  `image.tar.zst.sig` sidecar plus its `Manifest` record (correct
  digests, so Portage's `_verify_binpkg` would trip the same
  `signature_exist` branch and remove it): file and stanza gone, the
  unsigned xpak still rewrites, removal still reports progress.
- `signed_gpkg_move_is_skipped`: updates file reduced to a move for the
  signed package: zero moves, file stays.
- `versioned_slotmove_reslots_only_the_matching_binpkg` (backlog #583):
  scratch fixture copy plus a `slotpkg-2.0.gpkg.tar` clone with an
  appended `Packages` stanza; an inline `>=dev-libs/slotpkg-2` command
  rewrites only 2.0's embedded `SLOT` (1, count 1) while 1.0 keeps 0.
  The shared fixture is untouched, so no capture pin moves.

## fn apply_binpkg_cmd
- One command's binpkg moves, mirroring `apply_vdb_cmd` for the per-command interleave (`_global_updates.py:190-206`): `move` through `move_binpkg` (`'%'`), `slotmove` through `move_binpkg_slot` (`'S'`), hook only on a nonzero count. Returns this command's counts; `apply_binpkg_moves` sums them and passes its hook through. `BinpkgState::load`, `WriterConfig::new` and `now_secs` are `pub(crate)` so the driver builds the state/writer once and loops commands; the writable-`PKGDIR` gate stays in the batch wrapper only.

## struct BinpkgMovesArgs (test-only)
- Takes the gated map alongside `repos` (repos now only supply the main-repo name). The whole-map function is test-only (`#[cfg(test)]`): the driver loops `apply_binpkg_cmd` per command for the `@%sS` interleave.
