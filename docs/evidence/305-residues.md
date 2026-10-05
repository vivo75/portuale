# #305 (feat#157) — residues collected during execution

Working list for step Z.2, which files each one under a free backlog number. Collected by the coordinator; references are plan steps.

- R1 (S1.4): files next_counter takes no lock, no max over entry COUNTERs, non-atomic write; real vartree.py:1304-1397 does all three.
- R2 (S2.1): musl/smoke_test.sh fails at `COPY fixtures/` — fixtures/ is a symlink to ../pmtest/fixtures, outside the podman build context (pre-existing since fixtures moved to pmtest).
- R3 (S1.4): quickpkg_from_vdb / PORTAGE_UPDATE_ENV need entry_path; database backends need a scratch copy (S4).
- R4 (S1.6): `emerge -C <name>` lists whole VDB before reading SLOTs (same syscalls, reordered).
- R5 (S2.2): seal_entry skips non-UTF-8 field files, so a sealed entry's aux_get returns "" where unsealed returns lossy text (pre-existing, pinned by conformance oddity test).
- R6 (S2.2): FilesDb aux_get memo keys on entry dir mtime; an in-place replace_file of one of the 23 field files would be served stale (only CONTENTS/NEEDED are rewritten today).
- R7: emerge_build::tests::a_hard_failure_kills_still_running_builds_instead_of_waiting_them_out is timing-sensitive; fails under heavy load (S1.10 run during a musl build), passes alone 3/3. Pre-existing.
- R8 (S3.1): pretend::tests::ask_* failed once under heavy load, passed on rerun (timing-sensitive, pre-existing).
- R9 (S4.1): with >=2 replaced same-slot instances (broken VDB state), on a database backend the earlier-unmerged instance stays visible to the later one's preserved-libs enumeration until the final commit (only others_in_slot is corrected).
- R10 (S4.1): crash after populate consumes a counter value on sqlite (gap, never reuse).
- R11 (S4.1): scratch dirs for sqlite entry-path consumers are cleaned with the build dir; an early-returning merge_binpkg leaves them like it leaves the build dir.
- R12 (S4.4): a native sqlite merge stores entry dir_mode as 0o755 (no type bits) while FilesDb::entry_image reports full st_mode (0o40755); verify(files, native sqlite) would flag "directory mode". Normalise to permission bits in one place.
- R13 (S4.4): something in the portuale test process changes the process umask while tests run in parallel (files-backend entry/payload modes come out 0o664/0o775 intermittently); a_binpkg_merged_on_sqlite_matches_the_files_merge still compares raw entry file modes and is exposed. Find the umask writer.
- R14 (S6.4): vendored .py helpers (doins.py, dohtml.py, xpak-helper.py, gpkg-helper.py, install.py) + their Python modules still come from 3rdparty/portage; L1 still needs the checkout.
- R15 (S6.4, pmtest): L1 preflight portuale_phase_helpers_preflight (run/lib.sh:92) checks the checkout's portageq-wrapper; stale now that the vendored shim answers. Point it at the remaining .py helpers instead.
- R16 (S6.4): remote.rs phase environment does not export PORTUALE_BIN / PORTUALE_VDB_*; remote phases use the shim's PATH fallback.
- R17 (S7.4): FUSE mount exercised 2026-10-05 (docs/evidence/305-s7-fuse.md: qlist/qsize/qfile/qcheck/eix/equery/emerge -p identical through the view). Still open, untested: same-size+same-mtime rewrite under a big-file handle unnoticed; files in-place replace_file invisible to the view until a dir mtime changes.
- R18 (S8.1): databases converted before d94bd768 have no 'unknown' dep_atom marker rows; re-convert or run rebuild-index (S8.3).
- R19 (S8): host-corpus index check re-run 2026-10-05 and green (PORTUALE_VDB_HOST_CORPUS=1 cargo test --release -p portage-vdb --features vdb-sqlite,vdb-redb --test corpus_round_trip: 6 passed, 3163 s). Left open: host_index_queries_match_the_scan_when_enabled alone takes ~50 min on one core (2130-entry host VDB); worth speeding up or parallelising.
