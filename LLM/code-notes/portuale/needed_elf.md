# crates/portuale/src/needed_elf.rs
`NEEDED.ELF.2` parsing and linkage-map computation for preserve-libs decisions.

## module
- Ports `NeededEntry` (lib/portage/util/_dyn_libs/NeededEntry.py): the data model for one parsed line of a `NEEDED.ELF.2` file -- the aux vdb metadata Portage's unmodified `bin/misc-functions.sh install_qa_check`'s own real `scanelf`-driven step generates, and which `ebuild_merge::populate_vdb_tmp` copies into every vdb entry that has one.
- Each step confirmed with the user before implementing: parsing (`NeededEntry`), `LinkageMap.rebuild()`'s initial data-gathering loop (`read_all_needed_entries`), `rebuild()`'s remaining indexing logic (`rebuild`), and `findConsumers()` + `_find_libs_to_preserve()`'s graph-reachability decision (`find_consumers`/`find_libs_to_preserve`, plus `getlibpaths` and a minimal `LibGraph`). Deliberately still missing: the live-`scanelf`-for-orphaned-preserved-libs branch inside `rebuild()` itself (LinkageMapELF.py:233-324 -- the one place Portage falls back to a raw ELF header read rather than `NEEDED.ELF.2`, out of scope until preserve-libs actually needs it).
- Wiring `find_libs_to_preserve`'s output into a merge/unmerge is a real, separate control-flow integration task across two already-tested files, not a "port this Python function" task: calling it at the right point in `ebuild_merge::merge_after_install`, writing results into the real `preserved_libs_registry.json` via the already-existing `write_plib_registry`, and making `ebuild_unmerge::remove_contents` skip deleting a preserved path. `#[allow(dead_code)]` is deliberate: this module has no real caller yet.
- What this slice does NOT do, even though the pure-computation port is complete: wire `find_libs_to_preserve`'s output into a merge/unmerge's control flow (calling it in `ebuild_merge::merge_after_install`, writing results into `preserved_libs_registry.json` via `write_plib_registry`, making `ebuild_unmerge::remove_contents` skip a preserved path). A separate control-flow integration task across two already-tested files, left for a following slice so it can be scoped and reviewed on its own, without risk to the already-shipped preserve-libs *consult/unregister* side. `#[allow(dead_code)]` is deliberate: the same "narrow, additive, no wiring until the next slice needs it" shape portuale used before (e.g. `masters =` parsing landing before eclass masters-chain search consumed it).

## struct NeededEntry
- Portage `NeededEntry.__slots__`: one parsed `NEEDED.ELF.2` line.
- `soname` is a plain (possibly empty) string, not `Option`, matching Portage Python exactly: Portage's `scanelf` genuinely reports an empty soname for some real libraries (e.g. musl's own `libc.so`, which has no `DT_SONAME` at all); precisely why Portage's `misc-functions.sh` deliberately never uses `scanelf -q` (see that script's own comment).
- `multilib_category`: `None` when absent (Portage pre-multilib-category data) or present-but-empty; `NeededEntry._MULTILIB_CAT_INDEX` is the 6th field.

