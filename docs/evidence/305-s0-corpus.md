# #305 (feat#157) S0.3 — Round-trip corpus definition

Status: defined 2026-10-04

## Overview

This document defines the round-trip corpus used to verify VDB backend conversions for feature #305 (swappable installed-package database). The corpus consists of fixture VDBs from the pmtest repository and optionally the host VDB.

### Environment variable rule

Tests read the **host VDB at `/var/db/pkg`** only when the environment variable `PORTUALE_VDB_HOST_CORPUS=1` is set. Tests never write to the host VDB.

### Helper script

Run the corpus analyzer with:
```bash
python3 portuale/docs/evidence/305-s0-corpus.py \
  pmtest/fixtures/var/db/pkg \
  pmtest/fixtures/quickpkgroot/var/db/pkg
```

To include the host VDB (when `PORTUALE_VDB_HOST_CORPUS=1` is set):
```bash
PORTUALE_VDB_HOST_CORPUS=1 python3 portuale/docs/evidence/305-s0-corpus.py \
  pmtest/fixtures/var/db/pkg \
  pmtest/fixtures/quickpkgroot/var/db/pkg \
  /var/db/pkg
```

## Corpus members

### 1. pmtest fixture VDB: `pmtest/fixtures/var/db/pkg`

**Path:** `/home/vivo/repo/PORTUALE/pmtest/fixtures/var/db/pkg`

**Statistics:**
- Categories: 4 (app-misc, dev-libs, sys-apps, virtual)
- Entries: 131
- Files: 535
- Total size (apparent): 4,553 bytes

**Distinct file names (16 types):**
- CATEGORY: 131 entries
- SLOT: 130 entries
- repository: 110 entries
- RDEPEND: 46 entries
- USE: 42 entries
- IUSE: 40 entries
- EAPI: 17 entries
- BDEPEND: 5 entries
- PF: 3 entries
- DEFINED_PHASES: 2 entries
- CFLAGS: 2 entries
- environment.bz2: 2 entries
- DEPEND: 2 entries
- CHOST: 1 entry
- BUILD_TIME: 1 entry
- metadata: 1 entry

**Unusual cases:**
- **Empty files (12):** The fixture intentionally contains 12 empty files to test handling of null USE and IUSE variables. These are legitimate test cases and expected.
  - Examples: dev-libs/instcyclea-1.0/{IUSE,USE}, dev-libs/rebuildconsumer-1.0/{IUSE,USE}
  
- **Missing metadata files (130 of 131 entries):** The fixture VDBs are used for unit tests and do not include consolidated `metadata` files. Only one entry (`dev-libs/stalesnapshot-1.0`) contains a metadata file to test the metadata mtime stamp logic (see below).
  
- **Stale metadata mtime (1 entry):** The entry `dev-libs/stalesnapshot-1.0` intentionally has a metadata file with a stale `#dir_mtime=` stamp (value 1) that does not match the directory's actual mtime. This is a deliberate test case for the real-Portage behavior when entries are not properly updated.

### 2. pmtest fixture VDB: `pmtest/fixtures/quickpkgroot/var/db/pkg`

**Path:** `/home/vivo/repo/PORTUALE/pmtest/fixtures/quickpkgroot/var/db/pkg`

**Statistics:**
- Categories: 1 (dev-libs)
- Entries: 1
- Files: 8
- Total size (apparent): 45 bytes

**Distinct file names (8 types):**
- repository: 1
- RDEPEND: 1
- CATEGORY: 1
- DEPEND: 1
- SLOT: 1
- IUSE: 1
- KEYWORDS: 1
- USE: 1

**Unusual cases:**
- **Missing metadata files (1 of 1 entries):** Like the main fixture, this small fixture does not include metadata files. The single entry is `dev-libs/quickpkgdirectpkg-1.0`.

## Test coverage rationale

### Fixture VDBs

