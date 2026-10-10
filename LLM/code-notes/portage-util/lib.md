# crates/portage-util/src/lib.rs
Filesystem helpers shared by the portage-* crates.

## module
- `read_dir_entries` is the single directory-listing seam the whole workspace reads through (`docs/history/second_python_copy_removal.md` section 9's second half, backlog #51).
- The `PORTUALE_SHUFFLE_DIRS` shuffle is test/CI-only so the determinism test can prove no consumer depends on readdir order.

## fn shuffle_seed
- Frozen at first use (backlog #113): the variable is fixed per process in test/CI use. Caching also removes the `set_var` race surface `cleanroom/parallelism.md` section 1.6 names from the hot path.

## const MERGING_IDENTIFIER
- Portage: `lib/portage/const.py`, `MERGING_IDENTIFIER`; skipped via `vartree.py` `vardbapi._excluded_dirs`.

## fn is_merging_vdb_entry
- Only the `MERGING_IDENTIFIER` arm of `vardbapi._excluded_dirs` is ported; the dotfile/`CVS`/`lost+found` arms stay unported.

## fn recursive_config_files
- VCS names are only a directory rule in Portage; `os.stat` is followed.
- Stack DFS pushing children in reverse, one ascending sort per directory over files and subdirectories together.

## tests: shuffle_seed_is_frozen_at_first_use...
- #113 freezes the value at first use, so a mid-process `set_var` is ignored; the candidate value can only be changed before the binary's first `shuffle_seed()` call, which this test cannot control. The first call here pins `None` (the harness never sets the variable), which also keeps this test from leaking a shuffled order into the sibling traversal test.

## tests: recursive_config_files_matches_portages_traversal_and_filters
- Backlog #89 S0. Oracle tree: nested fragments, backups, dotfiles, VCS dirs at every level, interleaved file/dir sort, symlinked file and directory.