## impl NeededEntry::parse
- Portage `NeededEntry.parse()`: `arch;filename;soname;rpaths;needed`, semicolon-delimited, an optional 6th `multilib_category` field, any further fields silently ignored.
- `None` for a malformed line is Portage's `InvalidData`; callers skip and keep going, the same tolerance Portage's `LinkageMap.rebuild()` has for a bad line (`writemsg_level` + `continue`, never aborting the whole read).
- `rpaths`'s own `"  -  "` sentinel (two spaces, a dash, two spaces) means "no rpath at all": it is Portage `scanelf`'s own `%r` output for an object with no rpath, since `misc-functions.sh` never passes `scanelf -q` (which would omit rpath-less-and-soname-less libraries like musl's `libc.so`) and must handle the placeholder itself.
- Extra fields beyond the 6th are silently ignored: Portage's "extra fields may exist for future extensions".

## impl NeededEntry::to_needed_line
- Ports `NeededEntry.__str__` (NeededEntry.py:67-87): "format this entry for writing to a NEEDED.ELF.2 file" -- used when Portage itself *rewrites* the file (`vardbapi.removeFromContents`/`writeContentsToContentsFile`, see `ebuild_merge::remove_from_contents`), as opposed to the original `scanelf`-generated file this module's own `parse`/`parse_file` read. Two asymmetries from the read side, both intentional, not bugs: an empty `runpaths` serializes as a plain empty string here (`":".join([])`), never the `"  -  "` sentinel `scanelf` itself emits; and the 6th (`multilib_category`) field is *always* present, even when `None` (as `""`), unlike the original file which may omit it entirely for pre-multilib-category data.

## fn compute_multilib_category
- Ports `ELFHeader.read` + `compute_multilib_category` (portage/util/_dyn_libs/NeededEntry.py, portage/dep/soname/multilib_category.py): read one ELF header's own `EI_CLASS`, `e_machine` and `e_flags` and classify it into the multilib category (`x86_64`, `x86_32`, `x86_x32`, `arm_64`, ...), or `None` for a non-ELF/unreadable file or an unrecognized machine/class combination (Portage's own `None` return, which makes the entry an "unrecognized ELF file" and keeps its 6th `NEEDED.ELF.2` field empty). Used by `ebuild_phases::write_post_install_soname_deps` to add the trailing field Portage's `_post_src_install_soname_symlinks` writes back.
- Backlog #39: hand-built ELF headers cover one branch per machine/class mapping.

## fn exclude_patterns
- Portage `SonameDepsProcessor._exclude_pattern`: `shlex.split` the `PROVIDES_EXCLUDE`/`REQUIRES_EXCLUDE` value, `lstrip("/")` each pattern, match any of them. The `shlex` part is a minimal whitespace/quote split -- Portage's own excludes are bare path globs. Portage `lstrip("/")`s each pattern.

## fn generate_soname_deps
- Ports Portage `SonameDepsProcessor` (`portage/util/_dyn_libs/soname_deps.py`): turn one package's `NEEDED.ELF.2` entries into the `PROVIDES` and `REQUIRES` strings Portage writes into `build-info` (and therefore into the archive's `metadata/PROVIDES`/`metadata/REQUIRES`, #39). Returns `(provides, requires)`, each `None` when the corresponding map is empty.
- Backlog #39. Portage writes no file when a map is empty; entries without a recognized category are Portage's "unrecognized ELF" case (`SonameDepsProcessor.add` asserts on a missing category), hence the caller filter.
- `_intersect`: a provided soname is never a requirement; an internal library without a soname resolves through a matching DT_RUNPATH entry.