The two fixture VDBs cover:
1. **Standard package repository format**: All 131 entries follow the Portage package database format with metadata stored in individual files (SLOT, EAPI, RDEPEND, USE, etc.).
2. **Edge cases in resolver testing**: The main fixture contains 131 carefully constructed entries used for resolver unit tests, including:
   - Entries with varying dependency complexity (RDEPEND, BDEPEND, DEPEND)
   - Entries with and without USE flags
   - Multiple slot variants of the same package
   - Dependency cycles and reverse-dependency scenarios
3. **Small corpus**: The quickpkgroot fixture is a minimal single-entry VDB used to test quick package operation paths.

### 3. Host VDB (optional): `/var/db/pkg`

Read only when `PORTUALE_VDB_HOST_CORPUS=1`; tests never write to it.
Measured 2026-10-04 with the helper script (the coordinator re-ran it):

- Categories: 102
- Entries: 2,130
- Files: 75,611
- Total size (apparent): 143,941,725 bytes (equals `du -s --apparent-size --block-size=1`)
- Every entry has `CONTENTS`, `COUNTER`, `SLOT`, `USE`, `IUSE`,
  `environment.bz2`, `BUILD_ID`, `repository`, `REPO_REVISIONS`, …;
  1,316 have `NEEDED.ELF.2`; 133 have `CC`.

**Unusual cases:**
- **Empty files (129):** all `DEBUGBUILD` markers (e.g.
  `app-admin/awscli-bin-2.36.34/DEBUGBUILD`). Their presence is the data,
  so a converter must keep empty files.
- **Consolidated `metadata` file present in only 630 of 2,130 entries.**
  The other 1,500 are read field by field. A converter must not add a
  `metadata` file to an entry that had none.
- **Stale `#dir_mtime=` stamps (6):** `app-text/poppler-26.09.0`,
  `dev-libs/tree-sitter-0.27.0`, `dev-libs/capstone-6.0.0_alpha10`,
  `kde-apps/kate-lib-26.08.1`, `media-libs/opencv-4.14.0`,
  `sci-libs/hdf5-2.2.0`. In each, the stamp is older than the
  directory's mtime, so real's reader ignores these `metadata` files.
- None found: invalid UTF-8 names, unusual modes, symlinks,
  `-MERGING-` directories, hidden files, non-directories at category
  level.

## Consequence for converters (input to S2.6 / S2.7)

A stale stamp must **stay stale** after a round trip. The fixture
`dev-libs/stalesnapshot-1.0` exists to prove that a stale snapshot is
ignored, and the six host entries rely on the same rule. If `FilesDb`
re-stamped every `metadata` file it writes, a stale snapshot would
become valid. Readers would then see the snapshot body instead of the
per-field files, which is a behaviour change. Rule: the database
backends store whether each entry's stamp was valid
(`metadata_stamp_valid`). On export, `FilesDb` writes a fresh stamp
only for entries whose stamp was valid. For stale entries it writes a
stamp that cannot match, such as the stored value, which is in the
past.

## Verification rules for round-trip tests

When converting between backends (files ↔ sqlite ↔ redb), tests check
that the following survive the round trip:

1. **Exact bytes** of every file in every entry, empty files included.
   Values and names may be invalid UTF-8 (design §7.3: bytes, not text).
2. **The set of files** per entry. No file is added or dropped,
   including `metadata`.
3. **File and directory modes** as found. Preserve them; do not
   normalise to 0644/0755.
4. **mtimes:** file mtimes and entry directory mtimes preserved. The
   `metadata` stamp is compared by its validity (equal to the
   directory's `st_mtime_ns` or not), not byte for byte (S2.6 rule).
5. **Counters** preserved (`COUNTER`, and the `counter` store).
6. **No `-MERGING-` directories** after conversion.

## Notes for future reference

- The fixture VDBs are intentionally simplified: most entries have no `metadata` file, which is also the common case on the host (1,500 of 2,130).
- The stale metadata mtime entry (`dev-libs/stalesnapshot-1.0`) tests the real-Portage behavior where entries may have out-of-sync metadata stamps if not properly updated.
- Empty file test cases (USE/IUSE) are legitimate: Portage represents empty variable values as empty files on disk.
- All file sizes in the corpus are small (4.5 KB for the main fixture), making round-trip testing fast.
