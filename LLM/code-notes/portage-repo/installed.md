# crates/portage-repo/src/installed.rs
Installed packages read from the VDB.

## fn installed_candidates
- Lists every installed `(version, slot, sub_slot)` triple for `category/package` found in the vdb under `root` (`<root>/var/db/pkg/<category>/<package>-<version>/`), reading each entry's `SLOT` file via `split_slot` (defaulting to `("0", "0")` if missing, same fallback as `list_candidates`).
- `pub` (unlike most of this crate's own internal helpers) so `portuale/src/pretend.rs`'s own `--deselect`/`-W` implementation can reuse it directly.
- Used for blocker matching, which needs slots (sub-slot included, same gap-closing reasoning as `Candidate::sub_slot`) to support slotted blocker atoms; `installed_versions` stays a plain version list for its callers.
- Portage's `action_deselect` only ever consults `vardb`/the world file, never repos/config: the one feature in portuale needing no repo/config-resolution machinery. `run_deselect` uses only `version`/`slot` (Portage's `Atom(f"{pkg.cp}:{pkg.slot}")` never includes sub-slot either), so `sub_slot` changes nothing there.
- Memo: a deep `emerge -pu` asks for the same few thousand cps over and over (12,684 calls on the reference workload); every miss re-reads a whole category directory plus one `SLOT` file per entry. Validated by each scanned category dir's mtime (1–2 `statx`, not ~80).
- Validity signal (feat#157 S1.2): the category directory's generation (`st_mtime` in nanos, 0 when missing -- every portuale vdb mutation replaces or removes a package dir, so the *category* dir mtime catches all of them).
- Backlog #124 E1: the old `.get(&key).cloned()` deep-cloned `Vec<(PathBuf, u64)>` solely to compare and drop it on every one of the 12,684 hits.

## fn installed_candidates_uncached
- Portage `profiles/updates/` package moves: `<category>/<package>` may be the *post*-`move` name while the vdb still holds the package under its pre-`move` `cat/pkg` dir. Every source name that maps onto this one (`installed_cp_sources`) is scanned; `slotmove` then rewrites each hit's `(slot, sub_slot)`. Backlog #391: a query for a *pre*-`move` name scans nothing -- its entry was renamed away (`installed_cp_sources` drops the moved-away self).
- `resolve_vdb_entry` resolves the same entry back from `(src_cat, src_pkg, version)`, so the value is unchanged on the fallback path (the old read `.trim()`ed, the helper normalises).
- Live entries only: Portage's `vardbapi._excluded_dirs`, in listing order.

## fn installed_pkg_repo
- `"__unknown__"` is Portage's `portage.versions._unknown_repo`.

## fn vdb_entry_slot
- The `SLOT` of the vdb entry named by its directory name `pf` (`foo-bar-1.2.3-r1`), read through the same [`vdb_aux_get`] seam every other installed-metadata consumer uses: the normalised `"slot/sub_slot"` string, multi-line values collapsed to one space-separated line, a present-but-invalid value translated to `"0"` (Portage `aux_get`'s `_get_slot_re` rule, #115 S1), `""` for an entry with no `SLOT` file or a `pf` [`split_pf`] cannot split.

## fn installed_versions
- Lists every installed version of `category/package` found in the vdb under `root` (`<root>/var/db/pkg/<category>/<package>-<version>/`).
- Multi-line values collapse to one space-separated line; no further splitting (#126 O5).

## fn installed_contents_files
- The vdb `CONTENTS` shape (Portage `vartree`/`dblink`): `obj <path> <md5> <mtime>`, `sym <path> -> <target> <mtime>`, `dir <path>`; `dev`/`fif`/`bin` device-node lines the same way.
- Only the path field is returned (the second whitespace token, with one leading `/` stripped -- the vdb records `${ROOT}`-absolute paths, the seam speaks `${ROOT}`-relative ones, matching the `mrg-director` `MemoryDb` convention).
- Vdb records `${ROOT}`-absolute paths, the seam speaks `${ROOT}`-relative ones (matching the `mrg-director` `MemoryDb` convention); the path field is the second whitespace token with one leading `/` stripped. Same tolerance `read_vdb_string` gives a missing key. Old code used `read_to_string`.

## fn installed_reverse_dependents
- Every installed `category/package-version` whose vdb-recorded `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND` (each flattened against that package's own vdb `USE`) names an atom the installed `consumer_category/consumer_package-consumer_version` satisfies.
- Matches via `portage_dep::match_from_list` against a `cat/pkg-ver:slot/sub_slot` candidate built from the consumer's own installed slot (the same candidate shape `best_installed_for_atom` matches against). Portage's `match` on a blocker is a conflict query, not a dependency.
- S8.1: the database backend hands over raw records; `files` keeps the scan with its exact syscall pattern.

## fn indexed_reverse_dependents
- The raw records `InstalledDb::reverse_dependents` returns for the consumer's `category/package` (a superset: the backend's index drops only entries that cannot name it).
- The backend's index drops only entries that cannot name the consumer. Per-entry steps mirror the scan: `split_pf`, the package-move identity, skip the consumer itself, the entry's own `USE`, the four `*DEPEND` keys.

## fn best_installed_for_atom
- Portage's `_select_pkg_from_installed` (`depgraph.py:8518`).

## fn complete_mode_withheld_version
- Backlog #210. Portage's `_complete_graph` swaps selection to `_select_pkg_from_graph` (`depgraph.py:8662`) -- graph-or-installed, never a new merge -- so a phase-1 merge left with no mergeable candidate (enforced parent-atom pin or slot-conflict reconciliation) stays installed when the installed instance satisfies everything: Portage keeps it (nomerge) and reports the skipped update instead of failing. A genuine absence keeps the abort.

## fn running_root_satisfies_atom
- Deliberately generic on `running_root` (just like `installed_versions`/`owns_path_pf` elsewhere): this function has no idea whether it is being pointed at a real host `/` or a fixture's own fake vdb tree.
- Conditional forms (`=`/`?`) stay vacuous here exactly as in `match_from_list` -- they are the caller's to evaluate first (both `root_deps_*` producers do, against the parent's own USE); what this enforces is whatever unconditional form the atom already carries.
- Keyed directly off the atom's parsed `category`/`package` (no wildcard-atom support needed: portuale's atom grammar never has an atom without an explicit category/package). Same idiom as `best_installed_matching`'s USE filter for the target root. Without the USE check an evaluated `[-flip]` still "matched" an installed `+flip` instance and the dep was wrongly dropped as running-root-satisfied (#133: Portage rebuilds it instead). Full grounding for the narrowed ESYSROOT-vs-ROOT scope lives in `enqueue_dependencies`'s doc.
- Portage's actual default is `SYSROOT` unset; only `pretend.rs`'s CLI boundary points this at the host `/`.

## fn vdb_aux_get
- **No `environment.bz2` search** (Portage bug 395463) for a key outside the 23-set.
- The code that used to live here (the `stat` of the package dir, the consolidated `metadata` snapshot check, the dir-mtime-keyed memo, the whitespace normalisation and the invalid-`SLOT` -> `"0"` translation) moved to `portage-vdb` unchanged (feat#157 S1.2).
- No `KeyError`: portuale has no caller wanting a raise (today's `unwrap_or_default()`).
- Backlog #125: Portage never `" ".join()`s a line-oriented key; out-of-set keys (`CONTENTS`, `NEEDED.*`) read the raw file.

## fn read_vdb_flag_set
- Reads `<root>/var/db/pkg/<category>/<package>-<version>/<filename>` (a vdb aux file, e.g. `USE` or `IUSE` -- same directory `SLOT`/`CATEGORY` already come from) as a set of flag names. The value comes from [`vdb_aux_get`], so a validated consolidated snapshot serves it without an `open()`.
- Default markers are irrelevant here: a reinstall check only cares which flags exist/are enabled, not declared defaults. A missing file (e.g. an older vdb entry from before fixtures modeled USE/IUSE) is an empty set -- the same "absence is a valid state" precedent `read_world_atoms` (pretend.rs) establishes.

## fn read_vdb_string
- The value is whitespace-normalised exactly like Portage `_aux_get` (`vartree.py:1044-1046`): `" ".join(myd.split())`, i.e. `split_whitespace().join(" ")` here. Portage applies this on the individual-file path too, so the bytes are the same whether the consolidated `metadata` snapshot validated or not.
- A line-oriented key (`CONTENTS`, `NEEDED.*`) bypasses the normalisation structurally -- [`vdb_aux_get`] routes anything outside Portage's 23-field `_METADATA_FILE_FIELDS` to the raw, un-normalised read (backlog #125).
- A vdb entry with nothing recorded for a key (e.g. `DEPEND=""` at merge time) is indistinguishable from -- and handled like -- a missing file. Portage applies the normalisation on the individual-file path too (`vartree.py:1044-1046`), so bytes match whether or not the consolidated `metadata` snapshot validated. Since #109 S3 the value comes from `vdb_aux_get`.

## fn libc_provider_cps
- Portage `find_libc_deps(vardb, realized=False)` (`portage.dep.libc`): the `(category, package)` identity of every atom `virtual/libc`'s own installed (vdb) `RDEPEND` names, once flattened against its own installed `USE`.
- A simplified, one-level port of Portage `expand_new_virt`: Portage Gentoo's own `virtual/libc` `RDEPEND` is always a flat `|| ( sys-libs/glibc sys-libs/musl ... )` of real (non-virtual) packages, so this doesn't replicate `expand_new_virt`'s own further case of recursing into a *second* virtual reached this way.
- The second half of Portage `_inject_libc_dep` (`doebuild.py:3026-3067`, backlog #39), which needs the realized provider versions on top -- see `ebuild_phases::inject_libc_dep`.
- Portage's `vardb.match("virtual/libc")` finding nothing is the empty case. Gentoo's `virtual/libc` `RDEPEND` is always a flat `|| ( sys-libs/glibc sys-libs/musl ... )` of non-virtual packages, so one level suffices; Portage's `strip_libc_deps` purpose is that practically every ebuild silently gains/loses an implicit libc dependency across revisions (noise, not a reportable change). Backlog #39 for the `_inject_libc_dep` half (`doebuild.py:3026-3067`).

## fn all_installed_packages
- Every package recorded under `<root>/var/db/pkg` (Portage `vartree.dbapi.cpv_all()`), each with its own main slot. Directory names are split into `package`/`version` by finding the last `-`-separated boundary whose right half `ververify`s.
- Directory-name split generalises `strip_version_prefix`'s "a version always starts like a version, a package-name word may not" disambiguation to names whose package isn't known ahead of time. A deep `emerge -pu` reads the whole installed set (~2000 `SLOT` files) at ~10 resolver call sites, several inside loops, and the vdb never changes mid-resolve; Portage's `vardbapi` invalidates its own `cp_all` cache on category-dir mtime the same way. The fingerprint is `FilesDb::generation` (feat#157 S1.2; was `vdb_fingerprint` here).

## fn all_installed_packages_uncached
- The category/package directory walk used to live here (feat#157 S1.3); `resolve_vdb_entry` resolves the scanned entry back from `(category, name, version)`. Entries surface under post-move identity the way global-updates rewrites the vdb on sync (portuale never writes -- see `resolve_vdb_entry`).

## fn candidate_use_flags_display
- The `GraphEntry::use_flags_display` list (`(iuse_flag, enabled)` pairs, `_alnum_sort_key`-sorted) for a tree ebuild `<category>/<package>-<version>` -- the same per-flag resolution `resolve_pretend_graph`'s own display loop does (`effective_use_flags` over the candidate's `IUSE`).
- Pulled out so `emerge --resume` can populate it on each reconstructed source `GraphEntry`: the `mtimedb` resume list records only `cat/pkg-ver`, so the resolver-loop population is skipped -- and without it `emerge_build::build_use_env` produces an empty `USE=`, which makes a `python-r1` / `distutils-r1` (or any USE-conditional) ebuild `die` in `src_compile`.
- Empty when the candidate can't be located or declares no `IUSE`. The `USE_EXPAND` re-grouping (`use_expand_display`) is deliberately not recomputed.

## fn candidate_effective_use_flags
- `bin/ebuild.sh`'s own `use()` and the build-info `USE` file that Portage `__dyn_install` writes need the implicit profile flags (`abi_x86_64 amd64 elibc_glibc kernel_linux`) too, which no package declares in `IUSE` (#37; the S0 recon's `metadata/USE` row, `TEST/findings/l2.md`).
- The caller filters this through `portage_profile::portage_use` (Portage `config.py:2261`'s own IUSE ∪ `IUSE_EFFECTIVE` narrowing) -- this function deliberately returns the resolver's raw answer.

## fn vdb_entry_is_dir
- `vdb_pkg_dir(..).is_dir()` of old: resolve the entry, then test the resolved entry again (two `statx` when the direct entry exists, as before).

## fn resolve_vdb_entry
- A missing `IUSE` key is an empty `IUSE`, not missing metadata: the Portage md5-cache writer omits empty fields (`porttest/emptydirs` has no `IUSE=` line at all), and Portage still computes the package's full effective USE -- the implicit profile flags -- for it (`TEST/findings/l2.md` "S0 recon").
- Was `vdb_pkg_dir`/`vdb_pkg_dir_meta`, which returned the path (feat#157 S1.3); the disk access is `FilesDb::has_entry` now, same `statx`.

## struct InstalledPackage
- One installed package in the vdb, `category/package-version` plus its own recorded main `SLOT` -- Portage `vartree.dbapi.cpv_all()` / `_pkg_str`.

## fn all_cp
- The category/package directory walk used to live here (`FilesDb::entries`, feat#157 S1.3), `-MERGING-` entries already skipped.
- Every `category/package` present in any of `repos` -- Portage `portdbapi.cp_all()` (`porttree.py`), used by `emerge --search`.