## fn read_all_needed_entries
- Ports `LinkageMap.rebuild()`'s initial data-gathering loop (`LinkageMapELF.py:218-231`): for every installed package (`dbapi.cpv_all()`), its vdb-stored `NEEDED.ELF.2` (`aux_get(cpv, [self._needed_aux_key])`), parsed via `NeededEntry::parse_file`. A `cpv` is still included, with an empty `Vec`, for a package with no such file, matching Portage `rebuild()`'s unconditional per-cpv walk (it never skips a `cpv` just because it owns no ELF content). Returns `(cpv, entries)` pairs in sorted vdb directory-listing order.
- Walks every installed package's vdb directory the same way `ebuild_merge::find_owners` does (Portage `dbapi.cpv_all()` has no ordering guarantee, hence sorted output for portuale determinism). A missing aux file degrades the way Portage's `aux_get` does (returning `""`).
- Still just raw per-package data: no soname map, no multilib/runpath resolution (`rebuild()`'s `libs`/`obj_properties` indexing, `providers`/`consumers` bucketing, `$ORIGIN` expansion -- separately-scoped work) -- nothing here yet answers "what does path X provide" or "what needs path X"; only "what did each installed package's `NEEDED.ELF.2` say".

## fn approx_multilib_category
- Ports `_approx_multilib_categories` (`LinkageMapELF.py:29-46`): maps an ELF `e_machine` value (`NEEDED.ELF.2`'s own `arch` field, already stripped of its `EM_` prefix by Portage's unmodified `bin/misc-functions.sh` before it's ever written) to an approximate multilib category. Falls back to the raw `arch` string unchanged for anything not in the table.
- Only consulted for pre-multilib-category data or portuale's own fixtures (which never emit the 6th field). Falls back exactly like Portage.

## fn normalize_path
- Ports `portage.util.normalize_path` (`lib/portage/util/__init__.py:139-153`): a lexical, non-symlink-resolving path normalizer (`os.path.normpath`, with Portage's own leading-`//`-vs-`/` fix) -- collapses `.`/empty segments, resolves `..` segments against already-collapsed segments, never touches the filesystem.

## fn expand_origin
- Ports `rebuild()`'s `varexpand(x, {"ORIGIN": os.path.dirname(entry.filename)}, ...)` narrowed to the one substitution `rebuild()` supplies a value for (Portage's general `varexpand` handles arbitrary `${VAR}`/`$VAR`).

## fn scan_preserved_lib_entries
- Portage `LinkageMap.rebuild`'s preserved-libs branch: preserved libraries are registered in no `NEEDED.ELF.2` file, so Portage runs the unmodified `scanelf` binary over them (`scanelf -BF '%a;%F;%S;%r;%n'`, deliberately without `-q` so soname-less libraries like musl's `libc.so` are not omitted) and indexes every reported line exactly like `NEEDED.ELF.2` data, owned by the preserving cpv. Each line's `EM_`-prefixed arch (`EM_X86_64`) is stripped to the bare `e_machine` name (`l[3:]`) before parsing; the multilib category comes from a live ELF header read (`compute_multilib_category`) and `None` falls back to the approximate table in `rebuild`, exactly like `_approx_multilib_categories`. Returns `(owner, entries)` groups ready to append to `read_all_needed_entries`'s output before `rebuild`.
- `preserved` is Portage's `getPreservedLibs()`. Directories are never passed (Portage `scanelf` would recurse into them and index files nothing preserves); anything else `scanelf` reports nothing for is skipped with Portage `InvalidData` tolerance. The `l[3:]` arch strip and grouping by owner match Portage (`plibs.pop(entry.filename)`).
- `EPREFIX` handling: Portage's `os.path.join(EPREFIX or "/", "usr/bin/scanelf")` (`LinkageMap.rebuild`): the EPREFIXed absolute path first, then `PATH`. Portuale assumes an empty `EPREFIX` like every other real-execution path, so that is `/usr/bin/scanelf`, falling back to a `PATH` lookup.
- Cuts: Portage generates a dummy `("", x, "", "", "")` line for a path `scanelf` reports nothing for (a non-ELF file) so `findConsumers` never raises `KeyError` -- portuale emits no dummy since `find_consumers` returns `Err` and callers treat that as the dummy's empty sets. Portage infers an implicit soname from the basename for an empty-soname object (bug 715162, via the `file` binary) -- skipped, since a `NEEDED.ELF.2`-indexed empty-soname library provides nothing either. A missing `scanelf` raises Portage's `CommandNotFound` but degrades loudly here (a stderr warning names the tool, since silent degradation would turn bump-2-style preserves into `{}` with zero log output); a `scanelf` that runs but fails still degrades silently (Portage tolerates unscannable lines the same way).
- First-wins on a duplicate path across cpvs; Portage's `plibs.update(...)` is last-wins (only observable if two cpvs register the same path).
- An owner already present in `read_all_needed_entries`'s output gains the scanned entries, matching Portage grouping everything by owner for the bundled-library runpath inference (see `owner_entries_with_preserved_orphans`).

## enum ObjKey
- Ports `LinkageMap._ObjectKey` (LinkageMapELF.py:98-148): a real `(dev, ino)` pair when the object still exists on disk (real `os.stat`, follows symlinks, matching Portage's `_obj_key` exactly) -- the same file reachable via multiple filesystem paths (symlinks, hardlinks) collapses to one entry, every path kept as an `alt_paths` entry (see `ObjProperties`). Falls back to the object's own literal path string when it doesn't exist. For nonexistent ones Portage falls back to `os.path.realpath(...)` (symlink-resolved, tolerant of a missing target) -- a deliberate narrower simplification for a case that should be rare (a `NEEDED.ELF.2` entry, read moments after `scanelf` itself confirmed the object's existence, gone by the time this runs).

## struct ObjProperties / struct SonameMap / struct LinkageMap
- Port `LinkageMap._obj_properties_class` / `LinkageMap._soname_map_class` (a real `slot_dict_class`) and `LinkageMap._libs`/`_obj_properties`, populated by `rebuild`.
- `ObjProperties::alt_paths`: every filename this exact object was indexed under -- always has at least one entry (the first-seen one, real `myprops` is only ever created once per `ObjKey`).
- `SonameMap`: every object that provides this soname (`DT_SONAME` matches it), and every object that needs it (this soname appears in its own `DT_NEEDED` list) -- within one multilib category's own map.
- `LinkageMap::libs` is Portage `self._libs`: multilib category -> soname -> providers/consumers.

## fn rebuild
- Ports LinkageMapELF.py:325-469. The bundled-library implicit runpath is LinkageMapELF.py:380-410 (accounts for internal library resolution a package implements itself, e.g. bundled libraries, without an explicit rpath). The alias rule is Portage's "only one set of data can be correct... mixing data may corrupt the index".

## fn grab_lines
- Ports `grabfile()` (lib/portage/util/__init__.py:156-185) narrowed to `getlibpaths`'s `read_ld_so_conf` needs, including its tolerance for a nonexistent file.

## fn getlibpaths
- Ports `getlibpaths()` (`lib/portage/util/__init__.py:1945-1963`): the default dynamic-linker library search path -- `LD_LIBRARY_PATH`, every line of the root-scoped `/etc/ld.so.conf` (`grabfile` semantics, see `grab_lines`), then the `/usr/lib`/`/lib` defaults -- each `normalize_path`'d.
- `LD_LIBRARY_PATH` is an explicit parameter here, not an ambient env read: "explicit parameter over ambient env read inside library code". The explicit-parameter-over-ambient-env-read follows the reasoning `portage_fetch::FetchOptions::gentoo_mirrors` established. The `include` cut matches the v1 cut portuale's `env_update.rs` documents and confirmed with the user for the *other* `/etc/ld.so.conf` reader (`run_env_update`'s candidate-lib-dir scan) -- a rare, admin-configured mechanism, not populated by anything portuale's fixtures do.

