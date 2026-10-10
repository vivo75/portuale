# crates/portuale/src/ebuild_phases/post_install_meta.rs
VDB metadata written after `src_install` (build-info keys, soname deps, libc dep, SIZE).

## fn write_post_install_metadata
- `__dyn_install`'s build-info loop (run unmodified, before this) writes `CATEGORY`/`SLOT`/`KEYWORDS`/`IUSE`/`USE`/`EAPI`/`DEFINED_PHASES`/… but not these keys; Portage's Python side fills them in. Found via the `binpkg.rs` scan buildout: without this the vdb entry and later `xpak`/`gpkg` carry no dependency metadata.
- Source is `settings.configdict["pkg"]`, i.e. the md5-cache entry (same already-trusted source `fetch_sources` reads `SRC_URI` from). Reduction is `paren_enclose(use_reduce(v, uselist=use))`, the bracket/`||`-preserving normalized token stream.
- `BUILD_TIME` is `int(time.time())`, written before any metadata key, unconditionally. The vdb entry copies it (`populate_vdb_tmp`) and `_consolidate_to_metadata_file` folds it into the consolidated `metadata` body; Portage's reader and binpkg multi-instance logic compare against it. Omitting it skewed every entry by the file and the field.
- Dependency tokens go through `Atom.evaluate_conditionals` (`token_class=Atom`): an unknown/disabled `flag?` use-dep is dropped (bug #386829's canonicalisation), while `flag?` group conditionals were already handled by `use_reduce_structured`. Non-atoms (`||`, bare parens) pass through.
- `IUSE` is verbatim from the package config ("in case it's corrupted due to local environment settings", bug #386829) and always present — a no-IUSE ebuild gets an empty `metadata/IUSE`, exactly as Portage archives carry. `IUSE_EFFECTIVE` (EAPI 5+) is the profile's `_calc_iuse_effective`, threaded as an ordinary phase env key; standalone runs have no such key, as in Portage's `configdict`.
- Empty value: Portage unlinks a stale `build-info/<k>`; bash never wrote these, so there is nothing to unlink.
- `SIZE` lands in `build-info/SIZE`, read by both the archive metadata (`metadata/SIZE`) and the vdb copy.

## fn bind_slot_operator
- Per-atom step of `_slot_operator._eval_deps`: `slot_operator == "="` rewrites to `:<slot>/<sub-slot>=` from the highest installed version in `<root>/var/db/pkg` satisfying it (`vardb.match(x)[-1]`). Anything else returns unchanged ("just leave it as-is for now ... keeping the information in vdb").
- A merged package records e.g. `dev-libs/foo:2/3=` in build-info: the data a later sub-slot rebuild check needs. No `:=` (or uninstalled dep) is byte-identical to before.
- Narrowing: Portage walks `RDEPEND`/`PDEPEND` against the target `ROOT` vdb and `DEPEND`/`BDEPEND` against target/running respectively; the single-root world binds every `*DEPEND` key against the one `<root>/var/db/pkg`. The `|| ( A:= B:= )` "record sub-slot on A only" TODO (bug #455904) is moot without disjunctive `:=` handling.

## fn write_post_install_soname_deps
- Runs where Portage runs it: after the QA misc-function sequence, in `_commands_exit`.
- Category field: Portage reads each object's ELF header (`ELFHeader.read`) and writes `NeededEntry.__str__`'s `arch;obj;soname;rpaths;needed;category` line back. An object that no longer reads as ELF keeps an empty category (`multilib_category = None`).
- `REQUIRES`/`PROVIDES` come from the recognized entries (`SonameDepsProcessor`, `PROVIDES_EXCLUDE`/`REQUIRES_EXCLUDE` aware); each file only when its map is non-empty.
- Portage reaches this only when `NEEDED.ELF.2` exists — hence the early return.

## fn inject_libc_dep
- Every ELF-bearing package records the dep so a binpkg merge cannot silently downgrade the libc it was built against (bug #753500). `find_libc_deps(..., realized=True)`: expand `virtual/libc`'s installed `RDEPEND` to provider cps, take each provider's lowest installed version (`portdb.match(atom)[0]`), skip when this package is itself a provider (Portage's `pkgcmp` self-check).
