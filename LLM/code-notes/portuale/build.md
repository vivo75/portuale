# crates/portuale/build.rs
Context moved out of the code comments.

## (see source)
- Backlog numbers removed from the comment (original text): Embeds the vendored ebuild phase runtime (`bin/`) and the package-set definitions (`cnf/sets/portage.conf`) into the binary (backlog #322 S2), so a `portuale` copied out of its build tree -- a container, a minimal host -- still has them. The output is a table, `ENTRIES`, that `src/embedded_runtime.rs` extracts at run time.  No dependencies: a directory walk and `include_bytes!` per file. The table is sorted (reproducible), records no mtimes and normalises modes to 0o755 (any exec bit) / 0o644 so the build host's umask never leaks in. The absolute paths in the generated `include_bytes!` calls exist only at compile time; they are not stored in the binary.