## struct LibGraph
- Portage `path_to_node` registers (or reuses) the graph node for `path`'s own object identity, recording `path` itself as one of that node's own `alt_paths` -- deliberately *not* itself an `add()` call (real `path_to_node` doesn't add an edge either; the caller decides separately whether and how this node connects). `_LibGraphNode` wraps an `_ObjectKey`, the same dedup-by-inode identity `ObjKey` gives this port; the `alt_paths` bookkeeping matches `path_to_node`'s "multiple recorded paths can be the same object".

## fn find_consumers
- Ports `LinkageMap.findConsumers()` (`LinkageMapELF.py:817-960`), narrowed to the one calling convention `_find_libs_to_preserve` uses: `obj` is always a path string (never an already-resolved `_ObjectKey`), and `exclude_providers` is always exactly one callable -- `exclude_provider: Option<&dyn Fn(&str) -> bool>` here.
- Portage's "shadowed by another version" check first: if a same-directory soname symlink exists and doesn't actually point at `obj` itself (dev/inode comparison), `obj` has no consumers of its own. Then `exclude_providers`/`greedy` consumer-satisfaction filtering: a consumer is dropped from the result if some *other*, non-excluded provider of the same soname already satisfies it (found in its own runpath or the default lib path, `defpath`). Finally, only consumers that can actually *reach* `obj` (`obj`'s own directory is in the consumer's runpath or `defpath`) are returned. `Err` for a `KeyError`: `obj` not an indexed object at all.
- `exclude_providers` is always exactly the 1-tuple `(installed_instance.isowner,)` at the one Portage call site. The shadowed check's context is binutils-style per-`CHOST` symlink indirection. `Err` matches Portage's `KeyError`, in portuale's error-string convention. The `greedy` / `exclude_providers` satisfaction filtering matches Portage.

## fn find_libs_to_preserve
- Ports `dblink._find_libs_to_preserve()` (`vartree.py:3491-3595`), narrowed to its pure computation. Algorithm: build a dependency graph from `findConsumers` results (an edge from each provider to each of its own consumers, skipping a consumer that's itself being removed and isn't also a provider); walk from every "root" consumer (nothing depends on it, and it isn't itself a provider) to find every provider transitively reachable -- those are the preserve candidates. `old_contents` is the *previous* same-slot instance's `CONTENTS` paths, already `ROOT`-relative absolute paths in portuale's convention.
- `old_owner_is_owner` is Portage `installed_instance.isowner` (does the *old*, being-replaced/removed instance own this path); `new_owner_is_owner` is Portage `self.isowner`, gated by Portage `not unmerge` at the one call site that matters (`vartree.py:3585`/`3589`), folded into this closure by the caller instead of a separate `unmerge: bool` parameter: an unmerge-only caller passes a closure that always returns `false`, exactly matching what `not unmerge and self.isowner(f)` collapses to when `unmerge` is `true`.
- The gating conditions (`_linkmap_broken`, `_plib_registry is None`, `FEATURES=preserve-libs` off, no `_installed_instance`) are the caller's responsibility (wiring deliberately not part of this slice).
- `old_contents` is Portage's `installed_instance.getcontents()`; portuale's `CONTENTS` entries never carry `ROOT`, so there is no `f_abs[root_len:]` stripping. `new_owner_is_owner` folds Portage's `not unmerge and self.isowner(f)` (vartree.py:3585/3589) into the closure: an unmerge-only caller passes always-`false`.
- Portage's hardlink/soname-symlink classification skips a candidate the *new* package already replaces (both file *and* soname symlink).

## fn soname_consumers
- Every recorded path of every object that lists `soname` in its own `DT_NEEDED` -- a soname-keyed reverse lookup across all multilib categories. Used by `ebuild_merge::find_unused_preserved_libs` as the fallback consumer scan for a preserved library whose owning package has already left the vdb (so the library itself is no longer an indexed object). Stands in for Portage's `scanelf`-for-orphaned-preserved-libs `rebuild()` branch (LinkageMapELF.py:233-324), which keeps such a library in the map; portuale never ported that branch.

## fn find_unneeded_preserved
- Ports `_find_unneeded_preserved_nodes()` (`vartree.py:1899-1943`): given `consumers_by_preserved` (a registered preserved-library path -> the paths that link against it, both sides already filtered to files that exist on disk), return the subset of `preserved_paths` that nothing needs. A preserved library is *needed* iff it has a consumer that is not itself a preserved library, or a consumer that is a preserved library which is itself needed (transitively). Paths are deduplicated by `_obj_key` (dev/inode) so a hardlink alias counts once.
- A preserved consumer is kept in the map so cycle propagation works. Portage bug 652382: a cycle of preserved libraries with no outside consumer is unneeded.

## tests: parse_reads_a_real_minimal_five_field_line
- Live-verified against `/usr/bin/true`; see `ebuild_merge.rs`'s `real_merge_copies_a_real_needed_elf2_into_the_vdb` test.

## tests: to_needed_line_writes_the_portage_rewrite_format_not_the_scanelf_read_format
- Portage `__str__`'s two asymmetries; the 6th field is always present even though the original 5-field line omitted it.

## tests: compute_multilib_category_classifies_real_elf_headers
- Backlog #39; x32 is `EM_X86_64`+`ELFCLASS32`.

## tests: generate_soname_deps_matches_the_portage_soname_deps_processor
- Backlog #39; the self-provided case is Portage's `_intersect`.
