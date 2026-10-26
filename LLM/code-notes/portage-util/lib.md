# crates/portage-util/src/lib.rs
Filesystem helpers shared by the portage-* crates.

## module
- `read_dir_entries` is the single directory-listing seam the whole workspace reads through (`docs/history/second_python_copy_removal.md` section 9's second half, backlog #51).
- The `PORTUALE_SHUFFLE_DIRS` shuffle is test/CI-only so the determinism test can prove no consumer depends on readdir order.

## fn shuffle_seed
- Frozen at first use (backlog #113): the variable is fixed per process in test/CI use. Caching also removes the `set_var` race surface `cleanroom/parallelism.md` section 1.6 names from the hot path.

## const MERGING_IDENTIFIER
- Portage: `lib/portage/const.py`, `MERGING_IDENTIFIER`; skipped via `vartree.py` `vardbapi._excluded_dirs`.
- Portage `MERGING_IDENTIFIER` (`lib/portage/const.py`): the `"-MERGING-"` prefix Portage `dblink.dbtmpdir` uses for its own temporary, not-yet-finalized vdb entry directory, a sibling of the Portage vdb entry under the same `<category>` directory. Portage readers skip such names (`vartree.py` `vardbapi._excluded_dirs`).

## fn is_merging_vdb_entry
- Only the `MERGING_IDENTIFIER` arm of `vardbapi._excluded_dirs` is ported; the dotfile/`CVS`/`lost+found` arms stay unported.

## fn recursive_config_files
- VCS names are only a directory rule in Portage; `os.stat` is followed.
- Stack DFS pushing children in reverse, one ascending sort per directory over files and subdirectories together.
- `Path::file_name` is `None` for `/`: Portage splits that to `""`, which passes both filters, so the root is walked.
- Sockets, fifos, devices: Portage checks `S_ISDIR` then `S_ISREG` and ignores everything else.

## tests: shuffle_seed_is_frozen_at_first_use...
- #113 freezes the value at first use, so a mid-process `set_var` is ignored; the candidate value can only be changed before the binary's first `shuffle_seed()` call, which this test cannot control. The first call here pins `None` (the harness never sets the variable), which also keeps this test from leaking a shuffled order into the sibling traversal test.

## tests: recursive_config_files_matches_portages_traversal_and_filters
- Backlog #89 S0. Oracle tree: nested fragments, backups, dotfiles, VCS dirs at every level, interleaved file/dir sort, symlinked file and directory.

## const VCS_DIRS
- Portage's `VCS_DIRS` (`lib/portage/const.py:278`): directory names `_recursive_file_list` never descends into.

## fn hex_lower
- Single lowercase-hex spelling for every digest value in the workspace. Needed because `digest 0.11` replaced `generic-array` with `hybrid-array` (changelog 0.11.0 [#1358]), and `hybrid-array 0.4.15` implements only `Debug` for `Array`, so `format!("{:x}", …)` on a hasher output no longer compiles. Every hasher moved to its `digest 0.11` release (`md-5 0.11.0`, `sha1 0.11.0`, `sha2 0.11.1`, `sha3 0.12.0`, `blake2 0.11.0`); type names (`Blake2b512`, `Sha3_256`, …) and the `new`/`update`/`finalize`/`digest` shapes are unchanged, and `.iter()`/`&[u8]`-coercion call sites needed no edit.
- No behaviour change: `known_answer_digests_are_unchanged` (`binpkg/tests/digest.rs`, values from host Portage 3.0.83 `perform_checksum`) stays green across the move.
