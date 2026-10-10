# crates/portage-repo/src/installed.rs
Installed packages read from the VDB.

## fn installed_candidates
- Used for blocker matching, which needs slots (sub-slot included, same gap-closing reasoning as `Candidate::sub_slot`) to support slotted blocker atoms; `installed_versions` stays a plain version list for its callers.
- Portage's `action_deselect` only ever consults `vardb`/the world file, never repos/config: the one feature in portuale needing no repo/config-resolution machinery. `run_deselect` uses only `version`/`slot` (Portage's `Atom(f"{pkg.cp}:{pkg.slot}")` never includes sub-slot either), so `sub_slot` changes nothing there.
- Memo: a deep `emerge -pu` asks for the same few thousand cps over and over (12,684 calls on the reference workload); every miss re-reads a whole category directory plus one `SLOT` file per entry. Validated by each scanned category dir's mtime (1–2 `statx`, not ~80).
- Validity signal (feat#157 S1.2): the category directory's generation (`st_mtime` in nanos, 0 when missing -- every portuale vdb mutation replaces or removes a package dir, so the *category* dir mtime catches all of them).
- Backlog #124 E1: the old `.get(&key).cloned()` deep-cloned `Vec<(PathBuf, u64)>` solely to compare and drop it on every one of the 12,684 hits.

## fn installed_candidates_uncached
- `resolve_vdb_entry` resolves the same entry back from `(src_cat, src_pkg, version)`, so the value is unchanged on the fallback path (the old read `.trim()`ed, the helper normalises).
- Live entries only: Portage's `vardbapi._excluded_dirs`, in listing order.

## fn installed_pkg_repo
- `"__unknown__"` is Portage's `portage.versions._unknown_repo`.

## fn vdb_entry_slot
- Multi-line values collapse to one space-separated line; no further splitting (#126 O5).

## fn installed_contents_files
- Vdb records `${ROOT}`-absolute paths, the seam speaks `${ROOT}`-relative ones (matching the `mrg-director` `MemoryDb` convention); the path field is the second whitespace token with one leading `/` stripped. Same tolerance `read_vdb_string` gives a missing key. Old code used `read_to_string`.

## fn installed_reverse_dependents
- Matches via `portage_dep::match_from_list` against a `cat/pkg-ver:slot/sub_slot` candidate built from the consumer's own installed slot (the same candidate shape `best_installed_for_atom` matches against). Portage's `match` on a blocker is a conflict query, not a dependency.
- S8.1: the database backend hands over raw records; `files` keeps the scan with its exact syscall pattern.

## fn indexed_reverse_dependents
- The backend's index drops only entries that cannot name the consumer. Per-entry steps mirror the scan: `split_pf`, the package-move identity, skip the consumer itself, the entry's own `USE`, the four `*DEPEND` keys.

## fn best_installed_for_atom
- Portage's `_select_pkg_from_installed` (`depgraph.py:8518`).

## fn complete_mode_withheld_version
- Backlog #210. Portage's `_complete_graph` swaps selection to `_select_pkg_from_graph` (`depgraph.py:8662`) -- graph-or-installed, never a new merge -- so a phase-1 merge left with no mergeable candidate (enforced parent-atom pin or slot-conflict reconciliation) stays installed when the installed instance satisfies everything: Portage keeps it (nomerge) and reports the skipped update instead of failing. A genuine absence keeps the abort.

## fn running_root_satisfies_atom
- Keyed directly off the atom's parsed `category`/`package` (no wildcard-atom support needed: portuale's atom grammar never has an atom without an explicit category/package). Same idiom as `best_installed_matching`'s USE filter for the target root. Without the USE check an evaluated `[-flip]` still "matched" an installed `+flip` instance and the dep was wrongly dropped as running-root-satisfied (#133: Portage rebuilds it instead). Full grounding for the narrowed ESYSROOT-vs-ROOT scope lives in `enqueue_dependencies`'s doc.
- Portage's actual default is `SYSROOT` unset; only `pretend.rs`'s CLI boundary points this at the host `/`.

## fn vdb_aux_get
- The code that used to live here (the `stat` of the package dir, the consolidated `metadata` snapshot check, the dir-mtime-keyed memo, the whitespace normalisation and the invalid-`SLOT` -> `"0"` translation) moved to `portage-vdb` unchanged (feat#157 S1.2).
- No `KeyError`: portuale has no caller wanting a raise (today's `unwrap_or_default()`).
- Backlog #125: Portage never `" ".join()`s a line-oriented key; out-of-set keys (`CONTENTS`, `NEEDED.*`) read the raw file.

## fn read_vdb_flag_set
- Default markers are irrelevant here: a reinstall check only cares which flags exist/are enabled, not declared defaults. A missing file (e.g. an older vdb entry from before fixtures modeled USE/IUSE) is an empty set -- the same "absence is a valid state" precedent `read_world_atoms` (pretend.rs) establishes.

## fn read_vdb_string
- A vdb entry with nothing recorded for a key (e.g. `DEPEND=""` at merge time) is indistinguishable from -- and handled like -- a missing file. Portage applies the normalisation on the individual-file path too (`vartree.py:1044-1046`), so bytes match whether or not the consolidated `metadata` snapshot validated. Since #109 S3 the value comes from `vdb_aux_get`.

## fn libc_provider_cps
- Portage's `vardb.match("virtual/libc")` finding nothing is the empty case. Gentoo's `virtual/libc` `RDEPEND` is always a flat `|| ( sys-libs/glibc sys-libs/musl ... )` of non-virtual packages, so one level suffices; Portage's `strip_libc_deps` purpose is that practically every ebuild silently gains/loses an implicit libc dependency across revisions (noise, not a reportable change). Backlog #39 for the `_inject_libc_dep` half (`doebuild.py:3026-3067`).

## fn all_installed_packages
- Directory-name split generalises `strip_version_prefix`'s "a version always starts like a version, a package-name word may not" disambiguation to names whose package isn't known ahead of time. A deep `emerge -pu` reads the whole installed set (~2000 `SLOT` files) at ~10 resolver call sites, several inside loops, and the vdb never changes mid-resolve; Portage's `vardbapi` invalidates its own `cp_all` cache on category-dir mtime the same way. The fingerprint is `FilesDb::generation` (feat#157 S1.2; was `vdb_fingerprint` here).

## fn all_installed_packages_uncached
- The category/package directory walk used to live here (feat#157 S1.3); `resolve_vdb_entry` resolves the scanned entry back from `(category, name, version)`. Entries surface under post-move identity the way global-updates rewrites the vdb on sync (portuale never writes -- see `resolve_vdb_entry`).

## fn resolve_vdb_entry
- Was `vdb_pkg_dir`/`vdb_pkg_dir_meta`, which returned the path (feat#157 S1.3); the disk access is `FilesDb::has_entry` now, same `statx`.
