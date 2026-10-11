# crates/portuale/src/embedded_runtime.rs
The phase runtime and package-set definitions compiled into the binary. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## module
- Backlog #322 S2: `build.rs` walks the repository's `bin/` and `cnf/` and writes `ENTRIES`; this module turns that table back into files.
- Why: the vendored `bin/` is the bash half of Portage (`ebuild.sh` and its source closure, the `ebuild-helpers/`), located at run time through the path of the tree the binary was built in. A binary copied to a container or a minimal host has no such tree. With the table embedded, it extracts its own copy (see `ebuild_phases::bin_dir`), so the one file is enough.
- Fidelity: modes are normalised to 0o755 (any exec bit) / 0o644, symlinks are kept with their relative targets, order is sorted so a directory comes before its contents. No mtimes are recorded.

## fn the_embedded_table_is_exactly_the_live_bin_and_cnf_trees
- Backlog #322 S2.1: the compiled-in table is exactly the live `bin/` and `cnf/` trees: same paths, bytes, exec bits, symlink targets, no extra and no missing entries. (A stale table would mean a missed `rerun-if-changed`.)

## fn extracting_the_embedded_bin_reproduces_the_live_tree_and_is_idempotent
- S2.2: extraction reproduces `bin/` (bytes, exec bits, relative symlinks), sets the normalised modes explicitly, and is idempotent (a second extraction over the first changes nothing). The extracted tree is re-keyed under `bin` to compare with the live one; a symlink stays a symlink to the same relative target.

## fn the_extracted_portuale_owned_helpers_are_executable
- #326 S2.2: the extracted shims and `ecompress-file` are executable (the `bin/chmod-lite` call at the end of every unpack and the `ecompress-file` call in `docompress` both exec them).

## fn a_phase_running_merge_works_from_an_extracted_runtime
- S2.5 in-process half: a phase-running remote merge (the hook-ordering fixture: `pkg_setup`, `pkg_preinst`, `pkg_postinst` on the client) driven by the binary with `PORTUALE_BIN_DIR` at an extracted copy of the embedded runtime, never the live `bin/`. Proves the extracted tree works as a phase runtime (modes, symlinks, helper lookups). The relocated-binary half (no build tree at all) is the container run in `LLM/evidence/322-s2.md`.

## fn the_embedded_package_sets_conf_equals_the_checkout_file
- S2.7: the set definitions fall back to the embedded copy, and the copy is the checkout's file when both exist (so `--list-sets` is byte-identical in-tree and relocated).
